//! Thin Codex settings IPC (P41). Rust resolves `$CODEX_HOME` and backup paths;
//! React sends identities, actions and approvals only, and never a credential.

use crate::{
    agents::{DiscoveryEnvironment, codex},
    config_apply::BackupStore,
    storage::{Storage, StorageStatus},
};
use tauri::Manager;

fn context(app: &tauri::AppHandle) -> Result<(DiscoveryEnvironment, BackupStore), codex::Error> {
    let environment =
        super::agents::environment(app).map_err(|_| codex::Error::TargetUnavailable)?;
    let backups = app
        .path()
        .app_data_dir()
        .map_err(|_| codex::Error::StorageUnavailable)?
        .join("backups");
    Ok((environment, BackupStore::new(backups)))
}

fn with_storage<T>(
    app: &tauri::AppHandle,
    operation: impl FnOnce(&Storage) -> Result<T, codex::Error>,
) -> Result<T, codex::Error> {
    let status = app
        .try_state::<StorageStatus>()
        .ok_or(codex::Error::StorageUnavailable)?;
    let storage = status
        .storage()
        .map_err(|_| codex::Error::StorageUnavailable)?;
    operation(storage)
}

/// Read owned provider tables, the owned default and a pending recovery.
#[tauri::command]
pub(crate) async fn codex_settings_state(
    app: tauri::AppHandle,
) -> Result<codex::State, codex::Error> {
    let (environment, backups) = context(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        with_storage(&app, |storage| {
            codex::read_state(storage, &environment, &backups)
        })
    })
    .await
    .map_err(|_| codex::Error::OutcomeUnknown)?
}

/// Read a bounded page of completed Codex settings changes in the active root.
#[tauri::command]
pub(crate) async fn codex_settings_history(
    app: tauri::AppHandle,
    request: codex::HistoryRequest,
) -> Result<codex::HistoryPage, codex::Error> {
    let (environment, _) = context(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        with_storage(&app, |storage| {
            codex::history(storage, &environment, &request)
        })
    })
    .await
    .map_err(|_| codex::Error::OutcomeUnknown)?
}

/// Build one masked plan and its approval without reading OS credentials.
#[tauri::command]
pub(crate) async fn preview_codex_settings_change(
    app: tauri::AppHandle,
    selection: codex::Selection,
) -> Result<codex::Preview, codex::Error> {
    let (environment, _) = context(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        with_storage(&app, |storage| {
            codex::preview(storage, &environment, &selection)
        })
    })
    .await
    .map_err(|_| codex::Error::OutcomeUnknown)?
}

/// Confirm an exact plan; only `deploy` opens the provider credential.
#[tauri::command]
pub(crate) async fn apply_codex_settings_change(
    app: tauri::AppHandle,
    request: codex::ApplyRequest,
) -> Result<codex::Applied, codex::Error> {
    let (environment, backups) = context(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        super::state::with_credential_lock(&app, codex::Error::OutcomeUnknown, || {
            with_storage(&app, |storage| {
                codex::apply(
                    storage,
                    &crate::credentials::OsCredentialStore,
                    &environment,
                    &request,
                    &backups,
                )
            })
        })
    })
    .await
    .map_err(|_| codex::Error::OutcomeUnknown)?
}

/// Explicitly reconcile an interrupted before/intended observation.
#[tauri::command]
pub(crate) async fn recover_codex_settings_change(
    app: tauri::AppHandle,
    request: codex::RecoveryRequest,
) -> Result<codex::Applied, codex::Error> {
    let (environment, backups) = context(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        with_storage(&app, |storage| {
            codex::recover(storage, &environment, &request, &backups)
        })
    })
    .await
    .map_err(|_| codex::Error::OutcomeUnknown)?
}
