//! Rebuild entry-level plans from central metadata, native `.claude.json` and
//! verified history. Raw documents stay internal; only masked entry fragments
//! leave Rust.

use super::{json_edit, repository, types::*};
use crate::mcp::{ConnectionRecord, McpRecord, get_definition};
use crate::{
    agents::DiscoveryEnvironment,
    config_apply::{BackupStore, OperationKind},
    config_plan::{SECRET_PLACEHOLDER, TargetFile, digest_hex, read_target_bytes},
    storage::Storage,
};
use serde_json::{Map, Value, json};
use std::path::{Path, PathBuf};

/// User-scope file name inside the selected root.
pub(super) const TARGET: &str = ".claude.json";

/// Names Claude Code reserves for built-in servers (documented for 2.1.x).
/// Names with spaces are already impossible under central name validation.
const RESERVED_NAMES: [&str; 3] = ["workspace", "claude-in-chrome", "computer-use"];

pub(super) struct Built {
    pub view: Preview,
    pub root: PathBuf,
    /// Exact current bytes (`{}` stands in for an absent file).
    pub original_bytes: Vec<u8>,
    pub before_owner: Option<Ownership>,
    /// Complete proposed entry; deploy placeholders are replaced in `service`.
    pub after_entry: Option<Value>,
    pub deployed_revision: i64,
}

/// Directory containing `.claude.json`. Claude Code 2.1.x reads user-scope MCP
/// servers from `$CLAUDE_CONFIG_DIR/.claude.json` when the variable is set and
/// from `<home>/.claude.json` otherwise — beside, not inside, `~/.claude/`.
/// An invalid override is unavailable; it never falls back to the home file.
pub(super) fn root(environment: &DiscoveryEnvironment) -> Result<PathBuf, Error> {
    let root = if environment.claude_directory.is_some() {
        crate::agents::claude_config_root(environment)
            .0
            .ok_or(Error::TargetUnavailable)?
    } else {
        environment.home.clone()
    };
    if !root.is_absolute()
        || root
            .to_str()
            .is_none_or(|text| text.chars().any(char::is_control))
    {
        return Err(Error::TargetUnavailable);
    }
    Ok(root)
}
pub(super) fn root_text(root: &Path) -> Result<&str, Error> {
    root.to_str().ok_or(Error::TargetUnavailable)
}
pub(super) fn valid_id(id: &str) -> bool {
    crate::shared::is_random_id(id)
}
pub(super) fn entry_digest(entry: &Value) -> String {
    digest_hex(json_edit::canonical(entry).as_bytes())
}

/// Bytes the editor works on: an absent file behaves as an empty object.
pub(super) fn working_bytes(bytes: Vec<u8>, exists: bool) -> Vec<u8> {
    if exists { bytes } else { b"{}".to_vec() }
}

pub(super) fn document(bytes: &[u8]) -> Result<json_edit::Document<'_>, Error> {
    json_edit::parse(bytes).map_err(|_| Error::InvalidConfig)
}

/// Read the target, refusing symlinks and read-only files before any plan.
pub(super) fn read(root: &Path) -> Result<(TargetFile, Vec<u8>), Error> {
    let path = root.join(TARGET);
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
    let bytes = working_bytes(bytes, target.exists);
    document(&bytes)?;
    Ok((target, bytes))
}

fn masked_fields(names: &[String]) -> Map<String, Value> {
    names
        .iter()
        .map(|name| (name.clone(), json!(SECRET_PLACEHOLDER)))
        .collect()
}

fn public_value(value: &str) -> Result<(), Error> {
    // Claude Code expands `${VAR}` / `${VAR:-default}` in user-scope command,
    // args, env, url and headers. A literal central value must stay literal.
    if value.contains("${") {
        return Err(Error::DynamicValueUnsupported);
    }
    Ok(())
}

