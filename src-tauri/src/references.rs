//! Read Agent ownership references before deleting central records.
//!
//! Provider and model deletion must not silently invalidate an Agent
//! configuration. Pi records the readable provider key it wrote, and Grok
//! records the model alias it owns. A binding is:
//!
//! - `active` while the named entry still exists in the live Agent file;
//! - `stale` after the user removed that entry, so the central row can be
//!   cleared without touching the Agent file;
//! - `unverified` when the file cannot be read or parsed, so deletion stays
//!   blocked rather than guessing.
//!
//! Codex rows are listed separately as [`CodexReference`] (the owned table,
//! the owned default or a pending change). They are never clearable here: the
//! Codex settings panel revokes or clears them, and a pending change is
//! resolved by its recovery.
//!
//! This module only reads Agent files. Clearing a binding removes vibemate's
//! ownership row; it never rewrites user or Agent content.

use crate::config_plan::read_target_bytes;
use crate::storage::Storage;
use rusqlite::params;
use serde::Serialize;
use std::path::Path;
use toml_edit::DocumentMut;

/// Whether a recorded binding still points at live Agent content.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReferenceState {
    /// The named entry still exists in the Agent file.
    Active,
    /// The Agent file no longer contains the named entry.
    Stale,
    /// The Agent file could not be read or parsed, so absence is unproven.
    Unverified,
}

/// One Agent binding that references a central Provider or model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentReference {
    /// Agent identity (`pi` or `grokBuild`).
    pub agent_id: String,
    /// Absolute configuration root recorded with the binding.
    pub config_root: String,
    /// Provider key in Pi or model alias in Grok.
    pub label: String,
    /// Model ID for a Grok alias, or the bound model for a Pi model reference.
    pub model_id: Option<String>,
    /// Live state described above.
    pub state: ReferenceState,
    /// True only for a stale binding that this build can clear.
    pub clearable: bool,
}

/// One owned Codex row that blocks deleting a central Provider or model.
/// Released only from Codex settings (revoke / clear default) or recovery.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexReference {
    /// Absolute `$CODEX_HOME` recorded with the row.
    pub config_root: String,
    /// `vibemate-…` key under `[model_providers]`.
    pub provider_key: String,
    /// Model of the owned default or of a pending default change, else null.
    pub model_id: Option<String>,
    /// Which owned Codex state this row is.
    pub binding: CodexBindingKind,
    /// `active` when the owned key is still in `config.toml`, `stale` when it
    /// is gone, `unverified` when unreadable or for a pending change.
    pub state: ReferenceState,
}

/// Which owned Codex state blocks deleting a central record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CodexBindingKind {
    /// `[model_providers.<label>]` written by vibemate; revoke it in Codex
    /// settings.
    Provider,
    /// Top-level `model`/`model_provider` written by "set as default"; clear
    /// the default in Codex settings.
    Default,
    /// An interrupted Codex settings change awaiting recovery.
    Pending,
}

/// Safe failures while reading binding rows or Agent files.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReferenceReadError {
    StorageUnavailable,
    ReadFailed,
}

/// Safe failures while clearing one stale binding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReferenceClearError {
    StorageUnavailable,
    ReadFailed,
    WriteFailed,
    InvalidRequest,
    NotFound,
    /// The binding is active, or its state cannot be proven, so it stays.
    InUse,
}

/// Read every Pi/Grok binding that names this Provider.
pub(crate) fn provider_references(
    storage: &Storage,
    provider_id: &str,
) -> Result<Vec<AgentReference>, ReferenceReadError> {
    let mut references = Vec::new();
    for (config_root, provider_key) in pi_provider_rows(storage, provider_id)? {
        let state = pi_provider_state(Path::new(&config_root), &provider_key);
        references.push(AgentReference {
            agent_id: "pi".into(),
            config_root,
            label: provider_key,
            model_id: None,
            state,
            clearable: state == ReferenceState::Stale,
        });
    }
    for (config_root, model_alias, model_id) in grok_rows(storage, provider_id, None)? {
        let state = grok_alias_state(Path::new(&config_root), &model_alias, Some(&model_id));
        references.push(AgentReference {
            agent_id: "grokBuild".into(),
            config_root,
            label: model_alias,
            model_id: Some(model_id),
            state,
            clearable: state == ReferenceState::Stale,
        });
    }
    sort(&mut references);
    Ok(references)
}

