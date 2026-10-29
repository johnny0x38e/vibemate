//! One running vibemate per app-data folder.
//!
//! At startup the process takes an exclusive OS lock on `vibemate.lock` in the
//! app-data folder and keeps the open file for its whole lifetime. The lock is
//! advisory and owned by the kernel: it is released when the process exits,
//! including a crash or a forced quit, so a stale lock file never blocks the
//! next launch. A second launch that finds the lock held exits cleanly before
//! it opens the database or touches any Agent folder.
//!
//! The lock is tied to the very folder whose database, journal and staging
//! rules it protects, so it holds for every way the app can be started (a
//! second binary, `open -n`, a development build sharing the identifier).
//! It uses `std::fs::File::try_lock` (`flock` on Unix, `LockFileEx` on
//! Windows) and needs no extra dependency, plugin or capability.

use std::fs::{self, File, OpenOptions, TryLockError};
use std::path::Path;

/// Lock file name inside the app-data folder.
pub(crate) const LOCK_FILE: &str = "vibemate.lock";

/// Outcome of trying to become the single running instance.
#[derive(Debug)]
pub(crate) enum InstanceLock {
    /// This process holds the lock; keep the file open until exit.
    Exclusive {
        /// Held open only to keep the lock.
        _file: File,
    },
    /// Another running process holds the lock.
    HeldElsewhere,
    /// The lock file could not be created or the filesystem does not support
    /// locking. The app still starts, but skips work that assumes it is the
    /// only instance (the startup sweep of deploy leftovers).
    Unavailable,
}

impl InstanceLock {
    /// Whether this process may assume it is the only running instance.
    pub(crate) fn is_exclusive(&self) -> bool {
        matches!(self, Self::Exclusive { .. })
    }
}

/// Try to take the instance lock in `data_directory` without waiting.
pub(crate) fn acquire(data_directory: &Path) -> InstanceLock {
    if fs::create_dir_all(data_directory).is_err() {
        return InstanceLock::Unavailable;
    }
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let Ok(file) = options.open(data_directory.join(LOCK_FILE)) else {
        return InstanceLock::Unavailable;
    };
    match file.try_lock() {
        Ok(()) => InstanceLock::Exclusive { _file: file },
        Err(TryLockError::WouldBlock) => InstanceLock::HeldElsewhere,
        Err(TryLockError::Error(_)) => InstanceLock::Unavailable,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn temp_dir() -> std::path::PathBuf {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        std::env::temp_dir().join(format!(
            "vibemate-instance-lock-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ))
    }

    #[test]
    fn a_second_holder_is_refused_until_the_first_releases() {
        let dir = temp_dir().join("app-data");
        let first = acquire(&dir);
        assert!(first.is_exclusive(), "{first:?}");
        // A second open file description conflicts like a second process.
        assert!(matches!(acquire(&dir), InstanceLock::HeldElsewhere));
        drop(first);
        let again = acquire(&dir);
        assert!(again.is_exclusive(), "released on drop: {again:?}");
        // The lock file is left in place; it carries no state.
        assert!(dir.join(LOCK_FILE).is_file());
        drop(again);
        let _ = fs::remove_dir_all(dir.parent().unwrap_or(&dir));
    }

    #[test]
    fn an_unusable_folder_is_reported_as_unavailable() {
        let base = temp_dir();
        fs::create_dir_all(&base).expect("base");
        let blocker = base.join("not-a-folder");
        fs::write(&blocker, "file").expect("file");
        assert!(matches!(acquire(&blocker), InstanceLock::Unavailable));
        let _ = fs::remove_dir_all(&base);
    }

    #[cfg(unix)]
    #[test]
    fn the_lock_file_is_owner_only_on_unix() {
        use std::os::unix::fs::PermissionsExt;
        let dir = temp_dir();
        let lock = acquire(&dir);
        assert!(lock.is_exclusive());
        let mode = fs::metadata(dir.join(LOCK_FILE))
            .expect("metadata")
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
        drop(lock);
        let _ = fs::remove_dir_all(&dir);
    }
}
