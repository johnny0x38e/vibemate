//! Central skill library facade. A skill is a directory with a `SKILL.md` file;
//! this module owns the managed copy, its content digest and its source record.
//! Importing never runs a skill command and never changes the source directory,
//! and deleting removes only the managed copy and its record.

mod deploy;
mod install;
mod manifest;
mod marketplace;
mod repository;
mod service;
mod types;
mod update;
mod validation;

pub use deploy::{
    SkillDeployGuard, apply_skill_deploy, disable_skill_deploy, dismiss_skill_deploy_recovery,
    finish_skill_deploy_recovery, list_agent_skill_deployments, list_skill_deploy_recoveries,
    list_skill_deployments, lock_skill_deploys, preview_agent_skill_targets, preview_skill_deploy,
    resync_skill_deploy, sweep_skill_deploy_leftovers,
};
pub use install::{install_skill, preview_skill_install};
pub use marketplace::{
    get_marketplace_skill_detail, install_marketplace_skill, list_marketplace_skills,
    preview_marketplace_install, search_marketplace_skills,
};
pub use repository::{get_skill, list_skills};
pub use service::{cleanup_deleted_skill_copies, delete_skill, import_folder, set_skill_source};
pub use types::{
    AgentSkillTarget, AgentSkillTargetState, ApplySkillUpdateRequest, CheckSkillRevisionRequest,
    DeleteSkillRequest, DeletedSkill, DisableSkillDeployRequest, FetchMethod, GitInstallSource,
    ImportFolderReport, ImportFolderRequest, MarketplaceBoard, MarketplaceInstallRequest,
    MarketplaceSkill, NpxInstallSource, PickedFolder, PreviewSkillUpdateRequest,
    ResyncSkillDeployRequest, ReviewedSkillDeploy, SetSkillSourceRequest, SkillDeployAgent,
    SkillDeployDisabled, SkillDeployMode, SkillDeployOperationKind, SkillDeployOperationState,
    SkillDeployPreview, SkillDeployRecoveryAction, SkillDeployRecoveryClassification,
    SkillDeployRecoveryItem, SkillDeployRecoveryResolved, SkillDeployTargetState, SkillDeployed,
    SkillDeploymentRecord, SkillError, SkillInstallPreview, SkillInstallReport, SkillInstallSource,
    SkillPage, SkillRecord, SkillResynced, SkillRevisionCheckStatus, SkillUpdateApplied,
    SkillUpdateDeployOutcome, SkillUpdateDeployResult, SkillUpdateDiff, SkillUpdatePreview,
    SkillUpdateStatus, SkillUpdateStrategy, SkippedSkill,
};
pub use update::{apply_skill_update, check_skill_revision, preview_skill_update};

use std::path::PathBuf;

#[cfg(test)]
mod tests;

/// Root directory that holds the managed copies, `<app data>/skills/<id>`.
///
/// The library keeps its own copy of every skill so later Agent deployment can
/// link to a stable path while the user's original directory stays untouched.
#[derive(Debug)]
pub struct SkillLibrary {
    root: PathBuf,
}

impl SkillLibrary {
    /// Build a library inside the private application data directory.
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    /// Directory that contains one managed copy per skill ID.
    pub fn root(&self) -> &std::path::Path {
        &self.root
    }
}
