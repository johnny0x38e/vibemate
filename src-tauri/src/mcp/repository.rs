//! MCP SQLite reads and metadata transactions; no credential store access.

use super::types::*;
use super::validation::{field_name, identity, validate};
use crate::storage::Storage;
use rusqlite::{OptionalExtension, params};
use std::collections::BTreeMap;

pub(super) fn lock(
    storage: &Storage,
) -> Result<std::sync::MutexGuard<'_, rusqlite::Connection>, McpError> {
    storage.lock().map_err(|_| McpError::StorageUnavailable)
}

fn random_id(connection: &rusqlite::Connection) -> Result<String, McpError> {
    connection
        .query_row("SELECT lower(hex(randomblob(16)))", [], |row| row.get(0))
        .map_err(|_| McpError::WriteFailed)
}

fn references(
    connection: &rusqlite::Connection,
    id: &str,
) -> Result<BTreeMap<(String, String), String>, McpError> {
    let mut statement = connection.prepare("SELECT kind, name, credential_ref FROM mcp_secret WHERE definition_id = ?1 ORDER BY kind, name").map_err(|_| McpError::ReadFailed)?;
    statement
        .query_map([id], |row| Ok(((row.get(0)?, row.get(1)?), row.get(2)?)))
        .map_err(|_| McpError::ReadFailed)?
        .collect::<Result<_, _>>()
        .map_err(|_| McpError::ReadFailed)
}

pub(super) fn secret_references(
    storage: &Storage,
    id: &str,
) -> Result<BTreeMap<(String, String), String>, McpError> {
    identity(id)?;
    let connection = lock(storage)?;
    references(&connection, id)
}

fn read_record(connection: &rusqlite::Connection, id: &str) -> Result<McpRecord, McpError> {
    let row: Option<(String,String,bool,i64,String,i64,i64)> = connection.query_row(
        "SELECT display_name,server_name,enabled,revision,connection,created_at,updated_at FROM mcp_definition WHERE id=?1", [id],
        |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?))).optional().map_err(|_| McpError::ReadFailed)?;
    let (display_name, server_name, enabled, revision, json, created_at_ms, updated_at_ms) =
        row.ok_or(McpError::NotFound)?;
    let stored: StoredConnection =
        serde_json::from_str(&json).map_err(|_| McpError::InvalidStoredDefinition)?;
    let refs = references(connection, id)?;
    let connection_record = match &stored {
        StoredConnection::Stdio { command, args, cwd } => ConnectionRecord::Stdio {
            command: command.clone(),
            args: args.clone(),
            cwd: cwd.clone(),
            env: refs.keys().map(|(_, name)| name.clone()).collect(),
        },
        StoredConnection::Http { url } => ConnectionRecord::Http {
            url: url.clone(),
            headers: refs.keys().map(|(_, name)| name.clone()).collect(),
        },
    };
    // Reuse input validation with no values. This validates persisted metadata
    // without reading the OS store or interpreting any stored command.
    let input_connection = match stored {
        StoredConnection::Stdio { command, args, cwd } => ConnectionInput::Stdio {
            command,
            args,
            cwd,
            env: refs
                .keys()
                .map(|(_, name)| SecretInput {
                    name: name.clone(),
                    value: None,
                })
                .collect(),
        },
        StoredConnection::Http { url } => ConnectionInput::Http {
            url,
            headers: refs
                .keys()
                .map(|(_, name)| SecretInput {
                    name: name.clone(),
                    value: None,
                })
                .collect(),
        },
    };
    let (_, _, _, valid) = validate(SaveMcpRequest {
        id: Some(id.to_string()),
        expected_revision: Some(revision),
        display_name: display_name.clone(),
        server_name: server_name.clone(),
        enabled,
        connection: input_connection,
    })
    .map_err(|_| McpError::InvalidStoredDefinition)?;
    if refs.iter().any(|((kind, name), reference)| {
        kind != valid.kind
            || field_name(name, kind).ok().as_ref() != Some(name)
            || !reference.starts_with(&format!("mcp-{id}-"))
            || reference.len() != 69
            || identity(&reference[37..]).is_err()
    }) || created_at_ms < 0
        || updated_at_ms < created_at_ms
    {
        return Err(McpError::InvalidStoredDefinition);
    }
    let cleanup_pending = connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM mcp_cleanup WHERE definition_id=?1)",
            [id],
            |r| r.get(0),
        )
        .map_err(|_| McpError::ReadFailed)?;
    Ok(McpRecord {
        id: id.to_string(),
        display_name,
        server_name,
        enabled,
        revision,
        connection: connection_record,
        created_at_ms,
        updated_at_ms,
        cleanup_pending,
    })
}

/// Read a definition without touching credentials, executing a process or networking.
///
/// # Errors
/// Returns safe errors for an invalid ID, missing row, inaccessible storage or
/// unrecognized stored metadata. Secret values are never included.
pub fn get_definition(storage: &Storage, id: &str) -> Result<McpRecord, McpError> {
    identity(id)?;
    let connection = lock(storage)?;
    read_record(&connection, id)
}

