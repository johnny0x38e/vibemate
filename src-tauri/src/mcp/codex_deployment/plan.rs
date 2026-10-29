//! Rebuild Codex entry-level plans from central metadata, native TOML and
//! verified history. Raw documents stay internal; only masked fragments leave.

use super::{repository, types::*};
use crate::mcp::{ConnectionRecord, get_definition};
use crate::{
    agents::DiscoveryEnvironment,
    config_apply::{BackupStore, OperationKind},
    config_plan::{SECRET_PLACEHOLDER, TargetFile, digest_hex, read_target_bytes},
    storage::Storage,
};
use serde_json::{Map, Value, json};
use std::path::{Path, PathBuf};
use toml_edit::{DocumentMut, Item, Table, value};

/// Native file below the Codex root.
pub(crate) const CONFIG_FILE: &str = "config.toml";
/// Longest server name accepted here; matches the ownership table bound.
const MAX_SERVER_NAME: usize = 64;
/// Keys vibemate maps for stdio entries.
const STDIO_KEYS: [&str; 5] = ["command", "args", "env", "cwd", "enabled"];
/// Keys vibemate maps for streamable HTTP entries.
const HTTP_KEYS: [&str; 3] = ["url", "http_headers", "enabled"];

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
    crate::agents::codex_config_root(environment)
        .0
        .ok_or(Error::TargetUnavailable)
}
pub(super) fn root_text(root: &Path) -> Result<&str, Error> {
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

/// Whether Codex's own `codex mcp` CLI accepts this server name.
pub(crate) fn valid_server_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_SERVER_NAME
        && name.bytes().all(|b| {
            b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b':' | b'@' | b'/' | b'.')
        })
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
    Ok(document)
}

