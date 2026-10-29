//! Coordinate approved Codex settings writes with durable intents and exact
//! ownership. The shared writer owns backups, 0600 permissions, journal
//! snapshots and the atomic rename.

use super::{plan, repository, types::*};
use crate::{
    agents::DiscoveryEnvironment,
    config_apply::{
        BackupStore, OperationKind, OperationState, PlannedFile, WriteTarget,
        apply_private_files_locked,
    },
    config_plan::{MAX_TARGET_BYTES, digest_hex, read_target_bytes},
    credentials::CredentialStore,
    storage::Storage,
};

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
    let mut built = plan::build(storage, environment, &request.selection)?;
    if built.view.approval != request.approval {
        return Err(Error::PlanStale);
    }
    let change_id = repository::random_id(storage)?;
    if request.selection.action == Action::Deploy {
        // The only credential read in this adapter, after the user approved
        // the exact masked plan. The key is written as plaintext by design.
        let provider = built.provider.as_ref().ok_or(Error::InvalidConfig)?;
        let secret = store
            .load(&provider.id.credential_reference())
            .map_err(|_| Error::CredentialUnavailable)?
            .ok_or(Error::CredentialUnavailable)?;
        let raw = secret.expose();
        if raw.is_empty()
            || raw.chars().any(char::is_control)
            || raw.encode_utf16().count() > crate::credentials::MAX_SECRET_UTF16_UNITS
        {
            return Err(Error::CredentialUnavailable);
        }
        plan::fill_token(&mut built.document, &built.view.provider_key, raw)?;
        let item = plan::provider_item(&built.document, &built.view.provider_key)
            .ok_or(Error::InvalidConfig)?;
        let entry_digest = plan::digest(&plan::item_json(item))?;
        let owner = built.after.provider.as_mut().ok_or(Error::InvalidConfig)?;
        owner.entry_digest = entry_digest;
        owner.generation = change_id.clone();
    }
    if request.selection.action == Action::SetDefault {
        built
            .after
            .default
            .as_mut()
            .ok_or(Error::InvalidConfig)?
            .generation = change_id.clone();
    }
    let metadata_only = !built.view.writes_file;
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
    // History shows the model a default change involved; other actions have none.
    let model_id = match request.selection.action {
        Action::SetDefault => request.selection.model_id.clone(),
        Action::ClearDefault => built.before.default.as_ref().map(|d| d.model_id.clone()),
        Action::Deploy | Action::Revoke => None,
    };
    let central_revision = built.provider.as_ref().map(|p| p.revision);
    let change = repository::Change {
        id: change_id.clone(),
        root: plan::root_text(&built.root)?.to_string(),
        provider_id: built.view.provider_id.clone(),
        provider_key: built.view.provider_key.clone(),
        action: request.selection.action.code().to_string(),
        state: "prepared".into(),
        before_digest: built.view.target.content_digest.clone(),
        intended_digest: if metadata_only {
            None
        } else {
            Some(digest_hex(&content))
        },
        before_owner: built.before,
        after_owner: built.after,
        operation_id: None,
        created_at: crate::shared::current_unix_millis().map_err(|_| Error::StorageUnavailable)?,
        model_id,
    };
    repository::prepare(storage, &change, central_revision)?;
    if metadata_only {
        repository::finalize(storage, &change, true, None)?;
        return Ok(Applied {
            change_id,
            restart_required: false,
        });
    }
    let result = apply_private_files_locked(
        storage,
        OperationKind::CodexSettings,
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
                restart_required: true,
            })
        }
        Ok(result) => {
            let current = read_target_bytes(&built.root.join(plan::CONFIG_FILE))
                .map_err(|_| Error::RecoveryRequired)?
                .0;
            if current.symlink || current.content_digest != change.before_digest {
                return Err(Error::RecoveryRequired);
            }
            repository::finalize(storage, &change, false, Some(&result.operation_id))?;
            Err(Error::WriteFailed)
        }
        // A journal or post-rename failure may have changed bytes. Keep the
        // durable intent and require explicit observation.
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
            OperationKind::CodexSettings,
        )
        .map_err(|error| match error {
            crate::config_apply::WriteError::PlanStale => Error::PlanStale,
            _ => Error::RecoveryRequired,
        })?;
    } else if intended {
        return Err(Error::RecoveryRequired);
    }
    // Recheck after journal resolution and immediately before publishing
    // ownership; external editors are not covered by the lock.
    let target = read_target_bytes(&root.join(plan::CONFIG_FILE))
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
        restart_required: intended,
    })
}
