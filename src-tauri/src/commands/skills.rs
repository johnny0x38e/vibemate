//! Skill library IPC: list entries, choose a folder, import its skills, record
//! a source, preview or apply an update from a recorded fetch method, delete a
//! reviewed entry, and deploy a skill into an Agent skills directory. Importing
//! copies files into the private library and never executes a skill command;
//! deleting removes only vibemate's own copy. Update preview acquires into a
//! temporary workspace and never writes the library; apply re-checks digests
//! before keep or replace. Deploy prefers a symlink and falls back to copy-sync
//! only when the user chooses it. All work happens on the blocking pool because
//! it touches the disk, and the folder dialog is opened from Rust so the
//! frontend never calls the dialog plugin's own commands.

use super::agents::environment;
use super::state::with_skill_library;
use tauri::Manager;
use tauri_plugin_dialog::DialogExt;

/// Longest accepted dialog title. The interface sends a translated label.
const MAX_TITLE_LENGTH: usize = 120;

/// Open the system folder picker and report the choice.
///
/// Closing the dialog is a normal outcome (`cancelled`), not a failure. The
/// picker accepts any folder: the import decides whether it is one skill or a
/// folder of skills.
#[tauri::command]
pub(crate) async fn pick_skill_folder(
    app: tauri::AppHandle,
    title: String,
) -> Result<crate::skills::PickedFolder, crate::skills::SkillError> {
    let trimmed = title.trim();
    if trimmed.is_empty() || trimmed.chars().count() > MAX_TITLE_LENGTH {
        return Err(crate::skills::SkillError::InvalidRequest);
    }
    let mut dialog = app.dialog().file().set_title(trimmed);
    // A window-scoped sheet keeps the dialog attached to the vibemate window on
    // platforms that support it; other platforms ignore the parent.
    if let Some(window) = app.get_webview_window("main") {
        dialog = dialog.set_parent(&window);
    }
    // The blocking call must stay off the main thread; `spawn_blocking` above
    // guarantees that, and the plugin dispatches the UI work to the main thread.
    let picked = tauri::async_runtime::spawn_blocking(move || dialog.blocking_pick_folder())
        .await
        .map_err(|_| crate::skills::SkillError::OperationFailed)?;
    match picked.and_then(|folder| folder.into_path().ok()) {
        Some(path) => match path.to_str() {
            Some(text) => Ok(crate::skills::PickedFolder::Selected {
                path: text.to_string(),
            }),
            // A path the interface cannot carry as text is reported like a cancel.
            None => Ok(crate::skills::PickedFolder::Cancelled),
        },
        None => Ok(crate::skills::PickedFolder::Cancelled),
    }
}

/// Import every skill in the chosen folder: the folder itself when it is a
/// skill, otherwise each of its immediate skill subfolders.
#[tauri::command]
pub(crate) async fn import_skill_folder(
    app: tauri::AppHandle,
    request: crate::skills::ImportFolderRequest,
) -> Result<crate::skills::ImportFolderReport, crate::skills::SkillError> {
    tauri::async_runtime::spawn_blocking(move || {
        let now = crate::shared::current_unix_millis()
            .map_err(|_| crate::skills::SkillError::OperationFailed)?;
        with_skill_library(&app, |library, storage| {
            crate::skills::import_folder(library, storage, request, now)
        })
    })
    .await
    .map_err(|_| crate::skills::SkillError::OperationFailed)?
}

/// Validate an install source and return the exact command for review.
///
/// This reads no directory, starts no process and changes no record. The
/// interface must show this result before it may call `install_skill`.
#[tauri::command]
pub(crate) fn preview_skill_install(
    source: crate::skills::SkillInstallSource,
) -> Result<crate::skills::SkillInstallPreview, crate::skills::SkillError> {
    crate::skills::preview_skill_install(source)
}

/// Search the skills.sh catalog for installable GitHub skills.
#[tauri::command]
pub(crate) async fn search_marketplace_skills(
    query: String,
    limit: Option<u32>,
) -> Result<Vec<crate::skills::MarketplaceSkill>, crate::skills::SkillError> {
    let limit = limit.unwrap_or(40) as usize;
    crate::skills::search_marketplace_skills(&query, limit).await
}

