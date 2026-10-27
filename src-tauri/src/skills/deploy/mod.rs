//! Deploy a central skill into an Agent's user-level `skills/` directory.
//!
//! Pi writes `<PI_CODING_AGENT_DIR|~/.pi/agent>/skills/<name>/`. Grok Build
//! writes `<GROK_HOME|~/.grok>/skills/<name>/`. Claude Code writes
//! `<CLAUDE_CONFIG_DIR|~/.claude>/skills/<name>/`. All three use the same
//! SKILL.md directory form. Symlink is preferred when the platform can create
//! a directory link; otherwise the caller must choose copy-sync. User content
//! at the same name is never overwritten. Ownership lives in
//! `skill_deployment`. Unfinished apply/disable work is journaled in
//! `skill_deploy_operation` for explicit Finish/Dismiss recovery.

mod repository;

#[cfg(test)]
mod tests;

use super::SkillLibrary;
use super::manifest;
use super::service::{copy, digest_tree};
use super::types::{
    DisableSkillDeployRequest, ReviewedSkillDeploy, SkillDeployAgent, SkillDeployDisabled,
    SkillDeployMode, SkillDeployOperationKind, SkillDeployPreview, SkillDeployRecoveryAction,
    SkillDeployRecoveryClassification, SkillDeployRecoveryItem, SkillDeployRecoveryResolved,
    SkillDeployTargetState, SkillDeployed, SkillDeploymentRecord, SkillError, SkillRecord,
    SkillUpdateDeployOutcome, SkillUpdateDeployResult,
};
use super::validation;
use crate::agents::{
    DirectorySource, DiscoveryEnvironment, claude_config_root, grok_config_root, pi_config_root,
};
use crate::storage::Storage;
use std::fs;
use std::path::{Path, PathBuf};

/// Skills live directly under each Agent configuration root.
const SKILLS_DIR: &str = "skills";
/// Temporary sibling used while copy-sync replaces an owned target.
const COPY_TEMP_PREFIX: &str = ".vibemate-deploy-";
/// Probe link name used only during symlink capability checks.
const SYMLINK_PROBE: &str = ".vibemate-symlink-probe";

fn resolve_root(
    agent: SkillDeployAgent,
    environment: &DiscoveryEnvironment,
) -> Result<PathBuf, SkillError> {
    let (root, source) = match agent {
        SkillDeployAgent::Pi => pi_config_root(environment),
        SkillDeployAgent::GrokBuild => grok_config_root(environment),
        SkillDeployAgent::ClaudeCode => claude_config_root(environment),
    };
    match (root, source) {
        (Some(path), DirectorySource::Default | DirectorySource::Environment) => Ok(path),
        _ => Err(SkillError::AgentLocationUnavailable),
    }
}

/// Preview deploying one library skill into the resolved Agent skills directory.
///
/// # Errors
/// Rejects an unknown skill, an unresolvable Agent root, and storage failures.
pub fn preview_skill_deploy(
    library: &SkillLibrary,
    storage: &Storage,
    environment: &DiscoveryEnvironment,
    agent: SkillDeployAgent,
    skill_id: &str,
) -> Result<SkillDeployPreview, SkillError> {
    validation::identity(skill_id)?;
    let record = super::repository::get_skill(storage, skill_id)?;
    let root = resolve_root(agent, environment)?;
    let config_root = display_root(&root)?;
    let target = root.join(SKILLS_DIR).join(&record.name);
    let target_path = display_root(&target)?;
    let managed = library.root().join(&record.id);
    let owned = repository::get_deployment(storage, skill_id, agent.as_str(), &config_root)?;
    let target_state = classify_target(&target, &managed, owned.as_ref())?;
    let symlink_available = probe_symlink_available(&root.join(SKILLS_DIR), &managed)?;
    let suggested_mode = if symlink_available {
        SkillDeployMode::Symlink
    } else {
        SkillDeployMode::Copy
    };
    let can_apply = target_state != SkillDeployTargetState::Foreign;
    Ok(SkillDeployPreview {
        agent_id: agent,
        skill_id: record.id,
        skill_name: record.name.clone(),
        skill_revision: record.revision,
        content_digest: record.content_digest,
        config_root,
        target_path,
        target_name: record.name,
        target_state,
        symlink_available,
        suggested_mode,
        can_apply,
    })
}

