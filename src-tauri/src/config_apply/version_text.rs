//! Shared sanitized field summaries and full text for configuration previews (H2).
//!
//! The UI defaults to a field-level view: every changed path, whether it was
//! added, removed or replaced, and the old/new display value. Credentials are
//! masked anywhere in the document by key name, not only in fields vibemate
//! owns. The detail dialog asks for complete text separately; both paths use
//! the same sanitizer, so a value hidden in the summary cannot appear in the
//! full text.
//!
//! JSON text is re-rendered pretty-printed (JSON has no comments or layout to
//! preserve). TOML uses `toml_edit` so comments and key order survive. Text is
//! bounded before and after sanitization by the same 1 MiB target limit;
//! an unparseable document is reported as unavailable rather than returned
//! unsanitized.

use serde::Serialize;
use serde_json::{Map, Value};
use toml_edit::{DocumentMut, Item, Value as TomlValue};

use super::OperationKind;
use crate::config_plan::{MAX_TARGET_BYTES, SECRET_PLACEHOLDER};

/// Most field changes one file summary lists before setting `truncated`.
pub(super) const MAX_SUMMARY_CHANGES: usize = 200;

/// Stable reason when a document cannot be parsed or safely sanitized.
pub(super) const TEXT_UNAVAILABLE: &str = "text_unavailable";

/// Format the detail dialog renders.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfigFormat {
    /// Claude Code and Pi configuration.
    Json,
    /// Grok Build configuration.
    Toml,
}

/// How one field differs between the current and retained documents.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FieldChangeKind {
    /// The field exists only in the retained version.
    Added,
    /// The field exists only in the current version.
    Removed,
    /// Both exist with different values.
    Replaced,
}

/// One sanitized field-level change.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FieldChange {
    /// Dotted/indexed path inside the document.
    pub path: String,
    /// Direction of the change.
    pub kind: FieldChangeKind,
    /// Current-side display text; `None` means the field is absent.
    pub before: Option<String>,
    /// Retained-side display text; `None` means the field is absent.
    pub after: Option<String>,
    /// Whether the displayed values are placeholders for a credential.
    pub secret: bool,
}

/// Sanitized field-level preview for one file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionFileSummary {
    /// Document format for the detail dialog.
    pub format: ConfigFormat,
    /// Field changes, in document order.
    pub changes: Vec<FieldChange>,
    /// True when the cap omitted further changes; never silently truncated.
    pub truncated: bool,
    /// Safe reason the summary cannot be produced, when applicable.
    pub reason: Option<String>,
}

/// One side of the detail dialog.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", tag = "state")]
pub enum VersionTextSide {
    /// The file did not exist on this side.
    Absent,
    /// Sanitized complete text and the original byte count.
    Present {
        /// Sanitized text, valid for the reported format.
        text: String,
        /// Original file size in bytes; the text may be longer or shorter.
        bytes: u64,
    },
    /// The side is present but cannot be shown safely.
    Unavailable {
        /// Stable reason, never raw content.
        reason: String,
    },
}

/// Complete sanitized current and retained text for one file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionText {
    /// Revision whose pre-mutation bytes are the target.
    pub revision_id: String,
    /// Absolute target path.
    pub target_path: String,
    /// Document format.
    pub format: ConfigFormat,
    /// Current file side.
    pub current: VersionTextSide,
    /// Retained pre-mutation side.
    pub target: VersionTextSide,
}

/// Summary of one file, or a safe reason when it cannot be produced.
pub(crate) fn summarize(
    kind: OperationKind,
    current: Option<&[u8]>,
    target: Option<&[u8]>,
) -> VersionFileSummary {
    let format = format_for(kind);
    let current = match current.map(|bytes| parse_node(format, bytes)).transpose() {
        Ok(node) => node,
        Err(reason) => return unavailable(format, reason),
    };
    let target = match target.map(|bytes| parse_node(format, bytes)).transpose() {
        Ok(node) => node,
        Err(reason) => return unavailable(format, reason),
    };
    let mut changes = Vec::new();
    let mut truncated = false;
    diff_node(
        current.as_ref(),
        target.as_ref(),
        "",
        false,
        &mut changes,
        &mut truncated,
    );
    VersionFileSummary {
        format,
        changes,
        truncated,
        reason: None,
    }
}