/// Native entry for a central definition with credential placeholders.
/// stdio and Streamable HTTP map independently, each with an explicit `type`.
pub(super) fn proposed_entry(record: &McpRecord) -> Result<Value, Error> {
    if RESERVED_NAMES
        .iter()
        .any(|name| name.eq_ignore_ascii_case(&record.server_name))
    {
        return Err(Error::UnsupportedServerName);
    }
    // There is no user-level per-server disable to mirror a disabled central
    // definition; writing it enabled would misrepresent the user's choice.
    if !record.enabled {
        return Err(Error::ToggleUnsupported);
    }
    match &record.connection {
        ConnectionRecord::Stdio {
            command,
            args,
            cwd,
            env,
        } => {
            // `claude mcp add-json` silently drops `cwd`; refuse instead of
            // writing a server that would start in another directory.
            if cwd.is_some() {
                return Err(Error::UnsupportedField);
            }
            for value in std::iter::once(command).chain(args.iter()) {
                public_value(value)?;
                if value.starts_with("~/") {
                    return Err(Error::PathSemanticsUnverified);
                }
            }
            let mut entry = json!({"type": "stdio", "command": command, "args": args});
            if !env.is_empty() {
                entry["env"] = Value::Object(masked_fields(env));
            }
            Ok(entry)
        }
        ConnectionRecord::Http { url, headers } => {
            public_value(url)?;
            // Streamable HTTP is `type: "http"`; legacy SSE is never inferred.
            let mut entry = json!({"type": "http", "url": url});
            if !headers.is_empty() {
                entry["headers"] = Value::Object(masked_fields(headers));
            }
            Ok(entry)
        }
    }
}

pub(super) fn sanitize(entry: Option<&Value>) -> Option<Value> {
    let mut value = entry?.clone();
    if let Some(map) = value.as_object_mut() {
        for key in ["env", "headers"] {
            if let Some(fields) = map.get_mut(key).and_then(Value::as_object_mut) {
                for field in fields.values_mut() {
                    *field = json!(SECRET_PLACEHOLDER);
                }
            }
        }
        for (key, value) in map.iter_mut() {
            if crate::config_apply::version_text::is_secret_key(key) {
                *value = json!(SECRET_PLACEHOLDER);
            }
        }
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
    if matches!(selection.action, Action::Enable | Action::Disable) {
        return Err(Error::ToggleUnsupported);
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
    let (target, bytes) = read(&root)?;
    let doc = document(&bytes)?;
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
    // Exact top-level name only. Local-scope entries under `projects.<path>`
    // are a different scope and are neither read as conflicts nor written.
    if before_owner.is_none() && doc.contains(&name) {
        return Err(Error::NameConflict);
    }
    let before_entry = doc.entry(&name).map_err(|_| Error::InvalidConfig)?;
    match (&before_owner, &before_entry) {
        (Some(owner), Some(entry)) if entry_digest(entry) != owner.entry_digest => {
            return Err(Error::OwnershipConflict);
        }
        (Some(_), None) if selection.action != Action::Revoke => {
            return Err(Error::OwnershipConflict);
        }
        (None, _) if selection.action == Action::Revoke => {
            return Err(Error::OwnershipConflict);
        }
        _ => {}
    }
    let mut deployed_revision = record.revision;
    let mut historical_digest = None;
    let after_entry = match selection.action {
        Action::Deploy => Some(proposed_entry(&record)?),
        Action::Enable | Action::Disable => return Err(Error::ToggleUnsupported),
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
                let retained = crate::config_apply::read_mcp_written_file_kind(
                    storage,
                    operation,
                    &root,
                    backups,
                    OperationKind::ClaudeMcp,
                )
                .map_err(|_| Error::SnapshotUnavailable)?;
                // Only this entry is taken from the retained copy; every other
                // key keeps its current bytes.
                let entry = json_edit::parse(&retained)
                    .and_then(|old| old.entry(&name))
                    .map_err(|_| Error::SnapshotUnavailable)?
                    .ok_or(Error::SnapshotUnavailable)?;
                if entry_digest(&entry) != owner.entry_digest {
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
    let before_safe = sanitize(before_entry.as_ref());
    let after_safe = sanitize(after_entry.as_ref());
    let fragment = |entry: &Option<Value>| {
        entry
            .as_ref()
            .map(|e| serde_json::to_vec(&json!({"mcpServers": {&name: e}})))
            .transpose()
            .map_err(|_| Error::InvalidConfig)
    };
    let before_fragment = fragment(&before_safe)?;
    let after_fragment = fragment(&after_safe)?;
    let summary = crate::config_apply::version_text::summarize(
        OperationKind::ClaudeMcp,
        before_fragment.as_deref(),
        after_fragment.as_deref(),
    );
    let last_change_id = repository::last_change_id(storage, root_key, &record.id)?;
    let approval = digest_hex(
        &serde_json::to_vec(&json!({
            "selection": selection,
            "target": target,
            "owner": before_owner,
            "lastChange": last_change_id,
            "historicalDigest": historical_digest,
            "before": before_safe,
            "after": after_safe,
        }))
        .map_err(|_| Error::InvalidRequest)?,
    );
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
    };
    Ok(Built {
        view,
        root,
        original_bytes: bytes,
        before_owner,
        after_entry,
        deployed_revision,
    })
}
