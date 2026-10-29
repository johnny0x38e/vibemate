//! Rebuild Codex provider/default plans from central metadata and native TOML.
//! Raw documents stay internal; only masked fragments leave this module.

use super::{repository, types::*};
use crate::{
    agents::DiscoveryEnvironment,
    config_apply::OperationKind,
    config_plan::{SECRET_PLACEHOLDER, TargetFile, digest_hex, read_target_bytes},
    model_catalog::{EffortLevel, ThinkingEffort},
    models::ProviderModel,
    providers::{ProviderKind, ProviderProtocol, ProviderRecord, base_url_for_protocol},
    storage::Storage,
};
use serde_json::{Map, Value, json};
use std::path::{Path, PathBuf};
use toml_edit::{DocumentMut, Item, Table, value};

/// Native file below `$CODEX_HOME`.
pub(super) const CONFIG_FILE: &str = "config.toml";
/// Provider IDs built into Codex 0.165.0; a user table with one of these keys
/// would be rejected or shadow the built-in.
pub(crate) const RESERVED_PROVIDER_IDS: [&str; 6] = [
    "openai",
    "amazon-bedrock",
    "amazon-bedrock-runtime",
    "gcp",
    "ollama",
    "lmstudio",
];
/// Every owned provider key and display name carries this prefix.
pub(crate) const KEY_PREFIX: &str = "vibemate-";
/// DeepSeek models whose Responses route is documented for Codex.
pub(crate) const DEEPSEEK_RESPONSES_MODELS: [&str; 2] = ["deepseek-flash", "deepseek-v4-pro"];
/// Plaintext credential field (user decision: no keychain helper in v1).
const TOKEN_FIELD: &str = "experimental_bearer_token";
/// Longest slug taken from the provider display name.
const SLUG_LIMIT: usize = 40;
/// Longest model id the default binding stores.
const MAX_MODEL_ID: usize = 160;

pub(super) struct Built {
    pub view: Preview,
    pub root: PathBuf,
    /// Full document; deploy fills the credential before serializing.
    pub document: DocumentMut,
    pub original_bytes: Vec<u8>,
    pub before: Owners,
    /// Ownership to publish; digests/generations needing the credential or the
    /// change id are completed by the service.
    pub after: Owners,
    /// Central provider; loaded only for `deploy` and `set_default`, which
    /// read central content. `revoke`/`clear_default` act on owned rows only.
    pub provider: Option<ProviderRecord>,
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
pub(super) fn digest(value: &Value) -> Result<String, Error> {
    serde_json::to_vec(value)
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
    if let Some(providers) = document.get("model_providers")
        && !providers.is_table_like()
    {
        return Err(Error::InvalidConfig);
    }
    Ok(document)
}

/// Read `config.toml`, refusing a symbolic link, a read-only file and a
/// missing `$CODEX_HOME` (vibemate never creates Codex's state directory).
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

fn value_json(value: &toml_edit::Value) -> Value {
    match value {
        toml_edit::Value::String(v) => json!(v.value()),
        toml_edit::Value::Integer(v) => json!(v.value()),
        toml_edit::Value::Float(v) => json!(v.value()),
        toml_edit::Value::Boolean(v) => json!(v.value()),
        toml_edit::Value::Datetime(v) => json!(v.value().to_string()),
        toml_edit::Value::Array(v) => Value::Array(v.iter().map(value_json).collect()),
        toml_edit::Value::InlineTable(v) => {
            sorted(v.iter().map(|(k, v)| (k.to_owned(), value_json(v))))
        }
    }
}
fn sorted(entries: impl Iterator<Item = (String, Value)>) -> Value {
    let mut entries: Vec<_> = entries.collect();
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    Value::Object(entries.into_iter().collect::<Map<_, _>>())
}

/// Semantic JSON of any TOML item with sorted keys: formatting, key order and
/// inline vs. standard tables digest identically, while every key (including
/// ones vibemate never writes, such as `env_key` or `auth`) is covered.
pub(crate) fn item_json(item: &Item) -> Value {
    match item {
        Item::None => Value::Null,
        Item::Value(v) => value_json(v),
        Item::Table(t) => sorted(t.iter().map(|(k, v)| (k.to_owned(), item_json(v)))),
        Item::ArrayOfTables(a) => Value::Array(
            a.iter()
                .map(|t| sorted(t.iter().map(|(k, v)| (k.to_owned(), item_json(v)))))
                .collect(),
        ),
    }
}

pub(super) fn provider_item<'a>(doc: &'a DocumentMut, key: &str) -> Option<&'a Item> {
    doc.get("model_providers")
        .and_then(Item::as_table_like)
        .and_then(|providers| providers.get(key))
}