/// Sanitize one complete document for the detail dialog.
///
/// Returns `None` for an absent file and a safe reason when the bytes are not
/// valid UTF-8/JSON/TOML. The returned text never contains a value under a
/// credential-like key.
pub(crate) fn render_text(
    format: ConfigFormat,
    bytes: &[u8],
) -> Result<VersionTextSide, &'static str> {
    if bytes.len() as u64 > MAX_TARGET_BYTES {
        return Err(TEXT_UNAVAILABLE);
    }
    let text = match format {
        ConfigFormat::Json => {
            let mut value = parse_json(bytes)?;
            sanitize_json(&mut value);
            if value.as_object().is_some_and(Map::is_empty) && is_blank(bytes) {
                String::new()
            } else {
                serde_json::to_string_pretty(&value).map_err(|_| TEXT_UNAVAILABLE)?
            }
        }
        ConfigFormat::Toml => {
            let mut document = parse_toml(bytes)?;
            sanitize_toml(&mut document);
            document.to_string()
        }
    };
    // Pretty printing can expand a bounded input substantially. Never return
    // truncated JSON/TOML or let that expansion bypass the display limit.
    if text.len() as u64 > MAX_TARGET_BYTES {
        return Err(TEXT_UNAVAILABLE);
    }
    Ok(VersionTextSide::Present {
        text,
        bytes: u64::try_from(bytes.len()).unwrap_or(u64::MAX),
    })
}

/// Format used by one Agent adapter.
pub(crate) fn format_for(kind: OperationKind) -> ConfigFormat {
    match kind {
        // Entry-level MCP summaries use JSON fragments for both Agents; the
        // Grok native file remains TOML, but the reviewed entry shape matches
        // the masked JSON already shown in the Grok MCP details dialog.
        OperationKind::ClaudeCodeSettings
        | OperationKind::PiSettings
        | OperationKind::PiMcp
        | OperationKind::GrokMcp => ConfigFormat::Json,
        OperationKind::GrokSettings => ConfigFormat::Toml,
    }
}

fn unavailable(format: ConfigFormat, reason: &'static str) -> VersionFileSummary {
    VersionFileSummary {
        format,
        changes: Vec::new(),
        truncated: false,
        reason: Some(reason.to_owned()),
    }
}

/// A summary that explicitly explains why no field list is shown.
pub(super) fn unavailable_summary(kind: OperationKind, reason: &str) -> VersionFileSummary {
    VersionFileSummary {
        format: format_for(kind),
        changes: Vec::new(),
        truncated: false,
        reason: Some(reason.to_owned()),
    }
}

/// A format-independent document tree used only for diffing.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Node {
    Null,
    Bool(bool),
    Number(String),
    String(String),
    Array(Vec<Node>),
    Object(Vec<(String, Node)>),
}

fn parse_node(format: ConfigFormat, bytes: &[u8]) -> Result<Node, &'static str> {
    match format {
        ConfigFormat::Json => Ok(node_from_json(&parse_json(bytes)?)),
        ConfigFormat::Toml => Ok(node_from_toml(&parse_toml(bytes)?)),
    }
}

fn parse_json(bytes: &[u8]) -> Result<Value, &'static str> {
    if is_blank(bytes) {
        return Ok(Value::Object(Map::new()));
    }
    serde_json::from_slice(bytes).map_err(|_| TEXT_UNAVAILABLE)
}

fn parse_toml(bytes: &[u8]) -> Result<DocumentMut, &'static str> {
    let text = std::str::from_utf8(bytes).map_err(|_| TEXT_UNAVAILABLE)?;
    if text.trim().is_empty() {
        return Ok(DocumentMut::new());
    }
    text.parse::<DocumentMut>().map_err(|_| TEXT_UNAVAILABLE)
}

fn is_blank(bytes: &[u8]) -> bool {
    bytes.iter().all(u8::is_ascii_whitespace)
}

