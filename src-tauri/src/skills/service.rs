//! Import local skill directories into the managed library, record sources and
//! remove reviewed entries again.
//!
//! The user's directories are only read. Content is copied into a temporary
//! sibling directory and renamed into place, so an interrupted import never
//! leaves a half-copied skill under a name that a record points at. Deleting
//! removes only vibemate's own copy: the managed directory is parked under a
//! name no record can point at before its record is removed, so a refused
//! deletion restores everything and a failed removal never deletes live content.
//! No command from a skill directory is ever executed.

use super::SkillLibrary;
use super::manifest::{self, SourcePlan};
use super::repository::{self, NewSkill};
use super::types::{
    DeleteSkillRequest, DeletedSkill, FetchMethod, ImportFolderReport, ImportFolderRequest,
    SetSkillSourceRequest, SkillError, SkillRecord, SkippedSkill,
};
use super::validation;
use crate::storage::Storage;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};

/// Domain prefix of the digest stream. Changing the layout requires a new value,
/// so digests from an older scheme never compare equal by accident.
const DIGEST_DOMAIN: &[u8] = b"vibemate-skill-v1\0";
/// Temporary directories are hidden and never match a 32-character ID.
const TEMPORARY_PREFIX: &str = ".import-";
/// Staging directory while an update copies candidate content into place.
const REPLACE_PREFIX: &str = ".replace-";
/// Previous managed copy parked during a content replace until metadata commits.
const REPLACE_OLD_PREFIX: &str = ".replace-old-";
/// Directory name prefix of a managed copy parked for removal. No record ever
/// points at such a name, so a cleanup sweep can never delete live content.
const PARKED_PREFIX: &str = ".delete-";

/// Provenance stored with one managed copy.
///
/// A local import records its source directory. A remote install records the
/// canonical address, the resolved revision and the reviewed command instead,
/// because its temporary workspace is removed after the import.
pub(super) struct Provenance<'a> {
    pub(super) origin_path: Option<&'a str>,
    pub(super) source_url: Option<&'a str>,
    pub(super) source_revision: Option<&'a str>,
    pub(super) fetch_method: Option<FetchMethod>,
    pub(super) installer_version: Option<&'a str>,
    pub(super) installer_command: Option<&'a str>,
}

impl Provenance<'_> {
    /// Provenance for a folder the user imported; no upstream method is known.
    fn local(origin_path: &str) -> Provenance<'_> {
        Provenance {
            origin_path: Some(origin_path),
            source_url: None,
            source_revision: None,
            fetch_method: None,
            installer_version: None,
            installer_command: None,
        }
    }
}

/// Import every skill inside one chosen folder.
///
/// The folder itself is imported when it is a skill; otherwise each immediate
/// subfolder that is a skill is imported. A folder that cannot be imported never
/// stops the others: the report says what was added, and names the refused
/// folders with a safe reason code the interface translates.
///
/// # Errors
/// Rejects a path that cannot be a source, a source that holds no skill, and one
/// that holds more candidate skills than a single import copies. A failure never
/// changes any source directory.
pub fn import_folder(
    library: &SkillLibrary,
    storage: &Storage,
    request: ImportFolderRequest,
    now_ms: i64,
) -> Result<ImportFolderReport, SkillError> {
    if now_ms < 0 {
        return Err(SkillError::InvalidRequest);
    }
    let source = resolve_source(library, &request.path)?;
    let candidates = manifest::discover(&source)?;
    if candidates.is_empty() {
        return Err(SkillError::NoSkillsFound);
    }
    let mut imported = Vec::new();
    let mut skipped = Vec::new();
    for candidate in candidates {
        match import_one(library, storage, &candidate, now_ms) {
            Ok(record) => imported.push(record),
            Err(code) => skipped.push(SkippedSkill {
                folder: validation::folder_label(&candidate),
                code,
            }),
        }
    }
    Ok(ImportFolderReport { imported, skipped })
}

/// Import one directory that was already discovered as a skill.
///
/// # Errors
/// Rejects an unreadable or changing source, a missing or invalid manifest, a
/// symbolic link or special file, every size limit, an existing name, and
/// storage failures. A failure never changes the source directory.
pub(super) fn import_one(
    library: &SkillLibrary,
    storage: &Storage,
    requested: &Path,
    now_ms: i64,
) -> Result<SkillRecord, SkillError> {
    let source = resolved_source_directory(library, requested)?;
    let origin_path = source
        .to_str()
        .ok_or(SkillError::SourcePathInvalid)?
        .to_string();
    import_prepared(
        library,
        storage,
        &source,
        now_ms,
        &Provenance::local(&origin_path),
    )
}