/// Apply a reviewed skill deployment.
///
/// # Errors
/// Rejects stale reviews, foreign targets, unavailable symlink mode, and write
/// failures. A filesystem write followed by a failed record reports
/// `outcome_unknown` when the target cannot be rolled back.
pub fn apply_skill_deploy(
    library: &SkillLibrary,
    storage: &Storage,
    environment: &DiscoveryEnvironment,
    reviewed: ReviewedSkillDeploy,
    now_ms: i64,
) -> Result<SkillDeployed, SkillError> {
    if now_ms < 0 {
        return Err(SkillError::InvalidRequest);
    }
    validation::identity(&reviewed.skill_id)?;
    if reviewed.expected_skill_revision < 1 {
        return Err(SkillError::InvalidRequest);
    }
    if matches!(reviewed.target_state, SkillDeployTargetState::Foreign) {
        return Err(SkillError::DeployModeInvalid);
    }
    let record = super::repository::get_skill(storage, &reviewed.skill_id)?;
    if record.revision != reviewed.expected_skill_revision {
        return Err(SkillError::RevisionConflict);
    }
    let root = resolve_root(reviewed.agent_id, environment)?;
    let config_root = display_root(&root)?;
    if config_root != reviewed.config_root {
        return Err(SkillError::TargetChanged);
    }
    let skills_parent = root.join(SKILLS_DIR);
    let target = skills_parent.join(&record.name);
    let managed = library.root().join(&record.id);
    if !managed.is_dir() {
        return Err(SkillError::NotFound);
    }
    let owned = repository::get_deployment(
        storage,
        &reviewed.skill_id,
        reviewed.agent_id.as_str(),
        &config_root,
    )?;
    let current = classify_target(&target, &managed, owned.as_ref())?;
    if current != reviewed.target_state {
        return Err(SkillError::TargetChanged);
    }
    if current == SkillDeployTargetState::Foreign {
        return Err(SkillError::TargetConflict);
    }
    let symlink_available = probe_symlink_available(&skills_parent, &managed)?;
    match reviewed.mode {
        SkillDeployMode::Symlink if !symlink_available => {
            return Err(SkillError::SymlinkUnavailable);
        }
        SkillDeployMode::Copy => {
            // Copy is always allowed; the interface must choose it when symlink
            // is unavailable. When symlink is available, copy remains a valid
            // explicit user choice.
        }
        _ => {}
    }

    let target_path = display_root(&target)?;
    let operation_id = super::repository::random_id(storage)?;
    repository::insert_operation(
        storage,
        &repository::OperationInsert {
            id: &operation_id,
            kind: SkillDeployOperationKind::Apply,
            skill_id: &record.id,
            agent_id: reviewed.agent_id.as_str(),
            config_root: &config_root,
            target_name: &record.name,
            target_path: &target_path,
            mode: Some(reviewed.mode),
            skill_revision: record.revision,
            content_digest: &record.content_digest,
            now_ms,
        },
    )?;

    if let Err(error) = fs::create_dir_all(&skills_parent) {
        let _ = error;
        let _ = repository::delete_operation(storage, &operation_id);
        return Err(SkillError::WriteFailed);
    }
    if matches!(
        current,
        SkillDeployTargetState::OwnedSymlink | SkillDeployTargetState::OwnedCopy
    ) && let Err(error) = remove_path(&target)
    {
        let _ = repository::delete_operation(storage, &operation_id);
        return Err(error);
    }

    // On write failure the owned target may already be gone; leave the journal
    // so recovery can classify the live path against the ownership row.
    match reviewed.mode {
        SkillDeployMode::Symlink => create_dir_symlink(&managed, &target)?,
        SkillDeployMode::Copy => {
            let plan = manifest::inspect(&managed)?;
            let temporary = skills_parent.join(format!("{COPY_TEMP_PREFIX}{}", record.id));
            let _ = fs::remove_dir_all(&temporary);
            if fs::create_dir(&temporary).is_err() {
                return Err(SkillError::WriteFailed);
            }
            if let Err(error) = copy(&managed, &plan, &temporary) {
                let _ = fs::remove_dir_all(&temporary);
                return Err(error);
            }
            if fs::rename(&temporary, &target).is_err() {
                let _ = fs::remove_dir_all(&temporary);
                return Err(SkillError::WriteFailed);
            }
        }
    };

    let upsert = repository::DeploymentUpsert {
        skill_id: &record.id,
        agent_id: reviewed.agent_id.as_str(),
        config_root: &config_root,
        target_name: &record.name,
        mode: reviewed.mode,
        skill_revision: record.revision,
        content_digest: &record.content_digest,
        deployed_at: now_ms,
    };
    if let Err(error) = repository::upsert_deployment(storage, &upsert) {
        if remove_path(&target).is_err() {
            let _ = repository::mark_operation_unknown(storage, &operation_id, now_ms);
            return Err(SkillError::OutcomeUnknown);
        }
        let _ = repository::delete_operation(storage, &operation_id);
        return Err(error);
    }
    // A failed journal delete leaves a completed row for recovery cleanup.
    let _ = repository::delete_operation(storage, &operation_id);
    Ok(SkillDeployed {
        agent_id: reviewed.agent_id,
        skill_id: record.id,
        skill_name: record.name,
        skill_revision: record.revision,
        mode: reviewed.mode,
        target_path,
    })
}

