//! Atomic, recoverable writes of Agent configuration files.
//!
//! A plan says what would change; this module is the only place that replaces a
//! file. It refuses to write when the file on disk no longer matches the plan's
//! change token, keeps a private copy of what it replaces, writes a temporary
//! file next to the target and renames it into place, and re-reads the result
//! instead of assuming success. Failures carry a stable code and a path only:
//! file contents never leave this module except through the backup directory,
//! which is private because a settings file can already hold a user's key.

use std::{
    fs::{self, OpenOptions},
    io::{ErrorKind, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use serde::Serialize;

use crate::config_plan::{MAX_TARGET_BYTES, PlanError, TargetFile, digest_hex, read_target_bytes};
use crate::shared;

/// A file a plan wants to replace, with the token the plan was built from.
#[derive(Debug, Clone)]
pub struct WriteTarget {
    /// Absolute path of the file a plan would change.
    pub path: PathBuf,
    /// SHA-256 the plan saw, or `None` when the plan saw no file at all.
    pub expected_digest: Option<String>,
}

impl WriteTarget {
    /// Take the path and token straight from a plan, so an apply step cannot be
    /// built from a different file than the one the user reviewed.
    pub fn from_plan(target: &TargetFile) -> Self {
        Self {
            path: PathBuf::from(&target.path),
            expected_digest: target.content_digest.clone(),
        }
    }
}

/// Private directory that holds the content this module replaced.
#[derive(Debug, Clone)]
pub struct BackupStore {
    root: PathBuf,
}

/// One stored copy of a file that was replaced.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Backup {
    /// Identifier of the write that stored this copy; also its directory name.
    pub id: String,
    /// Absolute path of the stored copy, so the user can locate the backup.
    pub path: String,
    /// SHA-256 of the replaced bytes, for a restore step to verify.
    pub content_digest: String,
}

impl BackupStore {
    /// Keep backups under `root`; the directory is created on the first backup.
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    /// Root directory of every stored copy.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Store the bytes that are about to be replaced.
    fn store(&self, id: &str, name: &str, bytes: &[u8]) -> Result<Backup, WriteError> {
        let directory = self.root.join(id);
        create_private_directory(&directory)?;
        let path = directory.join(name);
        create_private_file(&path, bytes)?;
        // Flush directory entries before a database row can rely on this copy.
        sync_directory(&directory).map_err(|_| WriteError::BackupUnavailable)?;
        sync_directory(&self.root).map_err(|_| WriteError::BackupUnavailable)?;
        if let Some(parent) = self.root.parent() {
            sync_directory(parent).map_err(|_| WriteError::BackupUnavailable)?;
        }
        Ok(Backup {
            id: id.to_owned(),
            path: path_text(&path)?,
            content_digest: digest_hex(bytes),
        })
    }

    /// Remove a copy whose write did not happen, keeping the store free of
    /// entries no operation refers to.
    ///
    /// Only the unjournaled low-level writer may discard an unreferenced copy.
    /// Recorded writes, restores and compensations retain their snapshots.
    fn discard(&self, backup: &Backup) -> Result<(), ()> {
        match fs::remove_dir_all(self.root.join(&backup.id)) {
            Ok(()) => Ok(()),
            // A copy that is already gone is not a leftover.
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
            Err(_) => Err(()),
        }
    }
}

/// What one applied file became on disk.
#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppliedFile {
    /// Identity of the target after the write, including its new digest.
    pub target: TargetFile,
    /// Copy of the replaced content; absent for a new file or an unchanged one.
    pub backup: Option<Backup>,
    /// Whether the file changed; `false` means the bytes already matched.
    pub changed: bool,
}

