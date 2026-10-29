//! Provider SQLite queries and transactions. Credential storage is handled by service.

use super::types::*;
use super::validation::validate_settings;
use crate::credentials::CommitFailure;
use crate::storage::Storage;
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use std::collections::BTreeMap;

/// Ask SQLite for a new random ID, holding the connection only for this query.
pub(super) fn generate_provider_id(storage: &Storage) -> Result<ProviderId, ProviderError> {
    let connection = storage
        .lock()
        .map_err(|_| ProviderError::StorageUnavailable)?;
    let generated: String = connection
        .query_row("SELECT lower(hex(randomblob(16)))", [], |row| row.get(0))
        .map_err(|_| ProviderError::WriteFailed)?;
    ProviderId::parse(&generated).ok_or(ProviderError::WriteFailed)
}

/// Insert a new instance and its credential reference in one transaction, so
/// either both rows exist afterwards or neither does.
///
/// Failures before `COMMIT` are `NotCommitted` (nothing was saved). A failed
/// `COMMIT` is `Uncertain`, because SQLite may report an I/O error after the
/// change already reached the file.
pub(super) fn insert_provider_with_reference(
    storage: &Storage,
    id: &ProviderId,
    kind: ProviderKind,
    settings: &ProviderSettings,
    reference: &str,
    now_ms: i64,
) -> Result<(), CommitFailure<ProviderError>> {
    let not_committed = |_| CommitFailure::NotCommitted(ProviderError::WriteFailed);
    let mut connection = storage
        .lock()
        .map_err(|_| CommitFailure::NotCommitted(ProviderError::StorageUnavailable))?;
    // Dropping a transaction without `commit` rolls it back, so every early `?`
    // below leaves the database unchanged.
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(not_committed)?;
    transaction
        .execute(
            "INSERT INTO provider_instance
                 (id, kind, display_name, base_url, protocol, revision, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, 1, ?6, ?6)",
            params![
                id.as_str(),
                kind.as_str(),
                settings.display_name,
                settings.base_url,
                settings.protocol.as_str(),
                now_ms
            ],
        )
        .map_err(not_committed)?;
    transaction
        .execute(
            "INSERT INTO provider_credential (provider_id, credential_ref, updated_at)
             VALUES (?1, ?2, ?3)",
            params![id.as_str(), reference, now_ms],
        )
        .map_err(not_committed)?;
    transaction
        .commit()
        .map_err(|_| CommitFailure::Uncertain(ProviderError::WriteFailed))
}

pub(super) fn commit_provider_update(
    storage: &Storage,
    request: &UpdateProviderRequest,
    reference: Option<&str>,
    now_ms: i64,
) -> Result<ProviderRecord, CommitFailure<ProviderError>> {
    let mut connection = storage
        .lock()
        .map_err(|_| CommitFailure::NotCommitted(ProviderError::StorageUnavailable))?;
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|_| CommitFailure::NotCommitted(ProviderError::WriteFailed))?;
    let record = update_in_transaction(&transaction, request, reference, now_ms)
        .map_err(CommitFailure::NotCommitted)?;
    transaction
        .commit()
        .map_err(|_| CommitFailure::Uncertain(ProviderError::WriteFailed))?;
    enrich_selected_model_count(&connection, record).map_err(CommitFailure::NotCommitted)
}

pub(super) fn commit_provider_delete(
    storage: &Storage,
    request: &DeleteProviderRequest,
) -> Result<DeletedProvider, CommitFailure<ProviderError>> {
    let mut connection = storage
        .lock()
        .map_err(|_| CommitFailure::NotCommitted(ProviderError::StorageUnavailable))?;
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|_| CommitFailure::NotCommitted(ProviderError::WriteFailed))?;
    let deleted =
        delete_in_transaction(&transaction, request).map_err(CommitFailure::NotCommitted)?;
    transaction
        .commit()
        .map_err(|_| CommitFailure::Uncertain(ProviderError::WriteFailed))?;
    Ok(deleted)
}

