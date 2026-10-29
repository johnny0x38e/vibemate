//! Read-only observations of owned provider tables and the owned default,
//! history pages and explicit recovery approvals.

use super::{plan, repository, types::*};
use crate::{
    agents::DiscoveryEnvironment,
    config_apply::BackupStore,
    config_plan::{digest_hex, read_target_bytes},
    storage::Storage,
};
use serde_json::json;
use std::path::Path;

pub(super) fn recovery(
    storage: &Storage,
    root: &Path,
    change: &repository::Change,
    backups: &BackupStore,
) -> Result<Recovery, Error> {
    if change.state != "prepared" || Action::from_code(&change.action).is_none() {
        return Err(Error::OutcomeUnknown);
    }
    let target = read_target_bytes(&root.join(plan::CONFIG_FILE))
        .ok()
        .map(|r| r.0);
    let observation = match &target {
        Some(target) if !target.symlink && target.content_digest == change.before_digest => {
            "before"
        }
        Some(target) if !target.symlink && target.content_digest == change.intended_digest => {
            "intended"
        }
        Some(_) => "conflict",
        None => "unreadable",
    };
    let operation_id = repository::operation_id(storage, change)?;
    let resolution = operation_id
        .as_deref()
        .map(|id| {
            crate::config_apply::plan_revision_resolution(storage, id, backups)
                .map_err(|_| Error::RecoveryRequired)
        })
        .transpose()?
        .flatten();
    let can_resolve = matches!(observation, "before" | "intended")
        && (observation != "intended" || operation_id.is_some())
        && resolution
            .as_ref()
            .is_none_or(|p| p.revisions.is_empty() || p.resolvable);
    let approval = if can_resolve {
        Some(digest_hex(
            &serde_json::to_vec(&json!({
                "id": change.id,
                "root": change.root,
                "before": change.before_digest,
                "intended": change.intended_digest,
                "beforeOwner": change.before_owner,
                "afterOwner": change.after_owner,
                "target": target,
                "operation": operation_id,
                "resolution": resolution,
                "observation": observation,
            }))
            .map_err(|_| Error::OutcomeUnknown)?,
        ))
    } else {
        None
    };
    Ok(Recovery {
        id: change.id.clone(),
        provider_id: change.provider_id.clone(),
        provider_key: change.provider_key.clone(),
        action: change.action.clone(),
        observation: observation.to_string(),
        approval,
    })
}

pub(super) fn read_state(
    storage: &Storage,
    environment: &DiscoveryEnvironment,
    backups: &BackupStore,
) -> Result<State, Error> {
    let root = plan::root(environment)?;
    let root_key = plan::root_text(&root)?;
    // Observation never fails on native problems; they become row states.
    let document = plan::read(&root).ok().map(|(_, _, doc)| doc);
    let providers = repository::bindings(storage, root_key)?
        .into_iter()
        .map(|(provider_id, display_name, revision, owner)| {
            let state = match &document {
                None => "unreadable",
                Some(doc) => match plan::provider_item(doc, &owner.provider_key) {
                    None => "missing",
                    Some(item) => {
                        if plan::digest(&plan::item_json(item))? == owner.entry_digest {
                            "written"
                        } else {
                            "modified"
                        }
                    }
                },
            };
            Ok(ProviderBinding {
                provider_id,
                display_name,
                outdated: revision != owner.provider_revision,
                owner,
                state: state.to_string(),
            })
        })
        .collect::<Result<Vec<_>, Error>>()?;
    let default = repository::default_owner(storage, root_key)?
        .map(|owner| {
            let state = match &document {
                None => "unreadable",
                Some(doc) => match plan::current_defaults(doc) {
                    Ok(values)
                        if plan::owned_defaults_digest(&values, owner.writes_effort)?
                            == owner.entry_digest =>
                    {
                        "written"
                    }
                    // Both identity keys gone: the user removed the default.
                    Ok(values) if values.model.is_none() && values.model_provider.is_none() => {
                        "missing"
                    }
                    Ok(_) => "modified",
                    Err(_) => "unreadable",
                },
            };
            Ok::<_, Error>(DefaultBinding {
                provider_id: owner.provider_id,
                model_id: owner.model_id,
                provider_key: owner.provider_key,
                state: state.to_string(),
            })
        })
        .transpose()?;
    let recovery = repository::pending(storage, root_key)?
        .as_ref()
        .map(|change| recovery(storage, &root, change, backups))
        .transpose()?;
    Ok(State {
        config_root: root_key.to_string(),
        target_path: root
            .join(plan::CONFIG_FILE)
            .to_str()
            .ok_or(Error::TargetUnavailable)?
            .to_string(),
        providers,
        default,
        profile_active: document
            .as_ref()
            .is_some_and(|doc| doc.get("profile").is_some()),
        recovery,
    })
}

pub(super) fn read_history(
    storage: &Storage,
    environment: &DiscoveryEnvironment,
    request: &HistoryRequest,
) -> Result<HistoryPage, Error> {
    if !(1..=100).contains(&request.limit)
        || request
            .provider_id
            .as_deref()
            .is_some_and(|id| !plan::valid_id(id))
        || request
            .after
            .as_deref()
            .is_some_and(|id| !plan::valid_id(id))
    {
        return Err(Error::InvalidRequest);
    }
    let root = plan::root(environment)?;
    repository::history(
        storage,
        plan::root_text(&root)?,
        request.provider_id.as_deref(),
        request.after.as_deref(),
        request.limit,
    )
}