/// Why a configuration write did not replace the target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WriteError {
    /// The caller asked for a write the module cannot describe: an empty step
    /// list, a path that cannot be shown, or a digest that is not SHA-256.
    InvalidRequest,
    /// The plan token no longer matches: the file appeared, vanished or changed.
    TargetChanged,
    /// The target path is a symbolic link; replacing it would write elsewhere.
    SymlinkRefused,
    /// The target exists but is read-only for the user.
    TargetReadOnly,
    /// The target path cannot be shown or is not absolute.
    TargetPathInvalid,
    /// The target is not a readable regular file.
    TargetUnreadable,
    /// The target is larger than the preview limit.
    TargetTooLarge,
    /// The private backup could not be stored; the target was left alone.
    BackupUnavailable,
    /// The target could not be replaced; its previous content is intact.
    WriteFailed,
    /// The replacement may or may not have happened; reload before another try.
    OutcomeUnknown,
    /// The private database is not open, or an earlier panic poisoned it.
    StorageUnavailable,
    /// The operation or one of its steps could not be recorded.
    LogWriteFailed,
    /// Recorded operations could not be read back.
    LogReadFailed,
    /// A stored operation row is malformed; the log is not shown as if it were valid.
    InvalidStoredOperation,
    /// The reviewed restore plan no longer matches the files on disk.
    PlanStale,
    /// The history entry belongs to an unfinished recorded write.
    HistoryActive,
    /// Interrupted mutations must be inspected before this entry can go.
    HistoryUnresolved,
    /// Other retained history still depends on this entry's journal rows.
    HistoryReferenced,
}

impl WriteError {
    /// Stable code for the operation log and the interface. [`WriteError`]
    /// serializes to the same text; a test keeps the two in step.
    pub fn code(self) -> &'static str {
        match self {
            Self::InvalidRequest => "invalid_request",
            Self::TargetChanged => "target_changed",
            Self::SymlinkRefused => "symlink_refused",
            Self::TargetReadOnly => "target_read_only",
            Self::TargetPathInvalid => "target_path_invalid",
            Self::TargetUnreadable => "target_unreadable",
            Self::TargetTooLarge => "target_too_large",
            Self::BackupUnavailable => "backup_unavailable",
            Self::WriteFailed => "write_failed",
            Self::OutcomeUnknown => "outcome_unknown",
            Self::StorageUnavailable => "storage_unavailable",
            Self::LogWriteFailed => "log_write_failed",
            Self::LogReadFailed => "log_read_failed",
            Self::InvalidStoredOperation => "invalid_stored_operation",
            Self::PlanStale => "plan_stale",
            Self::HistoryActive => "history_active",
            Self::HistoryUnresolved => "history_unresolved",
            Self::HistoryReferenced => "history_referenced",
        }
    }
}

impl std::fmt::Display for WriteError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::InvalidRequest => "The requested write cannot be described.",
            Self::TargetChanged => "The configuration file changed since the preview.",
            Self::SymlinkRefused => "The configuration path is a symbolic link.",
            Self::TargetReadOnly => "The configuration file is read-only.",
            Self::TargetPathInvalid => "The configuration path cannot be used.",
            Self::TargetUnreadable => "The configuration file could not be read.",
            Self::TargetTooLarge => "The configuration file is too large to replace.",
            Self::BackupUnavailable => "The backup could not be stored.",
            Self::WriteFailed => "The configuration file could not be written.",
            Self::OutcomeUnknown => "The configuration write result is unknown.",
            Self::StorageUnavailable => "Private storage is unavailable.",
            Self::LogWriteFailed => "The write operation could not be recorded.",
            Self::LogReadFailed => "Recorded write operations could not be read.",
            Self::InvalidStoredOperation => "A recorded write operation is invalid.",
            Self::PlanStale => "The reviewed restore plan no longer matches the files.",
            Self::HistoryActive => "The history entry belongs to an unfinished write.",
            Self::HistoryUnresolved => {
                "Interrupted history must be resolved before this entry can be deleted."
            }
            Self::HistoryReferenced => "Other retained history still depends on this entry.",
        })
    }
}

impl std::error::Error for WriteError {}

/// Which feature owns a recorded write operation.
///
/// Stored as text with a length bound only, so a later feature reuses the log
/// without rebuilding the table; Rust decides which values are valid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationKind {
    /// Claude Code user settings, written by the model-link apply step (P20).
    ClaudeCodeSettings,
    /// Pi provider/model definitions and default model selection.
    PiSettings,
    /// Pi user-level MCP entries, independently journaled from model settings.
    PiMcp,
    /// Grok Build model entry and default model selection in `config.toml`.
    GrokSettings,
    /// Grok user-level MCP entries in `config.toml`, journaled apart from models.
    GrokMcp,
    /// Claude Code user-scope MCP entries in `.claude.json` (top-level
    /// `mcpServers` only), journaled apart from `settings.json` model links.
    ClaudeMcp,
}

