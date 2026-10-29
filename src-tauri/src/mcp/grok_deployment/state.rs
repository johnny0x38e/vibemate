//! Read-only native observations, paginated registrations/history and explicit
//! recovery approvals. These states describe disk, not live MCP connections.

use super::{plan, repository, types::*};
use crate::{
    agents::DiscoveryEnvironment,
    config_apply::BackupStore,
    config_plan::{digest_hex, read_target_bytes},
    storage::Storage,
};
use serde_json::json;
use std::path::Path;
use toml_edit::Item;

pub(super) fn recovery(
    storage: &Storage,
    root: &Path,
    change: &repository::Change,
    backups: &BackupStore,
) -> Result<Recovery, Error> {
    if change.state != "prepared" || !repository::valid_action(&change.action) {
        return Err(Error::OutcomeUnknown);
    }
    let target = read_target_bytes(&root.join("config.toml"))
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
        definition_id: change.definition_id.clone(),
        server_name: change.server_name.clone(),
        action: change.action.clone(),
        observation: observation.to_string(),
        approval,
    })
}

pub(super) fn read_state(
    storage: &Storage,
    environment: &DiscoveryEnvironment,
    request: &StateRequest,
    backups: &BackupStore,
) -> Result<State, Error> {
    if !(1..=100).contains(&request.limit)
        || request
            .definition_id
            .as_deref()
            .is_some_and(|id| !plan::valid_id(id))
        || request
            .after
            .as_deref()
            .is_some_and(|id| !plan::valid_id(id))
        || request.definition_id.is_some() && request.after.is_some()
    {
        return Err(Error::InvalidRequest);
    }
    let root = plan::root(environment)?;
    let root_key = plan::root_text(&root)?;
    let mut rows = repository::deployments(
        storage,
        root_key,
        request.definition_id.as_deref(),
        request.after.as_deref(),
        if request.definition_id.is_some() {
            101
        } else {
            request.limit + 1
        },
    )?;
    if request.definition_id.is_some() && rows.len() > 100 {
        return Err(Error::StorageUnavailable);
    }
    let next_cursor = if request.definition_id.is_none() && rows.len() > request.limit as usize {
        rows.pop();
        rows.last().map(|r| r.0.clone())
    } else {
        None
    };
    let deployments = rows
        .into_iter()
        .map(|(definition_id, display_name, config_root, ownership)| {
            let root_path = Path::new(&config_root);
            if !root_path.is_absolute() || config_root.chars().any(char::is_control) {
                return Err(Error::StorageUnavailable);
            }
            let (state, unmanaged_keys) = match read_target_bytes(&root_path.join("config.toml")) {
                Ok((target, bytes)) if !target.symlink => {
                    match plan::parse_document(&bytes, target.exists) {
                        Ok(doc) => match doc
                            .get("mcp_servers")
                            .and_then(Item::as_table_like)
                            .and_then(|servers| servers.get(&ownership.server_name))
                        {
                            Some(item) => observe_entry(item, &ownership)?,
                            None => ("missing", Vec::new()),
                        },
                        Err(_) => ("unreadable", Vec::new()),
                    }
                }
                _ => ("unreadable", Vec::new()),
            };
            Ok(Deployment {
                definition_id,
                display_name,
                config_root,
                ownership,
                state: state.to_string(),
                unmanaged_keys,
            })
        })
        .collect::<Result<Vec<_>, Error>>()?;
    let pending = repository::pending(storage, root_key)?;
    let recovery = pending
        .as_ref()
        .map(|c| recovery(storage, &root, c, backups))
        .transpose()?;
    Ok(State {
        config_root: root_key.to_string(),
        target_path: root
            .join("config.toml")
            .to_str()
            .ok_or(Error::TargetUnavailable)?
            .to_string(),
        deployments,
        next_cursor,
        recovery,
    })
}

/// Compare the native entry with the recorded digest; key names only leave.
/// An entry vibemate cannot interpret (for example a non-boolean `enabled`)
/// was changed by hand, so it is `modified` for this row instead of failing
/// the whole state read. Same rule as the Codex adapter.
fn observe_entry(item: &Item, ownership: &Ownership) -> Result<(&'static str, Vec<String>), Error> {
    let Ok(entry) = plan::entry_from_toml(item) else {
        return Ok(("modified", Vec::new()));
    };
    let written = plan::entry_digest(&entry)? == ownership.entry_digest;
    // Display names only, within interface limits; the digest above used
    // the full key set.
    let unmanaged_keys = crate::mcp::unmanaged_keys::display_names(&entry);
    Ok((if written { "written" } else { "modified" }, unmanaged_keys))
}

pub(super) fn read_history(
    storage: &Storage,
    environment: &DiscoveryEnvironment,
    request: &HistoryRequest,
) -> Result<HistoryPage, Error> {
    if !plan::valid_id(&request.definition_id)
        || !(1..=100).contains(&request.limit)
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
        &request.definition_id,
        request.after.as_deref(),
        request.limit,
    )
}
