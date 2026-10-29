//! Provider save orchestration across SQLite and OS credentials, including compensation.

use super::repository::{
    commit_provider_delete, commit_provider_update, generate_provider_id, get_provider,
    insert_provider_with_reference, read_selected_model_count, read_status, save_reference,
};
use super::types::*;
use super::validation::{parse_kind, validate_settings};
use crate::credentials::{
    CommitFailure, CredentialStore, DeleteError, ReplaceError, SaveNewError, Secret,
    delete_then_commit, replace_then_commit, save_new_then_commit, validate_secret,
};
use crate::references::{self, ReferenceClearError, ReferenceReadError};
use crate::storage::Storage;
use rusqlite::OptionalExtension;

/// Validate a new instance and its API key, save both, and return the instance.
///
/// The key goes to the OS credential store and the instance to SQLite. The two
/// stores share no transaction, so the steps are ordered to never leave half a
/// provider behind:
/// 1. Validate every field, the key last, before touching either store.
/// 2. Generate the ID with SQLite's `randomblob` (16 random bytes as lowercase
///    hex). This runs outside the insert transaction, and the connection is
///    unlocked again before step 3, because the credential store may show a
///    system prompt and wait for the user.
/// 3. Save the key under `provider-<id>`. If that fails, nothing was saved.
/// 4. In one transaction, insert the instance and its `provider_credential` row.
/// 5. If an `INSERT` in step 4 fails, nothing was committed: delete the key again
///    (`save_new_then_commit`) and report `WriteFailed`. If even that delete
///    fails, report `CreateOutcomeUnknown`: the leftover entry has no reference,
///    so the app never reads it, but the user must check the list.
/// 6. If `COMMIT` itself fails, the provider may have been saved anyway, so the
///    key is kept (deleting it could leave a saved provider without a key) and
///    the result is `CreateOutcomeUnknown`.
///
/// Random IDs, unlike reused integer IDs, cannot point at a credential left
/// behind by an older or restored database, and the primary key rejects the
/// (practically impossible) duplicate instead of overwriting a row.
///
/// Extensions are validated but not stored: every allowlist is empty today, so a
/// validated map is always empty. Storage arrives with the first supported key.
///
/// # Errors
/// Validation codes (including `SecretInvalid`), `StorageUnavailable`,
/// credential-store codes (nothing was saved), `WriteFailed` (nothing was saved),
/// or `CreateOutcomeUnknown`.
pub fn create_provider(
    storage: &Storage,
    store: &dyn CredentialStore,
    request: CreateProviderRequest,
    now_ms: i64,
) -> Result<ProviderRecord, ProviderError> {
    let kind = parse_kind(&request.kind)?;
    let settings = validate_settings(
        kind,
        &request.display_name,
        &request.base_url,
        &request.protocol,
        &request.extensions,
    )?;
    let secret = validate_secret(request.secret)?;
    let id = generate_provider_id(storage)?;
    let reference = id.credential_reference();
    let saved = save_new_then_commit(store, &reference, &secret, || {
        insert_provider_with_reference(storage, &id, kind, &settings, &reference, now_ms)
    });
    match saved {
        Ok(()) => {
            let connection = storage
                .lock()
                .map_err(|_| ProviderError::StorageUnavailable)?;
            Ok(ProviderRecord {
                id: id.clone(),
                kind,
                display_name: settings.display_name,
                base_url: settings.base_url,
                protocol: settings.protocol,
                extensions: settings.extensions,
                revision: 1,
                created_at_ms: now_ms,
                updated_at_ms: now_ms,
                selected_model_count: read_selected_model_count(&connection, &id)?,
            })
        }
        Err(SaveNewError::Credential(error)) => Err(error.into()),
        // The key was deleted again, so the SQLite error describes the whole outcome.
        Err(SaveNewError::CommitFailed {
            source,
            cleaned_up: true,
        }) => Err(source),
        Err(SaveNewError::CommitFailed {
            cleaned_up: false, ..
        })
        | Err(SaveNewError::CommitUncertain { .. }) => Err(ProviderError::CreateOutcomeUnknown),
    }
}

