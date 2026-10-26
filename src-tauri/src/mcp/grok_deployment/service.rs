//! Coordinate approved Grok entry writes with durable intents and exact ownership.
//! The shared writer owns backup, snapshot, permission and atomic-rename behavior.

use super::{plan, repository, types::*};
use crate::{
    agents::DiscoveryEnvironment,
    config_apply::{
        BackupStore, OperationKind, OperationState, PlannedFile, WriteTarget,
        apply_private_files_locked,
    },
    config_plan::{MAX_TARGET_BYTES, digest_hex},
    credentials::CredentialStore,
    storage::Storage,
};
use serde_json::Value;

pub(super) fn apply(
    storage: &Storage,
    store: &dyn CredentialStore,
    environment: &DiscoveryEnvironment,
    request: &ApplyRequest,
    backups: &BackupStore,
) -> Result<Applied, Error> {
    if request.approval.len() != 64 {
        return Err(Error::InvalidRequest);
    }
    let guard = storage
        .lock_configuration()
        .map_err(|_| Error::StorageUnavailable)?;
    let mut built = plan::build(storage, environment, &request.selection, backups)?;
    if built.view.approval != request.approval {
        return Err(Error::PlanStale);
    }
    if request.selection.action == Action::Deploy {
        let refs =
            crate::mcp::repository::secret_references(storage, &request.selection.definition_id)
                .map_err(|_| Error::CredentialUnavailable)?;
        let entry = built.after_entry.as_mut().ok_or(Error::InvalidConfig)?;
        for ((kind, name), reference) in refs {
            let secret = store
                .load(&reference)
                .map_err(|_| Error::CredentialUnavailable)?
                .ok_or(Error::CredentialUnavailable)?;
            let raw = secret.expose();
            // Grok expands `${VAR}` in env/headers. Refuse unresolved expansion
            // syntax rather than writing a live lookup into the private file.
            if raw.is_empty()
                || raw.chars().any(char::is_control)
                || raw.contains("${")
                || raw.encode_utf16().count() > crate::credentials::MAX_SECRET_UTF16_UNITS
            {
                return Err(Error::CredentialUnavailable);
            }
            let field = match kind.as_str() {
                "env" => "env",
                "header" => "headers",
                _ => return Err(Error::CredentialUnavailable),
            };
            if entry.get(field).and_then(|v| v.get(&name)).is_none() {
                return Err(Error::CredentialUnavailable);
            }
            entry[field][&name] = Value::String(raw.to_owned());
        }
        plan::put_entry(&mut built.document, &built.view.server_name, entry)?;
    }
    // A missing file with a missing revoked entry needs only an ownership
    // release. Otherwise every successful change owns an independent result
    // copy, including an idempotent update with identical native bytes.
    let metadata_only = !built.view.target.exists && built.after_entry.is_none();
    let mut content = if built.view.target.exists
        && plan::parse_document(&built.original_bytes, true)?.to_string()
            == built.document.to_string()
    {
        built.original_bytes
    } else {
        built.document.to_string().into_bytes()
    };
    if content.len() as u64 > MAX_TARGET_BYTES {
        return Err(Error::TargetUnavailable);
    }
    let change_id = repository::random_id(storage)?;
    let after_owner = built
        .after_entry
        .as_ref()
        .map(|entry| {
            Ok(Ownership {
                server_name: built.view.server_name.clone(),
                definition_revision: built.deployed_revision,
                entry_digest: plan::entry_digest(entry)?,
                generation: change_id.clone(),
                enabled: entry
                    .get("enabled")
                    .and_then(Value::as_bool)
                    .ok_or(Error::InvalidConfig)?,
            })
        })
        .transpose()?;
    let change = repository::Change {
        id: change_id.clone(),
        root: plan::root_text(&built.root)?.to_string(),
        definition_id: request.selection.definition_id.clone(),
        server_name: built.view.server_name.clone(),
        definition_revision: request.selection.expected_revision,
        action: request.selection.action.code().to_string(),
        state: "prepared".into(),
        before_digest: built.view.target.content_digest.clone(),
        intended_digest: if metadata_only {
            None
        } else {
            Some(digest_hex(&content))
        },
        before_owner: built.before_owner,
        after_owner,
        operation_id: None,
        created_at: crate::shared::current_unix_millis().map_err(|_| Error::StorageUnavailable)?,
    };
    repository::prepare(storage, &change)?;
    if metadata_only {
        repository::finalize(storage, &change, true, None)?;
        return Ok(Applied {
            change_id,
            registered: false,
            reload_required: false,
        });
    }
    let result = apply_private_files_locked(
        storage,
        OperationKind::GrokMcp,
        Some(&change_id),
        &[PlannedFile {
            target: WriteTarget::from_plan(&built.view.target),
            content: std::mem::take(&mut content),
        }],
        backups,
        &guard,
    );
    match result {
        Ok(result) if result.state == OperationState::Applied => {
            let (target, _, _) = plan::read(&built.root).map_err(|_| Error::RecoveryRequired)?;
            if target.content_digest != change.intended_digest {
                return Err(Error::RecoveryRequired);
            }
            repository::finalize(storage, &change, true, Some(&result.operation_id))?;
            Ok(Applied {
                change_id,
                registered: change.after_owner.is_some(),
                reload_required: true,
            })
        }
        Ok(result) => {
            let current = crate::config_plan::read_target_bytes(&built.root.join("config.toml"))
                .map_err(|_| Error::RecoveryRequired)?
                .0;
            if current.symlink || current.content_digest != change.before_digest {
                return Err(Error::RecoveryRequired);
            }
            repository::finalize(storage, &change, false, Some(&result.operation_id))?;
            Err(Error::WriteFailed)
        }
        // A journal or post-rename failure may have changed bytes. Keep the
        // durable reference and require explicit observation after reloading.
        Err(_) => Err(Error::RecoveryRequired),
    }
}

