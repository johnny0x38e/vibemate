//! Rebuild entry-level plans from central metadata, native bytes and verified
//! history. Raw documents stay internal; only masked entry fragments leave.

use super::{repository, types::*};
use crate::mcp::{get_definition, preflight_pi_registration};
use crate::{
    agents::DiscoveryEnvironment,
    config_apply::{BackupStore, OperationKind},
    config_plan::{TargetFile, digest_hex, read_target_bytes},
    storage::Storage,
};
use serde_json::{Value, json};
use std::path::PathBuf;

pub(super) struct Built {
    pub view: Preview,
    pub root: PathBuf,
    pub document: Value,
    pub original_bytes: Vec<u8>,
    pub before_owner: Option<Ownership>,
    pub after_entry: Option<Value>,
    pub deployed_revision: i64,
}

pub(super) fn root(environment: &DiscoveryEnvironment) -> Result<PathBuf, Error> {
    crate::agents::pi_config_root(environment)
        .0
        .ok_or(Error::TargetUnavailable)
}
pub(super) fn root_text(root: &std::path::Path) -> Result<&str, Error> {
    root.to_str().ok_or(Error::TargetUnavailable)
}
pub(super) fn valid_id(id: &str) -> bool {
    crate::shared::is_random_id(id)
}
pub(super) fn entry_digest(entry: &Value) -> Result<String, Error> {
    serde_json::to_vec(entry)
        .map(|b| digest_hex(&b))
        .map_err(|_| Error::InvalidConfig)
}
pub(super) fn document(bytes: &[u8], exists: bool) -> Result<Value, Error> {
    if !exists {
        return Ok(json!({}));
    }
    let bytes = bytes.strip_prefix(b"\xef\xbb\xbf").unwrap_or(bytes);
    let value: Value = serde_json::from_slice(bytes).map_err(|_| Error::InvalidConfig)?;
    if !value.is_object() || value.get("mcpServers").is_some_and(|v| !v.is_object()) {
        return Err(Error::InvalidConfig);
    }
    Ok(value)
}
pub(super) fn read(root: &std::path::Path) -> Result<(TargetFile, Vec<u8>, Value), Error> {
    let path = root.join("mcp.json");
    let (target, bytes) = read_target_bytes(&path).map_err(|_| Error::TargetUnavailable)?;
    if target.symlink {
        return Err(Error::TargetUnavailable);
    }
    if target.exists
        && std::fs::metadata(&path)
            .map_err(|_| Error::TargetUnavailable)?
            .permissions()
            .readonly()
    {
        return Err(Error::TargetUnavailable);
    }
    let doc = document(&bytes, target.exists)?;
    Ok((target, bytes, doc))
}

pub(super) fn sanitize(entry: Option<&Value>) -> Option<Value> {
    let mut value = entry?.clone();
    if let Some(map) = value.as_object_mut() {
        for key in ["env", "headers"] {
            if let Some(fields) = map.get_mut(key).and_then(Value::as_object_mut) {
                for field in fields.values_mut() {
                    *field = json!(crate::config_plan::SECRET_PLACEHOLDER);
                }
            }
        }
        // Own entries normally contain only mapped fields. Mask credential-like
        // optional keys too, rather than relying on the current schema forever.
        for (key, value) in map.iter_mut() {
            if crate::config_apply::version_text::is_secret_key(key) {
                *value = json!(crate::config_plan::SECRET_PLACEHOLDER);
            }
        }
        // Credentials inside public fields: URL query/userinfo and `args`.
        crate::secret_text::mask_public_fields(map);
    }
    Some(value)
}

