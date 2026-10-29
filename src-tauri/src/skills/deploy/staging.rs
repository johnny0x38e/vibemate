//! Scratch folder for skill deploy writes, beside the real `skills/` folder.
//!
//! Every temporary entry that apply, copy refresh, resync and the in-place
//! symlink probe create lives in `.vibemate-staging`, a sibling of the
//! *resolved* skills directory. When `skills/` is a link to another volume the
//! staging folder follows it there, so the final `rename` into `skills/` stays
//! on one filesystem (a cross-device rename fails), and an Agent that scans
//! `skills/` never sees half-written, aside or probe entries.
//!
//! Names inside the staging folder: `stage-<operation id>` and
//! `aside-<operation id>` (resync), `deploy-<operation id>` (apply copy),
//! `refresh-<random id>` (copy refresh after a library update) and
//! `probe-<random id>` (symlink probe). Earlier builds wrote
//! `.vibemate-resync-<id>`, `.vibemate-deploy-<skill id>` and
//! `.vibemate-symlink-probe` inside `skills/`, and resync staging directly in
//! the Agent root; the sweep removes those leftovers as well. Nothing else is
//! ever removed: only these vibemate names with a well-formed id.

use super::super::types::SkillError;
use super::super::validation;
use super::{remove_path, repository};
use crate::storage::Storage;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// Staging folder name, a sibling of the resolved `skills/` directory.
pub(super) const STAGING_DIR: &str = ".vibemate-staging";
/// `stage-<operation id>`: a resync's new target before it is renamed in.
pub(super) const STAGE_PREFIX: &str = "stage-";
/// `aside-<operation id>`: the previous target a resync holds for rollback.
pub(super) const ASIDE_PREFIX: &str = "aside-";
/// `deploy-<operation id>`: an apply copy before it is renamed in.
pub(super) const DEPLOY_PREFIX: &str = "deploy-";
/// `refresh-<random id>`: a refreshed copy before it is renamed in.
pub(super) const REFRESH_PREFIX: &str = "refresh-";
/// `probe-<random id>`: the link created by the in-place symlink probe.
pub(super) const PROBE_PREFIX: &str = "probe-";
/// Resync aside name used inside `skills/` by earlier builds.
const LEGACY_ASIDE_PREFIX: &str = ".vibemate-resync-";
/// Copy temporary name used inside `skills/` by earlier builds.
const LEGACY_DEPLOY_PREFIX: &str = ".vibemate-deploy-";
/// Probe link name used inside `skills/` by earlier builds.
const LEGACY_PROBE: &str = ".vibemate-symlink-probe";
/// A refresh has no journal row, so every sweep (before an operation and at
/// startup) removes a refresh leftover only once it is older than this. The
/// deploy lock and the instance lock already keep a live refresh safe; the age
/// is a last guard against another vibemate build or a copy of the app that
/// does not share them. A refresh copies one bounded skill tree, far below an
/// hour, and a stale entry is only clutter until it ages out.
pub(super) const REFRESH_STALE_AFTER: Duration = Duration::from_secs(60 * 60);

/// The staging folder for one `skills/` directory: a sibling of its resolved
/// (canonical) path. `None` when `skills/` does not exist or cannot be
/// resolved to a directory with a parent.
pub(super) fn location(skills_parent: &Path) -> Option<PathBuf> {
    let real = fs::canonicalize(skills_parent).ok()?;
    if !real.is_dir() {
        return None;
    }
    Some(real.parent()?.join(STAGING_DIR))
}

/// Create `skills/` and its staging folder for a write, after sweeping
/// leftovers. The staging folder is private (0700 on Unix) and must be a real
/// directory: a link planted at its name would move entries elsewhere.
///
/// # Errors
/// `write_failed` when either folder cannot be created or resolved.
pub(super) fn prepare(storage: &Storage, skills_parent: &Path) -> Result<PathBuf, SkillError> {
    fs::create_dir_all(skills_parent).map_err(|_| SkillError::WriteFailed)?;
    let staging = location(skills_parent).ok_or(SkillError::WriteFailed)?;
    sweep(storage, skills_parent);
    match create_private_dir(&staging) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(_) => return Err(SkillError::WriteFailed),
    }
    if !fs::symlink_metadata(&staging).is_ok_and(|meta| meta.is_dir()) {
        return Err(SkillError::WriteFailed);
    }
    Ok(staging)
}

/// Drop the staging folder when it is empty; a non-empty one is swept later.
pub(super) fn release(staging: &Path) {
    let _ = fs::remove_dir(staging);
}