/// Inputs for replacing one managed skill copy with candidate content.
pub(super) struct ManagedReplace<'a> {
    pub(super) id: &'a str,
    pub(super) expected_revision: i64,
    pub(super) candidate: &'a Path,
    pub(super) source_revision: Option<&'a str>,
    pub(super) installer_command: Option<&'a str>,
    pub(super) now_ms: i64,
}

/// Replace one managed copy with candidate content and update its metadata.
///
/// Copies into a staging sibling, parks the current managed directory, then
/// renames the staging directory into place. Metadata is written only after the
/// filesystem swap; a refused write restores the parked copy. The skill name
/// must match the existing record.
///
/// # Errors
/// Rejects a stale revision, a mismatched skill name, copy failures and storage
/// write failures. A failed restore after a refused metadata write reports
/// `outcome_unknown`.
pub(super) fn replace_managed_content(
    library: &SkillLibrary,
    storage: &Storage,
    request: &ManagedReplace<'_>,
) -> Result<SkillRecord, SkillError> {
    validation::identity(request.id)?;
    if request.expected_revision < 1 || request.now_ms < 0 {
        return Err(SkillError::InvalidRequest);
    }
    let source = resolved_source_directory(library, request.candidate)?;
    let plan = manifest::inspect(&source)?;
    let current = repository::get_skill(storage, request.id)?;
    if current.revision != request.expected_revision {
        return Err(SkillError::RevisionConflict);
    }
    if plan.name != current.name {
        return Err(SkillError::SkillNotFoundInSource);
    }
    let root = library.root();
    let target = root.join(request.id);
    let temporary = root.join(format!("{REPLACE_PREFIX}{}", request.id));
    let parked = root.join(format!("{REPLACE_OLD_PREFIX}{}", request.id));
    let _ = fs::remove_dir_all(&temporary);
    let _ = fs::remove_dir_all(&parked);
    fs::create_dir(&temporary).map_err(|_| SkillError::WriteFailed)?;
    let digest = match copy(&source, &plan, &temporary) {
        Ok(digest) => digest,
        Err(error) => {
            let _ = fs::remove_dir_all(&temporary);
            return Err(error);
        }
    };
    if fs::rename(&target, &parked).is_err() {
        let _ = fs::remove_dir_all(&temporary);
        return Err(SkillError::WriteFailed);
    }
    if fs::rename(&temporary, &target).is_err() {
        let _ = fs::remove_dir_all(&temporary);
        if fs::rename(&parked, &target).is_err() {
            return Err(SkillError::OutcomeUnknown);
        }
        return Err(SkillError::WriteFailed);
    }
    let update = repository::ContentReplace {
        description: &plan.description,
        source_revision: request.source_revision,
        installer_command: request.installer_command,
        content_digest: &digest,
        file_count: plan.files.len() as i64,
        byte_size: plan.total_bytes as i64,
    };
    match repository::replace_skill_content(
        storage,
        request.id,
        request.expected_revision,
        &update,
        request.now_ms,
    ) {
        Ok(()) => {
            let _ = fs::remove_dir_all(&parked);
            repository::get_skill(storage, request.id)
        }
        Err(error) => {
            let _ = fs::remove_dir_all(&target);
            if fs::rename(&parked, &target).is_err() {
                return Err(SkillError::OutcomeUnknown);
            }
            Err(error)
        }
    }
}

/// Import one skill acquired from a remote source.
///
/// The temporary workspace is not part of the record: the caller supplies the
/// canonical source address, resolved revision and reviewed command instead.
pub(super) fn import_installed(
    library: &SkillLibrary,
    storage: &Storage,
    requested: &Path,
    provenance: &Provenance<'_>,
    now_ms: i64,
) -> Result<SkillRecord, SkillError> {
    if provenance.source_url.is_none() {
        return Err(SkillError::InvalidRequest);
    }
    let fields_consistent = match provenance.fetch_method {
        None => provenance.installer_version.is_none() && provenance.installer_command.is_none(),
        Some(FetchMethod::Git) => {
            provenance.source_revision.is_some()
                && provenance.installer_version.is_none()
                && provenance.installer_command.is_some()
        }
        Some(FetchMethod::Npx) => {
            provenance.source_revision.is_none()
                && provenance.installer_version.is_some()
                && provenance.installer_command.is_some()
        }
    };
    if !fields_consistent {
        return Err(SkillError::InvalidRequest);
    }
    let source = resolved_source_directory(library, requested)?;
    import_prepared(library, storage, &source, now_ms, provenance)
}

