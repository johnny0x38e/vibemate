//! Safe Grok MCP workflow contracts. None of these types contain resolved values.

use crate::config_apply::VersionFileSummary;
use crate::config_plan::TargetFile;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// One supported, explicitly reviewed entry mutation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    /// Create or refresh from the current central definition and OS credentials.
    Deploy,
    /// Enable the owned native entry without replacing its credentials.
    Enable,
    /// Disable the owned native entry without deleting it.
    Disable,
    /// Remove only the owned entry and release its central reference.
    Revoke,
    /// Restore only this entry from a retained successful change.
    Restore,
}
impl Action {
    pub(super) fn code(self) -> &'static str {
        match self {
            Self::Deploy => "deploy",
            Self::Enable => "enable",
            Self::Disable => "disable",
            Self::Revoke => "revoke",
            Self::Restore => "restore",
        }
    }
}

/// Strict selection, with no frontend path, configuration content or executable.
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Selection {
    /// Stable central definition.
    pub definition_id: String,
    /// Revision reviewed by the user.
    pub expected_revision: i64,
    /// Requested entry action.
    pub action: Action,
    /// Successful retained change, required only for restore.
    pub history_id: Option<String>,
}

/// Confirmation of an exact rebuilt entry plan.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ApplyRequest {
    /// Same selection used to obtain the preview.
    pub selection: Selection,
    /// Hash of target, central revision, ownership, action and history source.
    pub approval: String,
}

/// Safe failures; no native/credential diagnostics are serialized.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Error {
    /// Invalid identity, selection or pagination.
    InvalidRequest,
    /// Storage or metadata is unavailable.
    StorageUnavailable,
    /// The central definition is absent or changed.
    DefinitionChanged,
    /// Native target is unreadable, oversized, read-only or a symbolic link.
    TargetUnavailable,
    /// Native TOML has an invalid MCP container.
    InvalidConfig,
    /// A user entry already owns the requested exact name.
    NameConflict,
    /// Owned bytes changed outside vibemate; do not overwrite them.
    OwnershipConflict,
    /// A fresh preview is required after a state change.
    PlanStale,
    /// OS credentials are missing, invalid, dynamic or inaccessible.
    CredentialUnavailable,
    /// Another interrupted change must be explicitly reconciled first.
    RecoveryRequired,
    /// A retained result is missing, corrupt or outside the requested scope.
    SnapshotUnavailable,
    /// The shared writer refused the replacement; no successful apply is claimed.
    WriteFailed,
    /// A result could not be established; read state and reconcile before retrying.
    OutcomeUnknown,
    /// A central definition is still registered or involved in a pending change.
    DefinitionInUse,
    /// Grok cannot admit qualified tools with this server prefix.
    UnsupportedServerName,
    /// A public field would introduce Grok environment expansion.
    DynamicValueUnsupported,
    /// Relative cwd or leading home expansion has not been verified for Grok.
    PathSemanticsUnverified,
}

/// Exact native-entry ownership. Entry digest includes protected values but not
/// the rest of the file, so unrelated servers and model tables may change alone.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Ownership {
    /// Exact native server key under `mcp_servers`.
    pub server_name: String,
    /// Central revision last deployed (not a connection status).
    pub definition_revision: i64,
    /// SHA-256 of the complete native entry as a canonical JSON Value.
    pub entry_digest: String,
    /// Last successful change identity, preventing ABA approval reuse.
    pub generation: String,
    /// Enabled flag at the time ownership was recorded.
    pub enabled: bool,
}

/// Sanitized field summary and before/after entry details for confirmation.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Preview {
    /// Trusted target and current full-file digest.
    pub target: TargetFile,
    /// Central identity selected for this change.
    pub definition_id: String,
    /// Central revision checked by this preview.
    pub definition_revision: i64,
    /// Requested action.
    pub action: Action,
    /// Native entry key.
    pub server_name: String,
    /// Existing entry only, with all protected values masked.
    pub before_entry: Option<Value>,
    /// Proposed entry only, or null for revocation.
    pub after_entry: Option<Value>,
    /// Shared field-difference representation over sanitized entry fragments.
    pub summary: VersionFileSummary,
    /// Exact confirmation hash; never an executable or raw credential.
    pub approval: String,
    /// Whether `disabled_mcp_servers` independently lists this exact name.
    /// Enable writes do not clear that list; runtime may still stay inactive.
    pub disabled_by_user_list: bool,
}