/// Mask the credential and any secret-looking key in a canonical entry.
pub(super) fn mask(entry: &Value) -> Value {
    let mut value = entry.clone();
    if let Some(map) = value.as_object_mut() {
        for (key, field) in map.iter_mut() {
            if key == TOKEN_FIELD || crate::config_apply::version_text::is_secret_key(key) {
                *field = json!(SECRET_PLACEHOLDER);
            }
        }
        // A `base_url` may carry `?api_key=` or userinfo.
        crate::secret_text::mask_public_fields(map);
    }
    value
}

/// Codex 0.165.0 speaks only `wire_api = "responses"`. The provider's base URL
/// is reused when it already targets Responses; a template URL is mapped to the
/// template Responses URL; a custom URL for another protocol is unverified.
pub(crate) fn responses_base(provider: &ProviderRecord) -> Result<String, Error> {
    if !provider
        .kind
        .template()
        .protocols
        .contains(&ProviderProtocol::Responses)
    {
        return Err(Error::ProtocolUnsupported);
    }
    if provider.protocol == ProviderProtocol::Responses {
        return Ok(provider.base_url.clone());
    }
    if provider.base_url == base_url_for_protocol(provider.kind, provider.protocol) {
        return Ok(base_url_for_protocol(provider.kind, ProviderProtocol::Responses).to_owned());
    }
    Err(Error::BaseUrlUnverified)
}

/// Whether Codex can use this model through the provider's Responses route.
pub(crate) fn model_supported(provider: &ProviderRecord, model: &ProviderModel) -> bool {
    if !model.selected || model.provider_id != provider.id.as_str() {
        return false;
    }
    let route = crate::model_identity::protocol_endpoint(ProviderProtocol::Responses);
    let declares = |endpoints: &Option<Vec<String>>| {
        endpoints
            .as_ref()
            .map(|list| list.iter().any(|e| e == route))
    };
    match provider.kind {
        // Only models from DeepSeek's Codex guide (user decision 9).
        ProviderKind::DeepSeek => DEEPSEEK_RESPONSES_MODELS.contains(&model.model_id.as_str()),
        // Command Code routes per model; require an explicit `/responses` route.
        ProviderKind::CommandCode => declares(&model.supported_endpoints) == Some(true),
        // OpenRouter's Responses API covers its catalog unless a model's
        // endpoint list says otherwise.
        ProviderKind::OpenRouter => declares(&model.supported_endpoints) != Some(false),
    }
}

/// Codex reasoning value for the central default effort, if one is stated.
pub(crate) fn reasoning_effort(model: &ProviderModel) -> Option<&'static str> {
    let level = match model.thinking_effort.as_ref()? {
        ThinkingEffort::Any { default_level } | ThinkingEffort::Listed { default_level, .. } => {
            (*default_level)?
        }
    };
    // Codex 0.165.0's ReasoningEffort accepts every central level name.
    Some(match level {
        EffortLevel::Max => "max",
        EffortLevel::XHigh => "xhigh",
        EffortLevel::High => "high",
        EffortLevel::Medium => "medium",
        EffortLevel::Low => "low",
        EffortLevel::Minimal => "minimal",
        EffortLevel::Off => "none",
    })
}

