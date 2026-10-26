//! Thin Pi and Grok MCP lifecycle IPC. Rust chooses every native/backup path;
//! React sends identities, actions and approvals only. No server execution API.

use crate::{
    agents::DiscoveryEnvironment,
    config_apply::BackupStore,
    mcp::grok_deployment as grok,
    mcp::pi_deployment as deploy,
    storage::{Storage, StorageStatus},
};
use tauri::Manager;

fn context(app: &tauri::AppHandle) -> Result<(DiscoveryEnvironment, BackupStore), deploy::Error> {
    let environment =
        super::agents::environment(app).map_err(|_| deploy::Error::TargetUnavailable)?;
    let backups = app
        .path()
        .app_data_dir()
        .map_err(|_| deploy::Error::StorageUnavailable)?
        .join("backups");
    Ok((environment, BackupStore::new(backups)))
}
fn grok_context(
    app: &tauri::AppHandle,
) -> Result<(DiscoveryEnvironment, BackupStore), grok::Error> {
    let environment =
        super::agents::environment(app).map_err(|_| grok::Error::TargetUnavailable)?;
    let backups = app
        .path()
        .app_data_dir()
        .map_err(|_| grok::Error::StorageUnavailable)?
        .join("backups");
    Ok((environment, BackupStore::new(backups)))
}
fn with_storage<T>(
    app: &tauri::AppHandle,
    operation: impl FnOnce(&Storage) -> Result<T, deploy::Error>,
) -> Result<T, deploy::Error> {
    let status = app
        .try_state::<StorageStatus>()
        .ok_or(deploy::Error::StorageUnavailable)?;
    let storage = status
        .storage()
        .map_err(|_| deploy::Error::StorageUnavailable)?;
    operation(storage)
}
fn with_grok_storage<T>(
    app: &tauri::AppHandle,
    operation: impl FnOnce(&Storage) -> Result<T, grok::Error>,
) -> Result<T, grok::Error> {
    let status = app
        .try_state::<StorageStatus>()
        .ok_or(grok::Error::StorageUnavailable)?;
    let storage = status
        .storage()
        .map_err(|_| grok::Error::StorageUnavailable)?;
    operation(storage)
}

/// Read root-scoped registrations and a pending recovery observation, without keys.
#[tauri::command]
pub(crate) async fn pi_mcp_state(
    app: tauri::AppHandle,
    request: deploy::StateRequest,
) -> Result<deploy::State, deploy::Error> {
    let (environment, backups) = context(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        with_storage(&app, |storage| {
            deploy::read_state(storage, &environment, &request, &backups)
        })
    })
    .await
    .map_err(|_| deploy::Error::OutcomeUnknown)?
}

/// Read a bounded page of one definition's retained entry results, without bytes.
#[tauri::command]
pub(crate) async fn pi_mcp_history(
    app: tauri::AppHandle,
    request: deploy::HistoryRequest,
) -> Result<deploy::HistoryPage, deploy::Error> {
    let (environment, _) = context(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        with_storage(&app, |storage| {
            deploy::history(storage, &environment, &request)
        })
    })
    .await
    .map_err(|_| deploy::Error::OutcomeUnknown)?
}

/// Rebuild one native action and return only its masked summary and approval.
#[tauri::command]
pub(crate) async fn preview_pi_mcp_change(
    app: tauri::AppHandle,
    selection: deploy::Selection,
) -> Result<deploy::Preview, deploy::Error> {
    let (environment, backups) = context(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        with_storage(&app, |storage| {
            deploy::preview(storage, &environment, &selection, &backups)
        })
    })
    .await
    .map_err(|_| deploy::Error::OutcomeUnknown)?
}

/// Confirm an exact preview; only deploy/update opens current OS credentials.
#[tauri::command]
pub(crate) async fn apply_pi_mcp_change(
    app: tauri::AppHandle,
    request: deploy::ApplyRequest,
) -> Result<deploy::Applied, deploy::Error> {
    let (environment, backups) = context(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        super::state::with_credential_lock(&app, deploy::Error::OutcomeUnknown, || {
            with_storage(&app, |storage| {
                deploy::apply(
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
    .map_err(|_| deploy::Error::OutcomeUnknown)?
}

/// Explicitly reconcile an interrupted before/intended observation; never replay.
#[tauri::command]
pub(crate) async fn recover_pi_mcp_change(
    app: tauri::AppHandle,
    request: deploy::RecoveryRequest,
) -> Result<deploy::Applied, deploy::Error> {
    let (environment, backups) = context(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        with_storage(&app, |storage| {
            deploy::recover(storage, &environment, &request, &backups)
        })
    })
    .await
    .map_err(|_| deploy::Error::OutcomeUnknown)?
}

/// Read Grok root-scoped registrations and a pending recovery observation.
#[tauri::command]
pub(crate) async fn grok_mcp_state(
    app: tauri::AppHandle,
    request: grok::StateRequest,
) -> Result<grok::State, grok::Error> {
    let (environment, backups) = grok_context(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        with_grok_storage(&app, |storage| {
            grok::read_state(storage, &environment, &request, &backups)
        })
    })
    .await
    .map_err(|_| grok::Error::OutcomeUnknown)?
}

/// Read a bounded page of one definition's retained Grok entry results.
#[tauri::command]
pub(crate) async fn grok_mcp_history(
    app: tauri::AppHandle,
    request: grok::HistoryRequest,
) -> Result<grok::HistoryPage, grok::Error> {
    let (environment, _) = grok_context(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        with_grok_storage(&app, |storage| {
            grok::history(storage, &environment, &request)
        })
    })
    .await
    .map_err(|_| grok::Error::OutcomeUnknown)?
}

/// Rebuild one Grok native action and return only its masked summary and approval.
#[tauri::command]
pub(crate) async fn preview_grok_mcp_change(
    app: tauri::AppHandle,
    selection: grok::Selection,
) -> Result<grok::Preview, grok::Error> {
    let (environment, backups) = grok_context(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        with_grok_storage(&app, |storage| {
            grok::preview(storage, &environment, &selection, &backups)
        })
    })
    .await
    .map_err(|_| grok::Error::OutcomeUnknown)?
}

/// Confirm an exact Grok preview; only deploy/update opens current OS credentials.
#[tauri::command]
pub(crate) async fn apply_grok_mcp_change(
    app: tauri::AppHandle,
    request: grok::ApplyRequest,
) -> Result<grok::Applied, grok::Error> {
    let (environment, backups) = grok_context(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        super::state::with_credential_lock(&app, grok::Error::OutcomeUnknown, || {
            with_grok_storage(&app, |storage| {
                grok::apply(
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
    .map_err(|_| grok::Error::OutcomeUnknown)?
}

/// Explicitly reconcile an interrupted Grok before/intended observation.
#[tauri::command]
pub(crate) async fn recover_grok_mcp_change(
    app: tauri::AppHandle,
    request: grok::RecoveryRequest,
) -> Result<grok::Applied, grok::Error> {
    let (environment, backups) = grok_context(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        with_grok_storage(&app, |storage| {
            grok::recover(storage, &environment, &request, &backups)
        })
    })
    .await
    .map_err(|_| grok::Error::OutcomeUnknown)?
}