/// Delete one instance inside the caller's transaction. The credential entry
/// itself is removed by the service before this commit step runs.
///
/// The stored revision is rechecked here, so a stale confirmation cannot delete a
/// configuration that changed after the dialog was opened.
fn delete_in_transaction(
    transaction: &rusqlite::Transaction<'_>,
    request: &DeleteProviderRequest,
) -> Result<DeletedProvider, ProviderError> {
    let id = ProviderId::parse(&request.id).ok_or(ProviderError::InvalidRequest)?;
    if request.expected_revision < 1 {
        return Err(ProviderError::InvalidRequest);
    }
    let stored: Option<(String, i64)> = transaction
        .query_row(
            "SELECT display_name, revision FROM provider_instance WHERE id = ?1",
            [id.as_str()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(|_| ProviderError::ReadFailed)?;
    let (display_name, revision) = stored.ok_or(ProviderError::NotFound)?;
    if revision != request.expected_revision {
        return Err(ProviderError::RevisionConflict);
    }
    let saved_models: i64 = transaction
        .query_row(
            "SELECT COUNT(*) FROM provider_model WHERE provider_id = ?1",
            [id.as_str()],
            |row| row.get(0),
        )
        .map_err(|_| ProviderError::ReadFailed)?;
    // Binding rows have no foreign key to provider_instance, so the cascade
    // below would leave them pointing at a removed Provider. Check them inside
    // this transaction as well as in the service's early guard.
    let bindings: i64 = transaction
        .query_row(
            "SELECT
                (SELECT COUNT(*) FROM pi_provider_binding WHERE provider_id = ?1) +
                (SELECT COUNT(*) FROM grok_model_binding WHERE provider_id = ?1) +
                (SELECT COUNT(*) FROM codex_provider_binding WHERE provider_id = ?1) +
                (SELECT COUNT(*) FROM codex_default_binding WHERE provider_id = ?1) +
                (SELECT COUNT(*) FROM codex_settings_change
                 WHERE provider_id = ?1 AND state = 'prepared')",
            [id.as_str()],
            |row| row.get(0),
        )
        .map_err(|_| ProviderError::ReadFailed)?;
    if bindings > 0 {
        return Err(ProviderError::ProviderInUse);
    }
    // The foreign keys cascade to provider_model, provider_model_fetch and
    // provider_credential, so this one row removes everything the provider owns.
    let removed = transaction
        .execute(
            "DELETE FROM provider_instance WHERE id = ?1 AND revision = ?2",
            params![id.as_str(), request.expected_revision],
        )
        .map_err(|_| ProviderError::WriteFailed)?;
    if removed != 1 {
        return Err(ProviderError::RevisionConflict);
    }
    Ok(DeletedProvider {
        id,
        display_name,
        selected_model_count: u32::try_from(saved_models).map_err(|_| ProviderError::ReadFailed)?,
    })
}

fn update_in_transaction(
    transaction: &rusqlite::Transaction<'_>,
    request: &UpdateProviderRequest,
    reference: Option<&str>,
    now_ms: i64,
) -> Result<ProviderRecord, ProviderError> {
    let id = ProviderId::parse(&request.id).ok_or(ProviderError::InvalidRequest)?;
    if request.expected_revision < 1 {
        return Err(ProviderError::InvalidRequest);
    }
    let stored: Option<(String, i64)> = transaction
        .query_row(
            "SELECT kind, revision FROM provider_instance WHERE id = ?1",
            [id.as_str()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(|_| ProviderError::ReadFailed)?;
    let (stored_kind, stored_revision) = stored.ok_or(ProviderError::NotFound)?;
    if stored_revision != request.expected_revision {
        return Err(ProviderError::RevisionConflict);
    }
    // The protocol is checked against the stored kind, never a kind from the form.
    let kind =
        ProviderKind::from_value(&stored_kind).ok_or(ProviderError::InvalidStoredProvider)?;
    let settings = validate_settings(
        kind,
        &request.display_name,
        &request.base_url,
        &request.protocol,
        &request.extensions,
    )?;
    let changed = transaction
        .execute(
            "UPDATE provider_instance
             SET display_name = ?1, base_url = ?2, protocol = ?3,
                 revision = revision + 1, updated_at = max(?4, created_at)
             WHERE id = ?5 AND revision = ?6",
            params![
                settings.display_name,
                settings.base_url,
                settings.protocol.as_str(),
                now_ms,
                id.as_str(),
                request.expected_revision
            ],
        )
        .map_err(|_| ProviderError::WriteFailed)?;
    // The immediate transaction already excludes concurrent writers. Still check
    // the affected row count: a trigger or a future query change could suppress
    // the UPDATE. Never report stale data as a successful save. Returning early
    // drops the transaction and rolls back its changes.
    if changed != 1 {
        return Err(ProviderError::RevisionConflict);
    }
    if let Some(reference) = reference {
        // A legacy instance may have no key reference yet. Keep this upsert in
        // the same transaction as its settings, so either both rows commit or
        // both roll back before the credential helper restores the old key.
        transaction
            .execute(
                "INSERT INTO provider_credential (provider_id, credential_ref, updated_at)
                 VALUES (?1, ?2, ?3)
                 ON CONFLICT (provider_id) DO UPDATE SET updated_at = excluded.updated_at",
                params![id.as_str(), reference, now_ms],
            )
            .map_err(|_| ProviderError::WriteFailed)?;
    }
    // Read before commit so a malformed response cannot leave a partial save.
    read_record(transaction, &id)
        .map_err(|_| ProviderError::WriteFailed)?
        .ok_or(ProviderError::WriteFailed)
}

/// Read one instance by ID.
///
/// # Errors
/// `InvalidRequest` for a malformed ID, `NotFound`, `StorageUnavailable`,
/// `ReadFailed`, or `InvalidStoredProvider`.
pub fn get_provider(storage: &Storage, id: &str) -> Result<ProviderRecord, ProviderError> {
    let id = ProviderId::parse(id).ok_or(ProviderError::InvalidRequest)?;
    let connection = storage
        .lock()
        .map_err(|_| ProviderError::StorageUnavailable)?;
    let record = read_record(&connection, &id)?.ok_or(ProviderError::NotFound)?;
    enrich_selected_model_count(&connection, record)
}

/// Read one bounded page ordered by `created_at` ascending, then `id` ascending.
///
/// The ID breaks ties between instances created in the same millisecond, so the
/// order is total and stable. Editing never moves an instance, because edits do
/// not change `created_at`. One extra row is fetched to learn whether another
/// page exists. A row this build cannot read fails the whole page rather than
/// being skipped silently. Every page also reports the stored total, which may
/// be larger than the rows on the page.
///
/// # Errors
/// `InvalidRequest` for a limit outside 1..=`MAX_PAGE_SIZE` or a malformed
/// cursor, `StorageUnavailable`, `ReadFailed`, or `InvalidStoredProvider`.
pub fn list_providers(
    storage: &Storage,
    request: &ListProvidersRequest,
) -> Result<ProviderPage, ProviderError> {
    if request.limit == 0 || request.limit > MAX_PAGE_SIZE {
        return Err(ProviderError::InvalidRequest);
    }
    let after = match &request.after {
        Some(cursor) => Some(parse_cursor(cursor)?),
        None => None,
    };
    let (after_created_at, after_id) = match &after {
        Some((created_at, id)) => (Some(*created_at), Some(id.as_str())),
        None => (None, None),
    };
    let connection = storage
        .lock()
        .map_err(|_| ProviderError::StorageUnavailable)?;
    // The count covers the whole table, so a caller can state the stored total
    // without reading every page.
    let total: i64 = connection
        .query_row("SELECT COUNT(*) FROM provider_instance", [], |row| {
            row.get(0)
        })
        .map_err(|_| ProviderError::ReadFailed)?;
    // `(a, b) > (x, y)` is SQLite's row-value comparison: a > x, or a = x and b > y.
    // With no cursor, `?1 IS NULL` is true and the comparison is not needed.
    let mut statement = connection
        .prepare(&format!(
            "SELECT {RECORD_COLUMNS},
                    COALESCE(
                        (SELECT COUNT(*) FROM provider_model m
                         WHERE m.provider_id = provider_instance.id AND m.selected = 1),
                        0
                    ) AS selected_model_count
             FROM provider_instance
             WHERE ?1 IS NULL OR (created_at, id) > (?1, ?2)
             ORDER BY created_at, id
             LIMIT ?3"
        ))
        .map_err(|_| ProviderError::ReadFailed)?;
    let rows = statement
        .query_map(
            params![after_created_at, after_id, i64::from(request.limit) + 1],
            ListedRow::read,
        )
        .map_err(|_| ProviderError::ReadFailed)?;
    let mut items = Vec::new();
    for row in rows {
        let stored = row.map_err(|_| ProviderError::ReadFailed)?;
        items.push(stored.into_record()?);
    }
    let page_size = usize::try_from(request.limit).map_err(|_| ProviderError::InvalidRequest)?;
    let next_cursor = if items.len() > page_size {
        items.truncate(page_size);
        items.last().map(cursor_after)
    } else {
        None
    };
    Ok(ProviderPage {
        items,
        next_cursor,
        total,
    })
}

/// Columns read for a record, in the order `StoredRow::read` expects.
const RECORD_COLUMNS: &str =
    "id, kind, display_name, base_url, protocol, revision, created_at, updated_at";

/// Read one record, returning `None` when the ID does not exist.
fn read_record(
    connection: &Connection,
    id: &ProviderId,
) -> Result<Option<ProviderRecord>, ProviderError> {
    let stored = connection
        .query_row(
            &format!("SELECT {RECORD_COLUMNS} FROM provider_instance WHERE id = ?1"),
            [id.as_str()],
            StoredRow::read,
        )
        .optional()
        .map_err(|_| ProviderError::ReadFailed)?;
    stored.map(StoredRow::into_record).transpose()
}

/// Raw column values before they are checked against this build's types.
struct StoredRow {
    id: String,
    kind: String,
    display_name: String,
    base_url: String,
    protocol: String,
    revision: i64,
    created_at: i64,
    updated_at: i64,
}

impl StoredRow {
    /// Copy columns from a row selected with `RECORD_COLUMNS`.
    fn read(row: &rusqlite::Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get(0)?,
            kind: row.get(1)?,
            display_name: row.get(2)?,
            base_url: row.get(3)?,
            protocol: row.get(4)?,
            revision: row.get(5)?,
            created_at: row.get(6)?,
            updated_at: row.get(7)?,
        })
    }

    /// Convert to a typed record. Unknown values are reported, never replaced.
    fn into_record(self) -> Result<ProviderRecord, ProviderError> {
        self.into_record_with_selected_count(0)
    }

    fn into_record_with_selected_count(
        self,
        selected_model_count: u32,
    ) -> Result<ProviderRecord, ProviderError> {
        let invalid = ProviderError::InvalidStoredProvider;
        Ok(ProviderRecord {
            id: ProviderId::parse(&self.id).ok_or(invalid)?,
            kind: ProviderKind::from_value(&self.kind).ok_or(invalid)?,
            display_name: self.display_name,
            base_url: self.base_url,
            protocol: ProviderProtocol::from_value(&self.protocol).ok_or(invalid)?,
            extensions: BTreeMap::new(),
            revision: self.revision,
            created_at_ms: self.created_at,
            updated_at_ms: self.updated_at,
            selected_model_count,
        })
    }
}

/// One list row including the selected-model count from the list query.
struct ListedRow {
    row: StoredRow,
    selected_model_count: i64,
}

impl ListedRow {
    fn read(row: &rusqlite::Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            row: StoredRow::read(row)?,
            selected_model_count: row.get(8)?,
        })
    }

    fn into_record(self) -> Result<ProviderRecord, ProviderError> {
        let count =
            u32::try_from(self.selected_model_count).map_err(|_| ProviderError::ReadFailed)?;
        self.row.into_record_with_selected_count(count)
    }
}