/// Revoke one vibemate-managed skill deployment.
///
/// When the live path no longer matches ownership, the filesystem is left alone
/// and the record is still cleared so a later deploy can conflict-check honestly.
///
/// # Errors
/// Rejects an unknown deployment, an unresolvable root, and storage failures.
pub fn disable_skill_deploy(
    library: &SkillLibrary,
    storage: &Storage,
    environment: &DiscoveryEnvironment,
    request: DisableSkillDeployRequest,
) -> Result<SkillDeployDisabled, SkillError> {
    validation::identity(&request.skill_id)?;
    let root = resolve_root(request.agent_id, environment)?;
    let config_root = display_root(&root)?;
    if config_root != request.config_root {
        return Err(SkillError::TargetChanged);
    }
    let deployment = repository::get_deployment(
        storage,
        &request.skill_id,
        request.agent_id.as_str(),
        &config_root,
    )?
    .ok_or(SkillError::NotFound)?;
    let record = super::repository::get_skill(storage, &request.skill_id)?;
    let target = root.join(SKILLS_DIR).join(&deployment.target_name);
    let target_path = display_root(&target)?;
    let managed = library.root().join(&record.id);
    let owned = Some(deployment.clone());
    let state = classify_target(&target, &managed, owned.as_ref())?;
    let now_ms = crate::shared::current_unix_millis().unwrap_or(0);
    let operation_id = super::repository::random_id(storage)?;
    repository::insert_operation(
        storage,
        &repository::OperationInsert {
            id: &operation_id,
            kind: SkillDeployOperationKind::Disable,
            skill_id: &request.skill_id,
            agent_id: request.agent_id.as_str(),
            config_root: &config_root,
            target_name: &deployment.target_name,
            target_path: &target_path,
            mode: Some(deployment.mode),
            skill_revision: deployment.skill_revision,
            content_digest: &deployment.content_digest,
            now_ms,
        },
    )?;
    let (target_removed, left_foreign) = match state {
        SkillDeployTargetState::OwnedSymlink | SkillDeployTargetState::OwnedCopy => {
            if let Err(error) = remove_path(&target) {
                let _ = repository::delete_operation(storage, &operation_id);
                return Err(error);
            }
            (true, false)
        }
        SkillDeployTargetState::Absent => (false, false),
        SkillDeployTargetState::Foreign => (false, true),
    };
    // On ownership-clear failure the target may already be gone; leave the
    // journal for recovery.
    repository::delete_deployment(
        storage,
        &request.skill_id,
        request.agent_id.as_str(),
        &config_root,
    )?;
    let _ = repository::delete_operation(storage, &operation_id);
    Ok(SkillDeployDisabled {
        agent_id: request.agent_id,
        skill_id: request.skill_id,
        target_path,
        target_removed,
        left_foreign,
    })
}