impl OperationKind {
    /// Value stored in `config_operation.kind`.
    fn as_str(self) -> &'static str {
        match self {
            Self::ClaudeCodeSettings => "claude_code_settings",
            Self::PiSettings => "pi_settings",
            Self::PiMcp => "pi_mcp",
            Self::GrokSettings => "grok_settings",
            Self::GrokMcp => "grok_mcp",
            Self::ClaudeMcp => "claude_mcp",
        }
    }

    /// Parse a stored value; an unknown kind is a malformed row, not a new feature.
    fn from_stored(value: &str) -> Option<Self> {
        match value {
            "claude_code_settings" => Some(Self::ClaudeCodeSettings),
            "pi_settings" => Some(Self::PiSettings),
            "pi_mcp" => Some(Self::PiMcp),
            "grok_settings" => Some(Self::GrokSettings),
            "grok_mcp" => Some(Self::GrokMcp),
            "claude_mcp" => Some(Self::ClaudeMcp),
            _ => None,
        }
    }
}

/// How far a recorded operation got.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationState {
    /// Recorded before the first replacement; steps say how far it got.
    Planned,
    /// Every step replaced its file.
    Applied,
    /// No step replaced its file, so nothing needs undoing.
    Failed,
    /// Some steps replaced their files and the operation then stopped.
    Partial,
    /// A step's outcome could not be verified; the files must be read before trusting them.
    Unknown,
    /// A failed operation whose applied steps were rolled back from their backups.
    Compensated,
}

impl OperationState {
    /// Value stored in `config_operation.state`.
    fn as_str(self) -> &'static str {
        match self {
            Self::Planned => "planned",
            Self::Applied => "applied",
            Self::Failed => "failed",
            Self::Partial => "partial",
            Self::Unknown => "unknown",
            Self::Compensated => "compensated",
        }
    }

    /// Parse a stored value; an unknown state is a malformed row.
    fn from_stored(value: &str) -> Option<Self> {
        match value {
            "planned" => Some(Self::Planned),
            "applied" => Some(Self::Applied),
            "failed" => Some(Self::Failed),
            "partial" => Some(Self::Partial),
            "unknown" => Some(Self::Unknown),
            "compensated" => Some(Self::Compensated),
            _ => None,
        }
    }

    /// Whether the operation still needs an answer after a restart.
    pub fn is_unfinished(self) -> bool {
        matches!(self, Self::Planned | Self::Partial | Self::Unknown)
    }
}

/// What one step of an operation did to its file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StepState {
    /// Recorded and not attempted yet.
    Pending,
    /// The file was replaced with the planned content.
    Applied,
    /// The file was left as it was, and `error_code` says why.
    Failed,
}

impl StepState {
    /// Value stored in `config_operation_step.state`.
    fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Applied => "applied",
            Self::Failed => "failed",
        }
    }

    /// Parse a stored value; an unknown state is a malformed row.
    fn from_stored(value: &str) -> Option<Self> {
        match value {
            "pending" => Some(Self::Pending),
            "applied" => Some(Self::Applied),
            "failed" => Some(Self::Failed),
            _ => None,
        }
    }
}

/// What a rollback did to one applied step.
///
/// Stored per step so a partly rolled back operation can be retried: without it
/// a second attempt would treat a file that already holds its previous content
/// as one the user edited, and the operation would stay `partial` forever.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RestoreState {
    /// The previous content is back, or a file this module created is gone.
    Restored,
    /// The file changed after the write, so the rollback left it untouched.
    Conflict,
    /// The stored copy is missing or damaged, so the previous content cannot be produced.
    Unavailable,
    /// Writing the previous content back did not complete.
    Failed,
    /// Whether the rollback happened could not be checked.
    Unknown,
}