/// Validate a canonical source directory and keep it outside the library.
fn resolved_source_directory(
    library: &SkillLibrary,
    requested: &Path,
) -> Result<PathBuf, SkillError> {
    // Canonicalizing resolves a linked source directory into the real one, so the
    // managed copy and the recorded origin path describe the same files.
    let source = fs::canonicalize(requested).map_err(|_| SkillError::SourceNotFound)?;
    if !fs::metadata(&source)
        .map_err(|_| SkillError::SourceNotFound)?
        .is_dir()
    {
        return Err(SkillError::SourceNotDirectory);
    }
    // Compare canonical paths: a platform temporary directory is often reached
    // through a link, and the library directory may not exist yet on first use.
    let library_root =
        fs::canonicalize(library.root()).unwrap_or_else(|_| library.root().to_path_buf());
    if source.starts_with(library_root) {
        return Err(SkillError::SourcePathInvalid);
    }
    Ok(source)
}

/// Copy one inspected skill and store its provenance in one transaction.
fn import_prepared(
    library: &SkillLibrary,
    storage: &Storage,
    source: &Path,
    now_ms: i64,
    provenance: &Provenance<'_>,
) -> Result<SkillRecord, SkillError> {
    if now_ms < 0 {
        return Err(SkillError::InvalidRequest);
    }
    let plan = manifest::inspect(source)?;
    // Checked before copying so a duplicate name fails without writing files; the
    // insert repeats the check inside its transaction for the concurrent case.
    if repository::name_exists(storage, &plan.name)? {
        return Err(SkillError::NameTaken);
    }
    let id = repository::random_id(storage)?;
    let root = library.root();
    fs::create_dir_all(root).map_err(|_| SkillError::WriteFailed)?;
    let temporary = root.join(format!("{TEMPORARY_PREFIX}{id}"));
    let target = root.join(&id);
    fs::create_dir(&temporary).map_err(|_| SkillError::WriteFailed)?;
    let digest = match copy(source, &plan, &temporary) {
        Ok(digest) => digest,
        Err(error) => {
            let _ = fs::remove_dir_all(&temporary);
            return Err(error);
        }
    };
    if fs::rename(&temporary, &target).is_err() {
        let _ = fs::remove_dir_all(&temporary);
        return Err(SkillError::WriteFailed);
    }
    let new = NewSkill {
        id: &id,
        name: &plan.name,
        description: &plan.description,
        origin_path: provenance.origin_path,
        source_url: provenance.source_url,
        source_revision: provenance.source_revision,
        fetch_method: provenance.fetch_method,
        installer_version: provenance.installer_version,
        installer_command: provenance.installer_command,
        content_digest: &digest,
        file_count: plan.files.len() as i64,
        byte_size: plan.total_bytes as i64,
    };
    match repository::insert_skill(storage, &new, now_ms) {
        Ok(()) => repository::get_skill(storage, &id),
        Err((error, uncertain)) => {
            // Without a stored record the copied directory is unreachable, so it is
            // removed. If either step is uncertain, the caller must reload.
            let removed = fs::remove_dir_all(&target).is_ok();
            Err(if removed && !uncertain {
                error
            } else {
                SkillError::OutcomeUnknown
            })
        }
    }
}

/// Validate a chosen folder path and return the canonical directory.
fn resolve_source(library: &SkillLibrary, value: &str) -> Result<PathBuf, SkillError> {
    let requested = validation::source_path(value)?;
    let source = fs::canonicalize(&requested).map_err(|_| SkillError::SourceNotFound)?;
    if !fs::metadata(&source)
        .map_err(|_| SkillError::SourceNotFound)?
        .is_dir()
    {
        return Err(SkillError::SourceNotDirectory);
    }
    let library_root =
        fs::canonicalize(library.root()).unwrap_or_else(|_| library.root().to_path_buf());
    if source.starts_with(library_root) {
        return Err(SkillError::SourcePathInvalid);
    }
    Ok(source)
}