/// List deployments recorded for one resolved Agent configuration root.
///
/// # Errors
/// Rejects an unresolvable Agent root and storage failures.
pub fn list_agent_skill_deployments(
    library: &SkillLibrary,
    storage: &Storage,
    environment: &DiscoveryEnvironment,
    agent: SkillDeployAgent,
) -> Result<Vec<SkillDeploymentRecord>, SkillError> {
    let root = resolve_root(agent, environment)?;
    let config_root = display_root(&root)?;
    let rows = repository::list_deployments_for_root(storage, agent.as_str(), &config_root)?;
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        out.push(to_record(library, storage, &root, row)?);
    }
    Ok(out)
}

/// List deployments recorded for one central skill (any Agent root).
///
/// # Errors
/// Rejects an invalid identity and storage failures.
pub fn list_skill_deployments(
    library: &SkillLibrary,
    storage: &Storage,
    skill_id: &str,
) -> Result<Vec<SkillDeploymentRecord>, SkillError> {
    validation::identity(skill_id)?;
    let rows = repository::list_deployments_for_skill(storage, skill_id)?;
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        let root = PathBuf::from(&row.config_root);
        out.push(to_record(library, storage, &root, row)?);
    }
    Ok(out)
}

/// Whether any deployment row still references this skill.
pub(super) fn skill_has_deployments(storage: &Storage, skill_id: &str) -> Result<bool, SkillError> {
    repository::skill_has_deployments(storage, skill_id)
}

/// Whether any unfinished deploy journal row still names this skill.
pub(super) fn skill_has_operations(storage: &Storage, skill_id: &str) -> Result<bool, SkillError> {
    repository::skill_has_operations(storage, skill_id)
}

/// List unfinished skill deploy/disable operations with live classifications.
///
/// # Errors
/// Rejects storage failures while reading journal or ownership rows.
pub fn list_skill_deploy_recoveries(
    library: &SkillLibrary,
    storage: &Storage,
) -> Result<Vec<SkillDeployRecoveryItem>, SkillError> {
    let rows = repository::list_operations(storage)?;
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        out.push(to_recovery_item(library, storage, &row)?);
    }
    Ok(out)
}

