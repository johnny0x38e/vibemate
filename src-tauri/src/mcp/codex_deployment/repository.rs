//! Non-secret ownership, intent and entry-history persistence for Codex MCP.
//! Call mutation helpers only under the shared configuration guard.

use super::types::{Action, Error, HistoryEntry, HistoryPage, Ownership};
use crate::storage::Storage;
use rusqlite::{OptionalExtension, params};

pub(super) fn lock(
    storage: &Storage,
) -> Result<std::sync::MutexGuard<'_, rusqlite::Connection>, Error> {
    storage.lock().map_err(|_| Error::StorageUnavailable)
}
pub(super) fn random_id(storage: &Storage) -> Result<String, Error> {
    lock(storage)?
        .query_row("SELECT lower(hex(randomblob(16)))", [], |r| r.get(0))
        .map_err(|_| Error::StorageUnavailable)
}
pub(super) fn owner(storage: &Storage, root: &str, id: &str) -> Result<Option<Ownership>, Error> {
    let connection = lock(storage)?;
    owner_in(&connection, root, id)
}
fn owner_in(db: &rusqlite::Connection, root: &str, id: &str) -> Result<Option<Ownership>, Error> {
    db.query_row(
        "SELECT server_name,definition_revision,entry_digest,generation,enabled FROM codex_mcp_deployment WHERE config_root=?1 AND definition_id=?2",
        params![root, id],
        |r| {
            Ok(Ownership {
                server_name: r.get(0)?,
                definition_revision: r.get(1)?,
                entry_digest: r.get(2)?,
                generation: r.get(3)?,
                enabled: r.get(4)?,
            })
        },
    )
    .optional()
    .map_err(|_| Error::StorageUnavailable)
}

#[derive(Clone)]
pub(super) struct Change {
    pub id: String,
    pub root: String,
    pub definition_id: String,
    pub server_name: String,
    pub definition_revision: i64,
    pub action: String,
    pub state: String,
    pub before_digest: Option<String>,
    pub intended_digest: Option<String>,
    pub before_owner: Option<Ownership>,
    pub after_owner: Option<Ownership>,
    pub operation_id: Option<String>,
    pub created_at: i64,
}
fn change_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Change> {
    let before: Option<String> = r.get(9)?;
    let after: Option<String> = r.get(10)?;
    let decode = |value: Option<String>| -> rusqlite::Result<Option<Ownership>> {
        value
            .map(|value| serde_json::from_str(&value).map_err(|_| rusqlite::Error::InvalidQuery))
            .transpose()
    };
    Ok(Change {
        id: r.get(0)?,
        root: r.get(1)?,
        definition_id: r.get(2)?,
        server_name: r.get(3)?,
        definition_revision: r.get(4)?,
        action: r.get(5)?,
        state: r.get(6)?,
        before_digest: r.get(7)?,
        intended_digest: r.get(8)?,
        before_owner: decode(before)?,
        after_owner: decode(after)?,
        operation_id: r.get(11)?,
        created_at: r.get(12)?,
    })
}
const COLUMNS: &str = "id,config_root,definition_id,server_name,definition_revision,action,state,before_digest,intended_digest,before_owner,after_owner,operation_id,created_at";
pub(super) fn pending(storage: &Storage, root: &str) -> Result<Option<Change>, Error> {
    lock(storage)?
        .query_row(
            &format!(
                "SELECT {COLUMNS} FROM codex_mcp_change WHERE config_root=?1 AND state='prepared'"
            ),
            [root],
            change_row,
        )
        .optional()
        .map_err(|_| Error::StorageUnavailable)
}
pub(super) fn retained(
    storage: &Storage,
    root: &str,
    definition: &str,
    id: &str,
) -> Result<Change, Error> {
    lock(storage)?
        .query_row(
            &format!(
                "SELECT {COLUMNS} FROM codex_mcp_change WHERE config_root=?1 AND definition_id=?2 AND id=?3 AND state='completed'"
            ),
            params![root, definition, id],
            change_row,
        )
        .optional()
        .map_err(|_| Error::StorageUnavailable)?
        .ok_or(Error::SnapshotUnavailable)
}