/// Edit one instance identified by `request.id`, keeping its ID, kind, and
/// creation time, and return the committed record.
///
/// The edit succeeds only when the stored `revision` still equals
/// `request.expected_revision` (optimistic concurrency): a form loaded before
/// another window's save cannot overwrite that newer save. The check and the
/// write run in one immediate transaction, so another app instance cannot slip
/// a change in between.
///
/// `updated_at` uses `max(now, created_at)`, so a clock that moved backwards
/// cannot make an edit look older than the creation.
///
/// # Errors
/// `InvalidRequest` for a malformed ID or revision, `NotFound`,
/// `RevisionConflict`, `InvalidStoredProvider`, validation codes,
/// `StorageUnavailable`, `ReadFailed`, or `WriteFailed` before commit. A failed
/// commit returns `OperationFailed`, because the outcome cannot be confirmed.
pub fn update_provider(
    storage: &Storage,
    request: &UpdateProviderRequest,
    now_ms: i64,
) -> Result<ProviderRecord, ProviderError> {
    commit_provider_update(storage, request, None, now_ms).map_err(|error| match error {
        CommitFailure::NotCommitted(source) => source,
        CommitFailure::Uncertain(_) => ProviderError::OperationFailed,
    })
}

/// Save settings and optionally replace an API key through one coordinated operation.
///
/// `None` preserves the key and never accesses the credential store. A supplied key
/// is validated along with the settings before any OS prompt. The old key is kept
/// only in memory while settings and its reference commit in one SQLite transaction.
/// A concurrent settings change is rechecked after the OS call, and a definite
/// database failure restores the previous key. No database lock spans an OS prompt.
/// Callers must serialize credential writes within the app process.
///
/// # Errors
/// Returns validation, revision, storage or credential-store codes when nothing
/// changed (or was restored). Failed rollback or uncertain commit returns
/// `OperationFailed`: callers must reload and must not claim success or rollback.
/// As with other credential writes, separate app processes are not serialized.
pub fn update_provider_with_secret(
    storage: &Storage,
    store: &dyn CredentialStore,
    request: &UpdateProviderRequest,
    secret: Option<Secret>,
    now_ms: i64,
) -> Result<ProviderRecord, ProviderError> {
    let Some(secret) = secret else {
        return update_provider(storage, request, now_ms);
    };
    let record = get_provider(storage, &request.id)?;
    if request.expected_revision < 1 {
        return Err(ProviderError::InvalidRequest);
    }
    if request.expected_revision != record.revision {
        return Err(ProviderError::RevisionConflict);
    }
    validate_settings(
        record.kind,
        &request.display_name,
        &request.base_url,
        &request.protocol,
        &request.extensions,
    )?;
    let secret = validate_secret(secret)?;
    let reference = record.id.credential_reference();
    let result = replace_then_commit(store, &reference, &secret, || {
        commit_provider_update(storage, request, Some(&reference), now_ms)
    });
    match result {
        Ok(record) => Ok(record),
        Err(ReplaceError::Credential(error)) => Err(error.into()),
        Err(ReplaceError::CommitFailed {
            source,
            restored: true,
        }) => Err(source),
        Err(ReplaceError::CommitFailed {
            restored: false, ..
        })
        | Err(ReplaceError::CommitUncertain { .. }) => Err(ProviderError::OperationFailed),
    }
}