/// Finish one unfinished skill deploy journal row after re-classification.
///
/// # Errors
/// Rejects an unknown id, a changed classification, a conflict finish, and
/// storage or filesystem classification failures.
pub fn finish_skill_deploy_recovery(
    library: &SkillLibrary,
    storage: &Storage,
    request: SkillDeployRecoveryAction,
) -> Result<SkillDeployRecoveryResolved, SkillError> {
    validation::identity(&request.id)?;
    let row = repository::get_operation(storage, &request.id)?.ok_or(SkillError::NotFound)?;
    let item = to_recovery_item(library, storage, &row)?;
    if item.classification != request.expected_classification {
        return Err(SkillError::TargetChanged);
    }
    match item.classification {
        SkillDeployRecoveryClassification::NeedsRecord => {
            let mode = row.mode.ok_or(SkillError::InvalidStoredSkill)?;
            repository::upsert_deployment(
                storage,
                &repository::DeploymentUpsert {
                    skill_id: &row.skill_id,
                    agent_id: &row.agent_id,
                    config_root: &row.config_root,
                    target_name: &row.target_name,
                    mode,
                    skill_revision: row.skill_revision,
                    content_digest: &row.content_digest,
                    deployed_at: row.created_at,
                },
            )?;
            repository::delete_operation(storage, &row.id)?;
        }
        SkillDeployRecoveryClassification::NeedsRecordClear => {
            // Ownership may already be gone after a concurrent clear; treat a
            // missing row as success so Finish stays idempotent for this case.
            match repository::delete_deployment(
                storage,
                &row.skill_id,
                &row.agent_id,
                &row.config_root,
            ) {
                Ok(()) | Err(SkillError::NotFound) => {}
                Err(error) => return Err(error),
            }
            repository::delete_operation(storage, &row.id)?;
        }
        SkillDeployRecoveryClassification::Completed
        | SkillDeployRecoveryClassification::NotStarted => {
            repository::delete_operation(storage, &row.id)?;
        }
        SkillDeployRecoveryClassification::Conflict => {
            return Err(SkillError::TargetConflict);
        }
    }
    Ok(SkillDeployRecoveryResolved {
        id: row.id,
        classification: item.classification,
    })
}

/// Dismiss one unfinished skill deploy journal row without touching files.
///
/// # Errors
/// Rejects an unknown id, a changed classification, and dismissals that would
/// hide a real ownership inconsistency.
pub fn dismiss_skill_deploy_recovery(
    library: &SkillLibrary,
    storage: &Storage,
    request: SkillDeployRecoveryAction,
) -> Result<SkillDeployRecoveryResolved, SkillError> {
    validation::identity(&request.id)?;
    let row = repository::get_operation(storage, &request.id)?.ok_or(SkillError::NotFound)?;
    let item = to_recovery_item(library, storage, &row)?;
    if item.classification != request.expected_classification {
        return Err(SkillError::TargetChanged);
    }
    match item.classification {
        SkillDeployRecoveryClassification::Completed
        | SkillDeployRecoveryClassification::NotStarted
        | SkillDeployRecoveryClassification::Conflict => {
            repository::delete_operation(storage, &row.id)?;
        }
        SkillDeployRecoveryClassification::NeedsRecord
        | SkillDeployRecoveryClassification::NeedsRecordClear => {
            return Err(SkillError::RecoveryConflict);
        }
    }
    Ok(SkillDeployRecoveryResolved {
        id: row.id,
        classification: item.classification,
    })
}

fn to_recovery_item(
    library: &SkillLibrary,
    storage: &Storage,
    row: &repository::StoredOperation,
) -> Result<SkillDeployRecoveryItem, SkillError> {
    let managed = library.root().join(&row.skill_id);
    let target = PathBuf::from(&row.config_root)
        .join(SKILLS_DIR)
        .join(&row.target_name);
    let owned =
        repository::get_deployment(storage, &row.skill_id, &row.agent_id, &row.config_root)?;
    let ownership_present = owned.is_some();
    let journal_owned = journal_as_owned(row);
    let target_match = match classify_target(&target, &managed, journal_owned.as_ref()) {
        Ok(SkillDeployTargetState::Absent) => TargetMatch::Absent,
        Ok(SkillDeployTargetState::OwnedSymlink | SkillDeployTargetState::OwnedCopy) => {
            TargetMatch::Matches
        }
        Ok(SkillDeployTargetState::Foreign) | Err(_) => TargetMatch::Conflict,
    };
    let classification = classify_recovery(row.kind, target_match, ownership_present);
    let skill_name = super::repository::get_skill(storage, &row.skill_id)
        .map(|skill| skill.name)
        .unwrap_or_else(|_| row.target_name.clone());
    Ok(SkillDeployRecoveryItem {
        id: row.id.clone(),
        kind: row.kind,
        state: row.state,
        classification,
        skill_id: row.skill_id.clone(),
        skill_name,
        agent_id: row.agent_id.clone(),
        config_root: row.config_root.clone(),
        target_path: row.target_path.clone(),
        mode: row.mode,
        can_finish: classification != SkillDeployRecoveryClassification::Conflict,
        can_dismiss: matches!(
            classification,
            SkillDeployRecoveryClassification::Completed
                | SkillDeployRecoveryClassification::NotStarted
                | SkillDeployRecoveryClassification::Conflict
        ),
    })
}