pub(super) fn read_selected_model_count(
    connection: &Connection,
    id: &ProviderId,
) -> Result<u32, ProviderError> {
    let count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM provider_model WHERE provider_id = ?1 AND selected = 1",
            [id.as_str()],
            |row| row.get(0),
        )
        .map_err(|_| ProviderError::ReadFailed)?;
    u32::try_from(count).map_err(|_| ProviderError::ReadFailed)
}

fn enrich_selected_model_count(
    connection: &Connection,
    mut record: ProviderRecord,
) -> Result<ProviderRecord, ProviderError> {
    record.selected_model_count = read_selected_model_count(connection, &record.id)?;
    Ok(record)
}

/// Build the cursor that continues after `record`: `"<created_at_ms>.<id>"`.
///
/// The frontend treats the cursor as opaque text and only sends it back.
fn cursor_after(record: &ProviderRecord) -> String {
    format!("{}.{}", record.created_at_ms, record.id.as_str())
}

/// Split a cursor made by `cursor_after` back into its sort key.
fn parse_cursor(cursor: &str) -> Result<(i64, ProviderId), ProviderError> {
    let (created_at, id) = cursor
        .split_once('.')
        .ok_or(ProviderError::InvalidRequest)?;
    // Only plain ASCII digits: `parse` alone would also accept "+5" and "-5",
    // and `cursor_after` never produces a sign. Too many digits overflow `parse`.
    if created_at.is_empty() || !created_at.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(ProviderError::InvalidRequest);
    }
    let created_at = created_at
        .parse::<i64>()
        .map_err(|_| ProviderError::InvalidRequest)?;
    let id = ProviderId::parse(id).ok_or(ProviderError::InvalidRequest)?;
    Ok((created_at, id))
}