fn slug(text: &str) -> Option<String> {
    let mut out = String::new();
    for character in text.chars() {
        if out.len() >= SLUG_LIMIT {
            break;
        }
        if character.is_ascii_alphanumeric() {
            out.push(character.to_ascii_lowercase());
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    let out = out.trim_matches('-').to_owned();
    (!out.is_empty()).then_some(out)
}

/// Pick `vibemate-<slug>`; if another provider in this root owns it, append
/// the first six hex digits of this provider's ID. A table vibemate does not
/// own, or a reserved Codex ID, is a conflict and is never taken over.
fn choose_key(
    storage: &Storage,
    root: &str,
    provider: &ProviderRecord,
    doc: &DocumentMut,
) -> Result<String, Error> {
    let base = format!(
        "{KEY_PREFIX}{}",
        slug(&provider.display_name).unwrap_or_else(|| provider.kind.as_str().to_owned())
    );
    let key = match repository::key_owner(storage, root, &base)? {
        None => base,
        Some(_) => {
            let alternative = format!("{base}-{}", &provider.id.as_str()[..6]);
            if repository::key_owner(storage, root, &alternative)?.is_some() {
                return Err(Error::NameConflict);
            }
            alternative
        }
    };
    if RESERVED_PROVIDER_IDS.contains(&key.as_str()) || provider_item(doc, &key).is_some() {
        return Err(Error::NameConflict);
    }
    Ok(key)
}

/// Proposed table; the credential stays masked until apply.
fn proposed_entry(provider: &ProviderRecord, base: &str) -> Value {
    let mut entry = json!({
        "base_url": base,
        TOKEN_FIELD: SECRET_PLACEHOLDER,
        "name": format!("{KEY_PREFIX}{}", provider.display_name),
        "wire_api": "responses",
    });
    if provider.kind == ProviderKind::OpenRouter {
        // OpenRouter's Codex Desktop guide disables the WebSocket transport.
        entry["supports_websockets"] = json!(false);
    }
    sorted(
        entry
            .as_object()
            .into_iter()
            .flatten()
            .map(|(k, v)| (k.clone(), v.clone())),
    )
}

/// Write the owned table, keeping an existing table's position and leading
/// comment. Only mapped keys are emitted; never `env_key`, `auth` or `profile`.
pub(super) fn put_provider(doc: &mut DocumentMut, key: &str, entry: &Value) -> Result<(), Error> {
    let object = entry.as_object().ok_or(Error::InvalidConfig)?;
    let providers = match doc.get_mut("model_providers") {
        Some(item) => item.as_table_mut().ok_or(Error::InvalidConfig)?,
        None => {
            let mut table = Table::new();
            // Keep the parent implicit so only `[model_providers.<key>]` appears.
            table.set_implicit(true);
            doc.insert("model_providers", Item::Table(table));
            doc.get_mut("model_providers")
                .and_then(Item::as_table_mut)
                .ok_or(Error::InvalidConfig)?
        }
    };
    let mut table = Table::new();
    if let Some(old) = providers.get(key).and_then(Item::as_table) {
        *table.decor_mut() = old.decor().clone();
        if let Some(position) = old.position() {
            table.set_position(Some(position));
        }
    }
    for field in ["name", "base_url", "wire_api", TOKEN_FIELD] {
        let text = object
            .get(field)
            .and_then(Value::as_str)
            .ok_or(Error::InvalidConfig)?;
        table.insert(field, value(text));
    }
    if let Some(flag) = object.get("supports_websockets") {
        table.insert(
            "supports_websockets",
            value(flag.as_bool().ok_or(Error::InvalidConfig)?),
        );
    }
    providers.insert(key, Item::Table(table));
    Ok(())
}

/// Fill the plaintext credential into an already-planned owned table.
pub(super) fn fill_token(doc: &mut DocumentMut, key: &str, token: &str) -> Result<(), Error> {
    doc.get_mut("model_providers")
        .and_then(Item::as_table_mut)
        .and_then(|providers| providers.get_mut(key))
        .and_then(Item::as_table_mut)
        .ok_or(Error::InvalidConfig)?
        .insert(TOKEN_FIELD, value(token));
    Ok(())
}

pub(super) fn remove_provider(doc: &mut DocumentMut, key: &str) {
    if let Some(providers) = doc.get_mut("model_providers").and_then(Item::as_table_mut) {
        providers.remove(key);
        if providers.is_empty() {
            doc.remove("model_providers");
        }
    }
}

/// Current top-level default keys; a non-string value is invalid for Codex.
pub(super) fn current_defaults(doc: &DocumentMut) -> Result<DefaultValues, Error> {
    let text = |key: &str| -> Result<Option<String>, Error> {
        doc.get(key)
            .map(|item| item.as_str().map(str::to_owned).ok_or(Error::InvalidConfig))
            .transpose()
    };
    Ok(DefaultValues {
        model: text("model")?,
        model_provider: text("model_provider")?,
        model_reasoning_effort: text("model_reasoning_effort")?,
    })
}

pub(super) fn defaults_json(values: &DefaultValues) -> Value {
    json!({
        "model": values.model,
        "model_provider": values.model_provider,
        "model_reasoning_effort": values.model_reasoning_effort,
    })
}

/// The part of `values` a default owns. `model_reasoning_effort` belongs to
/// vibemate only when it wrote one (`writes_effort`); otherwise it is the
/// user's value, so changing it neither marks the default `modified` nor gets
/// restored by `clear_default`.
pub(super) fn owned_defaults(values: &DefaultValues, writes_effort: bool) -> DefaultValues {
    DefaultValues {
        model: values.model.clone(),
        model_provider: values.model_provider.clone(),
        model_reasoning_effort: values
            .model_reasoning_effort
            .clone()
            .filter(|_| writes_effort),
    }
}

/// Digest of the owned part of `values`, compared with `DefaultOwner::entry_digest`.
pub(super) fn owned_defaults_digest(
    values: &DefaultValues,
    writes_effort: bool,
) -> Result<String, Error> {
    digest(&defaults_json(&owned_defaults(values, writes_effort)))
}

/// Set or remove `model` and `model_provider` (and `model_reasoning_effort`
/// only with `writes_effort`), keeping each existing value's position and
/// trailing comment. An effort vibemate does not own is never touched.
fn write_defaults(doc: &mut DocumentMut, values: &DefaultValues, writes_effort: bool) {
    let effort = [("model_reasoning_effort", &values.model_reasoning_effort)];
    let effort = if writes_effort { &effort[..] } else { &[] };
    for &(key, wanted) in [
        ("model", &values.model),
        ("model_provider", &values.model_provider),
    ]
    .iter()
    .chain(effort)
    {
        match wanted {
            Some(text) => match doc.get_mut(key).and_then(Item::as_value_mut) {
                Some(existing) => {
                    let decor = existing.decor().clone();
                    *existing = text.as_str().into();
                    *existing.decor_mut() = decor;
                }
                None => {
                    doc.insert(key, value(text.as_str()));
                }
            },
            None => {
                doc.remove(key);
            }
        }
    }
}

pub(super) fn build(
    storage: &Storage,
    environment: &DiscoveryEnvironment,
    selection: &Selection,
) -> Result<Built, Error> {
    let model_ok = |id: &String| {
        !id.is_empty() && id.chars().count() <= MAX_MODEL_ID && !id.chars().any(char::is_control)
    };
    if !valid_id(&selection.provider_id)
        || selection.expected_revision < 1
        || (selection.action == Action::SetDefault) != selection.model_id.is_some()
        || selection.model_id.as_ref().is_some_and(|id| !model_ok(id))
    {
        return Err(Error::InvalidRequest);
    }
    // Deploy/set_default copy central content, so they pin the central
    // revision. Revoke/clear_default only undo owned state, so they pin the
    // owned binding revision and never need the central row.
    let central = match selection.action {
        Action::Deploy | Action::SetDefault => {
            let provider = crate::providers::get_provider(storage, &selection.provider_id)
                .map_err(|_| Error::ProviderChanged)?;
            if provider.revision != selection.expected_revision {
                return Err(Error::ProviderChanged);
            }
            Some(provider)
        }
        Action::Revoke | Action::ClearDefault => None,
    };
    let root = root(environment)?;
    let root_key = root_text(&root)?;
    if repository::pending(storage, root_key)?.is_some()
        || repository::mcp_pending(storage, root_key)?
    {
        return Err(Error::RecoveryRequired);
    }
    let provider_id = selection.provider_id.as_str();
    let before = repository::owners(storage, root_key, provider_id)?;
    let reviewed_revision = match &central {
        Some(provider) => provider.revision,
        None => {
            if selection.action == Action::ClearDefault {
                let owner = before.default.as_ref().ok_or(Error::NotDeployed)?;
                if owner.provider_id != provider_id {
                    return Err(Error::InvalidRequest);
                }
            }
            // A default always has its provider table binding: revoke is
            // refused while the provider is the default.
            let owned = before.provider.as_ref().ok_or(Error::NotDeployed)?;
            if owned.provider_revision != selection.expected_revision {
                return Err(Error::ProviderChanged);
            }
            owned.provider_revision
        }
    };
    let (target, bytes, mut doc) = read(&root)?;
    let mut notices = Vec::new();
    let mut after = before.clone();
    let mut model_digest = None;
    let mut writes_file = true;
    // Owned table state: absent, or present and matching the recorded digest.
    let owned_entry = |doc: &DocumentMut, owner: &ProviderOwner| -> Result<Option<Value>, Error> {
        match provider_item(doc, &owner.provider_key) {
            None => Ok(None),
            Some(item) => {
                let entry = item_json(item);
                if digest(&entry)? != owner.entry_digest {
                    return Err(Error::OwnershipConflict);
                }
                Ok(Some(entry))
            }
        }
    };
    let (key, before_fragment, after_fragment) = match selection.action {
        Action::Deploy => {
            let provider = central.as_ref().ok_or(Error::ProviderChanged)?;
            let base = responses_base(provider)?;
            let (key, before_entry) = match &before.provider {
                // A missing owned table is recreated under the same key.
                Some(owner) => (owner.provider_key.clone(), owned_entry(&doc, owner)?),
                None => (choose_key(storage, root_key, provider, &doc)?, None),
            };
            let entry = proposed_entry(provider, &base);
            put_provider(&mut doc, &key, &entry)?;
            notices.push(Notice::PlaintextCredential);
            after.provider = Some(ProviderOwner {
                provider_key: key.clone(),
                provider_revision: provider.revision,
                entry_digest: String::new(),
                generation: String::new(),
            });
            let wrap = |e: Value| json!({"model_providers": {&key: mask(&e)}});
            (key.clone(), before_entry.map(wrap), Some(wrap(entry)))
        }
        Action::Revoke => {
            let owner = before.provider.clone().ok_or(Error::NotDeployed)?;
            let current = current_defaults(&doc)?;
            if before
                .default
                .as_ref()
                .is_some_and(|d| d.provider_id == provider_id)
                || current.model_provider.as_deref() == Some(owner.provider_key.as_str())
            {
                return Err(Error::DefaultInUse);
            }
            let before_entry = owned_entry(&doc, &owner)?;
            writes_file = before_entry.is_some();
            remove_provider(&mut doc, &owner.provider_key);
            after.provider = None;
            let key = owner.provider_key;
            let fragment = before_entry.map(|e| json!({"model_providers": {&key: mask(&e)}}));
            (key, fragment, None)
        }
        Action::SetDefault => {
            let provider = central.as_ref().ok_or(Error::ProviderChanged)?;
            let owner = before.provider.clone().ok_or(Error::NotDeployed)?;
            owned_entry(&doc, &owner)?.ok_or(Error::OwnershipConflict)?;
            let model_id = selection.model_id.clone().ok_or(Error::InvalidRequest)?;
            let snapshot = crate::models::get_model_snapshot(
                storage,
                &crate::models::GetModelSnapshotRequest {
                    provider_id: provider.id.as_str().to_owned(),
                    model_id: model_id.clone(),
                },
            )
            .map_err(|_| Error::ModelUnsupported)?;
            if !model_supported(provider, &snapshot.model) {
                return Err(Error::ModelUnsupported);
            }
            model_digest = Some(snapshot.content_digest.clone());
            let current = current_defaults(&doc)?;
            // Keep the values from before vibemate's first default while the
            // file still shows vibemate's default; otherwise the current values
            // (possibly chosen with Codex `/model`) become the restore point.
            let mut kept = None;
            if let Some(owner) = &before.default
                && owned_defaults_digest(&current, owner.writes_effort)? == owner.entry_digest
            {
                kept = Some(owner);
            }
            let mut previous = kept.map_or_else(|| current.clone(), |o| o.previous.clone());
            if kept.is_some_and(|o| !o.writes_effort) {
                // The effort was never vibemate's: the user's latest value is
                // the restore point, not the one seen at the first default.
                previous.model_reasoning_effort = current.model_reasoning_effort.clone();
            }
            // Reasoning effort is written only when the central model states
            // one. Otherwise the user's `model_reasoning_effort` stays as is;
            // only an effort vibemate itself wrote for the replaced default is
            // put back to the value from before vibemate, so it cannot linger.
            let effort = reasoning_effort(&snapshot.model).map(str::to_owned);
            let writes_effort = effort.is_some();
            let restores_effort = !writes_effort && kept.is_some_and(|o| o.writes_effort);
            let wanted = DefaultValues {
                model: Some(model_id.clone()),
                model_provider: Some(owner.provider_key.clone()),
                model_reasoning_effort: if restores_effort {
                    previous.model_reasoning_effort.clone()
                } else if writes_effort {
                    effort
                } else {
                    current.model_reasoning_effort.clone()
                },
            };
            write_defaults(&mut doc, &wanted, writes_effort || restores_effort);
            if doc.get("profile").is_some() {
                notices.push(Notice::ProfileOverridesDefault);
            }
            after.default = Some(DefaultOwner {
                provider_id: provider_id.to_owned(),
                model_id,
                provider_key: owner.provider_key.clone(),
                entry_digest: owned_defaults_digest(&wanted, writes_effort)?,
                previous,
                writes_effort,
                generation: String::new(),
            });
            (
                owner.provider_key,
                Some(defaults_json(&current)),
                Some(defaults_json(&wanted)),
            )
        }
        Action::ClearDefault => {
            let owner = before.default.clone().ok_or(Error::NotDeployed)?;
            // Restore only over vibemate's exact default. `modified` and
            // `missing` defaults release ownership without writing, so a
            // value chosen or removed outside vibemate is never overwritten
            // or re-added; non-string keys fail as `invalid_config`.
            let current = current_defaults(&doc)?;
            let unchanged =
                owned_defaults_digest(&current, owner.writes_effort)? == owner.entry_digest;
            let after_values = if unchanged {
                // Only the keys vibemate wrote go back; a user-owned effort
                // stays whatever it is now.
                write_defaults(&mut doc, &owner.previous, owner.writes_effort);
                DefaultValues {
                    model_reasoning_effort: if owner.writes_effort {
                        owner.previous.model_reasoning_effort.clone()
                    } else {
                        current.model_reasoning_effort.clone()
                    },
                    ..owner.previous.clone()
                }
            } else {
                notices.push(Notice::ReleaseOnly);
                writes_file = false;
                current.clone()
            };
            after.default = None;
            (
                owner.provider_key,
                Some(defaults_json(&current)),
                Some(defaults_json(&after_values)),
            )
        }
    };
    let as_bytes = |fragment: &Option<Value>| {
        fragment
            .as_ref()
            .map(serde_json::to_vec)
            .transpose()
            .map_err(|_| Error::InvalidConfig)
    };
    let summary = crate::config_apply::version_text::summarize(
        OperationKind::CodexSettings,
        as_bytes(&before_fragment)?.as_deref(),
        as_bytes(&after_fragment)?.as_deref(),
    );
    let last_change = repository::last_change_id(storage, root_key)?;
    let approval = digest_hex(
        &serde_json::to_vec(&json!({
            "selection": selection,
            "target": target,
            "owners": before,
            "lastChange": last_change,
            "providerRevision": reviewed_revision,
            "model": model_digest,
            "before": before_fragment,
            "after": after_fragment,
            "notices": notices,
            "writesFile": writes_file,
        }))
        .map_err(|_| Error::InvalidRequest)?,
    );
    let view = Preview {
        target,
        provider_id: provider_id.to_owned(),
        provider_revision: reviewed_revision,
        action: selection.action,
        provider_key: key,
        model_id: selection.model_id.clone(),
        before: before_fragment,
        after: after_fragment,
        summary,
        notices,
        writes_file,
        approval,
    };
    Ok(Built {
        view,
        root,
        document: doc,
        original_bytes: bytes,
        before,
        after,
        provider: central,
    })
}