impl RestoreState {
    /// Value stored in `config_operation_step.restore_state`.
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Restored => "restored",
            Self::Conflict => "conflict",
            Self::Unavailable => "unavailable",
            Self::Failed => "failed",
            Self::Unknown => "unknown",
        }
    }

    /// Parse a stored value; an unknown value is a malformed row.
    pub(crate) fn from_stored(value: &str) -> Option<Self> {
        match value {
            "restored" => Some(Self::Restored),
            "conflict" => Some(Self::Conflict),
            "unavailable" => Some(Self::Unavailable),
            "failed" => Some(Self::Failed),
            "unknown" => Some(Self::Unknown),
            _ => None,
        }
    }
}

/// One recorded file of an operation, without any file content.
#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StepRecord {
    /// Position of the step in the operation, counted from zero.
    pub ordinal: u32,
    /// Absolute path this step replaces.
    pub target_path: String,
    /// SHA-256 the plan saw, or `None` when the file did not exist yet.
    pub expected_digest: Option<String>,
    /// What happened to this file.
    pub state: StepState,
    /// Stable code of the failure; present exactly when `state` is `failed`.
    pub error_code: Option<String>,
    /// Copy of the content this step replaced, when it replaced one.
    pub backup: Option<Backup>,
    /// SHA-256 written by this step; present exactly when it applied.
    pub applied_digest: Option<String>,
    /// Whether the target existed before this step ran; present exactly when it
    /// applied. A step without a copy either created the file (`false`) or found
    /// the planned content already in place (`true`), and a restore must not
    /// confuse the two. `None` means an older build wrote the row without it.
    pub previous_existed: Option<bool>,
    /// What a rollback did to this file; `None` until one is attempted.
    pub restore_state: Option<RestoreState>,
}

/// One recorded write operation with its steps.
#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationRecord {
    /// Stored identity of the operation.
    pub id: String,
    /// Feature that owns the operation.
    pub kind: OperationKind,
    /// Reference to what the operation wrote configuration for, when it wrote one.
    pub owner_ref: Option<String>,
    /// How far the operation got.
    pub state: OperationState,
    /// Unix milliseconds when the operation was recorded.
    pub created_at: i64,
    /// Unix milliseconds of its last change.
    pub updated_at: i64,
    /// Steps in the order they are applied.
    pub steps: Vec<StepRecord>,
}

/// Replace one configuration file with `content` after checking the plan token.
///
/// The file is written through a temporary file in the same directory and then
/// renamed, so a reader sees either the previous or the new content. The
/// previous bytes are stored in `backups` first; a write that cannot be backed
/// up does not happen. An unchanged target reports `changed: false` without
/// touching the disk.
///
/// # Errors
/// Returns `TargetChanged` when the file no longer matches the plan token,
/// `SymlinkRefused`, `TargetReadOnly`, `TargetPathInvalid`, `TargetUnreadable`
/// or `TargetTooLarge` for a target this module refuses to replace,
/// `BackupUnavailable` when the previous content could not be stored, and
/// `WriteFailed` or `OutcomeUnknown` when the replacement did not complete or
/// could not be verified. No error carries file content.
pub fn apply_file(
    target: &WriteTarget,
    content: &[u8],
    backups: &BackupStore,
) -> Result<AppliedFile, WriteError> {
    apply_file_with_mode(target, content, backups, true, None)
}

/// Recorded service path; private writes replace Unix permissions with 0600.
fn apply_recorded_file(
    target: &WriteTarget,
    content: &[u8],
    backups: &BackupStore,
    private: bool,
    revision: &revisions::RevisionContext<'_>,
) -> Result<AppliedFile, WriteError> {
    apply_file_with_mode(target, content, backups, !private, Some(revision))
}

