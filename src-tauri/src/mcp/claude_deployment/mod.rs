//! Claude Code's user-scope MCP entry lifecycle. React selects/reviews actions;
//! Rust owns the native path, OS credentials, entry ownership, safe writes and
//! recovery. No operation in this module starts a server or claims a live
//! connection.
//!
//! Scope boundary: vibemate writes only the top-level `mcpServers.<name>`
//! member of `<CLAUDE_CONFIG_DIR|home>/.claude.json`. It never writes
//! `projects.<path>.mcpServers` (local scope), `.mcp.json` (project scope) or
//! `managed-mcp.json` (enterprise scope). Every byte outside the owned member is
//! preserved exactly, because Claude Code keeps account, session and per-project
//! state in the same file.
//!
//! Claude Code has no user-level per-server enable flag (disabling is a
//! per-project `/mcp` toggle), so enable/disable are reported as unsupported
//! rather than emulated.

mod json_edit;
mod plan;
mod repository;
mod service;
mod state;
mod types;
pub use types::*;

use crate::{
    agents::DiscoveryEnvironment, config_apply::BackupStore, credentials::CredentialStore,
    storage::Storage,
};

/// Build a sanitized entry change and exact approval without accessing OS keys.
/// Paths come only from discovery. Errors report stale central state, unowned
/// or externally changed entries, unavailable copies, unsupported toggles and
/// pending recovery.
pub fn preview(
    storage: &Storage,
    environment: &DiscoveryEnvironment,
    selection: &Selection,
    backups: &BackupStore,
) -> Result<Preview, Error> {
    Ok(plan::build(storage, environment, selection, backups)?.view)
}

/// Confirm an exact plan, resolving OS keys only for deploy/update, and write
/// through the shared private journaled writer. Call under the app's credential
/// mutation lock. Cross-instance configuration locking spans intent and metadata.
/// Errors never contain values; uncertain results retain a recoverable intent.
pub fn apply(
    storage: &Storage,
    store: &dyn CredentialStore,
    environment: &DiscoveryEnvironment,
    request: &ApplyRequest,
    backups: &BackupStore,
) -> Result<Applied, Error> {
    service::apply(storage, store, environment, request, backups)
}

/// Observe registered entries and one pending root mutation without writing.
/// Rejects invalid scopes/cursors and unavailable storage; native problems are
/// explicit states, not inferred disconnection or successful registration.
pub fn read_state(
    storage: &Storage,
    environment: &DiscoveryEnvironment,
    request: &StateRequest,
    backups: &BackupStore,
) -> Result<State, Error> {
    state::read_state(storage, environment, request, backups)
}

/// Read a newest-first retained entry-history page for the active Claude root.
/// Returns safe request/storage errors; no snapshot paths or values leave Rust.
pub fn history(
    storage: &Storage,
    environment: &DiscoveryEnvironment,
    request: &HistoryRequest,
) -> Result<HistoryPage, Error> {
    state::read_history(storage, environment, request)
}

/// Confirm an exact interrupted-state observation, never replaying a file write.
/// Finalizes intended ownership or preserves the before-state only when exact
/// digests and journal snapshots agree; divergence remains blocked and retryable.
pub fn recover(
    storage: &Storage,
    environment: &DiscoveryEnvironment,
    request: &RecoveryRequest,
    backups: &BackupStore,
) -> Result<Applied, Error> {
    service::recover(storage, environment, request, backups)
}

#[cfg(test)]
mod tests;