/// Remove leftovers that a crashed or failed operation left for this
/// `skills/` directory, in the staging folder, in the Agent-root staging
/// folder of earlier builds, and the legacy names inside `skills/`.
///
/// Journal-backed names (`stage-`, `aside-`, `deploy-`, legacy
/// `.vibemate-resync-`) are kept while their journal row exists: that
/// operation awaits recovery. Probe entries are removed only when they are
/// links (unlinked, never followed). Errors are ignored: a leftover is clutter,
/// not a correctness problem, and the next sweep tries again.
pub(super) fn sweep(storage: &Storage, skills_parent: &Path) {
    sweep_with(storage, skills_parent, REFRESH_STALE_AFTER);
}

/// `sweep` with an explicit refresh age, so tests need not age a folder.
pub(super) fn sweep_with(storage: &Storage, skills_parent: &Path, refresh_stale_after: Duration) {
    let mut folders = Vec::with_capacity(2);
    if let Some(staging) = location(skills_parent) {
        folders.push(staging);
    }
    if let Some(root) = skills_parent.parent() {
        folders.push(root.join(STAGING_DIR));
    }
    for folder in &folders {
        for (name, path) in entries(folder) {
            if staged_leftover(storage, &name, &path, refresh_stale_after) {
                let _ = remove_path(&path);
            }
        }
        // Only an empty real directory; a link or junction planted at the
        // name is left alone (Windows `remove_dir` would delete a junction).
        if fs::symlink_metadata(folder).is_ok_and(|meta| meta.file_type().is_dir()) {
            let _ = fs::remove_dir(folder);
        }
    }
    for (name, path) in entries(skills_parent) {
        if legacy_leftover(storage, &name, &path) {
            let _ = remove_path(&path);
        }
    }
}

/// Entries of a real directory; a link at `dir` is not listed (`read_dir`
/// would list its target).
fn entries(dir: &Path) -> Vec<(String, PathBuf)> {
    if !fs::symlink_metadata(dir).is_ok_and(|meta| meta.is_dir()) {
        return Vec::new();
    }
    let Ok(listing) = fs::read_dir(dir) else {
        return Vec::new();
    };
    listing
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            Some((name, entry.path()))
        })
        .collect()
}

fn staged_leftover(
    storage: &Storage,
    name: &str,
    path: &Path,
    refresh_stale_after: Duration,
) -> bool {
    if let Some(id) = [STAGE_PREFIX, ASIDE_PREFIX, DEPLOY_PREFIX]
        .iter()
        .find_map(|prefix| name.strip_prefix(prefix))
    {
        return journal_gone(storage, id);
    }
    if let Some(id) = name.strip_prefix(REFRESH_PREFIX) {
        return crate::shared::is_random_id(id) && older_than(path, refresh_stale_after);
    }
    if let Some(id) = name.strip_prefix(PROBE_PREFIX) {
        return crate::shared::is_random_id(id) && is_link(path);
    }
    false
}

fn legacy_leftover(storage: &Storage, name: &str, path: &Path) -> bool {
    if let Some(id) = name.strip_prefix(LEGACY_ASIDE_PREFIX) {
        return journal_gone(storage, id);
    }
    if let Some(id) = name.strip_prefix(LEGACY_DEPLOY_PREFIX) {
        // Current builds never write this name; any well-formed one is stale.
        return validation::identity(id).is_ok();
    }
    name == LEGACY_PROBE && is_link(path)
}

fn journal_gone(storage: &Storage, id: &str) -> bool {
    crate::shared::is_random_id(id) && matches!(repository::get_operation(storage, id), Ok(None))
}

fn is_link(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|meta| meta.file_type().is_symlink())
}

fn older_than(path: &Path, age: Duration) -> bool {
    fs::symlink_metadata(path)
        .and_then(|meta| meta.modified())
        .ok()
        .and_then(|modified| SystemTime::now().duration_since(modified).ok())
        .is_some_and(|elapsed| elapsed > age)
}

/// Create one directory that only the owner can open (0700 on Unix; default
/// permissions elsewhere).
pub(super) fn create_private_dir(path: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        fs::DirBuilder::new().mode(0o700).create(path)
    }
    #[cfg(not(unix))]
    {
        fs::create_dir(path)
    }
}

/// Create a directory and any missing parents, each private (0700 on Unix).
/// Existing parents keep their permissions.
pub(super) fn create_private_dir_all(path: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(path)
    }
    #[cfg(not(unix))]
    {
        fs::create_dir_all(path)
    }
}
