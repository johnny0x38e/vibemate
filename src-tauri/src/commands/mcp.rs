//! Central MCP metadata and credential IPC commands; no server is executed or contacted.

use super::state::{CredentialLock, with_mcp_storage};
use crate::credentials::OsCredentialStore;
use tauri::Manager;

/// Preview a Rust-selected Pi MCP target without credentials, writes or execution.
#[tauri::command]
pub(crate) async fn preview_pi_mcp_registration(
    app: tauri::AppHandle,
    request: crate::mcp::PreviewPiMcpRequest,
) -> Result<crate::mcp::PiMcpPreview, crate::mcp::PiMcpPreviewError> {
    use crate::mcp::PiMcpPreviewError;
    let environment =
        super::agents::environment(&app).map_err(|_| PiMcpPreviewError::TargetUnavailable)?;
    tauri::async_runtime::spawn_blocking(move || {
        let status = app
            .try_state::<crate::storage::StorageStatus>()
            .ok_or(PiMcpPreviewError::DefinitionUnavailable)?;
        let storage = status
            .storage()
            .map_err(|_| PiMcpPreviewError::DefinitionUnavailable)?;
        crate::mcp::preview_pi_registration(storage, &environment, &request)
    })
    .await
    .map_err(|_| PiMcpPreviewError::OperationFailed)?
}

/// Preview the Rust-selected Grok TOML target without keys, writes or execution.
#[tauri::command]
pub(crate) async fn preview_grok_mcp_registration(
    app: tauri::AppHandle,
    request: crate::mcp::PreviewGrokMcpRequest,
) -> Result<crate::mcp::GrokMcpPreview, crate::mcp::GrokMcpPreviewError> {
    use crate::mcp::GrokMcpPreviewError;
    let environment =
        super::agents::environment(&app).map_err(|_| GrokMcpPreviewError::TargetUnavailable)?;
    tauri::async_runtime::spawn_blocking(move || {
        let status = app
            .try_state::<crate::storage::StorageStatus>()
            .ok_or(GrokMcpPreviewError::DefinitionUnavailable)?;
        let storage = status
            .storage()
            .map_err(|_| GrokMcpPreviewError::DefinitionUnavailable)?;
        crate::mcp::preview_grok_registration(storage, &environment, &request)
    })
    .await
    .map_err(|_| GrokMcpPreviewError::OperationFailed)?
}

/// Read central MCP metadata, without executing servers or opening credentials.
#[tauri::command]
pub(crate) async fn list_mcp_definitions(
    app: tauri::AppHandle,
    after: Option<String>,
    limit: u32,
) -> Result<crate::mcp::McpPage, crate::mcp::McpError> {
    tauri::async_runtime::spawn_blocking(move || {
        with_mcp_storage(&app, |storage| {
            crate::mcp::list_definitions(storage, after.as_deref(), limit)
        })
    })
    .await
    .map_err(|_| crate::mcp::McpError::OperationFailed)?
}

/// Read one non-secret central definition for editing or result reconciliation.
#[tauri::command]
pub(crate) async fn get_mcp_definition(
    app: tauri::AppHandle,
    id: String,
) -> Result<crate::mcp::McpRecord, crate::mcp::McpError> {
    tauri::async_runtime::spawn_blocking(move || {
        with_mcp_storage(&app, |storage| crate::mcp::get_definition(storage, &id))
    })
    .await
    .map_err(|_| crate::mcp::McpError::OperationFailed)?
}

/// Save only central metadata and OS credential references. No process or HTTP
/// API is involved; the credential lock excludes in-process secret mutations.
#[tauri::command]
pub(crate) async fn save_mcp_definition(
    app: tauri::AppHandle,
    request: crate::mcp::SaveMcpRequest,
) -> Result<crate::mcp::McpRecord, crate::mcp::McpError> {
    tauri::async_runtime::spawn_blocking(move || {
        with_mcp_storage(&app, |storage| {
            let credential_lock = app
                .try_state::<CredentialLock>()
                .ok_or(crate::mcp::McpError::OperationFailed)?;
            let _guard = credential_lock
                .0
                .lock()
                .map_err(|_| crate::mcp::McpError::OperationFailed)?;
            let now = crate::shared::current_unix_millis()
                .map_err(|_| crate::mcp::McpError::OperationFailed)?;
            crate::mcp::save_definition(storage, &OsCredentialStore, request, now)
        })
    })
    .await
    .map_err(|_| crate::mcp::McpError::OperationFailed)?
}

/// Retry deletion of recorded obsolete credential entries and return safe status.
#[tauri::command]
pub(crate) async fn cleanup_mcp_credentials(
    app: tauri::AppHandle,
    id: String,
) -> Result<crate::mcp::McpRecord, crate::mcp::McpError> {
    tauri::async_runtime::spawn_blocking(move || {
        with_mcp_storage(&app, |storage| {
            let credential_lock = app
                .try_state::<CredentialLock>()
                .ok_or(crate::mcp::McpError::OperationFailed)?;
            let _guard = credential_lock
                .0
                .lock()
                .map_err(|_| crate::mcp::McpError::OperationFailed)?;
            crate::mcp::get_definition(storage, &id)?;
            crate::mcp::cleanup_credentials(storage, &OsCredentialStore, &id)?;
            crate::mcp::get_definition(storage, &id)
        })
    })
    .await
    .map_err(|_| crate::mcp::McpError::OperationFailed)?
}

/// Delete a revision-checked central definition; Agent configuration is untouched.
#[tauri::command]
pub(crate) async fn delete_mcp_definition(
    app: tauri::AppHandle,
    request: crate::mcp::DeleteMcpRequest,
) -> Result<crate::mcp::DeletedMcp, crate::mcp::McpError> {
    tauri::async_runtime::spawn_blocking(move || {
        with_mcp_storage(&app, |storage| {
            let lock = app
                .try_state::<CredentialLock>()
                .ok_or(crate::mcp::McpError::OperationFailed)?;
            let _guard = lock
                .0
                .lock()
                .map_err(|_| crate::mcp::McpError::OperationFailed)?;
            crate::mcp::delete_definition(storage, &OsCredentialStore, request)
        })
    })
    .await
    .map_err(|_| crate::mcp::McpError::OperationFailed)?
}

/// Retry durable credential cleanup for removed definitions, in bounded batches.
#[tauri::command]
pub(crate) async fn cleanup_deleted_mcp_credentials(
    app: tauri::AppHandle,
) -> Result<bool, crate::mcp::McpError> {
    tauri::async_runtime::spawn_blocking(move || {
        with_mcp_storage(&app, |storage| {
            let lock = app
                .try_state::<CredentialLock>()
                .ok_or(crate::mcp::McpError::OperationFailed)?;
            let _guard = lock
                .0
                .lock()
                .map_err(|_| crate::mcp::McpError::OperationFailed)?;
            crate::mcp::cleanup_deleted_credentials(storage, &OsCredentialStore)
        })
    })
    .await
    .map_err(|_| crate::mcp::McpError::OperationFailed)?
}