/// Read every binding that names this Provider/model pair.
///
/// Grok aliases have an explicit ownership row. Pi stores the model inside its
/// provider entry, so a model reference is derived from the live `models.json`
/// and cannot be cleared centrally; changing or restoring the Pi configuration
/// is the only way to remove it.
pub(crate) fn model_references(
    storage: &Storage,
    provider_id: &str,
    model_id: &str,
) -> Result<Vec<AgentReference>, ReferenceReadError> {
    let mut references = Vec::new();
    for (config_root, model_alias, bound_model) in grok_rows(storage, provider_id, Some(model_id))?
    {
        let state = grok_alias_state(Path::new(&config_root), &model_alias, Some(&bound_model));
        references.push(AgentReference {
            agent_id: "grokBuild".into(),
            config_root,
            label: model_alias,
            model_id: Some(bound_model),
            state,
            clearable: state == ReferenceState::Stale,
        });
    }
    for (config_root, provider_key) in pi_provider_rows(storage, provider_id)? {
        if let Some(state) = pi_model_state(Path::new(&config_root), &provider_key, model_id) {
            references.push(AgentReference {
                agent_id: "pi".into(),
                config_root,
                label: provider_key,
                model_id: Some(model_id.to_owned()),
                state,
                clearable: false,
            });
        }
    }
    sort(&mut references);
    Ok(references)
}

fn sort(references: &mut [AgentReference]) {
    references.sort_by(|left, right| {
        (&left.agent_id, &left.config_root, &left.label).cmp(&(
            &right.agent_id,
            &right.config_root,
            &right.label,
        ))
    });
}

/// Codex rows exactly as counted by [`provider_has_bindings`] (no model) or
/// [`model_has_bindings`] (with a model): owned tables, the owned default and
/// pending changes for a Provider; only the owned default for a model.
pub(crate) fn codex_references(
    storage: &Storage,
    provider_id: &str,
    model_id: Option<&str>,
) -> Result<Vec<CodexReference>, ReferenceReadError> {
    type Row = (String, String, Option<String>, CodexBindingKind);
    let rows: Vec<Row> = {
        let connection = storage
            .lock()
            .map_err(|_| ReferenceReadError::StorageUnavailable)?;
        let mut rows = Vec::new();
        let mut collect = |sql: &str, kind: CodexBindingKind| {
            let mut statement = connection
                .prepare(sql)
                .map_err(|_| ReferenceReadError::ReadFailed)?;
            let mapped = statement
                .query_map(params![provider_id, model_id], |row| {
                    Ok((row.get(0)?, row.get(1)?, row.get(2)?, kind))
                })
                .map_err(|_| ReferenceReadError::ReadFailed)?;
            for row in mapped {
                rows.push(row.map_err(|_| ReferenceReadError::ReadFailed)?);
            }
            Ok::<(), ReferenceReadError>(())
        };
        collect(
            "SELECT config_root, provider_key, model_id FROM codex_default_binding
             WHERE provider_id=?1 AND (?2 IS NULL OR model_id=?2)",
            CodexBindingKind::Default,
        )?;
        if model_id.is_none() {
            collect(
                "SELECT config_root, provider_key, NULL FROM codex_provider_binding
                 WHERE provider_id=?1 AND ?2 IS NULL",
                CodexBindingKind::Provider,
            )?;
            collect(
                "SELECT config_root, provider_key, model_id FROM codex_settings_change
                 WHERE provider_id=?1 AND ?2 IS NULL AND state='prepared'",
                CodexBindingKind::Pending,
            )?;
        }
        rows
    };
    let mut references: Vec<CodexReference> = rows
        .into_iter()
        .map(|(config_root, provider_key, model_id, binding)| {
            let state = codex_state(Path::new(&config_root), &provider_key, binding);
            CodexReference {
                config_root,
                provider_key,
                model_id,
                binding,
                state,
            }
        })
        .collect();
    references.sort_by(|left, right| {
        (&left.config_root, left.binding as u8, &left.provider_key).cmp(&(
            &right.config_root,
            right.binding as u8,
            &right.provider_key,
        ))
    });
    Ok(references)
}

