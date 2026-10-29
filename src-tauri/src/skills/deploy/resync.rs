//! Resync a recorded skill deployment whose live target drifted, and the
//! read-only batch target preview for one Agent.
//!
//! Resync is the only path that may overwrite a target classified as
//! `foreign`, and only while vibemate holds the ownership row for that
//! (Agent, skill, configuration root). It accepts two kinds of drift: an owned
//! copy edited outside vibemate (still a real directory) and an owned symlink
//! that now points somewhere else. Any other occupant is refused with
//! `target_conflict`. The edited content is copied into a private backup under
//! the application data directory before anything is replaced; the newest
//! `BACKUPS_KEPT` backups per (Agent, skill) are kept.
//!
//! The write is: journal row, backup, stage the new target in the staging
//! folder, re-classify, move the old target aside into the same folder,
//! rename the staged target into place, record ownership, then drop the aside
//! copy and prune old backups. Every failure before the record is written
//! restores the old target; if that restore fails the journal row is marked
//! unknown and the caller gets `outcome_unknown`.
//!
//! The staging folder (see `staging`) is a sibling of the resolved `skills/`
//! directory, not inside it, so an Agent that scans `skills/` never sees
//! half-written or aside entries, and not in app data, which may live on
//! another volume: the final step is a `rename`, which only works within one
//! filesystem. Leftovers from a crash are swept at startup and before the next
//! write at that root (only vibemate-owned names whose journal row is gone).
//!
//! Backups are private: on Unix every backup folder is 0700 and every copied
//! file 0600, whatever the modes of the replaced content were.

use super::super::SkillLibrary;
use super::super::manifest;
use super::super::service::copy;
use super::super::types::{
    AgentSkillTarget, AgentSkillTargetState, ResyncSkillDeployRequest, SkillDeployAgent,
    SkillDeployMode, SkillDeployOperationKind, SkillDeployTargetState, SkillDeployed, SkillError,
    SkillResynced,
};
use super::super::validation;
use super::staging::{self, ASIDE_PREFIX, STAGE_PREFIX};
use super::{
    SKILLS_DIR, classify_target, create_dir_symlink, display_root, probe_symlink_available,
    remove_path, repository, resolve_root, shared_duplicate, symlink_support,
};
use crate::agents::DiscoveryEnvironment;
use crate::storage::Storage;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Backups kept per (Agent, skill); older ones are pruned after a success.
const BACKUPS_KEPT: usize = 5;
/// Private backup root, next to the managed library inside app data.
const BACKUP_DIR: &str = "skill-deploy-backups";
/// File that records a replaced symlink's destination inside a backup.
const LINK_NOTE: &str = "link-target.txt";
/// File that lists links and special entries found inside a replaced copy.
const SKIPPED_NOTE: &str = "skipped-entries.txt";
/// Backup bounds; a larger edited tree is refused rather than lost.
const BACKUP_MAX_ENTRIES: usize = 10_000;
const BACKUP_MAX_BYTES: u64 = 256 * 1024 * 1024;
const BACKUP_MAX_DEPTH: usize = 32;
/// Page size used when the batch preview walks the whole library.
const LIBRARY_PAGE: u32 = 100;