pub(super) fn recover(
    storage: &Storage,
    environment: &DiscoveryEnvironment,
    request: &RecoveryRequest,
    backups: &BackupStore,
) -> Result<Applied, Error> {
    if !plan::valid_id(&request.change_id) || request.approval.len() != 64 {
        return Err(Error::InvalidRequest);
    }
    let guard = storage
        .lock_configuration()
        .map_err(|_| Error::StorageUnavailable)?;
    let root = plan::root(environment)?;
    let change = repository::pending(storage, plan::root_text(&root)?)?.ok_or(Error::PlanStale)?;
    let view = super::state::recovery(storage, &root, &change, backups)?;
    if change.id != request.change_id || view.approval.as_deref() != Some(request.approval.as_str())
    {
        return Err(Error::PlanStale);
    }
    let intended = view.observation == "intended";
    let operation = repository::operation_id(storage, &change)?;
    if let Some(id) = operation.as_deref() {
        crate::config_apply::resolve_mcp_observation_kind_locked(
            storage,
            id,
            &root,
            backups,
            intended,
            &guard,
            OperationKind::GrokMcp,
        )
        .map_err(|error| match error {
            crate::config_apply::WriteError::BackupUnavailable => Error::SnapshotUnavailable,
            crate::config_apply::WriteError::PlanStale => Error::PlanStale,
            _ => Error::RecoveryRequired,
        })?;
    } else if intended {
        return Err(Error::RecoveryRequired);
    }
    // Recheck the full target after snapshot/journal resolution and immediately
    // before publishing ownership. External editors are not covered by our lock.
    let target = crate::config_plan::read_target_bytes(&root.join("config.toml"))
        .map_err(|_| Error::RecoveryRequired)?
        .0;
    let expected = if intended {
        &change.intended_digest
    } else {
        &change.before_digest
    };
    if target.symlink || &target.content_digest != expected {
        return Err(Error::PlanStale);
    }
    repository::finalize(storage, &change, intended, operation.as_deref())?;
    Ok(Applied {
        change_id: change.id,
        registered: if intended {
            change.after_owner.is_some()
        } else {
            change.before_owner.is_some()
        },
        reload_required: intended,
    })
}
