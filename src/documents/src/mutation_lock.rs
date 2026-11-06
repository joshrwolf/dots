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
    file: File,
}

impl Drop for MutationLock {
    fn drop(&mut self) {
        // Forked children can share this description until exec closes it.
        // Explicit unlock releases our ownership; closing remains the fallback.
        let _ = self.file.unlock();
    }
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
            Ok(()) => Ok(Self { file }),
            Err(TryLockError::WouldBlock) => Err(crate::Error::Busy),
            Err(TryLockError::Error(error)) => Err(io("acquire mutation lock", path, error)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::staged_directory::StagedDirectory;

    #[test]
    fn dropping_owner_releases_lock_even_with_an_inherited_description()
    -> std::result::Result<(), Box<dyn std::error::Error>> {
        let directory = StagedDirectory::create(&std::env::temp_dir())?;
        let path = directory.path().join("mutation.lock");
        let owner = MutationLock::acquire(&path)?;
        let inherited = owner.file.try_clone()?;
        drop(owner);
        let replacement = MutationLock::acquire(&path)?;
        drop(inherited);
        assert!(matches!(
            MutationLock::acquire(&path),
            Err(crate::Error::Busy)
        ));
        drop(replacement);
        let _last = MutationLock::acquire(&path)?;
        Ok(())
    }
}
