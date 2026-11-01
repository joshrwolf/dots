//! Exclusive coordination for operations spanning document bytes and the catalog.
use std::fs::{File, OpenOptions, TryLockError};
use std::path::Path;

use crate::{Result, directories, invalid, io, paths};

/// A kernel-held lock. Keep it alive through the file operation and catalog commit.
/// Acquire it before a database write transaction, never from inside one.
/// The stable lock file must never be removed or replaced by cooperating callers.
#[derive(Debug)]
#[must_use]
pub struct MutationLock {
    _file: File,
}

impl MutationLock {
    /// Acquire without waiting; contention is a recoverable busy error.
    /// Closing the last handle releases the lock, including on process exit.
    pub fn acquire(path: &Path) -> Result<Self> {
        if !path.is_absolute() {
            return Err(invalid("mutation lock path must be absolute"));
        }
        paths::reject_symlink(path)?;
        let parent = path.parent().ok_or_else(|| invalid("lock has no parent"))?;
        directories::create(parent)?;
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt as _;
            options.mode(0o600);
        }
        let file = options
            .open(path)
            .map_err(|error| io("open mutation lock", path, error))?;
        if !file
            .metadata()
            .map_err(|error| io("inspect mutation lock", path, error))?
            .is_file()
        {
            return Err(invalid("mutation lock must be a regular file"));
        }
        match file.try_lock() {
            Ok(()) => Ok(Self { _file: file }),
            Err(TryLockError::WouldBlock) => Err(invalid(
                "workspace mutation is busy; retry after the writer finishes",
            )),
            Err(TryLockError::Error(error)) => Err(io("acquire mutation lock", path, error)),
        }
    }
}