fn apply_file_with_mode(
    target: &WriteTarget,
    content: &[u8],
    backups: &BackupStore,
    preserve_mode: bool,
    revision: Option<&revisions::RevisionContext<'_>>,
) -> Result<AppliedFile, WriteError> {
    let current = read_target(&target.path)?;
    if current.symlink {
        return Err(WriteError::SymlinkRefused);
    }
    if current.content_digest != target.expected_digest {
        return Err(WriteError::TargetChanged);
    }
    let new_digest = digest_hex(content);
    if current.content_digest.as_deref() == Some(new_digest.as_str())
        && (preserve_mode || has_private_mode(&target.path)?)
    {
        return Ok(AppliedFile {
            target: current,
            backup: None,
            changed: false,
        });
    }
    let previous = match current.exists {
        true => {
            if is_read_only(&target.path)? {
                return Err(WriteError::TargetReadOnly);
            }
            Some(read_replaced_bytes(&target.path)?)
        }
        false => None,
    };
    // A second read must not silently back up bytes different from the approved
    // token. A concurrent external writer still cannot be locked by this app.
    if previous.as_ref().map(|bytes| digest_hex(bytes)) != target.expected_digest {
        return Err(WriteError::TargetChanged);
    }
    let id = operation_id();
    let backup = match &previous {
        Some(bytes) => Some(backups.store(&id, &file_name(&target.path), bytes)?),
        None => None,
    };
    if let Some(revision) = revision {
        revision.prepare(&id, target, backup.as_ref(), Some(&new_digest))?;
    }
    let result = (|| {
        // Recheck after backup/database I/O as well as before collecting bytes.
        let checked = read_target(&target.path)?;
        if checked.symlink || checked.content_digest != target.expected_digest {
            return Err(WriteError::TargetChanged);
        }
        replace(
            &target.path,
            content,
            previous.is_some() && preserve_mode,
            &id,
        )?;
        let after = read_target(&target.path).map_err(|_| WriteError::OutcomeUnknown)?;
        if after.content_digest.as_deref() != Some(new_digest.as_str()) {
            return Err(WriteError::OutcomeUnknown);
        }
        Ok(after)
    })();
    if let Some(revision) = revision {
        // Even failed writes retain the referenced snapshot. In particular an
        // uncertain rename must never discard its only recoverable pre-image.
        revision.finish(&id, &result)?;
    } else if result.is_err()
        && result != Err(WriteError::OutcomeUnknown)
        && let Some(backup) = &backup
    {
        let _ = backups.discard(backup);
    }
    Ok(AppliedFile {
        target: result?,
        backup,
        changed: true,
    })
}

/// Replace the target through a same-directory temporary file.
///
/// A failure removes the temporary file and leaves the target untouched.
fn replace(path: &Path, content: &[u8], keep_mode: bool, id: &str) -> Result<(), WriteError> {
    let directory = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .ok_or(WriteError::TargetPathInvalid)?;
    // A first-time user has no configuration directory yet; the preview already
    // reported the target as a new file at this path.
    fs::create_dir_all(directory).map_err(|_| WriteError::WriteFailed)?;
    let temporary = directory.join(format!(".{}.vibemate-{id}.tmp", file_name(path)));
    write_temporary(&temporary, content)?;
    if keep_mode && let Err(error) = copy_mode(path, &temporary) {
        let _ = fs::remove_file(&temporary);
        return Err(error);
    }
    if fs::rename(&temporary, path).is_err() {
        let _ = fs::remove_file(&temporary);
        return Err(WriteError::WriteFailed);
    }
    sync_directory(directory)
}

/// Create the temporary file with private access and write the planned bytes.
fn write_temporary(path: &Path, content: &[u8]) -> Result<(), WriteError> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        // The file can hold a credential before it replaces the target.
        options.mode(0o600);
    }
    let mut file = options.open(path).map_err(|_| WriteError::WriteFailed)?;
    if file
        .write_all(content)
        .and_then(|()| file.sync_all())
        .is_err()
    {
        drop(file);
        let _ = fs::remove_file(path);
        return Err(WriteError::WriteFailed);
    }
    Ok(())
}