fn node_from_json(value: &Value) -> Node {
    match value {
        Value::Null => Node::Null,
        Value::Bool(value) => Node::Bool(*value),
        Value::Number(value) => Node::Number(value.to_string()),
        Value::String(value) => Node::String(value.clone()),
        Value::Array(items) => Node::Array(items.iter().map(node_from_json).collect()),
        Value::Object(map) => Node::Object(
            map.iter()
                .map(|(key, value)| (key.clone(), node_from_json(value)))
                .collect(),
        ),
    }
}

fn node_from_toml(document: &DocumentMut) -> Node {
    Node::Object(
        document
            .as_table()
            .iter()
            .filter_map(|(key, item)| node_from_item(item).map(|node| (key.to_owned(), node)))
            .collect(),
    )
}

fn node_from_item(item: &Item) -> Option<Node> {
    if item.is_none() {
        return None;
    }
    if let Some(table) = item.as_table_like() {
        return Some(Node::Object(
            table
                .iter()
                .filter_map(|(key, item)| node_from_item(item).map(|node| (key.to_owned(), node)))
                .collect(),
        ));
    }
    item.as_value().map(node_from_toml_value)
}

fn node_from_toml_value(value: &TomlValue) -> Node {
    match value {
        TomlValue::String(value) => Node::String(value.value().clone()),
        TomlValue::Integer(value) => Node::Number(value.value().to_string()),
        TomlValue::Float(value) => Node::Number(value.value().to_string()),
        TomlValue::Boolean(value) => Node::Bool(*value.value()),
        TomlValue::Datetime(value) => Node::String(value.value().to_string()),
        TomlValue::Array(items) => Node::Array(items.iter().map(node_from_toml_value).collect()),
        TomlValue::InlineTable(table) => Node::Object(
            table
                .iter()
                .map(|(key, value)| (key.to_owned(), node_from_toml_value(value)))
                .collect(),
        ),
    }
}

/// Compare current (before) with the retained version (after).
fn diff_node(
    before: Option<&Node>,
    after: Option<&Node>,
    path: &str,
    secret: bool,
    changes: &mut Vec<FieldChange>,
    truncated: &mut bool,
) {
    if *truncated {
        return;
    }
    if secret {
        if before == after {
            return;
        }
        push_change(
            changes,
            truncated,
            FieldChange {
                path: path.to_owned(),
                kind: change_kind(before, after),
                before: before.map(|_| SECRET_PLACEHOLDER.to_owned()),
                after: after.map(|_| SECRET_PLACEHOLDER.to_owned()),
                secret: true,
            },
        );
        return;
    }
    match (before, after) {
        (None, None) => {}
        (Some(Node::Object(left)), Some(Node::Object(right))) => {
            for (key, value) in left {
                let child = right
                    .iter()
                    .find(|(other, _)| other == key)
                    .map(|(_, node)| node);
                diff_node(
                    Some(value),
                    child,
                    &key_path(path, key),
                    is_secret_key(key),
                    changes,
                    truncated,
                );
            }
            for (key, value) in right {
                if !left.iter().any(|(other, _)| other == key) {
                    diff_node(
                        None,
                        Some(value),
                        &key_path(path, key),
                        is_secret_key(key),
                        changes,
                        truncated,
                    );
                }
            }
        }
        (Some(Node::Array(left)), Some(Node::Array(right))) => {
            for index in 0..left.len().max(right.len()) {
                diff_node(
                    left.get(index),
                    right.get(index),
                    &index_path(path, index),
                    false,
                    changes,
                    truncated,
                );
            }
        }
        (Some(Node::Object(_) | Node::Array(_)), None) => {
            remove_children(before, path, changes, truncated);
        }
        (None, Some(Node::Object(_) | Node::Array(_))) => {
            add_children(after, path, changes, truncated);
        }
        (Some(left), Some(right)) if left == right => {}
        // A container changing type is shown as its removed and added leaves,
        // never as a serialized value that could contain a nested credential.
        (Some(left), Some(right)) if is_container(left) || is_container(right) => {
            remove_children(Some(left), path, changes, truncated);
            add_children(Some(right), path, changes, truncated);
        }
        (left, right) => push_change(
            changes,
            truncated,
            FieldChange {
                path: path.to_owned(),
                kind: change_kind(left, right),
                before: left.map(node_text),
                after: right.map(node_text),
                secret: false,
            },
        ),
    }
}

