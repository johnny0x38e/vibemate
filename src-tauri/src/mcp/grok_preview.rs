//! Read-only Grok MCP preview orchestration. Rust selects the user-level TOML
//! target; neither frontend paths nor existing native document values leave it.

use serde::{Deserialize, Serialize};

use super::{GrokMcpPreflight, GrokMcpPreflightError, McpError, preflight_grok_registration};
use crate::{
    agents::DiscoveryEnvironment,
    config_plan::{TargetFile, read_target_bytes},
    storage::Storage,
};

/// Strict central selection, deliberately excluding native paths and documents.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PreviewGrokMcpRequest {
    /// Stable central definition identity.
    pub definition_id: String,
    /// Revision displayed in the central list.
    pub expected_revision: i64,
}

/// Safe failures with no native values, filesystem diagnostics or parser text.
#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GrokMcpPreviewError {
    /// Identity or revision is invalid.
    InvalidRequest,
    /// Central metadata is missing, invalid or unavailable.
    DefinitionUnavailable,
    /// The definition changed since the list was read.
    RevisionConflict,
    /// Root resolution, file type, size or read access prevents preview.
    TargetUnavailable,
    /// TOML or a relevant native container cannot be read safely.
    InvalidConfig,
    /// The exact native server name already exists; it cannot be adopted.
    NameConflict,
    /// Grok cannot admit qualified tool names with this server prefix.
    UnsupportedServerName,
    /// A public stdio field contains unsupported runtime interpolation.
    DynamicValueUnsupported,
    /// Relative cwd or home expansion needs verified native semantics first.
    PathSemanticsUnverified,
    /// Desktop work could not finish.
    OperationFailed,
}

/// Rust-selected target metadata and a masked first-registration proposal.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GrokMcpPreview {
    /// Observed target identity, not the contents of the existing file.
    pub target: TargetFile,
    /// Current central revision and proposed new entry, without credential values.
    pub proposal: GrokMcpPreflight,
}

/// Preview `<GROK_HOME|~/.grok>/config.toml` without writing or running Grok.
///
/// Uses Agent discovery's absolute-root rules and the shared target reader.
/// Reads only central metadata and this user-level file: no OS credentials,
/// executable/version metadata, project configs, processes or network requests.
/// A readable symlink is reported, not replaced or approved for later writing.
/// No directories or files are created. The personal disable-list observation
/// does not account for project/policy/compatibility overrides or live sessions.
///
/// # Errors
/// Returns fixed request, revision, target, container, name and unsupported-input
/// codes. No native bytes or parser/filesystem diagnostics enter the response.
pub fn preview_grok_registration(
    storage: &Storage,
    environment: &DiscoveryEnvironment,
    request: &PreviewGrokMcpRequest,
) -> Result<GrokMcpPreview, GrokMcpPreviewError> {
    use GrokMcpPreviewError::*;
    if !crate::shared::is_random_id(&request.definition_id) || request.expected_revision < 1 {
        return Err(InvalidRequest);
    }
    let (root, _) = crate::agents::grok_config_root(environment);
    let path = root.ok_or(TargetUnavailable)?.join("config.toml");
    let (target, bytes) = read_target_bytes(&path).map_err(|_| TargetUnavailable)?;
    let proposal = preflight_grok_registration(
        storage,
        &request.definition_id,
        request.expected_revision,
        target.exists.then_some(bytes.as_slice()),
    )
    .map_err(|error| match error {
        GrokMcpPreflightError::Definition(McpError::RevisionConflict) => RevisionConflict,
        GrokMcpPreflightError::Definition(_) => DefinitionUnavailable,
        GrokMcpPreflightError::InvalidConfig => InvalidConfig,
        GrokMcpPreflightError::NameConflict => NameConflict,
        GrokMcpPreflightError::UnsupportedServerName => UnsupportedServerName,
        GrokMcpPreflightError::DynamicValueUnsupported => DynamicValueUnsupported,
        GrokMcpPreflightError::PathSemanticsUnverified => PathSemanticsUnverified,
    })?;
    let mut proposal = proposal;
    // Display-only copy: the writer uses the preflight entry, never this one.
    if let Some(map) = proposal.redacted_entry.as_object_mut() {
        crate::secret_text::mask_public_fields(map);
    }
    Ok(GrokMcpPreview { target, proposal })
}
