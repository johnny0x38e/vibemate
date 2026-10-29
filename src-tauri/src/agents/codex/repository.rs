//! Non-secret ownership, intent and history persistence for Codex settings.
//! Mutation helpers run only under the shared configuration guard.

use super::types::{
    DefaultOwner, DefaultValues, Error, HistoryEntry, HistoryPage, Owners, ProviderOwner,
};
use crate::storage::Storage;
use rusqlite::{Connection, OptionalExtension, params};

pub(super) fn lock(storage: &Storage) -> Result<std::sync::MutexGuard<'_, Connection>, Error> {
    storage.lock().map_err(|_| Error::StorageUnavailable)
}
pub(super) fn random_id(storage: &Storage) -> Result<String, Error> {
    lock(storage)?
        .query_row("SELECT lower(hex(randomblob(16)))", [], |r| r.get(0))
        .map_err(|_| Error::StorageUnavailable)
}

fn provider_in(
    db: &Connection,
    root: &str,
    provider: &str,
) -> Result<Option<ProviderOwner>, Error> {
    db.query_row(
        "SELECT provider_key,provider_revision,entry_digest,generation FROM codex_provider_binding WHERE config_root=?1 AND provider_id=?2",
        params![root, provider],
        |r| {
            Ok(ProviderOwner {
                provider_key: r.get(0)?,
                provider_revision: r.get(1)?,
                entry_digest: r.get(2)?,
                generation: r.get(3)?,
            })
        },
    )
    .optional()
    .map_err(|_| Error::StorageUnavailable)
}

fn default_in(db: &Connection, root: &str) -> Result<Option<DefaultOwner>, Error> {
    db.query_row(
        "SELECT provider_id,model_id,provider_key,entry_digest,previous,generation,writes_effort FROM codex_default_binding WHERE config_root=?1",
        [root],
        |r| {
            let previous: String = r.get(4)?;
            Ok(DefaultOwner {
                provider_id: r.get(0)?,
                model_id: r.get(1)?,
                provider_key: r.get(2)?,
                entry_digest: r.get(3)?,
                previous: serde_json::from_str::<DefaultValues>(&previous)
                    .map_err(|_| rusqlite::Error::InvalidQuery)?,
                generation: r.get(5)?,
                writes_effort: r.get(6)?,
            })
        },
    )
    .optional()
    .map_err(|_| Error::StorageUnavailable)
}

fn owners_in(db: &Connection, root: &str, provider: &str) -> Result<Owners, Error> {
    Ok(Owners {
        provider: provider_in(db, root, provider)?,
        default: default_in(db, root)?,
    })
}

pub(super) fn owners(storage: &Storage, root: &str, provider: &str) -> Result<Owners, Error> {
    let db = lock(storage)?;
    owners_in(&db, root, provider)
}

/// Provider owning `key` in this root, if any.
pub(super) fn key_owner(storage: &Storage, root: &str, key: &str) -> Result<Option<String>, Error> {
    lock(storage)?
        .query_row(
            "SELECT provider_id FROM codex_provider_binding WHERE config_root=?1 AND provider_key=?2",
            params![root, key],
            |r| r.get(0),
        )
        .optional()
        .map_err(|_| Error::StorageUnavailable)
}

#[derive(Clone)]
pub(super) struct Change {
    pub id: String,
    pub root: String,
    pub provider_id: String,
    pub provider_key: String,
    pub action: String,
    pub state: String,
    pub before_digest: Option<String>,
    pub intended_digest: Option<String>,
    pub before_owner: Owners,
    pub after_owner: Owners,
    pub operation_id: Option<String>,
    pub created_at: i64,
    /// Model set or released by a default change; null otherwise.
    pub model_id: Option<String>,
}

const COLUMNS: &str = "id,config_root,provider_id,provider_key,action,state,before_digest,intended_digest,before_owner,after_owner,operation_id,created_at,model_id";

fn change_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Change> {
    let decode = |value: String| -> rusqlite::Result<Owners> {
        serde_json::from_str(&value).map_err(|_| rusqlite::Error::InvalidQuery)
    };
    Ok(Change {
        id: r.get(0)?,
        root: r.get(1)?,
        provider_id: r.get(2)?,
        provider_key: r.get(3)?,
        action: r.get(4)?,
        state: r.get(5)?,
        before_digest: r.get(6)?,
        intended_digest: r.get(7)?,
        before_owner: decode(r.get(8)?)?,
        after_owner: decode(r.get(9)?)?,
        operation_id: r.get(10)?,
        created_at: r.get(11)?,
        model_id: r.get(12)?,
    })
}