/// Remove one instance together with its saved models and its API key.
///
/// Steps:
/// 1. Check the ID and revision and confirm in SQLite that the provider exists.
///    An unknown or stale provider never reaches the credential store.
/// 2. `delete_then_commit` reads the key as an in-memory backup and removes it.
/// 3. Its commit step deletes the `provider_instance` row; the foreign keys
///    cascade to the saved models, the fetch record and the credential reference.
/// 4. If the transaction fails before `COMMIT`, the key is written back, so the
///    provider stays usable and the SQLite error describes the whole outcome.
/// 5. If `COMMIT` itself fails, the key is restored as well: either the provider
///    still exists and needs it, or the entry is unreferenced and never read. The
///    result is `DeleteOutcomeUnknown`, and the caller must reload the list.
///
/// Agent configuration files are not touched; only vibemate's own storage changes.
///
/// # Errors
/// `InvalidRequest`, `NotFound`, `RevisionConflict`, `StorageUnavailable`,
/// `ReadFailed`, `WriteFailed` (nothing was deleted; the key is back),
/// credential-store codes (nothing was deleted), or `DeleteOutcomeUnknown`.
pub fn delete_provider(
    storage: &Storage,
    store: &dyn CredentialStore,
    request: DeleteProviderRequest,
) -> Result<DeletedProvider, ProviderError> {
    let id = ProviderId::parse(&request.id).ok_or(ProviderError::InvalidRequest)?;
    if request.expected_revision < 1 {
        return Err(ProviderError::InvalidRequest);
    }
    {
        // Release the connection before the credential store, which may wait for
        // a system prompt.
        let connection = storage
            .lock()
            .map_err(|_| ProviderError::StorageUnavailable)?;
        let revision: Option<i64> = connection
            .query_row(
                "SELECT revision FROM provider_instance WHERE id = ?1",
                [id.as_str()],
                |row| row.get(0),
            )
            .optional()
            .map_err(|_| ProviderError::ReadFailed)?;
        let revision = revision.ok_or(ProviderError::NotFound)?;
        if revision != request.expected_revision {
            return Err(ProviderError::RevisionConflict);
        }
    }
    if references::provider_has_bindings(storage, id.as_str()).map_err(map_reference_read)? {
        return Err(ProviderError::ProviderInUse);
    }
    let reference = id.credential_reference();
    let deleted = delete_then_commit(store, &reference, || {
        commit_provider_delete(storage, &request)
    });
    match deleted {
        Ok(value) => Ok(value),
        Err(DeleteError::Credential(error)) => Err(error.into()),
        // The key is back, so the SQLite error describes the whole outcome.
        Err(DeleteError::CommitFailed {
            source,
            restored: true,
        }) => Err(source),
        Err(DeleteError::CommitFailed {
            restored: false, ..
        })
        | Err(DeleteError::CommitUncertain { .. }) => Err(ProviderError::DeleteOutcomeUnknown),
    }
}

/// List the Pi/Grok ownership rows and Claude choices a Provider deletion affects.
///
/// # Errors
/// `InvalidRequest` for a malformed ID, `NotFound`, `StorageUnavailable`, or
/// `ReadFailed`.
pub fn provider_deletion_impact(
    storage: &Storage,
    provider_id: &str,
) -> Result<ProviderDeletionImpact, ProviderError> {
    let id = ProviderId::parse(provider_id).ok_or(ProviderError::InvalidRequest)?;
    get_provider(storage, id.as_str())?;
    let references =
        references::provider_references(storage, id.as_str()).map_err(map_reference_read)?;
    let remembered_preferences = references::remembered_preferences(storage, id.as_str(), None)
        .map_err(map_reference_read)?;
    let codex_references =
        references::codex_references(storage, id.as_str(), None).map_err(map_reference_read)?;
    // The count repeats every blocking table as a safety net.
    let bound =
        references::provider_has_bindings(storage, id.as_str()).map_err(map_reference_read)?;
    Ok(ProviderDeletionImpact {
        provider_id: id.as_str().to_owned(),
        can_delete: references.is_empty() && codex_references.is_empty() && !bound,
        references,
        codex_references,
        remembered_preferences,
    })
}

/// Remove one stale Pi Provider binding or Grok alias binding.
///
/// The Agent file must already be free of the named entry. An active or
/// unreadable binding is refused, so this never hides a live reference.
///
/// # Errors
/// `InvalidRequest` for malformed fields, `NotFound` when the binding is gone,
/// `ProviderInUse` when the Agent file still names it or cannot be read, and
/// storage failures.
pub fn clear_provider_reference(
    storage: &Storage,
    request: ClearProviderReferenceRequest,
) -> Result<(), ProviderError> {
    let provider_id =
        ProviderId::parse(&request.provider_id).ok_or(ProviderError::InvalidRequest)?;
    if !matches!(request.agent_id.as_str(), "pi" | "grokBuild")
        || !valid_reference_text(&request.config_root, 4096)
        || !valid_reference_text(&request.label, 4096)
    {
        return Err(ProviderError::InvalidRequest);
    }
    references::clear_provider_reference(
        storage,
        provider_id.as_str(),
        &request.agent_id,
        &request.config_root,
        &request.label,
    )
    .map_err(map_reference_clear)
}