/// Record or clear the upstream source of an existing skill.
///
/// The upstream address is metadata only: no download or update check happens
/// here. Clearing it keeps the local origin path, which is the P25 source of
/// truth for an imported directory.
///
/// # Errors
/// Rejects an invalid address or revision, a missing skill, a stale revision and
/// storage failures.
pub fn set_skill_source(
    storage: &Storage,
    request: SetSkillSourceRequest,
    now_ms: i64,
) -> Result<SkillRecord, SkillError> {
    validation::identity(&request.id)?;
    if request.expected_revision < 1 || now_ms < 0 {
        return Err(SkillError::InvalidRequest);
    }
    let url = request
        .url
        .as_deref()
        .map(validation::source_url)
        .transpose()?;
    let revision = request
        .revision
        .as_deref()
        .map(validation::source_revision)
        .transpose()?;
    let installer_version = request
        .installer_version
        .as_deref()
        .map(validation::installer_version)
        .transpose()?;
    // A revision alone cannot be interpreted later, so it needs an address.
    if url.is_none() && revision.is_some() {
        return Err(SkillError::InvalidRequest);
    }
    if url.is_none() && (request.fetch_method.is_some() || installer_version.is_some()) {
        return Err(SkillError::InvalidRequest);
    }
    let current = repository::get_skill(storage, &request.id)?;
    // Every record must keep one explanation of where the content came from.
    if url.is_none() && current.origin_path.is_none() {
        return Err(SkillError::InvalidRequest);
    }
    let (url, revision, installer_version, installer_command) = match (url, request.fetch_method) {
        (Some(url), None) => {
            // A hand-entered address stays metadata only; a version belongs to
            // an installer, not to an address with no fetch method.
            if installer_version.is_some() {
                return Err(SkillError::InvalidRequest);
            }
            (
                Some(url.to_string()),
                revision.map(str::to_string),
                None,
                None,
            )
        }
        (Some(url), Some(method)) => {
            let url = validation::github_repository(url)?;
            let (revision, installer_version, command) = super::install::reviewed_source_fields(
                method,
                &url,
                revision,
                installer_version,
                &current.name,
            )?;
            (Some(url), revision, installer_version, Some(command))
        }
        (None, None) => (None, None, None, None),
        (None, Some(_)) => return Err(SkillError::InvalidRequest),
    };
    repository::update_source(
        storage,
        &request.id,
        request.expected_revision,
        &repository::SourceUpdate {
            url: url.as_deref(),
            revision: revision.as_deref(),
            fetch_method: request.fetch_method,
            installer_version: installer_version.as_deref(),
            installer_command: installer_command.as_deref(),
        },
        now_ms,
    )?;
    repository::get_skill(storage, &request.id)
}

/// Remove one reviewed skill: its record and the managed copy only.
///
/// The managed directory is first parked with an atomic rename, then the record
/// is removed at the reviewed revision, and only then is the parked copy
/// deleted. A refused deletion renames the copy back, so nothing changes. The
/// user's imported folder is never touched.
///
/// # Errors
/// Rejects an invalid identity or revision, a missing skill, a stale revision,
/// and a copy that cannot be parked. A copy that cannot be deleted after the
/// record is gone reports `cleanup_pending` instead of an error; an uncertain
/// restore or commit requires a reload before another change.
pub fn delete_skill(
    library: &SkillLibrary,
    storage: &Storage,
    request: DeleteSkillRequest,
) -> Result<DeletedSkill, SkillError> {
    validation::identity(&request.id)?;
    if request.expected_revision < 1 {
        return Err(SkillError::InvalidRequest);
    }
    // Deployment relationships and unfinished deploy journals block deletion
    // until the user revokes or recovers them; the files must not vanish while
    // linked or mid-write.
    if super::deploy::skill_has_deployments(storage, &request.id)?
        || super::deploy::skill_has_operations(storage, &request.id)?
    {
        return Err(SkillError::SkillInUse);
    }
    let root = library.root();
    let target = root.join(&request.id);
    let parked = root.join(format!("{PARKED_PREFIX}{}", request.id));
    // A missing copy is not an obstacle: the record can still be removed, which
    // also cleans a copy parked by an earlier interrupted delete of this id.
    let parked_now = match fs::rename(&target, &parked) {
        Ok(()) => true,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
        Err(_) => return Err(SkillError::WriteFailed),
    };
    if let Err(error) = repository::commit_skill_delete(storage, &request) {
        // Put the copy back so a refused deletion changes nothing. If even the
        // restore fails, the caller must reload before another change.
        if parked_now && fs::rename(&parked, &target).is_err() {
            return Err(SkillError::OutcomeUnknown);
        }
        return Err(error);
    }
    // With the record gone, every parked copy of this id is content to remove.
    let complete = match fs::remove_dir_all(&parked) {
        Ok(()) => true,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => true,
        Err(_) => false,
    };
    Ok(DeletedSkill {
        id: request.id,
        cleanup_pending: !complete,
    })
}