/// Load one skills.sh leaderboard page (GitHub-installable rows only).
#[tauri::command]
pub(crate) async fn list_marketplace_skills(
    board: crate::skills::MarketplaceBoard,
) -> Result<Vec<crate::skills::MarketplaceSkill>, crate::skills::SkillError> {
    crate::skills::list_marketplace_skills(board).await
}

/// Show the Git command for installing one marketplace skill at HEAD.
#[tauri::command]
pub(crate) fn preview_marketplace_install(
    request: crate::skills::MarketplaceInstallRequest,
) -> Result<crate::skills::SkillInstallPreview, crate::skills::SkillError> {
    crate::skills::preview_marketplace_install(request)
}

/// Install one marketplace skill from GitHub HEAD into the central library.
#[tauri::command]
pub(crate) async fn install_marketplace_skill(
    app: tauri::AppHandle,
    request: crate::skills::MarketplaceInstallRequest,
) -> Result<crate::skills::SkillInstallReport, crate::skills::SkillError> {
    tauri::async_runtime::spawn_blocking(move || {
        let now = crate::shared::current_unix_millis()
            .map_err(|_| crate::skills::SkillError::OperationFailed)?;
        with_skill_library(&app, |library, storage| {
            crate::skills::install_marketplace_skill(library, storage, request, now)
        })
    })
    .await
    .map_err(|_| crate::skills::SkillError::OperationFailed)?
}

/// Resolve remote Git `HEAD` for one skill and store the comparison outcome.
///
/// Does not download the tree or bump the skill metadata revision. npx sources
/// are refused with `update_unavailable`.
#[tauri::command]
pub(crate) async fn check_skill_revision(
    app: tauri::AppHandle,
    request: crate::skills::CheckSkillRevisionRequest,
) -> Result<crate::skills::SkillRecord, crate::skills::SkillError> {
    tauri::async_runtime::spawn_blocking(move || {
        let now = crate::shared::current_unix_millis()
            .map_err(|_| crate::skills::SkillError::OperationFailed)?;
        with_skill_library(&app, |_, storage| {
            crate::skills::check_skill_revision(storage, request, now)
        })
    })
    .await
    .map_err(|_| crate::skills::SkillError::OperationFailed)?
}

/// Acquire a reviewed source in a private workspace and import its skills.
///
/// The installer only runs after the interface's explicit confirmation. A
/// failed acquisition leaves the central library unchanged.
#[tauri::command]
pub(crate) async fn install_skill(
    app: tauri::AppHandle,
    source: crate::skills::SkillInstallSource,
) -> Result<crate::skills::SkillInstallReport, crate::skills::SkillError> {
    tauri::async_runtime::spawn_blocking(move || {
        let now = crate::shared::current_unix_millis()
            .map_err(|_| crate::skills::SkillError::OperationFailed)?;
        with_skill_library(&app, |library, storage| {
            crate::skills::install_skill(library, storage, source, now)
        })
    })
    .await
    .map_err(|_| crate::skills::SkillError::OperationFailed)?
}

/// Read central skill metadata without copying or inspecting any directory.
#[tauri::command]
pub(crate) async fn list_skills(
    app: tauri::AppHandle,
    after: Option<String>,
    limit: u32,
) -> Result<crate::skills::SkillPage, crate::skills::SkillError> {
    tauri::async_runtime::spawn_blocking(move || {
        with_skill_library(&app, |_, storage| {
            crate::skills::list_skills(storage, after.as_deref(), limit)
        })
    })
    .await
    .map_err(|_| crate::skills::SkillError::OperationFailed)?
}

/// Read one skill's safe metadata for the source editor or a retry.
#[tauri::command]
pub(crate) async fn get_skill(
    app: tauri::AppHandle,
    id: String,
) -> Result<crate::skills::SkillRecord, crate::skills::SkillError> {
    tauri::async_runtime::spawn_blocking(move || {
        with_skill_library(&app, |_, storage| crate::skills::get_skill(storage, &id))
    })
    .await
    .map_err(|_| crate::skills::SkillError::OperationFailed)?
}

