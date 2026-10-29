//! Read-only Pi MCP preview orchestration. Resolve the user-level target in Rust;
//! neither frontend paths nor raw native documents are accepted or returned.

use serde::{Deserialize, Serialize};

use super::{McpError, PiMcpPreflight, PiMcpPreflightError, preflight_pi_registration};
use crate::{
    agents::DiscoveryEnvironment,
    config_plan::{TargetFile, read_target_bytes},
    storage::Storage,
};

/// Revision-checked request; deliberately has no path or document field.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PreviewPiMcpRequest {
    /// Stable central definition identity.
    pub definition_id: String,
    /// Revision shown in the central list.
    pub expected_revision: i64,
}

/// Safe preview failures without native file content, paths or parser messages.
#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PiMcpPreviewError {
    /// Malformed identity or revision.
    InvalidRequest,
    /// Central definition is absent, invalid or storage is unavailable.
    DefinitionUnavailable,
    /// The central definition changed since the list was read.
    RevisionConflict,
    /// Target location, file type, size or read access prevents preview.
    TargetUnavailable,
    /// Existing file is not a valid Pi MCP JSON container.
    InvalidConfig,
    /// An existing native entry owns the requested namespace.
    NameConflict,
    /// Desktop work could not finish.
    OperationFailed,
}

/// New-entry preview only; no merge, ownership claim or application token.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PiMcpPreview {
    /// Rust-selected target and observed identity; no native content is included.
    pub target: TargetFile,
    /// Sanitized proposal built from the current central definition.
    pub proposal: PiMcpPreflight,
}

/// Resolve `<agent-dir>/mcp.json` and preview one new native entry.
///
/// Honors the same absolute `PI_CODING_AGENT_DIR` override/default rules as Agent
/// discovery. Uses the shared target reader and configuration size checks, never
/// opens the OS credential store, project files or executable metadata. Native
/// file bytes are inspected only for container shape and namespace collisions;
/// no existing values enter the result. Creates no directories or files. This
/// does not prove a running session uses built-in MCP or trusts any project.
///
/// # Errors
/// Returns safe request, target, central-revision and namespace errors. No raw
/// filesystem or JSON error is retained in the response.
pub fn preview_pi_registration(
    storage: &Storage,
    environment: &DiscoveryEnvironment,
    request: &PreviewPiMcpRequest,
) -> Result<PiMcpPreview, PiMcpPreviewError> {
    use PiMcpPreviewError::*;
    if !crate::shared::is_random_id(&request.definition_id) || request.expected_revision < 1 {
        return Err(InvalidRequest);
    }
    let (root, _) = crate::agents::pi_config_root(environment);
    let path = root.ok_or(TargetUnavailable)?.join("mcp.json");
    let (target, bytes) = read_target_bytes(&path).map_err(|_| TargetUnavailable)?;
    let proposal = preflight_pi_registration(
        storage,
        &request.definition_id,
        request.expected_revision,
        target.exists.then_some(bytes.as_slice()),
    )
    .map_err(|error| match error {
        PiMcpPreflightError::Definition(McpError::RevisionConflict) => RevisionConflict,
        PiMcpPreflightError::Definition(_) => DefinitionUnavailable,
        PiMcpPreflightError::InvalidConfig => InvalidConfig,
        PiMcpPreflightError::NameConflict => NameConflict,
    })?;
    let mut proposal = proposal;
    // Display-only copy: the writer uses the preflight entry, never this one.
    if let Some(map) = proposal.redacted_entry.as_object_mut() {
        crate::secret_text::mask_public_fields(map);
    }
    Ok(PiMcpPreview { target, proposal })
}