pub(super) fn prepare(storage: &Storage, change: &Change) -> Result<(), Error> {
    let mut db = lock(storage)?;
    let tx = db
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(|_| Error::StorageUnavailable)?;
    let revision: Option<i64> = tx
        .query_row(
            "SELECT revision FROM mcp_definition WHERE id=?1",
            [&change.definition_id],
            |r| r.get(0),
        )
        .optional()
        .map_err(|_| Error::StorageUnavailable)?;
    if revision != Some(change.definition_revision) {
        return Err(Error::DefinitionChanged);
    }
    if owner_in(&tx, &change.root, &change.definition_id)? != change.before_owner {
        return Err(Error::PlanStale);
    }
    let encode = |value: &Option<Ownership>| {
        value
            .as_ref()
            .map(serde_json::to_string)
            .transpose()
            .map_err(|_| Error::StorageUnavailable)
    };
    tx.execute(
        "INSERT INTO codex_mcp_change(id,config_root,definition_id,server_name,definition_revision,action,state,before_digest,intended_digest,before_owner,after_owner,created_at) VALUES(?1,?2,?3,?4,?5,?6,'prepared',?7,?8,?9,?10,?11)",
        params![
            change.id,
            change.root,
            change.definition_id,
            change.server_name,
            change.definition_revision,
            change.action,
            change.before_digest,
            change.intended_digest,
            encode(&change.before_owner)?,
            encode(&change.after_owner)?,
            change.created_at
        ],
    )
    .map_err(|_| Error::RecoveryRequired)?;
    tx.commit().map_err(|_| Error::OutcomeUnknown)
}

/// The intent ID is the shared operation's owner reference. This lookup survives
/// a crash between the writer returning and metadata finalization.
pub(super) fn operation_id(storage: &Storage, change: &Change) -> Result<Option<String>, Error> {
    let db = lock(storage)?;
    let mut statement = db
        .prepare("SELECT id FROM config_operation WHERE kind='codex_mcp' AND owner_ref=?1 LIMIT 2")
        .map_err(|_| Error::StorageUnavailable)?;
    let values = statement
        .query_map([&change.id], |r| r.get::<_, String>(0))
        .map_err(|_| Error::StorageUnavailable)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| Error::StorageUnavailable)?;
    if values.len() > 1
        || change
            .operation_id
            .as_ref()
            .is_some_and(|id| values.first() != Some(id))
    {
        return Err(Error::OutcomeUnknown);
    }
    Ok(values.into_iter().next())
}

pub(super) fn finalize(
    storage: &Storage,
    change: &Change,
    completed: bool,
    operation_id: Option<&str>,
) -> Result<(), Error> {
    let mut db = lock(storage)?;
    let tx = db
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(|_| Error::StorageUnavailable)?;
    let state: String = tx
        .query_row(
            "SELECT state FROM codex_mcp_change WHERE id=?1 AND config_root=?2",
            params![change.id, change.root],
            |r| r.get(0),
        )
        .map_err(|_| Error::OutcomeUnknown)?;
    if state != "prepared"
        || owner_in(&tx, &change.root, &change.definition_id)? != change.before_owner
    {
        return Err(Error::PlanStale);
    }
    if completed {
        tx.execute(
            "DELETE FROM codex_mcp_deployment WHERE config_root=?1 AND definition_id=?2",
            params![change.root, change.definition_id],
        )
        .map_err(|_| Error::OutcomeUnknown)?;
        if let Some(owner) = &change.after_owner {
            tx.execute(
                "INSERT INTO codex_mcp_deployment(config_root,definition_id,server_name,definition_revision,entry_digest,generation,enabled) VALUES(?1,?2,?3,?4,?5,?6,?7)",
                params![
                    change.root,
                    change.definition_id,
                    owner.server_name,
                    owner.definition_revision,
                    owner.entry_digest,
                    owner.generation,
                    owner.enabled
                ],
            )
            .map_err(|_| Error::OutcomeUnknown)?;
        }
    }
    tx.execute(
        "UPDATE codex_mcp_change SET state=?2,operation_id=?3 WHERE id=?1 AND state='prepared'",
        params![
            change.id,
            if completed { "completed" } else { "aborted" },
            operation_id
        ],
    )
    .map_err(|_| Error::OutcomeUnknown)?;
    tx.commit().map_err(|_| Error::OutcomeUnknown)
}