/// Rewrite one vibemate-owned skill target from the managed library copy.
///
/// # Errors
/// `not_found` without an ownership row or managed copy, `revision_conflict`
/// for a stale library revision, `target_changed` when the root, path or live
/// state differs from the review, `target_conflict` when the occupant is not a
/// drift vibemate may overwrite, `symlink_unavailable`, `write_failed`, and
/// `outcome_unknown` when a failed write could not be rolled back.
pub fn resync_skill_deploy(
    library: &SkillLibrary,
    storage: &Storage,
    environment: &DiscoveryEnvironment,
    request: ResyncSkillDeployRequest,
    now_ms: i64,
) -> Result<SkillResynced, SkillError> {
    let _deploy = super::lock::lock_skill_deploys()?;
    if now_ms < 0 || request.expected_skill_revision < 1 {
        return Err(SkillError::InvalidRequest);
    }
    validation::identity(&request.skill_id)?;
    let record = super::super::repository::get_skill(storage, &request.skill_id)?;
    if record.revision != request.expected_skill_revision {
        return Err(SkillError::RevisionConflict);
    }
    let root = resolve_root(request.agent_id, environment)?;
    let config_root = display_root(&root)?;
    if config_root != request.config_root {
        return Err(SkillError::TargetChanged);
    }
    let owned =
        repository::get_deployment(storage, &record.id, request.agent_id.as_str(), &config_root)?
            .ok_or(SkillError::NotFound)?;
    if owned.target_name != record.name {
        return Err(SkillError::TargetChanged);
    }
    let skills_parent = root.join(SKILLS_DIR);
    let target = skills_parent.join(&record.name);
    let target_path = display_root(&target)?;
    if target_path != request.reviewed_target_path {
        return Err(SkillError::TargetChanged);
    }
    let managed = library.root().join(&record.id);
    if !managed.is_dir() {
        return Err(SkillError::NotFound);
    }
    let current = classify_target(&target, &managed, Some(&owned))?;
    if current != request.reviewed_target_state {
        return Err(SkillError::TargetChanged);
    }
    if current == SkillDeployTargetState::Foreign && !replaceable_drift(&target, owned.mode)? {
        return Err(SkillError::TargetConflict);
    }

    // Directories below the Agent root are created only from here on.
    let staging = staging::prepare(storage, &skills_parent)?;
    if request.mode == SkillDeployMode::Symlink
        && !probe_symlink_available(storage, &staging, &managed)?
    {
        staging::release(&staging);
        return Err(SkillError::SymlinkUnavailable);
    }

    let operation_id = super::super::repository::random_id(storage)?;
    repository::insert_operation(
        storage,
        &repository::OperationInsert {
            id: &operation_id,
            kind: SkillDeployOperationKind::Apply,
            skill_id: &record.id,
            agent_id: request.agent_id.as_str(),
            config_root: &config_root,
            target_name: &record.name,
            target_path: &target_path,
            mode: Some(request.mode),
            skill_revision: record.revision,
            content_digest: &record.content_digest,
            now_ms,
        },
    )?;
    let abandon = |error: SkillError| {
        let _ = repository::delete_operation(storage, &operation_id);
        // Succeeds only when empty; a non-empty one is swept next time.
        staging::release(&staging);
        error
    };

    // Only a `foreign` (edited) target carries user changes worth keeping;
    // owned states hold vibemate's own content and absent holds nothing.
    let backup_folder = if current == SkillDeployTargetState::Foreign {
        let folder = match private_backup_folder(
            library,
            request.agent_id,
            &record.id,
            &format!("{now_ms:013}-{operation_id}"),
        ) {
            Ok(folder) => folder,
            Err(()) => return Err(abandon(SkillError::WriteFailed)),
        };
        if let Err(error) = backup_target(&target, &folder.join(&record.name)) {
            let _ = fs::remove_dir_all(&folder);
            return Err(abandon(error));
        }
        Some(folder)
    } else {
        None
    };

    let staged = staging.join(format!("{STAGE_PREFIX}{operation_id}"));
    if let Err(error) = stage(&managed, &staged, request.mode) {
        let _ = remove_path(&staged);
        return Err(abandon(error));
    }

    // Re-check right before the swap so an edit made after the review, or
    // during the backup, is reported instead of overwritten.
    match classify_target(&target, &managed, Some(&owned)) {
        Ok(state) if state == request.reviewed_target_state => {}
        Ok(_) => {
            let _ = remove_path(&staged);
            return Err(abandon(SkillError::TargetChanged));
        }
        Err(error) => {
            let _ = remove_path(&staged);
            return Err(abandon(error));
        }
    }

    let aside = (current != SkillDeployTargetState::Absent)
        .then(|| staging.join(format!("{ASIDE_PREFIX}{operation_id}")));
    if let Some(aside) = &aside
        && fs::rename(&target, aside).is_err()
    {
        let _ = remove_path(&staged);
        return Err(abandon(SkillError::WriteFailed));
    }
    if fs::rename(&staged, &target).is_err() {
        let _ = remove_path(&staged);
        return Err(restore_or_unknown(
            storage,
            &operation_id,
            &target,
            aside.as_deref(),
            now_ms,
            SkillError::WriteFailed,
        ));
    }

    let upsert = repository::DeploymentUpsert {
        skill_id: &record.id,
        agent_id: request.agent_id.as_str(),
        config_root: &config_root,
        target_name: &record.name,
        mode: request.mode,
        skill_revision: record.revision,
        content_digest: &record.content_digest,
        deployed_at: now_ms,
    };
    if let Err(error) = repository::upsert_deployment(storage, &upsert) {
        if remove_path(&target).is_err() {
            let _ = repository::mark_operation_unknown(storage, &operation_id, now_ms);
            return Err(SkillError::OutcomeUnknown);
        }
        return Err(restore_or_unknown(
            storage,
            &operation_id,
            &target,
            aside.as_deref(),
            now_ms,
            error,
        ));
    }
    if let Some(aside) = &aside {
        // The new target and record are in place; a leftover aside entry is
        // only clutter (a foreign one was already backed up).
        let _ = remove_path(aside);
    }
    staging::release(&staging);
    let _ = repository::delete_operation(storage, &operation_id);
    if let Some(folder) = &backup_folder
        && let Some(parent) = folder.parent()
    {
        prune_backups(parent);
    }
    Ok(SkillResynced {
        deployed: SkillDeployed {
            agent_id: request.agent_id,
            skill_id: record.id,
            skill_name: record.name,
            skill_revision: record.revision,
            mode: request.mode,
            target_path,
        },
        overwrote_user_changes: backup_folder.is_some(),
        backup_path: backup_folder.map(|folder| folder.to_string_lossy().into_owned()),
    })
}