fn remove_children(
    node: Option<&Node>,
    path: &str,
    changes: &mut Vec<FieldChange>,
    truncated: &mut bool,
) {
    match node {
        Some(Node::Object(entries)) => {
            for (key, value) in entries {
                diff_node(
                    Some(value),
                    None,
                    &key_path(path, key),
                    is_secret_key(key),
                    changes,
                    truncated,
                );
            }
        }
        Some(Node::Array(items)) => {
            for (index, value) in items.iter().enumerate() {
                diff_node(
                    Some(value),
                    None,
                    &index_path(path, index),
                    false,
                    changes,
                    truncated,
                );
            }
        }
        Some(value) => push_change(
            changes,
            truncated,
            FieldChange {
                path: path.to_owned(),
                kind: FieldChangeKind::Removed,
                before: Some(node_text(value)),
                after: None,
                secret: false,
            },
        ),
        None => {}
    }
}

fn add_children(
    node: Option<&Node>,
    path: &str,
    changes: &mut Vec<FieldChange>,
    truncated: &mut bool,
) {
    match node {
        Some(Node::Object(entries)) => {
            for (key, value) in entries {
                diff_node(
                    None,
                    Some(value),
                    &key_path(path, key),
                    is_secret_key(key),
                    changes,
                    truncated,
                );
            }
        }
        Some(Node::Array(items)) => {
            for (index, value) in items.iter().enumerate() {
                diff_node(
                    None,
                    Some(value),
                    &index_path(path, index),
                    false,
                    changes,
                    truncated,
                );
            }
        }
        Some(value) => push_change(
            changes,
            truncated,
            FieldChange {
                path: path.to_owned(),
                kind: FieldChangeKind::Added,
                before: None,
                after: Some(node_text(value)),
                secret: false,
            },
        ),
        None => {}
    }
}

fn is_container(node: &Node) -> bool {
    matches!(node, Node::Array(_) | Node::Object(_))
}

fn push_change(changes: &mut Vec<FieldChange>, truncated: &mut bool, mut change: FieldChange) {
    if changes.len() >= MAX_SUMMARY_CHANGES {
        *truncated = true;
        return;
    }
    // An external edit can replace the whole JSON object with a scalar. Use
    // the JSON root marker instead of an empty, invalid IPC field path.
    if change.path.is_empty() {
        change.path = "$".to_owned();
    }
    changes.push(change);
}

fn change_kind(before: Option<&Node>, after: Option<&Node>) -> FieldChangeKind {
    match (before, after) {
        (None, Some(_)) => FieldChangeKind::Added,
        (Some(_), None) => FieldChangeKind::Removed,
        _ => FieldChangeKind::Replaced,
    }
}

/// Display text for a scalar value; containers are never emitted unsanitized.
fn node_text(node: &Node) -> String {
    match node {
        Node::Null => "null".to_owned(),
        Node::Bool(value) => value.to_string(),
        Node::Number(value) | Node::String(value) => value.clone(),
        Node::Array(_) | Node::Object(_) => "[complex value]".to_owned(),
    }
}

fn key_path(base: &str, key: &str) -> String {
    let plain = key
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || matches!(character, '_' | '-'));
    if plain {
        if base.is_empty() {
            key.to_owned()
        } else {
            format!("{base}.{key}")
        }
    } else {
        let quoted = serde_json::to_string(key).unwrap_or_else(|_| "\"?\"".to_owned());
        format!("{base}[{quoted}]")
    }
}

fn index_path(base: &str, index: usize) -> String {
    format!("{base}[{index}]")
}