pub(super) fn pending(storage: &Storage, root: &str) -> Result<Option<Change>, Error> {
    lock(storage)?
        .query_row(
            &format!(
                "SELECT {COLUMNS} FROM codex_settings_change WHERE config_root=?1 AND state='prepared'"
            ),
            [root],
            change_row,
        )
        .optional()
        .map_err(|_| Error::StorageUnavailable)
}

/// Codex MCP entries share `config.toml`; their pending intent pins the same
/// file digests, so settings writes wait until it is reconciled.
pub(super) fn mcp_pending(storage: &Storage, root: &str) -> Result<bool, Error> {
    lock(storage)?
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM codex_mcp_change WHERE config_root=?1 AND state='prepared')",
            [root],
            |r| r.get(0),
        )
        .map_err(|_| Error::StorageUnavailable)
}

fn encode(owners: &Owners) -> Result<String, Error> {
    serde_json::to_string(owners).map_err(|_| Error::StorageUnavailable)
}

/// Record a prepared intent. `central_revision` is pinned for actions that
/// copy central content (`deploy`/`set_default`); `revoke`/`clear_default`
/// pass `None` and rely on the exact owned-binding snapshot instead.
pub(super) fn prepare(
    storage: &Storage,
    change: &Change,
    central_revision: Option<i64>,
) -> Result<(), Error> {
    let mut db = lock(storage)?;
    let tx = db
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(|_| Error::StorageUnavailable)?;
    if let Some(revision) = central_revision {
        let current: Option<i64> = tx
            .query_row(
                "SELECT revision FROM provider_instance WHERE id=?1",
                [&change.provider_id],
                |r| r.get(0),
            )
            .optional()
            .map_err(|_| Error::StorageUnavailable)?;
        if current != Some(revision) {
            return Err(Error::ProviderChanged);
        }
    }
    if owners_in(&tx, &change.root, &change.provider_id)? != change.before_owner {
        return Err(Error::PlanStale);
    }
    tx.execute(
        "INSERT INTO codex_settings_change(id,config_root,provider_id,provider_key,action,state,before_digest,intended_digest,before_owner,after_owner,created_at,model_id) VALUES(?1,?2,?3,?4,?5,'prepared',?6,?7,?8,?9,?10,?11)",
        params![
            change.id,
            change.root,
            change.provider_id,
            change.provider_key,
            change.action,
            change.before_digest,
            change.intended_digest,
            encode(&change.before_owner)?,
            encode(&change.after_owner)?,
            change.created_at,
            change.model_id
        ],
    )
    .map_err(|_| Error::RecoveryRequired)?;
    tx.commit().map_err(|_| Error::OutcomeUnknown)
}

/// The intent ID is the shared operation's owner reference, so this lookup
/// survives a crash between the writer returning and finalization.
pub(super) fn operation_id(storage: &Storage, change: &Change) -> Result<Option<String>, Error> {
    let db = lock(storage)?;
    let mut statement = db
        .prepare(
            "SELECT id FROM config_operation WHERE kind='codex_settings' AND owner_ref=?1 LIMIT 2",
        )
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
            "SELECT state FROM codex_settings_change WHERE id=?1 AND config_root=?2",
            params![change.id, change.root],
            |r| r.get(0),
        )
        .map_err(|_| Error::OutcomeUnknown)?;
    if state != "prepared"
        || owners_in(&tx, &change.root, &change.provider_id)? != change.before_owner
    {
        return Err(Error::PlanStale);
    }
    if completed {
        tx.execute(
            "DELETE FROM codex_provider_binding WHERE config_root=?1 AND provider_id=?2",
            params![change.root, change.provider_id],
        )
        .map_err(|_| Error::OutcomeUnknown)?;
        tx.execute(
            "DELETE FROM codex_default_binding WHERE config_root=?1",
            [&change.root],
        )
        .map_err(|_| Error::OutcomeUnknown)?;
        if let Some(owner) = &change.after_owner.provider {
            tx.execute(
                "INSERT INTO codex_provider_binding(config_root,provider_id,provider_key,provider_revision,entry_digest,generation) VALUES(?1,?2,?3,?4,?5,?6)",
                params![
                    change.root,
                    change.provider_id,
                    owner.provider_key,
                    owner.provider_revision,
                    owner.entry_digest,
                    owner.generation
                ],
            )
            .map_err(|_| Error::OutcomeUnknown)?;
        }
        if let Some(owner) = &change.after_owner.default {
            tx.execute(
                "INSERT INTO codex_default_binding(config_root,provider_id,model_id,provider_key,entry_digest,previous,generation,writes_effort) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
                params![
                    change.root,
                    owner.provider_id,
                    owner.model_id,
                    owner.provider_key,
                    owner.entry_digest,
                    serde_json::to_string(&owner.previous).map_err(|_| Error::OutcomeUnknown)?,
                    owner.generation,
                    owner.writes_effort
                ],
            )
            .map_err(|_| Error::OutcomeUnknown)?;
        }
    }
    tx.execute(
        "UPDATE codex_settings_change SET state=?2,operation_id=?3 WHERE id=?1 AND state='prepared'",
        params![
            change.id,
            if completed { "completed" } else { "aborted" },
            operation_id
        ],
    )
    .map_err(|_| Error::OutcomeUnknown)?;
    tx.commit().map_err(|_| Error::OutcomeUnknown)
}