#[derive(Clone, Copy)]
enum TargetMatch {
    Absent,
    Matches,
    Conflict,
}

fn classify_recovery(
    kind: SkillDeployOperationKind,
    target: TargetMatch,
    ownership_present: bool,
) -> SkillDeployRecoveryClassification {
    match (kind, target, ownership_present) {
        (SkillDeployOperationKind::Apply, TargetMatch::Absent, true)
        | (SkillDeployOperationKind::Disable, TargetMatch::Absent, true) => {
            SkillDeployRecoveryClassification::NeedsRecordClear
        }
        (SkillDeployOperationKind::Apply, TargetMatch::Absent, false) => {
            SkillDeployRecoveryClassification::NotStarted
        }
        (SkillDeployOperationKind::Disable, TargetMatch::Absent, false) => {
            SkillDeployRecoveryClassification::Completed
        }
        (SkillDeployOperationKind::Apply, TargetMatch::Matches, true) => {
            SkillDeployRecoveryClassification::Completed
        }
        (SkillDeployOperationKind::Apply, TargetMatch::Matches, false) => {
            SkillDeployRecoveryClassification::NeedsRecord
        }
        (SkillDeployOperationKind::Disable, TargetMatch::Matches, true) => {
            SkillDeployRecoveryClassification::NotStarted
        }
        (SkillDeployOperationKind::Disable, TargetMatch::Matches, false) => {
            SkillDeployRecoveryClassification::Conflict
        }
        (_, TargetMatch::Conflict, _) => SkillDeployRecoveryClassification::Conflict,
    }
}

fn journal_as_owned(row: &repository::StoredOperation) -> Option<repository::StoredDeployment> {
    let mode = row.mode?;
    Some(repository::StoredDeployment {
        skill_id: row.skill_id.clone(),
        agent_id: row.agent_id.clone(),
        config_root: row.config_root.clone(),
        target_name: row.target_name.clone(),
        mode,
        skill_revision: row.skill_revision,
        content_digest: row.content_digest.clone(),
        deployed_at: row.created_at,
    })
}

/// Refresh owned deployments after a successful central skill content replace.
///
/// Symlink targets that still point at the managed path only get ownership
/// metadata updated. Owned copy targets are rewritten from the new managed
/// content. Foreign or missing targets are reported as failed without deleting
/// user content. A storage failure after a filesystem rewrite still returns
/// `Failed` for that row; the central library change is left intact.
pub(super) fn refresh_skill_deployments_after_update(
    library: &SkillLibrary,
    storage: &Storage,
    skill: &SkillRecord,
    now_ms: i64,
) -> Result<Vec<SkillUpdateDeployResult>, SkillError> {
    let rows = repository::list_deployments_for_skill(storage, &skill.id)?;
    let managed = library.root().join(&skill.id);
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        let agent_id =
            SkillDeployAgent::parse(&row.agent_id).ok_or(SkillError::InvalidStoredSkill)?;
        let root = PathBuf::from(&row.config_root);
        let skills_parent = root.join(SKILLS_DIR);
        let target = skills_parent.join(&row.target_name);
        let owned = Some(row.clone());
        let state = match classify_target(&target, &managed, owned.as_ref()) {
            Ok(state) => state,
            Err(_) => {
                out.push(SkillUpdateDeployResult {
                    agent_id,
                    config_root: row.config_root,
                    mode: row.mode,
                    outcome: SkillUpdateDeployOutcome::Failed,
                });
                continue;
            }
        };
        let outcome = match (row.mode, state) {
            (SkillDeployMode::Symlink, SkillDeployTargetState::OwnedSymlink) => {
                match upsert_refresh(storage, skill, &row, now_ms) {
                    Ok(()) => SkillUpdateDeployOutcome::SymlinkUpdated,
                    Err(_) => SkillUpdateDeployOutcome::Failed,
                }
            }
            (SkillDeployMode::Copy, SkillDeployTargetState::OwnedCopy) => {
                match refresh_owned_copy(&skills_parent, &target, &managed, skill) {
                    Ok(()) => match upsert_refresh(storage, skill, &row, now_ms) {
                        Ok(()) => SkillUpdateDeployOutcome::Refreshed,
                        Err(_) => SkillUpdateDeployOutcome::Failed,
                    },
                    Err(_) => SkillUpdateDeployOutcome::Failed,
                }
            }
            _ => SkillUpdateDeployOutcome::Failed,
        };
        out.push(SkillUpdateDeployResult {
            agent_id,
            config_root: row.config_root,
            mode: row.mode,
            outcome,
        });
    }
    Ok(out)
}