/// Read a target's identity through the preview's reader, so both steps agree on
/// what an unchanged file means.
///
/// The writer deliberately does not parse the file: a settings document is JSON
/// for Claude Code and Pi but TOML for Grok Build, and its shape belongs to the
/// adapter that planned the change. What both sides must share is the identity of
/// the exact bytes at that path — existence, digest, symbolic-link flag and the
/// size limit.
fn read_target(path: &Path) -> Result<TargetFile, WriteError> {
    read_target_bytes(path)
        .map_err(|error| match error {
            PlanError::TargetPathInvalid => WriteError::TargetPathInvalid,
            PlanError::TargetTooLarge => WriteError::TargetTooLarge,
            _ => WriteError::TargetUnreadable,
        })
        .map(|(target, _bytes)| target)
}

/// Read the bytes that are about to be replaced, within the preview limit.
fn read_replaced_bytes(path: &Path) -> Result<Vec<u8>, WriteError> {
    let metadata = fs::metadata(path).map_err(|_| WriteError::TargetUnreadable)?;
    if metadata.len() > MAX_TARGET_BYTES {
        return Err(WriteError::TargetTooLarge);
    }
    fs::read(path).map_err(|_| WriteError::TargetUnreadable)
}

/// Whether the user marked the target read-only.
///
/// A read-only file inside a writable directory can still be replaced by a
/// rename, so the permission is checked explicitly instead of surfacing as a
/// surprising success.
fn is_read_only(path: &Path) -> Result<bool, WriteError> {
    fs::metadata(path)
        .map(|metadata| metadata.permissions().readonly())
        .map_err(|_| WriteError::TargetUnreadable)
}

/// Fresh identifier for one write: backup directory name and temporary suffix.
///
/// The value only has to make two writes of the same file distinct; the stored
/// operation record (P19.b) is what identifies a write across restarts.
fn operation_id() -> String {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let counter = NEXT.fetch_add(1, Ordering::Relaxed);
    let millis = shared::current_unix_millis().unwrap_or(0);
    format!("{millis}-{}-{counter}", std::process::id())
}

/// Name used inside a backup directory.
fn file_name(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .map(str::to_owned)
        .unwrap_or_else(|| "target".to_owned())
}

/// Absolute, printable path text matching the preview's rules.
fn path_text(path: &Path) -> Result<String, WriteError> {
    let text = path.to_str().ok_or(WriteError::TargetPathInvalid)?;
    (path.is_absolute() && text.len() <= 4096 && !text.chars().any(char::is_control))
        .then(|| text.to_owned())
        .ok_or(WriteError::TargetPathInvalid)
}

/// Create the backup directory chain with owner-only access.
#[cfg(unix)]
fn create_private_directory(directory: &Path) -> Result<(), WriteError> {
    use std::os::unix::fs::DirBuilderExt;
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true).mode(0o700);
    builder
        .create(directory)
        .map_err(|_| WriteError::BackupUnavailable)
}

/// Other platforms do not expose a comparable directory mode.
#[cfg(not(unix))]
fn create_private_directory(directory: &Path) -> Result<(), WriteError> {
    fs::create_dir_all(directory).map_err(|_| WriteError::BackupUnavailable)
}

/// Store bytes in a private file of the backup directory.
fn create_private_file(path: &Path, bytes: &[u8]) -> Result<(), WriteError> {
    let mut options = OpenOptions::new();
    // A reused identifier must fail rather than overwrite a retained version or
    // follow a link that unexpectedly appeared inside the backup directory.
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(path)
        .map_err(|_| WriteError::BackupUnavailable)?;
    file.write_all(bytes)
        .and_then(|()| file.sync_all())
        .map_err(|_| WriteError::BackupUnavailable)
}

/// Whether the existing file already has the private creation mode.
#[cfg(unix)]
fn has_private_mode(path: &Path) -> Result<bool, WriteError> {
    use std::os::unix::fs::PermissionsExt;
    fs::metadata(path)
        .map(|m| m.permissions().mode() & 0o7777 == 0o600)
        .map_err(|_| WriteError::WriteFailed)
}

/// Windows permissions are inherited from the user's directory; Unix mode bits
/// are not an ACL substitute. Native Windows validation remains separate.
#[cfg(not(unix))]
fn has_private_mode(_: &Path) -> Result<bool, WriteError> {
    Ok(true)
}

