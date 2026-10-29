//! Static Agent discovery and a separate user-confirmed CLI version operation.
//! Discovery never executes entries. Version execution is bounded, not sandboxed;
//! the launched program can have side effects outside vibemate's control.

pub(crate) mod claude_code;
pub mod codex;
mod configuration;
pub mod grok_build;
pub mod history;
pub mod pi;
mod pi_session;
pub mod recovery;
pub use configuration::{ConfigApplied, ConfigApplyError};
mod discovery;
pub mod history_delete;
mod version_probe;
pub mod version_restore;
pub use claude_code::{
    ApplyClaudeCodeRestoreRequest, ClaudeCodeApplied, ClaudeCodeApplyError, ClaudeCodeApplyInput,
    ClaudeCodePlanInput, ClaudeCodeRestoreError, ClaudeCodeRestorePlan, ClaudeCodeRestoreStep,
    GatewayStatus, ModelFacts, ProviderFacts, ReviewedPlan, ReviewedRestoreStepRequest,
    apply_claude_code_restore, apply_claude_code_settings, apply_config_restore, gateway_status,
    plan_claude_code_restore, plan_claude_code_settings, plan_config_restore,
};
pub use version_probe::{
    AgentVersionResult, VersionProbeError, VersionProbeGate, VersionProbeInput, probe_agent_version,
};
#[cfg(test)]
mod tests;

/// One apply request: the identity the user selected and the tokens of the plan
/// they reviewed.
///
/// The whole payload is checked before any storage or credential access, and
/// every token is compared with a fresh read before a file is written.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ApplyClaudeCodeSettingsRequest {
    /// Stored Provider instance the plan was built from.
    pub provider_id: String,
    /// Upstream model ID the plan was built from.
    pub model_id: String,
    /// Tokens the interface received with the plan.
    pub reviewed: ReviewedPlan,
}

/// Claude Code settings root, shared by discovery and the configuration preview.
pub(crate) use discovery::claude_config_root;
/// Codex user state root (`CODEX_HOME` or `~/.codex`), shared by discovery,
/// provider/MCP writes and skill deployment.
pub(crate) use discovery::codex_config_root;
pub use discovery::discover_agents;
/// Grok Build root (`GROK_HOME` or `~/.grok`), shared by configuration writes
/// and skill deployment.
pub(crate) use discovery::grok_config_root;
/// Pi agent directory (`PI_CODING_AGENT_DIR` or `~/.pi/agent`), shared by
/// configuration writes and skill deployment.
pub(crate) use discovery::pi_config_root;
use serde::{Deserialize, Serialize};
use std::{ffi::OsString, path::PathBuf};

/// Fixed Agent identities with read-only detection rules, not deployment support.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum AgentId {
    /// Anthropic Claude Code CLI.
    ClaudeCode,
    /// OpenAI Codex CLI.
    Codex,
    /// Anomaly OpenCode CLI, not the unrelated Go project with the same name.
    OpenCode,
    /// Nous Research Hermes Agent CLI.
    Hermes,
    /// Earendil Pi, not other tools named pi.
    Pi,
    /// xAI Grok Build.
    GrokBuild,
}

/// Optional entry paths supplied explicitly by the user; never executed.
#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DiscoveryInput {
    /// Absolute Claude Code entry, or automatic lookup when absent.
    #[serde(default)]
    pub claude_path: Option<String>,
    /// Absolute Codex entry, or automatic lookup when absent.
    #[serde(default)]
    pub codex_path: Option<String>,
    /// Absolute OpenCode entry, or automatic lookup when absent.
    #[serde(default)]
    pub opencode_path: Option<String>,
    /// Absolute Hermes entry, or automatic lookup when absent.
    #[serde(default)]
    pub hermes_path: Option<String>,
    /// Absolute Pi entry path, or automatic lookup when absent.
    pub pi_path: Option<String>,
    /// Absolute Grok entry path, or automatic lookup when absent.
    pub grok_path: Option<String>,
}