/// Live state of one Codex row. A pending change is never proven settled, so
/// it is `unverified` until its recovery runs. A symlinked or unparsable
/// `config.toml` is `unverified` as well.
fn codex_state(root: &Path, provider_key: &str, kind: CodexBindingKind) -> ReferenceState {
    if kind == CodexBindingKind::Pending {
        return ReferenceState::Unverified;
    }
    let document = match read_target_bytes(&root.join("config.toml")) {
        Ok((target, _)) if target.symlink => return ReferenceState::Unverified,
        Ok((target, _)) if !target.exists => return ReferenceState::Stale,
        Ok((_, bytes)) => match std::str::from_utf8(&bytes)
            .ok()
            .and_then(|text| text.parse::<DocumentMut>().ok())
        {
            Some(document) => document,
            None => return ReferenceState::Unverified,
        },
        Err(_) => return ReferenceState::Unverified,
    };
    let present = match kind {
        CodexBindingKind::Provider => document
            .get("model_providers")
            .and_then(|providers| providers.get(provider_key))
            .is_some(),
        _ => {
            document
                .get("model_provider")
                .and_then(|value| value.as_str())
                == Some(provider_key)
        }
    };
    if present {
        ReferenceState::Active
    } else {
        ReferenceState::Stale
    }
}

/// Count Claude Code remembered starting choices that will be removed by the
/// same foreign-key cascade as the Provider or model row.
pub(crate) fn remembered_preferences(
    storage: &Storage,
    provider_id: &str,
    model_id: Option<&str>,
) -> Result<u64, ReferenceReadError> {
    let connection = storage
        .lock()
        .map_err(|_| ReferenceReadError::StorageUnavailable)?;
    let count: i64 = match model_id {
        Some(model_id) => connection.query_row(
            "SELECT COUNT(*) FROM claude_effort_preference
             WHERE provider_id=?1 AND model_id=?2",
            params![provider_id, model_id],
            |row| row.get(0),
        ),
        None => connection.query_row(
            "SELECT COUNT(*) FROM claude_effort_preference WHERE provider_id=?1",
            [provider_id],
            |row| row.get(0),
        ),
    }
    .map_err(|_| ReferenceReadError::ReadFailed)?;
    Ok(count.max(0) as u64)
}

/// Whether any Pi/Grok/Codex binding row (or a pending Codex change) still names
/// this Provider.
pub(crate) fn provider_has_bindings(
    storage: &Storage,
    provider_id: &str,
) -> Result<bool, ReferenceReadError> {
    let connection = storage
        .lock()
        .map_err(|_| ReferenceReadError::StorageUnavailable)?;
    let count: i64 = connection
        .query_row(
            "SELECT
                (SELECT COUNT(*) FROM pi_provider_binding WHERE provider_id=?1) +
                (SELECT COUNT(*) FROM grok_model_binding WHERE provider_id=?1) +
                (SELECT COUNT(*) FROM codex_provider_binding WHERE provider_id=?1) +
                (SELECT COUNT(*) FROM codex_default_binding WHERE provider_id=?1) +
                (SELECT COUNT(*) FROM codex_settings_change
                 WHERE provider_id=?1 AND state='prepared')",
            [provider_id],
            |row| row.get(0),
        )
        .map_err(|_| ReferenceReadError::ReadFailed)?;
    Ok(count > 0)
}

/// Whether a Grok alias row or the owned Codex default still names this
/// Provider/model pair.
pub(crate) fn model_has_bindings(
    storage: &Storage,
    provider_id: &str,
    model_id: &str,
) -> Result<bool, ReferenceReadError> {
    let connection = storage
        .lock()
        .map_err(|_| ReferenceReadError::StorageUnavailable)?;
    let count: i64 = connection
        .query_row(
            "SELECT
                (SELECT COUNT(*) FROM grok_model_binding
                 WHERE provider_id=?1 AND model_id=?2) +
                (SELECT COUNT(*) FROM codex_default_binding
                 WHERE provider_id=?1 AND model_id=?2)",
            params![provider_id, model_id],
            |row| row.get(0),
        )
        .map_err(|_| ReferenceReadError::ReadFailed)?;
    Ok(count > 0)
}

