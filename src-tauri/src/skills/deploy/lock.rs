//! Process-wide lock that serializes every skill deploy write.
//!
//! Apply, resync, disable, recovery finish/dismiss, the copy refresh after a
//! library update and the leftover sweep all hold it for their whole run, so
//! one operation can never sweep or remove a staging folder (or a journal row)
//! that another operation in this process is still using. Previews and lists
//! are read-only and never take it.
//!
//! The guard is an owned token rather than a `MutexGuard`, so it can be taken
//! on one thread and released on another: application setup takes it
//! synchronously and hands it to the background startup sweep, which makes
//! every deploy command wait until that sweep has finished without blocking
//! the window. A waiter gives up after `WAIT_LIMIT` with `operation_failed`,
//! so a sweep stuck on an unresponsive network volume cannot hang a command
//! forever.

use super::super::types::SkillError;
use std::sync::{Condvar, Mutex, PoisonError};
use std::time::Duration;

/// Longest time a deploy command waits for the lock.
const WAIT_LIMIT: Duration = Duration::from_secs(120);

static HELD: Mutex<bool> = Mutex::new(false);
static FREED: Condvar = Condvar::new();

/// Proof that the caller holds the deploy lock; released on drop.
#[must_use = "the deploy lock is released as soon as the guard is dropped"]
pub struct SkillDeployGuard {
    _private: (),
}

impl std::fmt::Debug for SkillDeployGuard {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("SkillDeployGuard")
    }
}

/// Wait for and take the deploy lock.
///
/// # Errors
/// `operation_failed` when the lock is still held after `WAIT_LIMIT`.
pub fn lock_skill_deploys() -> Result<SkillDeployGuard, SkillError> {
    acquire(WAIT_LIMIT)
}

pub(super) fn acquire(limit: Duration) -> Result<SkillDeployGuard, SkillError> {
    // The flag is a plain bool, so a panic elsewhere cannot leave it torn.
    let held = HELD.lock().unwrap_or_else(PoisonError::into_inner);
    let (mut held, timeout) = FREED
        .wait_timeout_while(held, limit, |held| *held)
        .unwrap_or_else(PoisonError::into_inner);
    if timeout.timed_out() && *held {
        return Err(SkillError::OperationFailed);
    }
    *held = true;
    Ok(SkillDeployGuard { _private: () })
}

impl Drop for SkillDeployGuard {
    fn drop(&mut self) {
        let mut held = HELD.lock().unwrap_or_else(PoisonError::into_inner);
        *held = false;
        drop(held);
        FREED.notify_one();
    }
}
