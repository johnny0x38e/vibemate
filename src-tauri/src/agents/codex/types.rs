//! Safe Codex provider-settings contracts. None of these types carry a raw
//! credential: previews mask `experimental_bearer_token`, and ownership
//! records hold only digests.

use crate::config_apply::VersionFileSummary;
use crate::config_plan::TargetFile;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// One explicitly reviewed `config.toml` mutation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    /// Create or refresh `[model_providers.<key>]` from the central provider
    /// and its OS credential.
    Deploy,
    /// Remove only the owned provider table and release its binding.
    Revoke,
    /// Write top-level `model` + `model_provider` for an owned provider, plus
    /// `model_reasoning_effort` only when the central model states an effort
    /// (an existing user effort is otherwise left alone); previous values of
    /// the written keys are kept privately so `clear_default` can put them back.
    SetDefault,
    /// Restore the values recorded by `set_default` while the file still shows
    /// exactly the owned default (`written`). When the default is `modified`
    /// (changed by Codex `/model` or by hand) or `missing` (both top-level
    /// `model` and `model_provider` removed), only ownership is released and the
    /// file is not touched, so a value the user chose or deleted is never
    /// overwritten or re-added. An `unreadable` file is refused.
    ClearDefault,
}
impl Action {
    pub(super) fn code(self) -> &'static str {
        match self {
            Self::Deploy => "deploy",
            Self::Revoke => "revoke",
            Self::SetDefault => "set_default",
            Self::ClearDefault => "clear_default",
        }
    }
    pub(super) fn from_code(value: &str) -> Option<Self> {
        [
            Self::Deploy,
            Self::Revoke,
            Self::SetDefault,
            Self::ClearDefault,
        ]
        .into_iter()
        .find(|action| action.code() == value)
    }
}

/// Strict selection: identities only, never a path, key or file content.
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Selection {
    /// Central provider identity (32 lowercase hex).
    pub provider_id: String,
    /// Revision the user reviewed; which revision depends on the action:
    ///
    /// - `deploy` / `set_default` read central provider (and model) content,
    ///   so this is the current central `provider_instance.revision`.
    /// - `revoke` / `clear_default` only touch what vibemate owns, so this is
    ///   the owned binding's `owner.providerRevision` (the revision last
    ///   written to `config.toml`, as reported by `codex_settings_state`). These
    ///   actions keep working after the central provider was edited (an
    ///   `outdated` binding) and never require the central row to exist.
    ///
    /// A mismatch is `provider_changed`; re-read state and review again.
    pub expected_revision: i64,
    /// Requested action.
    pub action: Action,
    /// Model to make the Codex default; required for `set_default` only.
    pub model_id: Option<String>,
}

/// Confirmation of an exact rebuilt plan.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ApplyRequest {
    /// Same selection used to obtain the preview.
    pub selection: Selection,
    /// Preview approval digest.
    pub approval: String,
}

/// Safe failures; no native or credential diagnostics are serialized.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Error {
    /// Invalid identity, selection or pagination.
    InvalidRequest,
    /// Storage or metadata is unavailable.
    StorageUnavailable,
    /// The reviewed revision no longer matches: the central provider (or
    /// model) for `deploy`/`set_default`, the owned binding for
    /// `revoke`/`clear_default`.
    ProviderChanged,
    /// `$CODEX_HOME` is missing, or `config.toml` is unreadable, oversized,
    /// read-only or a symbolic link.
    TargetUnavailable,
    /// Native TOML is invalid or has a non-table `model_providers` container.
    InvalidConfig,
    /// The provider key is reserved by Codex or already used by a table
    /// vibemate does not own.
    NameConflict,
    /// Owned bytes changed outside vibemate (including added `env_key`/`auth`).
    OwnershipConflict,
    /// A fresh preview is required after a state change.
    PlanStale,
    /// The provider's OS credential is missing, invalid or inaccessible.
    CredentialUnavailable,
    /// An interrupted Codex change (settings or MCP) must be reconciled first.
    RecoveryRequired,
    /// The shared writer refused the replacement; no successful apply is claimed.
    WriteFailed,
    /// A result could not be established; read state and reconcile first.
    OutcomeUnknown,
    /// Codex 0.165.0 speaks only the Responses wire API, which this provider
    /// configuration cannot reach.
    ProtocolUnsupported,
    /// A custom base URL has no verified Responses equivalent.
    BaseUrlUnverified,
    /// The model is not selected or not known to serve Responses.
    ModelUnsupported,
    /// The provider is the Codex default; clear the default before revoking.
    DefaultInUse,
    /// `set_default` needs this provider's table to be deployed first.
    NotDeployed,
}

/// Owned `[model_providers.<key>]` table.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProviderOwner {
    /// Exact table key below `model_providers`.
    pub provider_key: String,
    /// Central provider revision last written.
    pub provider_revision: i64,
    /// SHA-256 of the whole table as canonical JSON, credential included.
    pub entry_digest: String,
    /// Last successful change identity, preventing approval reuse.
    pub generation: String,
}

/// Values of the three top-level keys before vibemate first set a default.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DefaultValues {
    pub model: Option<String>,
    pub model_provider: Option<String>,
    pub model_reasoning_effort: Option<String>,
}