/// Remove one stale Pi Provider binding or Grok alias binding.
pub(crate) fn clear_provider_reference(
    storage: &Storage,
    provider_id: &str,
    agent_id: &str,
    config_root: &str,
    label: &str,
) -> Result<(), ReferenceClearError> {
    let references = provider_references(storage, provider_id).map_err(map_read_error)?;
    let reference = references
        .iter()
        .find(|reference| {
            reference.agent_id == agent_id
                && reference.config_root == config_root
                && reference.label == label
        })
        .ok_or(ReferenceClearError::NotFound)?;
    if reference.state != ReferenceState::Stale {
        return Err(ReferenceClearError::InUse);
    }
    let connection = storage
        .lock()
        .map_err(|_| ReferenceClearError::StorageUnavailable)?;
    let removed = match agent_id {
        "pi" => connection.execute(
            "DELETE FROM pi_provider_binding
             WHERE provider_id=?1 AND config_root=?2 AND provider_key=?3",
            params![provider_id, config_root, label],
        ),
        "grokBuild" => connection.execute(
            "DELETE FROM grok_model_binding
             WHERE provider_id=?1 AND config_root=?2 AND model_alias=?3",
            params![provider_id, config_root, label],
        ),
        _ => return Err(ReferenceClearError::InvalidRequest),
    }
    .map_err(|_| ReferenceClearError::WriteFailed)?;
    if removed != 1 {
        return Err(ReferenceClearError::NotFound);
    }
    Ok(())
}

/// Remove one stale Grok alias binding for a Provider/model pair.
pub(crate) fn clear_model_reference(
    storage: &Storage,
    provider_id: &str,
    model_id: &str,
    agent_id: &str,
    config_root: &str,
    label: &str,
) -> Result<(), ReferenceClearError> {
    if agent_id != "grokBuild" {
        // A Pi model reference has no central row; changing the Pi configuration
        // is the only supported removal path.
        return Err(ReferenceClearError::InvalidRequest);
    }
    let references = model_references(storage, provider_id, model_id).map_err(map_read_error)?;
    let reference = references
        .iter()
        .find(|reference| {
            reference.agent_id == agent_id
                && reference.config_root == config_root
                && reference.label == label
                && reference.model_id.as_deref() == Some(model_id)
        })
        .ok_or(ReferenceClearError::NotFound)?;
    if reference.state != ReferenceState::Stale {
        return Err(ReferenceClearError::InUse);
    }
    let connection = storage
        .lock()
        .map_err(|_| ReferenceClearError::StorageUnavailable)?;
    let removed = connection
        .execute(
            "DELETE FROM grok_model_binding
             WHERE provider_id=?1 AND model_id=?2
               AND config_root=?3 AND model_alias=?4",
            params![provider_id, model_id, config_root, label],
        )
        .map_err(|_| ReferenceClearError::WriteFailed)?;
    if removed != 1 {
        return Err(ReferenceClearError::NotFound);
    }
    Ok(())
}

fn map_read_error(error: ReferenceReadError) -> ReferenceClearError {
    match error {
        ReferenceReadError::StorageUnavailable => ReferenceClearError::StorageUnavailable,
        ReferenceReadError::ReadFailed => ReferenceClearError::ReadFailed,
    }
}

fn pi_provider_rows(
    storage: &Storage,
    provider_id: &str,
) -> Result<Vec<(String, String)>, ReferenceReadError> {
    let connection = storage
        .lock()
        .map_err(|_| ReferenceReadError::StorageUnavailable)?;
    let mut statement = connection
        .prepare(
            "SELECT config_root, provider_key FROM pi_provider_binding
             WHERE provider_id=?1",
        )
        .map_err(|_| ReferenceReadError::ReadFailed)?;
    let rows = statement
        .query_map([provider_id], |row| Ok((row.get(0)?, row.get(1)?)))
        .map_err(|_| ReferenceReadError::ReadFailed)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| ReferenceReadError::ReadFailed)?;
    Ok(rows)
}