/// Where a resync of this target would back up user changes, or `None` when
/// it would not overwrite any (no ownership row, or not replaceable drift).
pub(super) fn pending_backup_dir(
    library: &SkillLibrary,
    agent: SkillDeployAgent,
    skill_id: &str,
    target: &Path,
    owned: Option<&repository::StoredDeployment>,
    state: SkillDeployTargetState,
) -> Result<Option<String>, SkillError> {
    let Some(owned) = owned else {
        return Ok(None);
    };
    if state != SkillDeployTargetState::Foreign || !replaceable_drift(target, owned.mode)? {
        return Ok(None);
    }
    display_root(&backup_skill_dir(library, agent, skill_id)).map(Some)
}

/// Keep the newest `BACKUPS_KEPT` backups in one (Agent, skill) folder.
/// Folder names are `<13-digit ms>-<operation id>`, so name order is time
/// order; anything else in the folder is left alone.
fn prune_backups(dir: &Path) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut names: Vec<String> = entries
        .flatten()
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| is_backup_name(name))
        .collect();
    names.sort();
    let excess = names.len().saturating_sub(BACKUPS_KEPT);
    for name in &names[..excess] {
        let _ = remove_path(&dir.join(name));
    }
}

fn is_backup_name(name: &str) -> bool {
    name.split_once('-').is_some_and(|(ms, id)| {
        ms.len() == 13
            && ms.bytes().all(|byte| byte.is_ascii_digit())
            && crate::shared::is_random_id(id)
    })
}

/// Classify every library skill's target at one Agent root without writing.
///
/// A missing `skills/` directory classifies each target as `absent`. A target
/// that cannot be read is reported as `unreadable`. Codex rows also name a
/// same-named `~/.agents/skills/<name>` entry (`shared_duplicate_path`).
///
/// # Errors
/// Rejects an unresolvable Agent root and storage failures.
pub fn preview_agent_skill_targets(
    library: &SkillLibrary,
    storage: &Storage,
    environment: &DiscoveryEnvironment,
    agent: SkillDeployAgent,
) -> Result<Vec<AgentSkillTarget>, SkillError> {
    let root = resolve_root(agent, environment)?;
    let config_root = display_root(&root)?;
    let symlink_available = symlink_support();
    let owned: HashMap<String, repository::StoredDeployment> =
        repository::list_deployments_for_root(storage, agent.as_str(), &config_root)?
            .into_iter()
            .map(|row| (row.skill_id.clone(), row))
            .collect();
    let mut out = Vec::new();
    let mut after: Option<String> = None;
    loop {
        let page = super::super::repository::list_skills(storage, after.as_deref(), LIBRARY_PAGE)?;
        for skill in page.items {
            let target = root.join(SKILLS_DIR).join(&skill.name);
            let target_path = display_root(&target)?;
            let managed = library.root().join(&skill.id);
            let target_state = classify_target(&target, &managed, owned.get(&skill.id)).map_or(
                AgentSkillTargetState::Unreadable,
                AgentSkillTargetState::from,
            );
            let shared_duplicate_path = shared_duplicate(agent, environment, &skill.name)?;
            out.push(AgentSkillTarget {
                skill_id: skill.id,
                target_state,
                target_path,
                symlink_available,
                shared_duplicate_path,
            });
        }
        match page.next_cursor {
            Some(cursor) => after = Some(cursor),
            None => break,
        }
    }
    Ok(out)
}