/// Owned top-level default.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DefaultOwner {
    pub provider_id: String,
    pub model_id: String,
    pub provider_key: String,
    /// Digest of the owned part of the written [`DefaultValues`]
    /// (`plan::owned_defaults`).
    pub entry_digest: String,
    /// What `clear_default` restores (the effort only with `writes_effort`).
    pub previous: DefaultValues,
    /// Whether vibemate wrote `model_reasoning_effort` (the central model
    /// states an effort). Snapshots recorded before this flag existed always
    /// managed the effort, hence the `true` default.
    #[serde(default = "writes_effort_default")]
    pub writes_effort: bool,
    pub generation: String,
}

fn writes_effort_default() -> bool {
    true
}

/// Ownership snapshot a change reads and replaces atomically: the selected
/// provider's binding in the root plus the root's (single) default binding.
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Owners {
    pub provider: Option<ProviderOwner>,
    pub default: Option<DefaultOwner>,
}

/// Non-blocking facts the confirmation dialog should show.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Notice {
    /// The key is written as plaintext `experimental_bearer_token` in a 0600
    /// file; Codex marks the field experimental. Private backups may hold it.
    PlaintextCredential,
    /// A top-level `profile` is set; Codex may take `model` from that profile
    /// instead of the default vibemate writes. vibemate never edits profiles.
    ProfileOverridesDefault,
    /// `clear_default` only releases ownership because the default was changed
    /// or removed outside vibemate; the file is not modified.
    ReleaseOnly,
}

/// Sanitized before/after fragments and an exact approval.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Preview {
    /// Trusted target and current full-file digest.
    pub target: TargetFile,
    pub provider_id: String,
    pub provider_revision: i64,
    pub action: Action,
    /// Table key below `model_providers`.
    pub provider_key: String,
    /// Model for `set_default`; null otherwise.
    pub model_id: Option<String>,
    /// Masked affected fragment before the change, or null.
    pub before: Option<Value>,
    /// Masked affected fragment after the change, or null.
    pub after: Option<Value>,
    /// Shared field-difference representation over the masked fragments.
    pub summary: VersionFileSummary,
    pub notices: Vec<Notice>,
    /// Whether the file will be written (false for a release-only change).
    pub writes_file: bool,
    pub approval: String,
}

/// Written result, distinct from runtime effectiveness.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Applied {
    pub change_id: String,
    /// Codex reads `config.toml` when a session starts.
    pub restart_required: bool,
}

/// One owned provider table and its observed relation to native bytes.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderBinding {
    pub provider_id: String,
    /// Current central display name.
    pub display_name: String,
    /// Owned table identity; `entry_digest` is never serialized here.
    #[serde(serialize_with = "public_owner")]
    pub owner: ProviderOwner,
    /// The central provider changed after the last deploy.
    pub outdated: bool,
    /// `written`, `modified`, `missing` or `unreadable`.
    pub state: String,
}

/// The owned default, if any, and its observed state.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DefaultBinding {
    pub provider_id: String,
    pub model_id: String,
    pub provider_key: String,
    /// Same vocabulary as provider bindings:
    /// - `written`: the three top-level keys are exactly what vibemate wrote;
    /// - `modified`: changed by Codex `/model` or by hand (at least one of
    ///   `model` / `model_provider` is still present);
    /// - `missing`: both `model` and `model_provider` are absent (removed by
    ///   hand, or `config.toml` itself is gone);
    /// - `unreadable`: the file or one of the keys cannot be interpreted.
    pub state: String,
}

/// Explicit pending-change observation, never a blind replay.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Recovery {
    pub id: String,
    pub provider_id: String,
    pub provider_key: String,
    pub action: String,
    /// `before`, `intended`, `conflict` or `unreadable`.
    pub observation: String,
    /// Present only for exact, unambiguous observations.
    pub approval: Option<String>,
}

/// Root-scoped settings state.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct State {
    /// Rust-selected `$CODEX_HOME`.
    pub config_root: String,
    pub target_path: String,
    pub providers: Vec<ProviderBinding>,
    pub default: Option<DefaultBinding>,
    /// A top-level `profile` key is set in `config.toml`.
    pub profile_active: bool,
    pub recovery: Option<Recovery>,
}

/// One retained successful change, without snapshot locations or values.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryEntry {
    pub id: String,
    pub action: String,
    pub provider_id: String,
    pub provider_key: String,
    /// Model the change set (`set_default`) or released (`clear_default`);
    /// null for `deploy` and `revoke`, which involve no model.
    pub model_id: Option<String>,
    pub created_at: i64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryPage {
    pub items: Vec<HistoryEntry>,
    pub next_cursor: Option<String>,
}

/// Newest-first page of completed changes in the active root.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HistoryRequest {
    /// Restrict to one provider, or null for the whole root.
    pub provider_id: Option<String>,
    pub after: Option<String>,
    /// Page bound (1–100).
    pub limit: u32,
}

/// Explicit confirmation of a pending-change observation.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RecoveryRequest {
    pub change_id: String,
    pub approval: String,
}

/// IPC view of [`ProviderOwner`]. `entry_digest` stays in Rust: it hashes the
/// whole table including the plaintext bearer token, and the frontend knows
/// every other field, so it could be used to test candidate keys offline.
fn public_owner<S: serde::Serializer>(
    owner: &ProviderOwner,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct View<'a> {
        provider_key: &'a str,
        provider_revision: i64,
        generation: &'a str,
    }
    View {
        provider_key: &owner.provider_key,
        provider_revision: owner.provider_revision,
        generation: &owner.generation,
    }
    .serialize(serializer)
}