fn grok_rows(
    storage: &Storage,
    provider_id: &str,
    model_id: Option<&str>,
) -> Result<Vec<(String, String, String)>, ReferenceReadError> {
    let connection = storage
        .lock()
        .map_err(|_| ReferenceReadError::StorageUnavailable)?;
    let mut statement = match model_id {
        Some(_) => connection
            .prepare(
                "SELECT config_root, model_alias, model_id FROM grok_model_binding
                 WHERE provider_id=?1 AND model_id=?2",
            )
            .map_err(|_| ReferenceReadError::ReadFailed)?,
        None => connection
            .prepare(
                "SELECT config_root, model_alias, model_id FROM grok_model_binding
                 WHERE provider_id=?1",
            )
            .map_err(|_| ReferenceReadError::ReadFailed)?,
    };
    let mapper = |row: &rusqlite::Row<'_>| Ok((row.get(0)?, row.get(1)?, row.get(2)?));
    let rows = match model_id {
        Some(model_id) => statement
            .query_map(params![provider_id, model_id], mapper)
            .map_err(|_| ReferenceReadError::ReadFailed)?,
        None => statement
            .query_map([provider_id], mapper)
            .map_err(|_| ReferenceReadError::ReadFailed)?,
    }
    .collect::<Result<Vec<_>, _>>()
    .map_err(|_| ReferenceReadError::ReadFailed)?;
    Ok(rows)
}

fn pi_provider_state(root: &Path, provider_key: &str) -> ReferenceState {
    match read_target_bytes(&root.join("models.json")) {
        Ok((target, _)) if !target.exists => ReferenceState::Stale,
        Ok((_, bytes)) if bytes.iter().all(u8::is_ascii_whitespace) => ReferenceState::Stale,
        Ok((_, bytes)) => match serde_json::from_slice::<serde_json::Value>(&bytes) {
            Ok(value) => match value
                .get("providers")
                .and_then(serde_json::Value::as_object)
                .and_then(|providers| providers.get(provider_key))
            {
                Some(_) => ReferenceState::Active,
                None => ReferenceState::Stale,
            },
            Err(_) => ReferenceState::Unverified,
        },
        Err(_) => ReferenceState::Unverified,
    }
}

fn pi_model_state(root: &Path, provider_key: &str, model_id: &str) -> Option<ReferenceState> {
    match read_target_bytes(&root.join("models.json")) {
        Ok((target, _)) if !target.exists => None,
        Ok((_, bytes)) if bytes.iter().all(u8::is_ascii_whitespace) => None,
        Ok((_, bytes)) => match serde_json::from_slice::<serde_json::Value>(&bytes) {
            Ok(value) => {
                let models = value
                    .get("providers")
                    .and_then(serde_json::Value::as_object)
                    .and_then(|providers| providers.get(provider_key))
                    .and_then(|provider| provider.get("models"))
                    .and_then(serde_json::Value::as_array)?;
                let found = models.iter().any(|model| {
                    model.get("id").and_then(serde_json::Value::as_str) == Some(model_id)
                        || model.as_str() == Some(model_id)
                });
                found.then_some(ReferenceState::Active)
            }
            Err(_) => Some(ReferenceState::Unverified),
        },
        Err(_) => Some(ReferenceState::Unverified),
    }
}

fn grok_alias_state(root: &Path, alias: &str, model_id: Option<&str>) -> ReferenceState {
    match read_target_bytes(&root.join("config.toml")) {
        Ok((target, _)) if !target.exists => ReferenceState::Stale,
        Ok((_, bytes)) if bytes.iter().all(u8::is_ascii_whitespace) => ReferenceState::Stale,
        Ok((_, bytes)) => match std::str::from_utf8(&bytes)
            .ok()
            .and_then(|text| text.parse::<DocumentMut>().ok())
        {
            Some(document) => match document.get("model").and_then(|models| models.get(alias)) {
                Some(entry) => match model_id {
                    Some(expected)
                        if entry.get("model").and_then(|value| value.as_str())
                            == Some(expected) =>
                    {
                        ReferenceState::Active
                    }
                    // The alias exists with different or missing model details,
                    // so vibemate no longer owns this entry.
                    Some(_) => ReferenceState::Stale,
                    None => ReferenceState::Active,
                },
                None => ReferenceState::Stale,
            },
            None => ReferenceState::Unverified,
        },
        Err(_) => ReferenceState::Unverified,
    }
}