/// Whether a `foreign` occupant is drift of the recorded placement: an owned
/// copy that is still a real directory, or an owned link that still is a link.
fn replaceable_drift(target: &Path, recorded: SkillDeployMode) -> Result<bool, SkillError> {
    let meta = fs::symlink_metadata(target).map_err(|_| SkillError::ReadFailed)?;
    let is_link = meta.file_type().is_symlink();
    Ok(match recorded {
        SkillDeployMode::Symlink => is_link,
        SkillDeployMode::Copy => !is_link && meta.is_dir(),
    })
}

fn stage(managed: &Path, staged: &Path, mode: SkillDeployMode) -> Result<(), SkillError> {
    match mode {
        SkillDeployMode::Symlink => create_dir_symlink(managed, staged),
        SkillDeployMode::Copy => {
            let plan = manifest::inspect(managed)?;
            fs::create_dir(staged).map_err(|_| SkillError::WriteFailed)?;
            copy(managed, &plan, staged).map(|_digest| ())
        }
    }
}

/// Put the aside entry back after a failed swap; report `outcome_unknown` and
/// keep the journal row when that is impossible.
fn restore_or_unknown(
    storage: &Storage,
    operation_id: &str,
    target: &Path,
    aside: Option<&Path>,
    now_ms: i64,
    error: SkillError,
) -> SkillError {
    let restored = match aside {
        Some(aside) => fs::symlink_metadata(target).is_err() && fs::rename(aside, target).is_ok(),
        None => fs::symlink_metadata(target).is_err(),
    };
    if restored {
        let _ = repository::delete_operation(storage, operation_id);
        error
    } else {
        let _ = repository::mark_operation_unknown(storage, operation_id, now_ms);
        SkillError::OutcomeUnknown
    }
}

/// `<app data>/skill-deploy-backups`, beside the managed library root.
fn backup_root(library: &SkillLibrary) -> PathBuf {
    library.root().parent().map_or_else(
        || library.root().join(BACKUP_DIR),
        |parent| parent.join(BACKUP_DIR),
    )
}

/// `<backup root>/<agent>/<skill id>`: one folder per (Agent, skill) holding
/// `<ms>-<operation id>/<skill name>` backups.
fn backup_skill_dir(library: &SkillLibrary, agent: SkillDeployAgent, skill_id: &str) -> PathBuf {
    backup_root(library).join(agent.as_str()).join(skill_id)
}

/// Create `<backup root>/<agent>/<skill id>/<name>` for one backup. Every
/// level is vibemate's own and is made private (0700 on Unix), including
/// levels an earlier build created with default permissions.
fn private_backup_folder(
    library: &SkillLibrary,
    agent: SkillDeployAgent,
    skill_id: &str,
    name: &str,
) -> Result<PathBuf, ()> {
    let root = backup_root(library);
    let levels = [
        root.clone(),
        root.join(agent.as_str()),
        backup_skill_dir(library, agent, skill_id),
    ];
    for level in &levels {
        staging::create_private_dir_all(level).map_err(|_| ())?;
        restrict(level, PRIVATE_DIR_MODE)?;
    }
    let folder = backup_skill_dir(library, agent, skill_id).join(name);
    staging::create_private_dir(&folder).map_err(|_| ())?;
    Ok(folder)
}

/// Owner-only modes for backup folders and files (Unix only).
const PRIVATE_DIR_MODE: u32 = 0o700;
const PRIVATE_FILE_MODE: u32 = 0o600;