pub(super) fn build(
    storage: &Storage,
    environment: &DiscoveryEnvironment,
    selection: &Selection,
    backups: &BackupStore,
) -> Result<Built, Error> {
    if !valid_id(&selection.definition_id)
        || selection.expected_revision < 1
        || (selection.action == Action::Restore) != selection.history_id.is_some()
        || selection
            .history_id
            .as_deref()
            .is_some_and(|id| !valid_id(id))
    {
        return Err(Error::InvalidRequest);
    }
    let record =
        get_definition(storage, &selection.definition_id).map_err(|_| Error::DefinitionChanged)?;
    if record.revision != selection.expected_revision {
        return Err(Error::DefinitionChanged);
    }
    let root = root(environment)?;
    let root_key = root_text(&root)?;
    if repository::pending(storage, root_key)?.is_some() {
        return Err(Error::RecoveryRequired);
    }
    let before_owner = repository::owner(storage, root_key, &record.id)?;
    let (target, bytes, mut doc) = read(&root)?;
    let historical = selection
        .history_id
        .as_deref()
        .map(|id| repository::retained(storage, root_key, &record.id, id))
        .transpose()?;
    let name = if let Some(history) = &historical {
        history.server_name.clone()
    } else {
        before_owner
            .as_ref()
            .map_or_else(|| record.server_name.clone(), |o| o.server_name.clone())
    };
    if selection.action == Action::Deploy && name != record.server_name {
        return Err(Error::OwnershipConflict);
    }
    if before_owner
        .as_ref()
        .is_some_and(|owner| owner.server_name != name)
    {
        return Err(Error::OwnershipConflict);
    }
    if let Some(servers) = doc.get("mcpServers").and_then(Value::as_object) {
        for key in servers.keys() {
            if key.replace('-', "_") == name.replace('-', "_")
                && !(key == &name && before_owner.is_some())
            {
                return Err(Error::NameConflict);
            }
        }
    }
    let before_entry = doc.get("mcpServers").and_then(|v| v.get(&name)).cloned();
    match (&before_owner, &before_entry) {
        (Some(owner), Some(entry)) if entry_digest(entry)? != owner.entry_digest => {
            return Err(Error::OwnershipConflict);
        }
        (Some(_), None) if selection.action != Action::Revoke => {
            return Err(Error::OwnershipConflict);
        }
        (None, _)
            if matches!(
                selection.action,
                Action::Enable | Action::Disable | Action::Revoke
            ) =>
        {
            return Err(Error::OwnershipConflict);
        }
        _ => {}
    }
    let mut deployed_revision = record.revision;
    let mut historical_digest = None;
    let after_entry = match selection.action {
        Action::Deploy => Some(
            preflight_pi_registration(storage, &record.id, record.revision, None)
                .map_err(|_| Error::DefinitionChanged)?
                .redacted_entry,
        ),
        Action::Enable | Action::Disable => {
            let mut entry = before_entry.clone().ok_or(Error::OwnershipConflict)?;
            entry["enabled"] = json!(selection.action == Action::Enable);
            deployed_revision = before_owner
                .as_ref()
                .ok_or(Error::OwnershipConflict)?
                .definition_revision;
            Some(entry)
        }
        Action::Revoke => None,
        Action::Restore => {
            let history = historical.as_ref().ok_or(Error::SnapshotUnavailable)?;
            if !repository::valid_action(&history.action) {
                return Err(Error::SnapshotUnavailable);
            }
            if let Some(owner) = &history.after_owner {
                let operation = history
                    .operation_id
                    .as_deref()
                    .ok_or(Error::SnapshotUnavailable)?;
                let retained =
                    crate::config_apply::read_mcp_written_file(storage, operation, &root, backups)
                        .map_err(|_| Error::SnapshotUnavailable)?;
                let old = document(&retained, true).map_err(|_| Error::SnapshotUnavailable)?;
                let entry = old
                    .get("mcpServers")
                    .and_then(|v| v.get(&name))
                    .cloned()
                    .ok_or(Error::SnapshotUnavailable)?;
                if entry_digest(&entry)? != owner.entry_digest {
                    return Err(Error::SnapshotUnavailable);
                }
                historical_digest = Some(digest_hex(&retained));
                deployed_revision = owner.definition_revision;
                Some(entry)
            } else {
                None
            }
        }
    };
    if let Some(entry) = &after_entry {
        if doc.get("mcpServers").is_none() {
            doc["mcpServers"] = json!({});
        }
        doc["mcpServers"][&name] = entry.clone();
    } else if let Some(servers) = doc.get_mut("mcpServers").and_then(Value::as_object_mut) {
        servers.remove(&name);
    }
    let before_safe = sanitize(before_entry.as_ref());
    let after_safe = sanitize(after_entry.as_ref());
    let fragment = |entry: &Option<Value>| {
        entry
            .as_ref()
            .map(|e| serde_json::to_vec(&json!({"mcpServers":{&name:e}})))
            .transpose()
            .map_err(|_| Error::InvalidConfig)
    };
    let before_fragment = fragment(&before_safe)?;
    let after_fragment = fragment(&after_safe)?;
    let summary = crate::config_apply::version_text::summarize(
        OperationKind::PiMcp,
        before_fragment.as_deref(),
        after_fragment.as_deref(),
    );
    let session_relative_cwd = after_entry
        .as_ref()
        .and_then(|e| e.get("cwd"))
        .and_then(Value::as_str)
        .is_some_and(|cwd| !std::path::Path::new(cwd).is_absolute() && !cwd.starts_with("~/"));
    let expands_home = after_entry.as_ref().is_some_and(|entry| {
        entry
            .get("command")
            .and_then(Value::as_str)
            .is_some_and(|v| v.starts_with("~/"))
            || entry
                .get("cwd")
                .and_then(Value::as_str)
                .is_some_and(|v| v.starts_with("~/"))
            || entry
                .get("args")
                .and_then(Value::as_array)
                .is_some_and(|args| {
                    args.iter()
                        .any(|arg| arg.as_str().is_some_and(|v| v.starts_with("~/")))
                })
    });
    let last_change_id = repository::last_change_id(storage, root_key, &record.id)?;
    let approval=digest_hex(&serde_json::to_vec(&json!({"selection":selection,"target":target,"owner":before_owner,"lastChange":last_change_id,"historicalDigest":historical_digest,"before":before_safe,"after":after_safe})).map_err(|_|Error::InvalidRequest)?);
    let view = Preview {
        target,
        definition_id: record.id,
        definition_revision: record.revision,
        action: selection.action,
        server_name: name,
        before_entry: before_safe,
        after_entry: after_safe,
        summary,
        approval,
        session_relative_cwd,
        expands_home,
    };
    Ok(Built {
        view,
        root,
        document: doc,
        original_bytes: bytes,
        before_owner,
        after_entry,
        deployed_revision,
    })
}
