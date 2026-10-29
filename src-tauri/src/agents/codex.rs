//! Codex write adapter (P41): user-level `$CODEX_HOME/config.toml` provider
//! tables and an optional owned default. MCP entries live in
//! `crate::mcp::codex_deployment`; Skills use the shared deployment module.
//!
//! Codex 0.165.0 accepts only `wire_api = "responses"`, so only Responses-
//! capable providers/models are written. vibemate owns exactly what SQLite
//! records: `[model_providers.vibemate-*]` tables (`codex_provider_binding`)
//! and, after an explicit "set as Codex default", the top-level `model` and
//! `model_provider` keys plus `model_reasoning_effort` when the central model
//! states an effort (`codex_default_binding.writes_effort`); a user's own
//! effort is never removed.
//! It never writes `profile`, `[profiles.*]`, `model_catalog_json`, `env_key`
//! or `auth`, and never takes over a table it did not create. The credential is
//! written as plaintext `experimental_bearer_token` into the 0600 file, read
//! from the OS store only in apply. Edits go through `toml_edit`, so comments,
//! key order and unrelated tables stay byte-identical. Codex itself rewrites
//! `model`/`model_reasoning_effort` (`/model`) and `[mcp_servers]`
//! (`codex mcp add`); those writes surface as `plan_stale` or `modified`.

mod plan;
mod repository;
mod service;
mod state;
mod types;
pub use types::*;

#[cfg(test)]
use plan::{DEEPSEEK_RESPONSES_MODELS, KEY_PREFIX, RESERVED_PROVIDER_IDS};

use crate::{
    agents::DiscoveryEnvironment, config_apply::BackupStore, credentials::CredentialStore,
    storage::Storage,
};

/// Build a masked plan and exact approval without reading any OS credential.
/// Errors cover stale central state, unsupported protocols/models, unowned or
/// externally changed tables, unavailable targets and pending recovery.
pub fn preview(
    storage: &Storage,
    environment: &DiscoveryEnvironment,
    selection: &Selection,
) -> Result<Preview, Error> {
    Ok(plan::build(storage, environment, selection)?.view)
}

/// Confirm an exact plan; only `deploy` reads the provider credential. Call
/// under the app's credential lock. Uncertain results keep a recoverable intent.
pub fn apply(
    storage: &Storage,
    store: &dyn CredentialStore,
    environment: &DiscoveryEnvironment,
    request: &ApplyRequest,
    backups: &BackupStore,
) -> Result<Applied, Error> {
    service::apply(storage, store, environment, request, backups)
}

/// Observe owned tables, the owned default and one pending change without writing.
pub fn read_state(
    storage: &Storage,
    environment: &DiscoveryEnvironment,
    backups: &BackupStore,
) -> Result<State, Error> {
    state::read_state(storage, environment, backups)
}

/// Newest-first completed changes in the active root; no values leave Rust.
pub fn history(
    storage: &Storage,
    environment: &DiscoveryEnvironment,
    request: &HistoryRequest,
) -> Result<HistoryPage, Error> {
    state::read_history(storage, environment, request)
}

/// Confirm an exact interrupted-state observation; never replays a write.
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
