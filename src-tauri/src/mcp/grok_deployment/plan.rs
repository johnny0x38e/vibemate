//! Rebuild entry-level plans from central metadata, native TOML and verified
//! history. Raw documents stay internal; only masked entry fragments leave.

use super::{repository, types::*};
use crate::mcp::{GrokMcpPreflightError, get_definition, preflight_grok_registration};
use crate::{
    agents::DiscoveryEnvironment,
    config_apply::{BackupStore, OperationKind},
    config_plan::{TargetFile, digest_hex, read_target_bytes},
    storage::Storage,
};
use serde_json::{Map, Value, json};
use std::path::PathBuf;
use toml_edit::{DocumentMut, Item, Table, value};

pub(super) struct Built {
    pub view: Preview,
    pub root: PathBuf,
    /// Mutable full document; deploy fills credentials then re-serializes.
    pub document: DocumentMut,
    pub original_bytes: Vec<u8>,
    pub before_owner: Option<Ownership>,
    pub after_entry: Option<Value>,
    pub deployed_revision: i64,
}

pub(super) fn root(environment: &DiscoveryEnvironment) -> Result<PathBuf, Error> {
    crate::agents::grok_config_root(environment)
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

pub(super) fn parse_document(bytes: &[u8], exists: bool) -> Result<DocumentMut, Error> {
    if !exists {
        return Ok(DocumentMut::new());
    }
    let text = std::str::from_utf8(bytes).map_err(|_| Error::InvalidConfig)?;
    let document = text
        .parse::<DocumentMut>()
        .map_err(|_| Error::InvalidConfig)?;
    if let Some(servers) = document.get("mcp_servers")
        && !servers.is_table_like()
    {
        return Err(Error::InvalidConfig);
    }
    if let Some(disabled) = document.get("disabled_mcp_servers") {
        let array = disabled.as_array().ok_or(Error::InvalidConfig)?;
        if array.iter().any(|item| item.as_str().is_none()) {
            return Err(Error::InvalidConfig);
        }
    }
    Ok(document)
}

pub(super) fn read(root: &std::path::Path) -> Result<(TargetFile, Vec<u8>, DocumentMut), Error> {
    let path = root.join("config.toml");
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
    let doc = parse_document(&bytes, target.exists)?;
    Ok((target, bytes, doc))
}

fn string_map(item: Option<&Item>) -> Result<Map<String, Value>, Error> {
    let Some(item) = item else {
        return Ok(Map::new());
    };
    let table = item.as_table_like().ok_or(Error::InvalidConfig)?;
    let mut map = Map::new();
    for (key, value) in table.iter() {
        let text = value.as_str().ok_or(Error::InvalidConfig)?;
        map.insert(key.to_owned(), json!(text));
    }
    Ok(map)
}

/// Canonical JSON for one Grok MCP table; used for ownership digests and IPC.
pub(super) fn entry_from_toml(item: &Item) -> Result<Value, Error> {
    let table = item.as_table_like().ok_or(Error::InvalidConfig)?;
    let enabled = table.get("enabled").and_then(Item::as_bool).unwrap_or(true);
    if table.get("command").is_some() {
        let command = table
            .get("command")
            .and_then(Item::as_str)
            .ok_or(Error::InvalidConfig)?
            .to_owned();
        let args = match table.get("args") {
            None => Vec::new(),
            Some(item) => {
                let array = item.as_array().ok_or(Error::InvalidConfig)?;
                array
                    .iter()
                    .map(|v| v.as_str().map(str::to_owned).ok_or(Error::InvalidConfig))
                    .collect::<Result<Vec<_>, _>>()?
            }
        };
        let mut entry = json!({
            "command": command,
            "args": args,
            "env": string_map(table.get("env"))?,
            "enabled": enabled
        });
        if let Some(cwd) = table.get("cwd") {
            entry["cwd"] = json!(cwd.as_str().ok_or(Error::InvalidConfig)?);
        }
        Ok(entry)
    } else if table.get("url").is_some() {
        Ok(json!({
            "url": table.get("url").and_then(Item::as_str).ok_or(Error::InvalidConfig)?,
            "headers": string_map(table.get("headers"))?,
            "enabled": enabled
        }))
    } else {
        Err(Error::InvalidConfig)
    }
}

fn set_string_map(table: &mut Table, key: &str, fields: &Map<String, Value>) -> Result<(), Error> {
    let mut nested = Table::new();
    nested.set_implicit(true);
    for (name, field) in fields {
        let text = field.as_str().ok_or(Error::InvalidConfig)?;
        nested.insert(name, value(text));
    }
    table.insert(key, Item::Table(nested));
    Ok(())
}

/// Write one canonical entry Value into `mcp_servers.<name>`, replacing any prior table.
pub(super) fn put_entry(
    document: &mut DocumentMut,
    name: &str,
    entry: &Value,
) -> Result<(), Error> {
    let servers = document
        .entry("mcp_servers")
        .or_insert_with(|| Item::Table(Table::new()))
        .as_table_mut()
        .ok_or(Error::InvalidConfig)?;
    let mut table = Table::new();
    let object = entry.as_object().ok_or(Error::InvalidConfig)?;
    if let Some(command) = object.get("command") {
        table.insert(
            "command",
            value(command.as_str().ok_or(Error::InvalidConfig)?),
        );
        let mut args = toml_edit::Array::new();
        for item in object
            .get("args")
            .and_then(Value::as_array)
            .ok_or(Error::InvalidConfig)?
        {
            args.push(item.as_str().ok_or(Error::InvalidConfig)?);
        }
        table.insert("args", Item::Value(args.into()));
        set_string_map(
            &mut table,
            "env",
            object
                .get("env")
                .and_then(Value::as_object)
                .ok_or(Error::InvalidConfig)?,
        )?;
        if let Some(cwd) = object.get("cwd") {
            table.insert("cwd", value(cwd.as_str().ok_or(Error::InvalidConfig)?));
        }
    } else {
        table.insert(
            "url",
            value(
                object
                    .get("url")
                    .and_then(Value::as_str)
                    .ok_or(Error::InvalidConfig)?,
            ),
        );
        set_string_map(
            &mut table,
            "headers",
            object
                .get("headers")
                .and_then(Value::as_object)
                .ok_or(Error::InvalidConfig)?,
        )?;
    }
    table.insert(
        "enabled",
        value(
            object
                .get("enabled")
                .and_then(Value::as_bool)
                .ok_or(Error::InvalidConfig)?,
        ),
    );
    servers.insert(name, Item::Table(table));
    Ok(())
}

pub(super) fn remove_entry(document: &mut DocumentMut, name: &str) {
    if let Some(servers) = document.get_mut("mcp_servers").and_then(Item::as_table_mut) {
        servers.remove(name);
        if servers.is_empty() {
            document.remove("mcp_servers");
        }
    }
}

fn disabled_list_contains(document: &DocumentMut, name: &str) -> Result<bool, Error> {
    let Some(item) = document.get("disabled_mcp_servers") else {
        return Ok(false);
    };
    let array = item.as_array().ok_or(Error::InvalidConfig)?;
    Ok(array.iter().any(|v| v.as_str() == Some(name)))
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
        for (key, value) in map.iter_mut() {
            if crate::config_apply::version_text::is_secret_key(key) {
                *value = json!(crate::config_plan::SECRET_PLACEHOLDER);
            }
        }
    }
    Some(value)
}