/// Read the key status, or `None` when no provider has this ID.
///
/// The LEFT JOIN keeps a provider without a reference row, which becomes `Missing`.
pub(super) fn read_status(
    connection: &Connection,
    id: &ProviderId,
) -> Result<Option<ProviderSecretStatus>, ProviderError> {
    let updated_at: Option<Option<i64>> = connection
        .query_row(
            "SELECT credential.updated_at
             FROM provider_instance AS provider
             LEFT JOIN provider_credential AS credential
                 ON credential.provider_id = provider.id
             WHERE provider.id = ?1",
            [id.as_str()],
            |row| row.get(0),
        )
        .optional()
        .map_err(|_| ProviderError::ReadFailed)?;
    Ok(updated_at.map(|updated_at| ProviderSecretStatus {
        provider_id: id.clone(),
        state: if updated_at.is_some() {
            ProviderSecretState::Set
        } else {
            ProviderSecretState::Missing
        },
        updated_at_ms: updated_at,
    }))
}

/// Insert or update the reference row for `id` in one transaction.
///
/// The provider is checked again inside the transaction: it could have been
/// removed while the credential store was busy. Only `provider_credential`
/// changes, so the provider's `revision` and `updated_at` stay as they were.
///
/// Failures before `COMMIT` are `NotCommitted`; a failed `COMMIT` is `Uncertain`.
pub(crate) fn save_reference(
    storage: &Storage,
    id: &ProviderId,
    reference: &str,
    now_ms: i64,
) -> Result<ProviderSecretStatus, CommitFailure<ProviderError>> {
    let not_committed = CommitFailure::NotCommitted;
    let mut connection = storage
        .lock()
        .map_err(|_| not_committed(ProviderError::StorageUnavailable))?;
    // Dropping a transaction without `commit` rolls it back.
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|_| not_committed(ProviderError::WriteFailed))?;
    let exists = transaction
        .query_row(
            "SELECT 1 FROM provider_instance WHERE id = ?1",
            [id.as_str()],
            |_| Ok(()),
        )
        .optional()
        .map_err(|_| not_committed(ProviderError::ReadFailed))?;
    if exists.is_none() {
        return Err(not_committed(ProviderError::NotFound));
    }
    transaction
        .execute(
            "INSERT INTO provider_credential (provider_id, credential_ref, updated_at)
             VALUES (?1, ?2, ?3)
             ON CONFLICT (provider_id) DO UPDATE SET updated_at = excluded.updated_at",
            params![id.as_str(), reference, now_ms],
        )
        .map_err(|_| not_committed(ProviderError::WriteFailed))?;
    transaction
        .commit()
        .map_err(|_| CommitFailure::Uncertain(ProviderError::WriteFailed))?;
    Ok(ProviderSecretStatus {
        provider_id: id.clone(),
        state: ProviderSecretState::Set,
        updated_at_ms: Some(now_ms),
    })
}