/// Preview updating one skill from its recorded Git or npx source.
///
/// Acquisition may contact the network. The central library and Agent targets
/// are never modified; the temporary workspace is removed before returning.
#[tauri::command]
pub(crate) async fn preview_skill_update(
    app: tauri::AppHandle,
    request: crate::skills::PreviewSkillUpdateRequest,
) -> Result<crate::skills::SkillUpdatePreview, crate::skills::SkillError> {
    tauri::async_runtime::spawn_blocking(move || {
        with_skill_library(&app, |library, storage| {
            crate::skills::preview_skill_update(library, storage, request)
        })
    })
    .await
    .map_err(|_| crate::skills::SkillError::OperationFailed)?
}

/// Apply a reviewed keep or replace strategy after an update preview.
///
/// Re-checks digests before acting. `keep` writes nothing; `replace` swaps the
/// managed copy and refreshes owned deployments. Partial deploy failures leave
/// the library change in place.
#[tauri::command]
pub(crate) async fn apply_skill_update(
    app: tauri::AppHandle,
    request: crate::skills::ApplySkillUpdateRequest,
) -> Result<crate::skills::SkillUpdateApplied, crate::skills::SkillError> {
    tauri::async_runtime::spawn_blocking(move || {
        let now = crate::shared::current_unix_millis()
            .map_err(|_| crate::skills::SkillError::OperationFailed)?;
        with_skill_library(&app, |library, storage| {
            crate::skills::apply_skill_update(library, storage, request, now)
        })
    })
    .await
    .map_err(|_| crate::skills::SkillError::OperationFailed)?
}

/// Record or clear the upstream source of an existing skill.
#[tauri::command]
pub(crate) async fn set_skill_source(
    app: tauri::AppHandle,
    request: crate::skills::SetSkillSourceRequest,
) -> Result<crate::skills::SkillRecord, crate::skills::SkillError> {
    tauri::async_runtime::spawn_blocking(move || {
        let now = crate::shared::current_unix_millis()
            .map_err(|_| crate::skills::SkillError::OperationFailed)?;
        with_skill_library(&app, |_, storage| {
            crate::skills::set_skill_source(storage, request, now)
        })
    })
    .await
    .map_err(|_| crate::skills::SkillError::OperationFailed)?
}

/// Delete one reviewed skill record and its managed copy; the user's imported
/// folder is untouched. A copy that cannot be removed is reported as pending
/// cleanup instead of making the deletion look refused.
#[tauri::command]
pub(crate) async fn delete_skill(
    app: tauri::AppHandle,
    request: crate::skills::DeleteSkillRequest,
) -> Result<crate::skills::DeletedSkill, crate::skills::SkillError> {
    tauri::async_runtime::spawn_blocking(move || {
        with_skill_library(&app, |library, storage| {
            crate::skills::delete_skill(library, storage, request)
        })
    })
    .await
    .map_err(|_| crate::skills::SkillError::OperationFailed)?
}

/// Retry the removal of parked copies left by finished deletions.
#[tauri::command]
pub(crate) async fn cleanup_deleted_skill_copies(
    app: tauri::AppHandle,
) -> Result<bool, crate::skills::SkillError> {
    tauri::async_runtime::spawn_blocking(move || {
        with_skill_library(&app, |library, storage| {
            crate::skills::cleanup_deleted_skill_copies(library, storage)
        })
    })
    .await
    .map_err(|_| crate::skills::SkillError::OperationFailed)?
}

/// Preview deploying one central skill into a resolved Agent skills directory.
#[tauri::command]
pub(crate) async fn preview_skill_deploy(
    app: tauri::AppHandle,
    agent_id: crate::skills::SkillDeployAgent,
    skill_id: String,
) -> Result<crate::skills::SkillDeployPreview, crate::skills::SkillError> {
    let environment =
        environment(&app).map_err(|_| crate::skills::SkillError::AgentLocationUnavailable)?;
    tauri::async_runtime::spawn_blocking(move || {
        with_skill_library(&app, |library, storage| {
            crate::skills::preview_skill_deploy(library, storage, &environment, agent_id, &skill_id)
        })
    })
    .await
    .map_err(|_| crate::skills::SkillError::OperationFailed)?
}