fn map_preflight(error: GrokMcpPreflightError) -> Error {
    match error {
        GrokMcpPreflightError::Definition(_) => Error::DefinitionChanged,
        GrokMcpPreflightError::InvalidConfig => Error::InvalidConfig,
        GrokMcpPreflightError::NameConflict => Error::NameConflict,
        GrokMcpPreflightError::UnsupportedServerName => Error::UnsupportedServerName,
        GrokMcpPreflightError::DynamicValueUnsupported => Error::DynamicValueUnsupported,
        GrokMcpPreflightError::PathSemanticsUnverified => Error::PathSemanticsUnverified,
    }
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
    // Exact name only: Grok keeps hyphens distinct from underscores.
    if before_owner.is_none()
        && doc
            .get("mcp_servers")
            .and_then(Item::as_table_like)
            .is_some_and(|servers| servers.contains_key(&name))
    {
        return Err(Error::NameConflict);
    }
    let before_entry = doc
        .get("mcp_servers")
        .and_then(Item::as_table_like)
        .and_then(|servers| servers.get(&name))
        .map(entry_from_toml)
        .transpose()?;
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
            preflight_grok_registration(storage, &record.id, record.revision, None)
                .map_err(map_preflight)?
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
                let retained = crate::config_apply::read_mcp_written_file_kind(
                    storage,
                    operation,
                    &root,
                    backups,
                    OperationKind::GrokMcp,
                )
                .map_err(|_| Error::SnapshotUnavailable)?;
                let old =
                    parse_document(&retained, true).map_err(|_| Error::SnapshotUnavailable)?;
                let entry = old
                    .get("mcp_servers")
                    .and_then(Item::as_table_like)
                    .and_then(|servers| servers.get(&name))
                    .map(entry_from_toml)
                    .transpose()
                    .map_err(|_| Error::SnapshotUnavailable)?
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
        put_entry(&mut doc, &name, entry)?;
    } else {
        remove_entry(&mut doc, &name);
    }
    let before_safe = sanitize(before_entry.as_ref());
    let after_safe = sanitize(after_entry.as_ref());
    let fragment = |entry: &Option<Value>| {
        entry
            .as_ref()
            .map(|e| serde_json::to_vec(&json!({"mcp_servers":{&name:e}})))
            .transpose()
            .map_err(|_| Error::InvalidConfig)
    };
    let before_fragment = fragment(&before_safe)?;
    let after_fragment = fragment(&after_safe)?;
    let summary = crate::config_apply::version_text::summarize(
        OperationKind::GrokMcp,
        before_fragment.as_deref(),
        after_fragment.as_deref(),
    );
    let disabled_by_user_list = disabled_list_contains(&doc, &name)?;
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
            "disabledByUserList": disabled_by_user_list,
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
        disabled_by_user_list,
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