/// Read a stable ID cursor page (1–100 rows), without credential access.
///
/// # Errors
/// Rejects invalid cursors or limits and reports storage/read/metadata failures.
pub fn list_definitions(
    storage: &Storage,
    after: Option<&str>,
    limit: u32,
) -> Result<McpPage, McpError> {
    if !(1..=100).contains(&limit) {
        return Err(McpError::InvalidRequest);
    }
    if let Some(cursor) = after {
        identity(cursor)?;
    }
    let connection = lock(storage)?;
    let total: i64 = connection
        .query_row("SELECT COUNT(*) FROM mcp_definition", [], |row| row.get(0))
        .map_err(|_| McpError::ReadFailed)?;
    let mut statement = connection
        .prepare("SELECT id FROM mcp_definition WHERE (?1 IS NULL OR id > ?1) ORDER BY id LIMIT ?2")
        .map_err(|_| McpError::ReadFailed)?;
    let ids: Vec<String> = statement
        .query_map(params![after, limit + 1], |r| r.get(0))
        .map_err(|_| McpError::ReadFailed)?
        .collect::<Result<_, _>>()
        .map_err(|_| McpError::ReadFailed)?;
    let next_cursor = if ids.len() > limit as usize {
        ids.get(limit as usize - 1).cloned()
    } else {
        None
    };
    let items = ids
        .iter()
        .take(limit as usize)
        .map(|id| read_record(&connection, id))
        .collect::<Result<_, _>>()?;
    Ok(McpPage {
        items,
        next_cursor,
        total,
        deleted_cleanup_pending: has_deleted_cleanup(&connection)?,
    })
}

pub(super) fn commit_definition(
    storage: &Storage,
    id: &str,
    expected: Option<i64>,
    enabled: bool,
    valid: &Validated,
    next: &BTreeMap<(String, String), String>,
    now: i64,
) -> Result<(), (McpError, bool)> {
    let operation = || -> Result<(), (McpError, bool)> {
        let mut connection = lock(storage).map_err(|e| (e, false))?;
        let tx = connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(|_| (McpError::WriteFailed, false))?;
        let clash: bool = tx
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM mcp_definition WHERE namespace=?1 AND id!=?2)",
                params![valid.namespace, id],
                |r| r.get(0),
            )
            .map_err(|_| (McpError::WriteFailed, false))?;
        if clash {
            return Err((McpError::ServerNameTaken, false));
        }
        let json =
            serde_json::to_string(&valid.connection).map_err(|_| (McpError::WriteFailed, false))?;
        if let Some(revision) = expected {
            let changed = tx
                .execute(
                    "UPDATE mcp_definition
                     SET display_name=?1, server_name=?2, namespace=?3,
                         enabled=?4, connection=?5, revision=revision+1,
                         updated_at=max(updated_at,?6)
                     WHERE id=?7 AND revision=?8",
                    params![
                        valid.display_name,
                        valid.server_name,
                        valid.namespace,
                        enabled,
                        json,
                        now,
                        id,
                        revision
                    ],
                )
                .map_err(|_| (McpError::WriteFailed, false))?;
            if changed != 1 {
                return Err((McpError::RevisionConflict, false));
            }
        } else {
            tx.execute(
                "INSERT INTO mcp_definition
                 (id, display_name, server_name, namespace, enabled, revision,
                  connection, created_at, updated_at)
                 VALUES(?1,?2,?3,?4,?5,1,?6,?7,?7)",
                params![
                    id,
                    valid.display_name,
                    valid.server_name,
                    valid.namespace,
                    enabled,
                    json,
                    now
                ],
            )
            .map_err(|_| (McpError::WriteFailed, false))?;
        }
        let old = references(&tx, id).map_err(|error| (error, false))?;
        for (key, reference) in &old {
            if next.get(key) != Some(reference) {
                tx.execute(
                    "INSERT OR IGNORE INTO mcp_cleanup(definition_id,credential_ref) VALUES(?1,?2)",
                    params![id, reference],
                )
                .map_err(|_| (McpError::WriteFailed, false))?;
            }
        }
        tx.execute("DELETE FROM mcp_secret WHERE definition_id=?1", [id])
            .map_err(|_| (McpError::WriteFailed, false))?;
        for ((kind, name), reference) in next {
            tx.execute(
                "INSERT INTO mcp_secret(definition_id,kind,name,credential_ref)
                 VALUES(?1,?2,?3,?4)",
                params![id, kind, name, reference],
            )
            .map_err(|_| (McpError::WriteFailed, false))?;
        }
        tx.commit().map_err(|_| (McpError::OutcomeUnknown, true))
    };
    operation()
}

/// Read obsolete references while excluding every currently active reference.
pub(super) fn pending_cleanup_references(
    storage: &Storage,
    id: &str,
) -> Result<Vec<String>, McpError> {
    let connection = lock(storage)?;
    let mut statement=connection.prepare("SELECT credential_ref FROM mcp_cleanup WHERE definition_id=?1 AND NOT EXISTS(SELECT 1 FROM mcp_secret WHERE mcp_secret.credential_ref=mcp_cleanup.credential_ref)").map_err(|_|McpError::ReadFailed)?;
    statement
        .query_map([id], |r| r.get(0))
        .map_err(|_| McpError::ReadFailed)?
        .collect::<Result<_, _>>()
        .map_err(|_| McpError::ReadFailed)
}