/// Whether a key names a credential value anywhere in a configuration.
///
/// The rule is deliberately conservative but avoids common non-secret names:
/// `max_tokens`, `input_tokens` and `env_key` (a reference) are not masked,
/// while `api_key`, `x-api-key`, `ANTHROPIC_AUTH_TOKEN`, `clientSecret` and
/// `password` are. Values under a masked key are replaced as a whole, including
/// nested objects and arrays.
pub(crate) fn is_secret_key(key: &str) -> bool {
    let lower = key.to_ascii_lowercase();
    let compact: String = lower
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .collect();
    if matches!(
        compact.as_str(),
        "token"
            | "secret"
            | "password"
            | "passwd"
            | "auth"
            | "authorization"
            | "bearer"
            | "credential"
            | "credentials"
            | "apikey"
    ) || compact.ends_with("apikey")
        || compact.ends_with("accesskey")
        || compact.ends_with("secret")
        || compact.ends_with("password")
        || compact.ends_with("privatekey")
        || compact.ends_with("secretkey")
        || compact.ends_with("accesstoken")
        || compact.ends_with("refreshtoken")
        || compact.ends_with("idtoken")
        || compact.ends_with("sessiontoken")
    {
        return true;
    }
    let words = normalized_words(&lower);
    words.ends_with("_token")
        || words.ends_with("_secret")
        || words.ends_with("_password")
        || words.ends_with("_api_key")
        || words.ends_with("_access_key")
        || words.ends_with("_private_key")
        || words.ends_with("_secret_key")
        || words.ends_with("_client_secret")
}

/// Lowercase key with runs of non-alphanumerics collapsed to `_`.
fn normalized_words(key: &str) -> String {
    let mut words = String::with_capacity(key.len());
    let mut separator = false;
    for character in key.chars() {
        if character.is_ascii_alphanumeric() {
            if separator && !words.is_empty() {
                words.push('_');
            }
            words.push(character.to_ascii_lowercase());
            separator = false;
        } else {
            separator = true;
        }
    }
    words
}