/// Apply a reviewed skill deployment after the interface shows the preview.
#[tauri::command]
pub(crate) async fn apply_skill_deploy(
    app: tauri::AppHandle,
    reviewed: crate::skills::ReviewedSkillDeploy,
) -> Result<crate::skills::SkillDeployed, crate::skills::SkillError> {
    let environment =
        environment(&app).map_err(|_| crate::skills::SkillError::AgentLocationUnavailable)?;
    tauri::async_runtime::spawn_blocking(move || {
        let now = crate::shared::current_unix_millis()
            .map_err(|_| crate::skills::SkillError::OperationFailed)?;
        with_skill_library(&app, |library, storage| {
            crate::skills::apply_skill_deploy(library, storage, &environment, reviewed, now)
        })
    })
    .await
    .map_err(|_| crate::skills::SkillError::OperationFailed)?
}

/// Revoke one vibemate-managed skill deployment without touching the library.
#[tauri::command]
pub(crate) async fn disable_skill_deploy(
    app: tauri::AppHandle,
    request: crate::skills::DisableSkillDeployRequest,
) -> Result<crate::skills::SkillDeployDisabled, crate::skills::SkillError> {
    let environment =
        environment(&app).map_err(|_| crate::skills::SkillError::AgentLocationUnavailable)?;
    tauri::async_runtime::spawn_blocking(move || {
        with_skill_library(&app, |library, storage| {
            crate::skills::disable_skill_deploy(library, storage, &environment, request)
        })
    })
    .await
    .map_err(|_| crate::skills::SkillError::OperationFailed)?
}

/// List deployments recorded for one resolved Agent configuration root.
#[tauri::command]
pub(crate) async fn list_agent_skill_deployments(
    app: tauri::AppHandle,
    agent_id: crate::skills::SkillDeployAgent,
) -> Result<Vec<crate::skills::SkillDeploymentRecord>, crate::skills::SkillError> {
    let environment =
        environment(&app).map_err(|_| crate::skills::SkillError::AgentLocationUnavailable)?;
    tauri::async_runtime::spawn_blocking(move || {
        with_skill_library(&app, |library, storage| {
            crate::skills::list_agent_skill_deployments(library, storage, &environment, agent_id)
        })
    })
    .await
    .map_err(|_| crate::skills::SkillError::OperationFailed)?
}

/// List deployments recorded for one central skill.
#[tauri::command]
pub(crate) async fn list_skill_deployments(
    app: tauri::AppHandle,
    skill_id: String,
) -> Result<Vec<crate::skills::SkillDeploymentRecord>, crate::skills::SkillError> {
    tauri::async_runtime::spawn_blocking(move || {
        with_skill_library(&app, |library, storage| {
            crate::skills::list_skill_deployments(library, storage, &skill_id)
        })
    })
    .await
    .map_err(|_| crate::skills::SkillError::OperationFailed)?
}

/// List unfinished skill deploy/disable journal rows with live classifications.
#[tauri::command]
pub(crate) async fn list_skill_deploy_recoveries(
    app: tauri::AppHandle,
) -> Result<Vec<crate::skills::SkillDeployRecoveryItem>, crate::skills::SkillError> {
    tauri::async_runtime::spawn_blocking(move || {
        with_skill_library(&app, |library, storage| {
            crate::skills::list_skill_deploy_recoveries(library, storage)
        })
    })
    .await
    .map_err(|_| crate::skills::SkillError::OperationFailed)?
}

/// Finish one unfinished skill deploy journal row after re-classification.
#[tauri::command]
pub(crate) async fn finish_skill_deploy_recovery(
    app: tauri::AppHandle,
    request: crate::skills::SkillDeployRecoveryAction,
) -> Result<crate::skills::SkillDeployRecoveryResolved, crate::skills::SkillError> {
    tauri::async_runtime::spawn_blocking(move || {
        with_skill_library(&app, |library, storage| {
            crate::skills::finish_skill_deploy_recovery(library, storage, request)
        })
    })
    .await
    .map_err(|_| crate::skills::SkillError::OperationFailed)?
}

/// Dismiss one unfinished skill deploy journal row without touching files.
#[tauri::command]
pub(crate) async fn dismiss_skill_deploy_recovery(
    app: tauri::AppHandle,
    request: crate::skills::SkillDeployRecoveryAction,
) -> Result<crate::skills::SkillDeployRecoveryResolved, crate::skills::SkillError> {
    tauri::async_runtime::spawn_blocking(move || {
        with_skill_library(&app, |library, storage| {
            crate::skills::dismiss_skill_deploy_recovery(library, storage, request)
        })
    })
    .await
    .map_err(|_| crate::skills::SkillError::OperationFailed)?
}