/// Remove a cleanup entry only after its OS credential deletion succeeds.
pub(super) fn acknowledge_cleanup(storage: &Storage, reference: String) -> Result<(), McpError> {
    lock(storage)?
        .execute(
            "DELETE FROM mcp_cleanup WHERE credential_ref=?1",
            [reference],
        )
        .map_err(|_| McpError::WriteFailed)?;
    Ok(())
}

/// Report whether retryable credential cleanup has finished.
pub(super) fn cleanup_complete(storage: &Storage, id: &str) -> Result<bool, McpError> {
    let connection = lock(storage)?;
    connection
        .query_row(
            "SELECT NOT EXISTS(SELECT 1 FROM mcp_cleanup WHERE definition_id=?1)",
            [id],
            |r| r.get(0),
        )
        .map_err(|_| McpError::ReadFailed)
}

/// Metadata snapshot and fresh IDs prepared before any OS credential operation.
pub(super) struct SaveSnapshot {
    pub(super) id: String,
    pub(super) old_refs: BTreeMap<(String, String), String>,
    pub(super) new_ids: Vec<Option<String>>,
}

/// Prepare a revision-checked snapshot, releasing the DB lock before store writes.
pub(super) fn prepare_definition_save(
    storage: &Storage,
    existing_id: Option<&str>,
    revision: Option<i64>,
    valid: &Validated,
) -> Result<SaveSnapshot, McpError> {
    let connection = lock(storage)?;
    let id = match existing_id {
        Some(id) => id.to_string(),
        None => random_id(&connection)?,
    };
    if let Some(expected) = revision
        && read_record(&connection, &id)?.revision != expected
    {
        return Err(McpError::RevisionConflict);
    }
    let old_refs = references(&connection, &id)?;
    let ids = valid
        .fields
        .iter()
        .map(|field| {
            if field.value.is_some() {
                random_id(&connection).map(Some)
            } else {
                Ok(None)
            }
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(SaveSnapshot {
        id,
        old_refs,
        new_ids: ids,
    })
}

fn has_deleted_cleanup(connection: &rusqlite::Connection) -> Result<bool, McpError> {
    connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM mcp_cleanup WHERE NOT EXISTS(SELECT 1 FROM mcp_definition WHERE id=mcp_cleanup.definition_id))",
        [], |row| row.get(0),
    ).map_err(|_| McpError::ReadFailed)
}

/// Read a bounded batch of removed identities so retries cannot do unlimited work.
pub(super) fn deleted_cleanup_ids(storage: &Storage) -> Result<Vec<String>, McpError> {
    let connection = lock(storage)?;
    let mut statement = connection.prepare(
        "SELECT DISTINCT definition_id FROM mcp_cleanup WHERE NOT EXISTS(SELECT 1 FROM mcp_definition WHERE id=mcp_cleanup.definition_id) ORDER BY definition_id LIMIT 100",
    ).map_err(|_| McpError::ReadFailed)?;
    statement
        .query_map([], |row| row.get(0))
        .map_err(|_| McpError::ReadFailed)?
        .collect::<Result<_, _>>()
        .map_err(|_| McpError::ReadFailed)
}

/// Queue credentials and delete metadata together; only confirmed commits allow OS cleanup.
pub(super) fn commit_definition_delete(
    storage: &Storage,
    request: &DeleteMcpRequest,
) -> Result<(), McpError> {
    let mut connection = lock(storage)?;
    let tx = connection
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(|_| McpError::WriteFailed)?;
    let record = read_record(&tx, &request.id)?;
    if record.revision != request.expected_revision {
        return Err(McpError::RevisionConflict);
    }
    let in_use: bool = tx
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM pi_mcp_deployment WHERE definition_id=?1)
            OR EXISTS(SELECT 1 FROM pi_mcp_change WHERE definition_id=?1 AND state='prepared')
            OR EXISTS(SELECT 1 FROM grok_mcp_deployment WHERE definition_id=?1)
            OR EXISTS(SELECT 1 FROM grok_mcp_change WHERE definition_id=?1 AND state='prepared')",
            [&request.id],
            |row| row.get(0),
        )
        .map_err(|_| McpError::ReadFailed)?;
    if in_use {
        return Err(McpError::McpInUse);
    }
    tx.execute(
        "INSERT OR IGNORE INTO mcp_cleanup(definition_id,credential_ref) SELECT definition_id,credential_ref FROM mcp_secret WHERE definition_id=?1",
        [&request.id],
    ).map_err(|_| McpError::WriteFailed)?;
    // Foreign keys remove only this definition's secret references. The cleanup
    // queue intentionally has no foreign key and survives deletion and restart.
    tx.execute(
        "DELETE FROM mcp_definition WHERE id=?1 AND revision=?2",
        params![request.id, request.expected_revision],
    )
    .map_err(|_| McpError::WriteFailed)?;
    tx.commit().map_err(|_| McpError::OutcomeUnknown)
}
