//! Desktop startup and command registration for vibemate.

pub mod agents;
pub mod appearance;
mod commands;
pub mod config_apply;
pub mod config_plan;
mod configuration_preview;
pub mod credentials;
mod log_access;
mod logging;
pub mod mcp;
pub mod model_catalog;
pub mod model_error;
pub mod model_fetch;
pub mod model_http;
pub mod model_identity;
pub mod model_search;
pub mod models;
pub mod providers;
pub mod references;
pub mod settings;
mod shared;
pub mod skills;
pub mod storage;

use tauri::Manager;

/// Start the desktop runtime, open private storage, and register UI commands.
///
/// The window opens even when private storage is unavailable. The storage outcome
/// stays in managed state, so commands can report it and a later backup restore can
/// rebuild the database. Only a failure of Tauri itself stops the process.
pub fn run() {
    tauri::Builder::default()
        // Native folder picker for skill imports; the frontend calls vibemate's
        // own command, so no dialog permission is granted to the webview.
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let file_logging_active = logging::initialize(app.handle());
            app.manage(log_access::LogAccess::new(
                app.path().app_log_dir().ok(),
                file_logging_active,
            ));
            // The database lives in the platform app-data folder, never in the project.
            let status = match app.path().app_data_dir() {
                Ok(data_directory) => {
                    // Managed skill copies live next to the database, so both move
                    // together and a backup of the app-data folder stays complete.
                    app.manage(skills::SkillLibrary::new(data_directory.join("skills")));
                    let status = storage::StorageStatus::open_in_directory(&data_directory);
                    if let Ok(storage) = status.storage()
                        && let Err(error) = config_apply::materialize_configuration_versions(
                            storage,
                            &config_apply::BackupStore::new(data_directory.join("backups")),
                        )
                    {
                        // Only the safe code is logged; copied configuration
                        // bytes and credentials never enter diagnostics.
                        log::error!(target: "vibemate", "event=configuration_versions_unavailable reason={}", error.code());
                    }
                    status
                }
                Err(_) => {
                    storage::StorageStatus::Unavailable(storage::StorageError::LocateDataDirectory)
                }
            };
            if let Err(error) = status.storage() {
                // Display is intentionally safe; never format Debug or the error source.
                log::error!(target: "vibemate", "event=storage_unavailable reason={error}");
            } else {
                log::info!(target: "vibemate", "event=storage_ready");
            }
            // Managed state is shared by all commands; `StorageStatus` is safe across threads.
            app.manage(status);
            app.manage(commands::CredentialLock::default());
            app.manage(model_fetch::ModelFetchRegistry::new());
            app.manage(agents::VersionProbeGate::default());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::app::get_app_info,
            commands::agents::discover_agents,
            commands::agents::probe_agent_version,
            commands::agents::plan_claude_code_settings,
            commands::claude_preference::get_claude_effort_preference,
            commands::claude_preference::save_claude_effort_preference,
            commands::agents::apply_claude_code_settings,
            commands::pi::plan_pi_settings,
            commands::grok::plan_grok_settings,
            commands::grok::apply_grok_settings,
            commands::grok::plan_grok_restore,
            commands::grok::apply_grok_restore,
            commands::pi::apply_pi_settings,
            commands::pi::plan_pi_restore,
            commands::pi::apply_pi_restore,
            commands::agents::plan_claude_code_restore,
            commands::agents::apply_claude_code_restore,
            commands::agents::configuration_history_page,
            commands::agents::plan_configuration_version_restore,
            commands::agents::apply_configuration_version_restore,
            commands::agents::delete_configuration_history_entry,
            commands::agents::read_configuration_version_text,
            commands::agents::read_current_configuration_text,
            commands::configuration_preview::read_model_configuration_preview,
            commands::agents::review_configuration_resolution,
            commands::agents::resolve_configuration_operation,
            commands::mcp_deploy::pi_mcp_state,
            commands::mcp_deploy::pi_mcp_history,
            commands::mcp_deploy::preview_pi_mcp_change,
            commands::mcp_deploy::apply_pi_mcp_change,
            commands::mcp_deploy::recover_pi_mcp_change,
            commands::mcp_deploy::grok_mcp_state,
            commands::mcp_deploy::grok_mcp_history,
            commands::mcp_deploy::preview_grok_mcp_change,
            commands::mcp_deploy::apply_grok_mcp_change,
            commands::mcp_deploy::recover_grok_mcp_change,
            commands::mcp::preview_pi_mcp_registration,
            commands::mcp::preview_grok_mcp_registration,
            commands::mcp::list_mcp_definitions,
            commands::mcp::get_mcp_definition,
            commands::mcp::save_mcp_definition,
            commands::mcp::cleanup_mcp_credentials,
            commands::mcp::delete_mcp_definition,
            commands::mcp::cleanup_deleted_mcp_credentials,
            commands::skills::preview_skill_install,
            commands::skills::install_skill,
            commands::skills::search_marketplace_skills,
            commands::skills::list_marketplace_skills,
            commands::skills::get_marketplace_skill_detail,
            commands::skills::preview_marketplace_install,
            commands::skills::install_marketplace_skill,
            commands::skills::list_skills,
            commands::skills::get_skill,
            commands::skills::pick_skill_folder,
            commands::skills::import_skill_folder,
            commands::skills::set_skill_source,
            commands::skills::preview_skill_update,
            commands::skills::apply_skill_update,
            commands::skills::check_skill_revision,
            commands::skills::delete_skill,
            commands::skills::cleanup_deleted_skill_copies,
            commands::skills::preview_skill_deploy,
            commands::skills::apply_skill_deploy,
            commands::skills::disable_skill_deploy,
            commands::skills::list_agent_skill_deployments,
            commands::skills::list_skill_deployments,
            commands::skills::list_skill_deploy_recoveries,
            commands::skills::finish_skill_deploy_recovery,
            commands::skills::dismiss_skill_deploy_recovery,
            commands::app::get_log_location,
            commands::app::open_log_file,
            commands::app::open_log_directory,
            commands::app::open_project_repository,
            commands::preferences::get_locale_preference,
            commands::preferences::save_locale_preference,
            commands::preferences::get_appearance_preference,
            commands::preferences::save_appearance_preference,
            commands::preferences::get_auto_version_probe,
            commands::preferences::save_auto_version_probe,
            commands::providers::list_provider_templates,
            commands::providers::list_providers,
            commands::providers::get_provider,
            commands::providers::create_provider,
            commands::providers::update_provider,
            commands::providers::get_provider_secret_status,
            commands::providers::replace_provider_secret,
            commands::providers::delete_provider,
            commands::providers::provider_deletion_impact,
            commands::providers::clear_provider_reference,
            commands::models::fetch_provider_models,
            commands::models::refresh_saved_provider_models,
            commands::models::browse_upstream_models_page,
            commands::models::cancel_provider_model_fetch,
            commands::models::get_provider_model_fetch_status,
            commands::models::list_provider_models,
            commands::models::count_provider_models,
            commands::models::save_provider_model_selections,
            commands::models::set_provider_models_selected,
            commands::models::add_manual_provider_model,
            commands::models::delete_manual_provider_model,
            commands::models::model_deletion_impact,
            commands::models::clear_model_reference,
            commands::models::get_provider_model_snapshot,
            commands::models::update_provider_model_edit,
        ])
        .run(tauri::generate_context!())
        .unwrap_or_else(|_| {
            log::error!(target: "vibemate", "event=desktop_runtime_failed");
            log::logger().flush();
            panic!("failed to start vibemate desktop runtime");
        });
    log::info!(target: "vibemate", "event=application_exit");
    log::logger().flush();
}