fn sanitize_json(value: &mut Value) {
    match value {
        Value::Object(map) => {
            for (key, child) in map.iter_mut() {
                if is_secret_key(key) {
                    *child = Value::String(SECRET_PLACEHOLDER.to_owned());
                } else {
                    sanitize_json(child);
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                sanitize_json(item);
            }
        }
        _ => {}
    }
}

fn sanitize_toml(document: &mut DocumentMut) {
    for (key, item) in document.as_table_mut().iter_mut() {
        sanitize_item(key.get(), item);
    }
}

fn sanitize_item(key: &str, item: &mut Item) {
    if is_secret_key(key) {
        *item = Item::Value(TomlValue::from(SECRET_PLACEHOLDER));
        return;
    }
    if let Some(table) = item.as_table_like_mut() {
        for (child_key, child) in table.iter_mut() {
            sanitize_item(child_key.get(), child);
        }
    } else if let Some(array) = item.as_array_mut() {
        for value in array.iter_mut() {
            sanitize_toml_value(value);
        }
    }
}

fn sanitize_toml_value(value: &mut TomlValue) {
    match value {
        TomlValue::InlineTable(table) => {
            for (key, child) in table.iter_mut() {
                if is_secret_key(key.get()) {
                    *child = TomlValue::from(SECRET_PLACEHOLDER);
                } else {
                    sanitize_toml_value(child);
                }
            }
        }
        TomlValue::Array(items) => {
            for item in items.iter_mut() {
                sanitize_toml_value(item);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn json(bytes: &[u8]) -> VersionFileSummary {
        summarize(OperationKind::ClaudeCodeSettings, Some(bytes), Some(bytes))
    }

    #[test]
    fn masks_credential_keys_but_not_token_counts_or_references() {
        for key in [
            "api_key",
            "apiKey",
            "x-api-key",
            "ANTHROPIC_AUTH_TOKEN",
            "clientSecret",
            "password",
            "private_key",
            "access_key",
            "authorization",
        ] {
            assert!(is_secret_key(key), "{key}");
        }
        for key in [
            "max_tokens",
            "maxTokens",
            "input_tokens",
            "context_window",
            "env_key",
            "model_key",
            "tokenizer",
            "apiKeyHelper",
        ] {
            assert!(!is_secret_key(key), "{key}");
        }
    }

    #[test]
    fn json_summary_masks_values_and_keeps_absence_distinct() {
        let current =
            br#"{"env":{"ANTHROPIC_API_KEY":"sk-current","KEEP":"a"},"max_tokens":100,"empty":""}"#;
        let target = br#"{"env":{"ANTHROPIC_API_KEY":"sk-old","KEEP":"b"},"max_tokens":null,"nullable":null}"#;
        let summary = summarize(
            OperationKind::ClaudeCodeSettings,
            Some(current),
            Some(target),
        );
        assert_eq!(summary.reason, None);
        let by_path = |path: &str| {
            summary
                .changes
                .iter()
                .find(|change| change.path == path)
                .unwrap_or_else(|| panic!("missing {path}"))
        };
        let key = by_path("env.ANTHROPIC_API_KEY");
        assert!(key.secret);
        assert_eq!(key.before.as_deref(), Some(SECRET_PLACEHOLDER));
        assert_eq!(key.after.as_deref(), Some(SECRET_PLACEHOLDER));
        assert_eq!(by_path("env.KEEP").before.as_deref(), Some("a"));
        assert_eq!(by_path("env.KEEP").after.as_deref(), Some("b"));
        assert_eq!(by_path("max_tokens").before.as_deref(), Some("100"));
        assert_eq!(by_path("max_tokens").after.as_deref(), Some("null"));
        assert_eq!(by_path("empty").before.as_deref(), Some(""));
        assert_eq!(by_path("empty").after, None);
        assert_eq!(by_path("nullable").before, None);
        assert_eq!(by_path("nullable").after.as_deref(), Some("null"));
        let rendered = serde_json::to_string(&summary).unwrap();
        assert!(!rendered.contains("sk-current"));
        assert!(!rendered.contains("sk-old"));
    }

    #[test]
    fn json_text_masks_nested_credentials_and_preserves_absence() {
        let bytes = br#"{"env":{"OPENAI_API_KEY":"sk-synthetic"},"extra":{"authorization":"Bearer synthetic"},"safe":"visible"}"#;
        let side = render_text(ConfigFormat::Json, bytes).unwrap();
        let VersionTextSide::Present { text, bytes: size } = side else {
            panic!("expected text");
        };
        assert!(!text.contains("sk-synthetic"));
        assert!(!text.contains("Bearer synthetic"));
        assert!(text.contains("visible"));
        assert_eq!(size, u64::try_from(bytes.len()).unwrap());
        assert_eq!(
            render_text(ConfigFormat::Json, b"").unwrap(),
            VersionTextSide::Present {
                text: String::new(),
                bytes: 0
            }
        );
        assert_eq!(
            render_text(ConfigFormat::Json, b"{not-json").unwrap_err(),
            TEXT_UNAVAILABLE
        );
    }

    #[test]
    fn toml_text_keeps_comments_and_masks_keys() {
        let bytes = b"# keep this comment\n[model.a]\napi_key = \"sk-synthetic\" # inline\nmodel = \"visible\"\n\n[env]\nAUTH_TOKEN = \"private\"\n";
        let side = render_text(ConfigFormat::Toml, bytes).unwrap();
        let VersionTextSide::Present { text, .. } = side else {
            panic!("expected text");
        };
        assert!(text.contains("# keep this comment"));
        assert!(text.contains("model = \"visible\""));
        assert!(!text.contains("sk-synthetic"));
        assert!(!text.contains("private"));
    }

    #[test]
    fn toml_summary_reports_nested_and_array_changes() {
        let current = b"[model.a]\nmodel = \"one\"\napi_key = \"sk-old\"\n[items]\nlist = [1, 2]\n";
        let target = b"[model.a]\nmodel = \"two\"\napi_key = \"sk-new\"\n[items]\nlist = [1]\n";
        let summary = summarize(OperationKind::GrokSettings, Some(current), Some(target));
        assert_eq!(summary.format, ConfigFormat::Toml);
        assert!(
            summary
                .changes
                .iter()
                .any(|change| { change.path == "model.a.api_key" && change.secret })
        );
        assert!(summary.changes.iter().any(|change| {
            change.path == "model.a.model"
                && change.before.as_deref() == Some("one")
                && change.after.as_deref() == Some("two")
        }));
        assert!(summary.changes.iter().any(|change| {
            change.path == "items.list[1]" && change.kind == FieldChangeKind::Removed
        }));
    }

    #[test]
    fn unparseable_documents_are_a_reason_not_raw_text() {
        let summary = summarize(
            OperationKind::GrokSettings,
            Some(b"[not toml"),
            Some(b"[model]\n"),
        );
        assert_eq!(summary.reason.as_deref(), Some(TEXT_UNAVAILABLE));
        assert!(summary.changes.is_empty());
        assert_eq!(
            render_text(ConfigFormat::Toml, b"[not toml").unwrap_err(),
            TEXT_UNAVAILABLE
        );
    }

    #[test]
    fn summaries_mark_explicit_truncation() {
        let mut map = Map::new();
        for index in 0..(MAX_SUMMARY_CHANGES + 5) {
            map.insert(format!("key-{index}"), Value::from(index as u64));
        }
        let current = serde_json::to_vec(&Value::Object(map.clone())).unwrap();
        let mut empty = map;
        empty.clear();
        let target = serde_json::to_vec(&Value::Object(empty)).unwrap();
        let summary = summarize(
            OperationKind::ClaudeCodeSettings,
            Some(&current),
            Some(&target),
        );
        assert!(summary.truncated);
        assert_eq!(summary.changes.len(), MAX_SUMMARY_CHANGES);
    }

    #[test]
    fn secret_values_under_containers_are_masked_whole() {
        let current = br#"{"auth":{"nested":{"value":"secret-one"}}}"#;
        let target = br#"{"auth":{"nested":{"value":"secret-two"}}}"#;
        let summary = summarize(
            OperationKind::ClaudeCodeSettings,
            Some(current),
            Some(target),
        );
        assert_eq!(summary.changes.len(), 1);
        assert_eq!(summary.changes[0].path, "auth");
        assert!(summary.changes[0].secret);
        assert_eq!(
            summary.changes[0].before.as_deref(),
            Some(SECRET_PLACEHOLDER)
        );
        assert_eq!(
            summary.changes[0].after.as_deref(),
            Some(SECRET_PLACEHOLDER)
        );
    }

    #[test]
    fn external_scalar_root_edits_have_an_explicit_valid_field_path() {
        let summary = summarize(
            OperationKind::ClaudeCodeSettings,
            Some(b"null"),
            Some(br#"{"model":"restored"}"#),
        );
        assert_eq!(summary.reason, None);
        assert!(summary.changes.iter().any(|change| change.path == "$"
            && change.kind == FieldChangeKind::Removed
            && change.before.as_deref() == Some("null")));
        assert!(summary.changes.iter().all(|change| !change.path.is_empty()));
    }

    #[test]
    fn two_absent_files_have_no_synthetic_root_change() {
        let summary = summarize(OperationKind::PiSettings, None, None);
        assert!(summary.changes.is_empty());
        assert!(!summary.truncated);
        assert_eq!(summary.reason, None);
    }

    #[test]
    fn text_size_limit_applies_before_and_after_pretty_printing() {
        let oversized = vec![b' '; MAX_TARGET_BYTES as usize + 1];
        assert_eq!(
            render_text(ConfigFormat::Json, &oversized).unwrap_err(),
            TEXT_UNAVAILABLE
        );
        let compact = format!("[{}]", vec!["0"; MAX_TARGET_BYTES as usize / 3].join(","));
        assert!(compact.len() < MAX_TARGET_BYTES as usize);
        assert_eq!(
            render_text(ConfigFormat::Json, compact.as_bytes()).unwrap_err(),
            TEXT_UNAVAILABLE
        );
    }

    #[test]
    fn json_summary_handles_arrays_objects_and_blank_files() {
        let current = br#"[1, {"a": true}]"#;
        let target = br#"[1, {"a": false}, 3]"#;
        let summary = summarize(OperationKind::PiSettings, Some(current), Some(target));
        assert!(summary.changes.iter().any(|change| {
            change.path == "[1].a"
                && change.before.as_deref() == Some("true")
                && change.after.as_deref() == Some("false")
        }));
        assert!(
            summary
                .changes
                .iter()
                .any(|change| { change.path == "[2]" && change.kind == FieldChangeKind::Added })
        );
        let blank = summarize(
            OperationKind::ClaudeCodeSettings,
            Some(b"  \n"),
            Some(b"{}"),
        );
        assert_eq!(blank.reason, None);
        assert!(!json(b"{}").truncated);
    }
}