/// Written change, distinct from runtime effectiveness or a live connection.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Applied {
    /// Durable change identity for history/reconciliation.
    pub change_id: String,
    /// Native entry remains registered after this change.
    pub registered: bool,
    /// Grok needs a new session or `/mcps` refresh; no server was started here.
    pub reload_required: bool,
}

/// One managed registration and the observed relationship to native bytes.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Deployment {
    /// Central identity.
    pub definition_id: String,
    /// Current central display name.
    pub display_name: String,
    /// Root the row owns; never accepted as a frontend write path.
    pub config_root: String,
    /// Recorded native ownership metadata.
    #[serde(serialize_with = "public_ownership")]
    pub ownership: Ownership,
    /// `written`, `modified`, `missing` or `unreadable`; never `connected`.
    pub state: String,
    /// Sorted native keys vibemate does not manage (added by hand). Key names
    /// only, never values. Non-empty only for a `modified` row, which every
    /// write refuses because rewriting the table would drop these keys.
    pub unmanaged_keys: Vec<String>,
}

/// One retained successful entry result, without snapshot locations or values.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryEntry {
    /// Durable selection identity.
    pub id: String,
    /// Action that produced this entry state.
    pub action: String,
    /// Unix milliseconds.
    pub created_at: i64,
    /// Native key at that time.
    pub server_name: String,
    /// Null means the entry was revoked.
    pub enabled: Option<bool>,
}

/// Explicit pending-change observation, not permission to blindly replay writes.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Recovery {
    /// Pending change identity.
    pub id: String,
    /// Central identity, retained even after an interrupted operation.
    pub definition_id: String,
    /// Affected native key.
    pub server_name: String,
    /// Original action.
    pub action: String,
    /// `before`, `intended`, `conflict` or `unreadable`.
    pub observation: String,
    /// Only exact, unambiguous observations receive a confirmation token.
    pub approval: Option<String>,
}

/// Fixed-root read state. Definitions can also list registrations in old roots
/// so users know which `GROK_HOME` must be selected before revocation.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct State {
    /// Current Rust-selected root.
    pub config_root: String,
    /// Current target file path.
    pub target_path: String,
    /// Managed rows (one definition across roots, or a root-scoped page).
    pub deployments: Vec<Deployment>,
    /// Next root-page definition identity, absent for a definition-scoped read.
    pub next_cursor: Option<String>,
    /// At most one pending mutation in this root.
    pub recovery: Option<Recovery>,
}

/// Bounded retained-entry history page for one definition and active root.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryPage {
    /// Newest retained changes first.
    pub items: Vec<HistoryEntry>,
    /// Last returned change identity when another page exists.
    pub next_cursor: Option<String>,
}

/// Strict state request; no target paths are accepted.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StateRequest {
    /// A definition across all roots, or null for active-root registrations.
    pub definition_id: Option<String>,
    /// Last root-page identity, or null.
    pub after: Option<String>,
    /// Page bound (1–100).
    pub limit: u32,
}

/// Strict history read scoped to the current environment and definition.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HistoryRequest {
    /// Central definition.
    pub definition_id: String,
    /// Last retained change identity, or null.
    pub after: Option<String>,
    /// Page bound (1–100).
    pub limit: u32,
}

/// Explicit confirmation of a pending-change observation.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RecoveryRequest {
    /// Pending change in the active Rust-selected root.
    pub change_id: String,
    /// Hash of the complete current observation.
    pub approval: String,
}

/// IPC view of [`Ownership`]. `entry_digest` stays in Rust: it is a hash over
/// the native entry including protected values (env / headers), so a client
/// that knows the other fields could test candidate secrets against it.
fn public_ownership<S: serde::Serializer>(
    ownership: &Ownership,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct View<'a> {
        server_name: &'a str,
        definition_revision: i64,
        generation: &'a str,
        enabled: bool,
    }
    View {
        server_name: &ownership.server_name,
        definition_revision: ownership.definition_revision,
        generation: &ownership.generation,
        enabled: ownership.enabled,
    }
    .serialize(serializer)
}