fn upsert_refresh(
    storage: &Storage,
    skill: &SkillRecord,
    row: &repository::StoredDeployment,
    now_ms: i64,
) -> Result<(), SkillError> {
    repository::upsert_deployment(
        storage,
        &repository::DeploymentUpsert {
            skill_id: &skill.id,
            agent_id: &row.agent_id,
            config_root: &row.config_root,
            target_name: &row.target_name,
            mode: row.mode,
            skill_revision: skill.revision,
            content_digest: &skill.content_digest,
            deployed_at: now_ms,
        },
    )
}

fn refresh_owned_copy(
    skills_parent: &Path,
    target: &Path,
    managed: &Path,
    skill: &SkillRecord,
) -> Result<(), SkillError> {
    let plan = manifest::inspect(managed)?;
    let temporary = skills_parent.join(format!("{COPY_TEMP_PREFIX}{}", skill.id));
    let _ = fs::remove_dir_all(&temporary);
    fs::create_dir(&temporary).map_err(|_| SkillError::WriteFailed)?;
    if let Err(error) = copy(managed, &plan, &temporary) {
        let _ = fs::remove_dir_all(&temporary);
        return Err(error);
    }
    remove_path(target)?;
    if fs::rename(&temporary, target).is_err() {
        let _ = fs::remove_dir_all(&temporary);
        return Err(SkillError::WriteFailed);
    }
    Ok(())
}

fn to_record(
    library: &SkillLibrary,
    storage: &Storage,
    root: &Path,
    row: repository::StoredDeployment,
) -> Result<SkillDeploymentRecord, SkillError> {
    let managed = library.root().join(&row.skill_id);
    let target = root.join(SKILLS_DIR).join(&row.target_name);
    let owned = Some(row.clone());
    let state = classify_target(&target, &managed, owned.as_ref())?;
    let in_sync = matches!(
        state,
        SkillDeployTargetState::OwnedSymlink | SkillDeployTargetState::OwnedCopy
    );
    let skill_name = super::repository::get_skill(storage, &row.skill_id)
        .map(|skill| skill.name)
        .unwrap_or_else(|_| row.target_name.clone());
    Ok(SkillDeploymentRecord {
        skill_id: row.skill_id,
        skill_name,
        agent_id: row.agent_id,
        config_root: row.config_root,
        mode: row.mode,
        skill_revision: row.skill_revision,
        content_digest: row.content_digest,
        in_sync,
        deployed_at_ms: row.deployed_at,
    })
}

fn display_root(path: &Path) -> Result<String, SkillError> {
    let value = path.to_str().ok_or(SkillError::AgentLocationUnavailable)?;
    if !path.is_absolute()
        || value.is_empty()
        || value.len() > 4096
        || value.chars().any(char::is_control)
    {
        return Err(SkillError::AgentLocationUnavailable);
    }
    Ok(value.to_owned())
}