fn valid_reference_text(value: &str, limit: usize) -> bool {
    !value.is_empty() && value.len() <= limit && !value.chars().any(char::is_control)
}

fn map_reference_read(error: ReferenceReadError) -> ProviderError {
    match error {
        ReferenceReadError::StorageUnavailable => ProviderError::StorageUnavailable,
        ReferenceReadError::ReadFailed => ProviderError::ReadFailed,
    }
}

fn map_reference_clear(error: ReferenceClearError) -> ProviderError {
    match error {
        ReferenceClearError::StorageUnavailable => ProviderError::StorageUnavailable,
        ReferenceClearError::ReadFailed => ProviderError::ReadFailed,
        ReferenceClearError::WriteFailed => ProviderError::WriteFailed,
        ReferenceClearError::InvalidRequest => ProviderError::InvalidRequest,
        ReferenceClearError::NotFound => ProviderError::NotFound,
        ReferenceClearError::InUse => ProviderError::ProviderInUse,
    }
}

/// Read a provider's key status from SQLite only.
///
/// # Errors
/// `InvalidRequest` for a malformed ID, `NotFound`, `StorageUnavailable`, or
/// `ReadFailed`.
pub fn get_secret_status(
    storage: &Storage,
    provider_id: &str,
) -> Result<ProviderSecretStatus, ProviderError> {
    let id = ProviderId::parse(provider_id).ok_or(ProviderError::InvalidRequest)?;
    let connection = storage
        .lock()
        .map_err(|_| ProviderError::StorageUnavailable)?;
    read_status(&connection, &id)?.ok_or(ProviderError::NotFound)
}

/// Replace (or, for a missing key, set) a provider's key and return the new status.
///
/// Steps:
/// 1. Check the ID and the key, then confirm in SQLite that the provider exists.
///    An unknown provider never reaches the credential store.
/// 2. `replace_then_commit` reads the old key as an in-memory backup and writes
///    the new one under `provider-<id>`.
/// 3. Its commit step inserts or updates the reference row in one transaction.
/// 4. If the transaction fails before `COMMIT`, the old key is written back (or the
///    new one deleted when there was none). If restoring fails too, the result is
///    `SecretOutcomeUnknown`.
/// 5. If `COMMIT` itself fails, the reference row may have been saved anyway, so
///    the new key is kept and the result is `SecretOutcomeUnknown`. Restoring here
///    could turn a first key into a `Set` row with no key behind it.
///
/// # Errors
/// `InvalidRequest`, `SecretInvalid`, `NotFound`, `StorageUnavailable`,
/// `ReadFailed`, credential-store codes (nothing changed), `WriteFailed` (the
/// previous key was restored), or `SecretOutcomeUnknown`.
pub fn replace_provider_secret(
    storage: &Storage,
    store: &dyn CredentialStore,
    request: ReplaceProviderSecretRequest,
    now_ms: i64,
) -> Result<ProviderSecretStatus, ProviderError> {
    let id = ProviderId::parse(&request.provider_id).ok_or(ProviderError::InvalidRequest)?;
    let secret = validate_secret(request.secret)?;
    {
        // Release the connection before the credential store, which may wait for
        // a system prompt.
        let connection = storage
            .lock()
            .map_err(|_| ProviderError::StorageUnavailable)?;
        read_status(&connection, &id)?.ok_or(ProviderError::NotFound)?;
    }
    let reference = id.credential_reference();
    let replaced = replace_then_commit(store, &reference, &secret, || {
        save_reference(storage, &id, &reference, now_ms)
    });
    match replaced {
        Ok(status) => Ok(status),
        Err(ReplaceError::Credential(error)) => Err(error.into()),
        // The previous key is back, so the SQLite error describes the whole outcome.
        Err(ReplaceError::CommitFailed {
            source,
            restored: true,
        }) => Err(source),
        Err(ReplaceError::CommitFailed {
            restored: false, ..
        })
        | Err(ReplaceError::CommitUncertain { .. }) => Err(ProviderError::SecretOutcomeUnknown),
    }
}