/// Latest change in the root, including aborted ones, so an approval cannot be
/// replayed after a round trip restores identical bytes.
pub(super) fn last_change_id(storage: &Storage, root: &str) -> Result<Option<String>, Error> {
    lock(storage)?
        .query_row(
            "SELECT id FROM codex_settings_change WHERE config_root=?1 ORDER BY rowid DESC LIMIT 1",
            [root],
            |row| row.get(0),
        )
        .optional()
        .map_err(|_| Error::StorageUnavailable)
}

/// Every provider binding in the root with the current central name/revision.
pub(super) fn bindings(
    storage: &Storage,
    root: &str,
) -> Result<Vec<(String, String, i64, ProviderOwner)>, Error> {
    let db = lock(storage)?;
    let mut statement = db
        .prepare(
            "SELECT b.provider_id,COALESCE(p.display_name,''),COALESCE(p.revision,0),b.provider_key,b.provider_revision,b.entry_digest,b.generation FROM codex_provider_binding b LEFT JOIN provider_instance p ON p.id=b.provider_id WHERE b.config_root=?1 ORDER BY b.provider_key",
        )
        .map_err(|_| Error::StorageUnavailable)?;
    statement
        .query_map([root], |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                ProviderOwner {
                    provider_key: r.get(3)?,
                    provider_revision: r.get(4)?,
                    entry_digest: r.get(5)?,
                    generation: r.get(6)?,
                },
            ))
        })
        .map_err(|_| Error::StorageUnavailable)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| Error::StorageUnavailable)
}

pub(super) fn default_owner(storage: &Storage, root: &str) -> Result<Option<DefaultOwner>, Error> {
    let db = lock(storage)?;
    default_in(&db, root)
}

pub(super) fn history(
    storage: &Storage,
    root: &str,
    provider: Option<&str>,
    after: Option<&str>,
    limit: u32,
) -> Result<HistoryPage, Error> {
    let db = lock(storage)?;
    let cursor = after
        .map(|id| {
            db.query_row(
                "SELECT created_at FROM codex_settings_change WHERE id=?1 AND config_root=?2 AND state='completed'",
                params![id, root],
                |r| r.get::<_, i64>(0),
            )
            .optional()
            .map_err(|_| Error::StorageUnavailable)?
            .ok_or(Error::InvalidRequest)
        })
        .transpose()?;
    let mut statement = db
        .prepare(
            "SELECT id,action,provider_id,provider_key,created_at,model_id FROM codex_settings_change WHERE config_root=?1 AND (?2 IS NULL OR provider_id=?2) AND state='completed' AND (?3 IS NULL OR created_at<?3 OR (created_at=?3 AND id<?4)) ORDER BY created_at DESC,id DESC LIMIT ?5",
        )
        .map_err(|_| Error::StorageUnavailable)?;
    let mut items = statement
        .query_map(params![root, provider, cursor, after, limit + 1], |r| {
            Ok(HistoryEntry {
                id: r.get(0)?,
                action: r.get(1)?,
                provider_id: r.get(2)?,
                provider_key: r.get(3)?,
                model_id: r.get(5)?,
                created_at: r.get(4)?,
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
