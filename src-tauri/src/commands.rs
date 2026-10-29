//! Desktop IPC composition. Each feature owns its command module; managed-state
//! coordination is shared here, while domain behavior remains independent of Tauri.

pub(crate) mod agents;
pub(crate) mod app;
pub(crate) mod claude_preference;
pub(crate) mod codex;
pub(crate) mod configuration_preview;
pub(crate) mod mcp;
pub(crate) mod mcp_deploy;
pub(crate) mod models;
pub(crate) mod preferences;
pub(crate) mod providers;
pub(crate) mod skills;
mod state;

pub(crate) use state::CredentialLock;

pub(crate) mod grok;
pub(crate) mod pi;
