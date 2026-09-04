use std::fs::{File, OpenOptions};
use std::path::Path;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context as _, Result, bail};

/// Stable FNV-1a for filesystem and process identities that must survive Rust
/// upgrades. This is not a security boundary; it only keeps names short.
pub(crate) fn stable_hash(value: &[u8]) -> u64 {
    value.iter().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

/// An operating-system lock that is released even when the process crashes.
#[derive(Debug)]
pub(crate) struct FileLock {
    _file: File,
}

impl FileLock {
    pub(crate) fn try_acquire(path: &Path) -> Result<Option<Self>> {
        let file = open(path)?;
        match file.try_lock() {
            Ok(()) => Ok(Some(Self { _file: file })),
            Err(std::fs::TryLockError::WouldBlock) => Ok(None),
            Err(std::fs::TryLockError::Error(error)) => {
                Err(error).with_context(|| format!("locking {}", path.display()))
            }
        }
    }

    pub(crate) fn acquire(path: &Path, timeout: Duration) -> Result<Self> {
        let started = Instant::now();
        loop {
            if let Some(lock) = Self::try_acquire(path)? {
                return Ok(lock);
            }
            if started.elapsed() >= timeout {
                bail!(
                    "could not acquire lock {} within {timeout:?}",
                    path.display()
                );
            }
            thread::sleep(Duration::from_millis(100));
        }
    }
}

fn open(path: &Path) -> Result<File> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
    }
    OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)
        .with_context(|| format!("opening {}", path.display()))
}