fn classify_target(
    target: &Path,
    managed: &Path,
    owned: Option<&repository::StoredDeployment>,
) -> Result<SkillDeployTargetState, SkillError> {
    let meta = match fs::symlink_metadata(target) {
        Ok(meta) => meta,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(SkillDeployTargetState::Absent);
        }
        Err(_) => return Err(SkillError::ReadFailed),
    };
    let Some(owned) = owned else {
        return Ok(SkillDeployTargetState::Foreign);
    };
    if meta.file_type().is_symlink() {
        if owned.mode != SkillDeployMode::Symlink {
            return Ok(SkillDeployTargetState::Foreign);
        }
        return Ok(if symlink_points_to(target, managed)? {
            SkillDeployTargetState::OwnedSymlink
        } else {
            SkillDeployTargetState::Foreign
        });
    }
    if !meta.is_dir() {
        return Ok(SkillDeployTargetState::Foreign);
    }
    if owned.mode != SkillDeployMode::Copy {
        return Ok(SkillDeployTargetState::Foreign);
    }
    match manifest::inspect(target).and_then(|plan| digest_tree(target, &plan)) {
        Ok(digest) if digest == owned.content_digest => Ok(SkillDeployTargetState::OwnedCopy),
        Ok(_) | Err(_) => Ok(SkillDeployTargetState::Foreign),
    }
}

fn symlink_points_to(link: &Path, managed: &Path) -> Result<bool, SkillError> {
    let target = fs::read_link(link).map_err(|_| SkillError::ReadFailed)?;
    let resolved = if target.is_absolute() {
        target
    } else {
        link.parent().ok_or(SkillError::ReadFailed)?.join(target)
    };
    let left = fs::canonicalize(&resolved).ok();
    let right = fs::canonicalize(managed).ok();
    Ok(left.is_some() && left == right)
}

fn probe_symlink_available(parent: &Path, managed: &Path) -> Result<bool, SkillError> {
    fs::create_dir_all(parent).map_err(|_| SkillError::WriteFailed)?;
    let probe = parent.join(SYMLINK_PROBE);
    // The probe points at the managed skill tree. Remove only that link; a
    // recursive delete would follow it and erase the library copy.
    let _ = remove_path(&probe);
    match create_dir_symlink(managed, &probe) {
        Ok(()) => {
            let _ = remove_path(&probe);
            Ok(true)
        }
        Err(_) => {
            let _ = remove_path(&probe);
            Ok(false)
        }
    }
}

fn create_dir_symlink(source: &Path, link: &Path) -> Result<(), SkillError> {
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(source, link).map_err(|_| SkillError::WriteFailed)
    }
    #[cfg(windows)]
    {
        std::os::windows::fs::symlink_dir(source, link).map_err(|_| SkillError::WriteFailed)
    }
}

fn remove_path(path: &Path) -> Result<(), SkillError> {
    let meta = match fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(_) => return Err(SkillError::WriteFailed),
    };
    let file_type = meta.file_type();
    if file_type.is_symlink() {
        // `remove_file` uses DeleteFileW, which refuses a directory symbolic
        // link. RemoveDirectory deletes that link and leaves its target alone.
        #[cfg(windows)]
        {
            use std::os::windows::fs::FileTypeExt;
            if file_type.is_symlink_dir() {
                return fs::remove_dir(path).map_err(|_| SkillError::WriteFailed);
            }
        }
        fs::remove_file(path).map_err(|_| SkillError::WriteFailed)
    } else if meta.is_file() {
        fs::remove_file(path).map_err(|_| SkillError::WriteFailed)
    } else if meta.is_dir() {
        fs::remove_dir_all(path).map_err(|_| SkillError::WriteFailed)
    } else {
        Err(SkillError::WriteFailed)
    }
}