/// Read `config.toml`, refusing a symbolic link (the writer never follows one)
/// and a read-only file. A missing root directory means Codex has not created
/// its state yet; vibemate does not invent it.
pub(super) fn read(root: &Path) -> Result<(TargetFile, Vec<u8>, DocumentMut), Error> {
    if !std::fs::metadata(root).is_ok_and(|m| m.is_dir()) {
        return Err(Error::TargetUnavailable);
    }
    let path = root.join(CONFIG_FILE);
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

/// Canonical JSON for one Codex MCP table; used for ownership digests and IPC.
///
/// Values are semantic: inline vs. dotted tables, key order, an omitted
/// `enabled = true` and omitted empty `args`/`env` all digest identically, so
/// Codex re-serializing `[mcp_servers]` does not look like a user edit. Keys
/// vibemate does not map (for example `enabled_tools` or timeouts added by
/// hand) are listed under `unmanagedKeys`, so they change the digest and an
/// owned entry carrying them is treated as externally modified.
pub(crate) fn entry_from_toml(item: &Item) -> Result<Value, Error> {
    let table = item.as_table_like().ok_or(Error::InvalidConfig)?;
    let enabled = match table.get("enabled") {
        None => true,
        Some(item) => item.as_bool().ok_or(Error::InvalidConfig)?,
    };
    let known: &[&str] = if table.get("command").is_some() {
        &STDIO_KEYS
    } else {
        &HTTP_KEYS
    };
    let mut unmanaged: Vec<String> = table
        .iter()
        .map(|(key, _)| key.to_owned())
        .filter(|key| !known.contains(&key.as_str()))
        .collect();
    unmanaged.sort();
    let mut entry = if table.get("command").is_some() {
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
        entry
    } else if table.get("url").is_some() {
        json!({
            "url": table.get("url").and_then(Item::as_str).ok_or(Error::InvalidConfig)?,
            "http_headers": string_map(table.get("http_headers"))?,
            "enabled": enabled
        })
    } else {
        return Err(Error::InvalidConfig);
    };
    if !unmanaged.is_empty() {
        entry["unmanagedKeys"] = json!(unmanaged);
    }
    Ok(entry)
}

fn set_string_map(table: &mut Table, key: &str, fields: &Map<String, Value>) -> Result<(), Error> {
    if fields.is_empty() {
        return Ok(());
    }
    let mut nested = toml_edit::InlineTable::new();
    for (name, field) in fields {
        let text = field.as_str().ok_or(Error::InvalidConfig)?;
        nested.insert(name, text.into());
    }
    table.insert(key, value(nested));
    Ok(())
}

/// Write one canonical entry into `mcp_servers.<name>`, replacing any prior
/// table. Only mapped keys are emitted: never `bearer_token`, `env_vars`,
/// `env_http_headers`, `http_headers_helper`, `auth`/`oauth` or a `type` key.
pub(super) fn put_entry(
    document: &mut DocumentMut,
    name: &str,
    entry: &Value,
) -> Result<(), Error> {
    let object = entry.as_object().ok_or(Error::InvalidConfig)?;
    if object.contains_key("unmanagedKeys") {
        return Err(Error::OwnershipConflict);
    }
    let servers = match document.get_mut("mcp_servers") {
        // An inline `mcp_servers = { … }` parent cannot hold a
        // `[mcp_servers.<name>]` table. Rewriting the user's layout is not
        // ours to do, so the preview (which runs this same edit) refuses it
        // as `invalid_config` before any approval exists.
        Some(item) => item.as_table_mut().ok_or(Error::InvalidConfig)?,
        None => {
            let mut table = Table::new();
            // Keep the parent implicit so only `[mcp_servers.<name>]` appears.
            table.set_implicit(true);
            document.insert("mcp_servers", Item::Table(table));
            document
                .get_mut("mcp_servers")
                .and_then(Item::as_table_mut)
                .ok_or(Error::InvalidConfig)?
        }
    };
    let mut table = Table::new();
    // Keep the replaced table's leading comment and position, like
    // `put_provider` does for provider tables.
    if let Some(old) = servers.get(name).and_then(Item::as_table) {
        *table.decor_mut() = old.decor().clone();
        if let Some(position) = old.position() {
            table.set_position(Some(position));
        }
    }
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
            "http_headers",
            object
                .get("http_headers")
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

/// Remove one entry. An inline `mcp_servers = { … }` parent is table-like
/// too, so a revoke never reports success while the entry stays on disk.
pub(super) fn remove_entry(document: &mut DocumentMut, name: &str) {
    if let Some(servers) = document
        .get_mut("mcp_servers")
        .and_then(Item::as_table_like_mut)
    {
        servers.remove(name);
        if servers.is_empty() {
            document.remove("mcp_servers");
        }
    }
}

pub(super) fn sanitize(entry: Option<&Value>) -> Option<Value> {
    let mut value = entry?.clone();
    if let Some(map) = value.as_object_mut() {
        for key in ["env", "http_headers"] {
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
        // Credentials inside public fields: URL query/userinfo and `args`.
        crate::secret_text::mask_public_fields(map);
    }
    Some(value)
}

/// Masked first-deploy entry from the central definition. Only names leave
/// central storage here; OS credentials are read later, in apply.
fn proposed_entry(record: &crate::mcp::McpRecord) -> Result<Value, Error> {
    if !valid_server_name(&record.server_name) {
        return Err(Error::UnsupportedServerName);
    }
    let masked = |names: &[String]| -> Map<String, Value> {
        names
            .iter()
            .map(|name| (name.clone(), json!(SECRET_PLACEHOLDER)))
            .collect()
    };
    match &record.connection {
        ConnectionRecord::Stdio {
            command,
            args,
            cwd,
            env,
        } => {
            for value in std::iter::once(command)
                .chain(args.iter())
                .chain(cwd.iter())
            {
                if value.contains("${") {
                    return Err(Error::DynamicValueUnsupported);
                }
                if value.starts_with("~/") {
                    return Err(Error::PathSemanticsUnverified);
                }
            }
            if cwd
                .as_ref()
                .is_some_and(|path| !Path::new(path).is_absolute())
            {
                return Err(Error::PathSemanticsUnverified);
            }
            let mut entry = json!({
                "command": command, "args": args,
                "env": masked(env), "enabled": record.enabled
            });
            if let Some(cwd) = cwd {
                entry["cwd"] = json!(cwd);
            }
            Ok(entry)
        }
        ConnectionRecord::Http { url, headers } => {
            if url.contains("${") {
                return Err(Error::DynamicValueUnsupported);
            }
            // Codex infers streamable HTTP from `url`; no `type` key exists and
            // an Authorization header is written as a plain header.
            Ok(json!({"url": url, "http_headers": masked(headers), "enabled": record.enabled}))
        }
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
    if repository::pending(storage, root_key)?.is_some()
        || repository::settings_pending(storage, root_key)?
    {
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
    // Exact key only; an unowned entry with this name is never replaced.
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
        Action::Deploy => Some(proposed_entry(&record)?),
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
                    OperationKind::CodexMcp,
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
        OperationKind::CodexMcp,
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
        document: doc,
        original_bytes: bytes,
        before_owner,
        after_entry,
        deployed_revision,
    })
}