/// Set an owner-only mode on Unix, never following a link; a no-op elsewhere,
/// where the app-data folder is already private to the user.
fn restrict(path: &Path, mode: u32) -> Result<(), ()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let meta = fs::symlink_metadata(path).map_err(|_| ())?;
        if meta.file_type().is_symlink() {
            return Err(());
        }
        fs::set_permissions(path, fs::Permissions::from_mode(mode)).map_err(|_| ())
    }
    #[cfg(not(unix))]
    {
        let _ = (path, mode);
        Ok(())
    }
}

/// Write one small note file into a backup with owner-only permissions.
fn write_private(path: &Path, bytes: &[u8]) -> Result<(), SkillError> {
    fs::write(path, bytes).map_err(|_| SkillError::WriteFailed)?;
    restrict(path, PRIVATE_FILE_MODE).map_err(|()| SkillError::WriteFailed)
}

/// Copy a drifted target into a private backup before it is replaced.
///
/// A symlink is recorded as its destination text. A directory is copied file
/// by file within fixed bounds; links and special entries inside it are listed
/// in a note instead of being followed.
fn backup_target(target: &Path, backup: &Path) -> Result<(), SkillError> {
    let meta = fs::symlink_metadata(target).map_err(|_| SkillError::ReadFailed)?;
    staging::create_private_dir(backup).map_err(|_| SkillError::WriteFailed)?;
    if meta.file_type().is_symlink() {
        let destination = fs::read_link(target).map_err(|_| SkillError::ReadFailed)?;
        return write_private(
            &backup.join(LINK_NOTE),
            destination.to_string_lossy().as_bytes(),
        );
    }
    let mut budget = Budget::default();
    let mut skipped = Vec::new();
    copy_tree(target, backup, Path::new(""), 0, &mut budget, &mut skipped)?;
    if !skipped.is_empty() {
        write_private(&backup.join(SKIPPED_NOTE), skipped.join("\n").as_bytes())?;
    }
    Ok(())
}

#[derive(Default)]
struct Budget {
    entries: usize,
    bytes: u64,
}

fn copy_tree(
    source: &Path,
    destination: &Path,
    relative: &Path,
    depth: usize,
    budget: &mut Budget,
    skipped: &mut Vec<String>,
) -> Result<(), SkillError> {
    if depth > BACKUP_MAX_DEPTH {
        return Err(SkillError::WriteFailed);
    }
    let entries = fs::read_dir(source).map_err(|_| SkillError::ReadFailed)?;
    for entry in entries {
        let entry = entry.map_err(|_| SkillError::ReadFailed)?;
        budget.entries += 1;
        if budget.entries > BACKUP_MAX_ENTRIES {
            return Err(SkillError::WriteFailed);
        }
        let name = entry.file_name();
        let from = entry.path();
        let to = destination.join(&name);
        let child = relative.join(&name);
        let file_type = entry.file_type().map_err(|_| SkillError::ReadFailed)?;
        if file_type.is_symlink() {
            let link = fs::read_link(&from)
                .map(|value| value.to_string_lossy().into_owned())
                .unwrap_or_default();
            skipped.push(format!("link {} -> {link}", child.to_string_lossy()));
        } else if file_type.is_dir() {
            staging::create_private_dir(&to).map_err(|_| SkillError::WriteFailed)?;
            copy_tree(&from, &to, &child, depth + 1, budget, skipped)?;
        } else if file_type.is_file() {
            let size = entry.metadata().map_err(|_| SkillError::ReadFailed)?.len();
            budget.bytes = budget.bytes.saturating_add(size);
            if budget.bytes > BACKUP_MAX_BYTES {
                return Err(SkillError::WriteFailed);
            }
            // `fs::copy` keeps the source mode; a backup must not stay
            // group- or world-readable (or executable) in app data.
            fs::copy(&from, &to).map_err(|_| SkillError::WriteFailed)?;
            restrict(&to, PRIVATE_FILE_MODE).map_err(|()| SkillError::WriteFailed)?;
        } else {
            skipped.push(format!("special {}", child.to_string_lossy()));
        }
    }
    Ok(())
}