/// Include even revoked/aborted generations in approvals. Otherwise a deploy
/// approval could be replayed after deploy -> revoke restores identical bytes.
pub(super) fn last_change_id(
    storage: &Storage,
    root: &str,
    definition: &str,
) -> Result<Option<String>, Error> {
    lock(storage)?
        .query_row(
            "SELECT id FROM codex_mcp_change WHERE config_root=?1 AND definition_id=?2 ORDER BY rowid DESC LIMIT 1",
            params![root, definition],
            |row| row.get(0),
        )
        .optional()
        .map_err(|_| Error::StorageUnavailable)
}

pub(super) fn deployments(
    storage: &Storage,
    root: &str,
    definition: Option<&str>,
    after: Option<&str>,
    limit: u32,
) -> Result<Vec<(String, String, String, Ownership)>, Error> {
    let db = lock(storage)?;
    let mut statement = db
        .prepare(
            "SELECT d.definition_id,m.display_name,d.config_root,d.server_name,d.definition_revision,d.entry_digest,d.generation,d.enabled FROM codex_mcp_deployment d JOIN mcp_definition m ON m.id=d.definition_id WHERE ((?2 IS NOT NULL AND d.definition_id=?2) OR (?2 IS NULL AND d.config_root=?1 AND (?3 IS NULL OR d.definition_id>?3))) ORDER BY d.definition_id,d.config_root LIMIT ?4",
        )
        .map_err(|_| Error::StorageUnavailable)?;
    statement
        .query_map(params![root, definition, after, limit], |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                Ownership {
                    server_name: r.get(3)?,
                    definition_revision: r.get(4)?,
                    entry_digest: r.get(5)?,
                    generation: r.get(6)?,
                    enabled: r.get(7)?,
                },
            ))
        })
        .map_err(|_| Error::StorageUnavailable)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| Error::StorageUnavailable)
}

pub(super) fn history(
    storage: &Storage,
    root: &str,
    definition: &str,
    after: Option<&str>,
    limit: u32,
) -> Result<HistoryPage, Error> {
    let db = lock(storage)?;
    let cursor = after
        .map(|id| {
            db.query_row(
                "SELECT created_at FROM codex_mcp_change WHERE id=?1 AND config_root=?2 AND definition_id=?3 AND state='completed'",
                params![id, root, definition],
                |r| r.get::<_, i64>(0),
            )
            .optional()
            .map_err(|_| Error::StorageUnavailable)?
            .ok_or(Error::InvalidRequest)
        })
        .transpose()?;
    let mut statement = db
        .prepare(
            "SELECT id,action,created_at,server_name,after_owner FROM codex_mcp_change WHERE config_root=?1 AND definition_id=?2 AND state='completed' AND (?3 IS NULL OR created_at<?3 OR (created_at=?3 AND id<?4)) ORDER BY created_at DESC,id DESC LIMIT ?5",
        )
        .map_err(|_| Error::StorageUnavailable)?;
    let mut items = statement
        .query_map(params![root, definition, cursor, after, limit + 1], |r| {
            let owner: Option<String> = r.get(4)?;
            let enabled = owner
                .map(|v| {
                    serde_json::from_str::<Ownership>(&v)
                        .map(|v| v.enabled)
                        .map_err(|_| rusqlite::Error::InvalidQuery)
                })
                .transpose()?;
            Ok(HistoryEntry {
                id: r.get(0)?,
                action: r.get(1)?,
                created_at: r.get(2)?,
                server_name: r.get(3)?,
                enabled,
            })
        })
        .map_err(|_| Error::StorageUnavailable)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| Error::StorageUnavailable)?;
    let next_cursor = if items.len() > limit as usize {
        items.pop();
        items.last().map(|v| v.id.clone())
    } else {
        None
    };
    Ok(HistoryPage { items, next_cursor })
}

pub(super) fn valid_action(value: &str) -> bool {
    [
        Action::Deploy,
        Action::Enable,
        Action::Disable,
        Action::Revoke,
        Action::Restore,
    ]
    .iter()
    .any(|a| a.code() == value)
}

/// Codex provider settings share `config.toml` with MCP entries. A pending
/// settings intent pins the file's before/intended digests, so an MCP write
/// must wait until that intent is reconciled (and vice versa).
pub(super) fn settings_pending(storage: &Storage, root: &str) -> Result<bool, Error> {
    lock(storage)?
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM codex_settings_change WHERE config_root=?1 AND state='prepared')",
            [root],
            |r| r.get(0),
        )
        .map_err(|_| Error::StorageUnavailable)
}