/// Keep the user's permission bits on the replacement file.
///
/// Preserving them keeps a private file private; the create mode of 0600 covers
/// the case where no earlier file existed.
#[cfg(unix)]
fn copy_mode(from: &Path, to: &Path) -> Result<(), WriteError> {
    use std::os::unix::fs::PermissionsExt;
    let mode = fs::metadata(from)
        .map_err(|_| WriteError::WriteFailed)?
        .permissions()
        .mode()
        & 0o7777;
    fs::set_permissions(to, fs::Permissions::from_mode(mode)).map_err(|_| WriteError::WriteFailed)
}

/// Other platforms do not carry a comparable permission model.
#[cfg(not(unix))]
fn copy_mode(_: &Path, _: &Path) -> Result<(), WriteError> {
    Ok(())
}

/// Flush the directory entry after the rename, so a crash cannot lose the name.
///
/// A failure here means the new content is in place but its durability is
/// unknown, which is reported instead of being called a success.
#[cfg(unix)]
fn sync_directory(directory: &Path) -> Result<(), WriteError> {
    let handle = fs::File::open(directory).map_err(|_| WriteError::OutcomeUnknown)?;
    handle.sync_all().map_err(|_| WriteError::OutcomeUnknown)
}

/// Windows does not open a directory through `File`; the rename itself is still
/// atomic and the content is flushed before it happens.
#[cfg(not(unix))]
fn sync_directory(_: &Path) -> Result<(), WriteError> {
    Ok(())
}

mod cleanup;
mod history;
mod history_delete;
mod inspection;
mod mcp_support;
mod operations;
mod resolution;
mod restore;
mod revisions;
mod service;
mod snapshots;
mod version_restore;
pub(crate) mod version_text;
mod versions;

pub use cleanup::{
    BackupCleanupOutcome, BackupInventory, UnreferencedBackup, cleanup_backup_copies,
    inspect_backup_copies,
};
pub use history::{
    HistoryCurrentConfig, HistoryCurrentFile, HistoryCurrentStatus, HistoryCursor, HistoryEntry,
    HistoryIntent, HistoryPage, HistoryResolution, HistoryRevision, HistoryRevisionState,
    HistoryScope, HistorySourceStep, configuration_history, configuration_versions,
};
pub use history_delete::{
    HistoryDeleteLeftover, HistoryDeleteOutcome, HistoryDeleteState, delete_configuration_version,
    delete_history_entry,
};
pub use inspection::{
    RevisionBackupState, RevisionInspection, RevisionMatch, inspect_interrupted_revisions,
    operation_requires_resolution,
};
pub(crate) use mcp_support::{
    read_mcp_written_file, read_mcp_written_file_kind, resolve_mcp_observation_kind_locked,
    resolve_mcp_observation_locked,
};
pub use operations::{
    begin_operation, finish_operation, has_recorded_target, operation, recent_operations,
    record_step_applied, record_step_failed, record_step_restored, unfinished_operations,
};
pub use resolution::{
    RevisionResolutionPlan, plan_revision_resolution, resolve_interrupted_revisions,
};
pub use restore::{
    RestoreAction, RestoreOutcome, RestorePlan, RestoreStepOutcome, RestoreStepPlan,
    ReviewedRestoreStep, apply_restore, plan_restore, remove_created_file, restore_file,
};
pub use service::{AppliedOperation, PlannedFile, StepOutcome, apply_files, compensate_operation};
pub(crate) use service::{apply_private_files, apply_private_files_locked};
pub use version_restore::{
    CurrentConfigText, ReviewedVersionFile, VersionRestoreAction, VersionRestoreFile,
    VersionRestoreFileOutcome, VersionRestoreOutcome, VersionRestorePlan, apply_version_restore,
    apply_written_version, plan_version_restore, plan_written_version, read_current_text,
    read_version_text, read_written_version_text,
};
pub use version_text::{
    ConfigFormat, FieldChange, FieldChangeKind, VersionFileSummary, VersionText, VersionTextSide,
};
pub use versions::materialize_configuration_versions;

#[cfg(test)]
mod operation_tests;
#[cfg(test)]
mod restore_tests;
#[cfg(test)]
mod revision_tests;
#[cfg(test)]
mod service_tests;
#[cfg(test)]
mod tests;