/// Process environment snapshot, injectable for isolated filesystem tests.
#[derive(Default)]
pub struct DiscoveryEnvironment {
    /// Platform home resolved by Tauri, not a project directory.
    pub home: PathBuf,
    /// Desktop process PATH; relative entries are ignored.
    pub path: Option<OsString>,
    /// Explicit Pi configuration directory from the desktop environment.
    pub pi_directory: Option<OsString>,
    /// Explicit Grok configuration root from the desktop environment.
    pub grok_directory: Option<OsString>,
    /// Claude user settings root (CLAUDE_CONFIG_DIR).
    pub claude_directory: Option<OsString>,
    /// Codex user state root (CODEX_HOME); explicit overrides must exist.
    pub codex_directory: Option<OsString>,
    /// OpenCode candidate directory (OPENCODE_CONFIG_DIR); semantics vary by version.
    pub opencode_directory: Option<OsString>,
    /// OpenCode's XDG configuration base when no specific directory is supplied.
    pub xdg_config_directory: Option<OsString>,
    /// Hermes user data root (HERMES_HOME), independent of its source checkout.
    pub hermes_directory: Option<OsString>,
    /// Windows LOCALAPPDATA for documented Hermes source-install paths.
    pub local_app_data: Option<OsString>,
}

/// Safe discovery failures; I/O diagnostics and user content are discarded.
#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DiscoveryError {
    /// A manual path is relative, too long or contains control characters.
    InvalidPath,
    /// The OS home cannot be located or represented safely.
    LocationUnavailable,
    /// The desktop blocking task could not complete.
    OperationFailed,
}

/// Filesystem observation, never a running/connected/install-authenticity state.
#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum EntryState {
    /// The selected entry resolves to a regular file.
    Found,
    /// No entry was found in the bounded search.
    NotFound,
    /// An entry exists but cannot be resolved or is not a regular file.
    Unavailable,
}

/// Evidence source for an entry; PATH lookup retains shadowing order.
#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum EntrySource {
    /// Explicit current-window input.
    Manual,
    /// First matching entry in the desktop process PATH.
    Path,
    /// Documented user installation entry.
    UserEntry,
    /// No entry located.
    None,
}

/// A version claim's provenance; neither source proves binary authenticity.
#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum VersionEvidence {
    /// Matching Pi package name and bin target in bounded package metadata.
    PackageMetadata,
    /// Grok versioned download filename only; always unverified.
    Filename,
    /// No safe version claim was available.
    None,
}

/// Applicability of the recorded configuration contract, not deployment status.
#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum CapabilityEvidence {
    /// Pi metadata matches the exact 1.1.0 contract documented in P02.
    Documented,
    /// Identity/version is insufficient to apply a version-specific contract.
    Unverified,
}

/// Configuration root provenance; SDK/CLI overrides are not observable here.
#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum DirectorySource {
    /// Default path below the platform home.
    Default,
    /// Absolute desktop-process environment override.
    Environment,
    /// Override was invalid; no fallback is presented as effective.
    Unavailable,
}

/// Presence/type only; no Agent configuration content is read.
#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum FileState {
    /// Existing regular file (symlinks may resolve to a regular file).
    Present,
    /// Path is absent.
    Missing,
    /// I/O failed or path points to another type.
    Unavailable,
}

/// A fixed known configuration filename and its filesystem observation.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigurationFile {
    /// Fixed filename from the adapter, never content extracted from a file.
    pub name: &'static str,
    /// Existence and type, without reading contents.
    pub state: FileState,
}

/// One Agent observation. Paths and version claims are bounded display data.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentObservation {
    /// Stable Agent identity.
    pub id: AgentId,
    /// Entry observation, independent from configuration presence.
    pub state: EntryState,
    /// How the entry was chosen.
    pub source: EntrySource,
    /// Original absolute entry path, when a candidate exists.
    pub entry_path: Option<String>,
    /// Resolved regular-file target, only when found.
    pub resolved_path: Option<String>,
    /// Safe semantic version claim; not obtained by running the Agent.
    pub version: Option<String>,
    /// Version provenance.
    pub version_evidence: VersionEvidence,
    /// Whether the recorded version-specific contract applies.
    pub capability_evidence: CapabilityEvidence,
    /// Chosen user config root; not necessarily effective at runtime.
    pub config_directory: Option<String>,
    /// Default/environment provenance for that root.
    pub directory_source: DirectorySource,
    /// Fixed config paths checked for presence without reading content.
    pub files: Vec<ConfigurationFile>,
}