/// Remove parked copies left behind by a finished delete, and only those.
///
/// A parked copy whose record still exists belongs to an interrupted delete that
/// did not reach the record; it is kept so its content stays recoverable, and a
/// later delete of that skill removes it. The result is `true` when no parked
/// copy without a record remains.
///
/// # Errors
/// Reports an unreadable library directory and storage failures. One copy that
/// cannot be removed does not stop the others; it is reported by the result.
pub fn cleanup_deleted_skill_copies(
    library: &SkillLibrary,
    storage: &Storage,
) -> Result<bool, SkillError> {
    let entries = match fs::read_dir(library.root()) {
        Ok(entries) => entries,
        // No library directory means nothing is parked.
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(true),
        Err(_) => return Err(SkillError::WriteFailed),
    };
    let mut unremovable = 0usize;
    for entry in entries {
        let name = entry.map_err(|_| SkillError::ReadFailed)?.file_name();
        let Some(name) = name.to_str() else { continue };
        let Some(id) = name.strip_prefix(PARKED_PREFIX) else {
            continue;
        };
        // Only well-formed parked names belong to this module; anything else is
        // left alone.
        if validation::identity(id).is_err() {
            continue;
        }
        if repository::skill_exists(storage, id)? {
            continue;
        }
        if fs::remove_dir_all(library.root().join(name)).is_err() {
            unremovable += 1;
        }
    }
    Ok(unremovable == 0)
}

/// Digest the planned files without writing, using the same stream as import.
///
/// Used by skill deployment to verify an owned copy still matches the recorded
/// content digest. Symbolic links inside the tree are refused by inspection.
///
/// # Errors
/// Reports an unreadable source file and a source that changed after inspection.
pub(super) fn digest_tree(source: &Path, plan: &SourcePlan) -> Result<String, SkillError> {
    let mut hasher = Sha256::new();
    hasher.update(DIGEST_DOMAIN);
    for file in &plan.files {
        let bytes =
            fs::read(source.join(&file.relative)).map_err(|_| SkillError::SourceNotFound)?;
        if bytes.len() as u64 != file.size {
            return Err(SkillError::SourceChanged);
        }
        hasher.update((file.relative.len() as u64).to_be_bytes());
        hasher.update(file.relative.as_bytes());
        hasher.update((bytes.len() as u64).to_be_bytes());
        hasher.update(&bytes);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

/// Copy the planned files and return the digest of exactly those bytes.
///
/// # Errors
/// Reports an unreadable source file, a source that changed after inspection,
/// and write failures inside the temporary directory.
pub(super) fn copy(source: &Path, plan: &SourcePlan, target: &Path) -> Result<String, SkillError> {
    let mut hasher = Sha256::new();
    hasher.update(DIGEST_DOMAIN);
    for file in &plan.files {
        let bytes =
            fs::read(source.join(&file.relative)).map_err(|_| SkillError::SourceNotFound)?;
        // The source may change between inspection and copy; a size difference
        // means the digest would describe neither version, so the import stops.
        if bytes.len() as u64 != file.size {
            return Err(SkillError::SourceChanged);
        }
        let destination = target.join(&file.relative);
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent).map_err(|_| SkillError::WriteFailed)?;
        }
        fs::write(&destination, &bytes).map_err(|_| SkillError::WriteFailed)?;
        set_mode(&destination, file.mode)?;
        // Length prefixes keep the stream unambiguous for any file content.
        hasher.update((file.relative.len() as u64).to_be_bytes());
        hasher.update(file.relative.as_bytes());
        hasher.update((bytes.len() as u64).to_be_bytes());
        hasher.update(&bytes);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

/// Preserve ordinary permission bits so bundled scripts stay executable.
#[cfg(unix)]
fn set_mode(path: &Path, mode: Option<u32>) -> Result<(), SkillError> {
    use std::os::unix::fs::PermissionsExt;
    if let Some(mode) = mode {
        fs::set_permissions(path, fs::Permissions::from_mode(mode))
            .map_err(|_| SkillError::WriteFailed)?;
    }
    Ok(())
}

/// Other platforms copy content without a comparable permission model.
#[cfg(not(unix))]
fn set_mode(_: &Path, _: Option<u32>) -> Result<(), SkillError> {
    Ok(())
}
