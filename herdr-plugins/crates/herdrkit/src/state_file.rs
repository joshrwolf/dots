//! One private, bounded JSON record under an OS-held exclusive lock.
//!
//! Callers choose a directory under a trusted parent. Existing unsafe paths are
//! rejected, never chmodded, followed, or adopted. The stable lock file must not
//! be removed while another process can hold it. This is not a hostile-same-user
//! filesystem sandbox, nor a multi-record database.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read as _, Write as _};
use std::os::unix::fs::{DirBuilderExt as _, MetadataExt as _, OpenOptionsExt as _};
use std::path::{Path, PathBuf};

use serde::{Serialize, de::DeserializeOwned};

pub const MAX_BYTES: usize = 64 * 1024;
const RECORD: &str = "record.json";
const TEMPORARY: &str = "record.next";

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    #[error("state directory is already locked")]
    Busy,
    #[error("state path must be private, owned, and not a symlink: {0}")]
    UnsafePath(PathBuf),
    #[error("state record exceeds 64 KiB")]
    TooLarge,
    #[error("state JSON is invalid")]
    Json(#[from] serde_json::Error),
    #[error("{operation}")]
    Io {
        operation: &'static str,
        #[source]
        source: io::Error,
    },
    #[error(
        "state record was published but durability was not confirmed; reread before reconciliation"
    )]
    CommitUncertain(#[source] io::Error),
}
type Result<T> = std::result::Result<T, Error>;

#[derive(Debug)]
pub struct StateFile {
    path: PathBuf,
    directory: File,
    _lock: File,
}

impl StateFile {
    /// Lock one slot. Its parent tree must already exist durably and must not
    /// be replaceable by other users. Contention fails immediately; locks are
    /// released on process exit.
    pub fn lock(path: &Path) -> Result<Self> {
        if !path.is_absolute() || path.file_name().is_none() {
            return Err(Error::UnsafePath(path.into()));
        }
        match fs::DirBuilder::new().mode(0o700).create(path) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(source) => {
                return Err(Error::Io {
                    operation: "creating state directory",
                    source,
                });
            }
        }
        let directory = operation(
            "opening state directory",
            OpenOptions::new()
                .read(true)
                .custom_flags(nix::libc::O_NOFOLLOW | nix::libc::O_DIRECTORY)
                .open(path),
        )?;
        check(
            path,
            &operation("inspecting state directory", directory.metadata())?,
            true,
        )?;
        let lock_path = path.join("record.lock");
        let lock = operation(
            "opening state lock",
            OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .mode(0o600)
                .custom_flags(nix::libc::O_NOFOLLOW | nix::libc::O_NONBLOCK)
                .open(&lock_path),
        )?;
        check(
            &lock_path,
            &operation("inspecting state lock", lock.metadata())?,
            false,
        )?;
        match lock.try_lock() {
            Ok(()) => {}
            Err(fs::TryLockError::WouldBlock) => return Err(Error::Busy),
            Err(fs::TryLockError::Error(source)) => {
                return Err(Error::Io {
                    operation: "locking state directory",
                    source,
                });
            }
        }
        // Sync even an existing slot's parent: a previous process may have
        // crashed after mkdir, before making that directory entry durable.
        let parent = operation(
            "opening state parent",
            File::open(
                path.parent()
                    .ok_or_else(|| Error::UnsafePath(path.into()))?,
            ),
        )?;
        operation("syncing state directory entry", parent.sync_all())?;
        Ok(Self {
            path: path.into(),
            directory,
            _lock: lock,
        })
    }

    /// Missing is distinct from unreadable, corrupt, unsafe, or oversized.
    pub fn read<T: DeserializeOwned>(&self) -> Result<Option<T>> {
        let path = self.path.join(RECORD);
        let file = match OpenOptions::new()
            .read(true)
            .custom_flags(nix::libc::O_NOFOLLOW | nix::libc::O_NONBLOCK)
            .open(&path)
        {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(source) => {
                return Err(Error::Io {
                    operation: "opening state record",
                    source,
                });
            }
        };
        check(
            &path,
            &operation("inspecting state record", file.metadata())?,
            false,
        )?;
        let mut bytes = Vec::new();
        operation(
            "reading state record",
            file.take((MAX_BYTES + 1) as u64).read_to_end(&mut bytes),
        )?;
        if bytes.len() > MAX_BYTES {
            return Err(Error::TooLarge);
        }
        Ok(Some(serde_json::from_slice(&bytes)?))
    }

    /// Serialize before touching disk; sync the temporary file, rename in the
    /// same directory, then sync that directory. No external effect is authorized
    /// unless this returns success. Any error requires rereading durable intent;
    /// `CommitUncertain` specifically means rename already succeeded.
    /// The OS lock excludes other handles; a mutable borrow excludes concurrent
    /// writes through the same handle.
    ///
    /// ```compile_fail,E0596
    /// use herdrkit::state_file::StateFile;
    /// fn overwrite_through_shared_handle(slot: &StateFile) {
    ///     slot.write(&"new state").unwrap();
    /// }
    /// ```
    pub fn write<T: Serialize>(&mut self, value: &T) -> Result<()> {
        self.write_with(value, |_| Ok(()))
    }

    fn write_with<T: Serialize>(
        &mut self,
        value: &T,
        checkpoint: impl Fn(Stage) -> io::Result<()>,
    ) -> Result<()> {
        let mut bytes = Limited {
            bytes: Vec::new(),
            exceeded: false,
        };
        if let Err(error) = serde_json::to_writer(&mut bytes, value) {
            return Err(if bytes.exceeded {
                Error::TooLarge
            } else {
                Error::Json(error)
            });
        }
        let target = self.path.join(RECORD);
        Self::check_existing(&target)?;
        let temporary = self.path.join(TEMPORARY);
        // An interrupted pre-rename write is never authoritative. Only discard
        // our private regular scratch file, and only while holding the lock.
        if Self::check_existing(&temporary)? {
            operation(
                "removing interrupted state write",
                fs::remove_file(&temporary),
            )?;
        }
        let mut file = operation(
            "creating temporary state record",
            OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .custom_flags(nix::libc::O_NOFOLLOW)
                .open(&temporary),
        )?;
        operation(
            "writing temporary state record",
            file.write_all(&bytes.bytes),
        )?;
        operation("checking state write", checkpoint(Stage::Written))?;
        operation("syncing temporary state record", file.sync_all())?;
        operation("checking state sync", checkpoint(Stage::Synced))?;
        operation("publishing state record", fs::rename(&temporary, &target))?;
        checkpoint(Stage::Renamed).map_err(Error::CommitUncertain)?;
        self.directory.sync_all().map_err(Error::CommitUncertain)?;
        Ok(())
    }

    fn check_existing(path: &Path) -> Result<bool> {
        match fs::symlink_metadata(path) {
            Ok(metadata) => {
                check(path, &metadata, false)?;
                Ok(true)
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(source) => Err(Error::Io {
                operation: "inspecting existing state file",
                source,
            }),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stage {
    Written,
    Synced,
    Renamed,
}

fn operation<T>(operation: &'static str, result: io::Result<T>) -> Result<T> {
    result.map_err(|source| Error::Io { operation, source })
}

fn check(path: &Path, metadata: &fs::Metadata, directory: bool) -> Result<()> {
    let kind = if directory {
        metadata.is_dir()
    } else {
        metadata.is_file() && metadata.nlink() == 1
    };
    if !kind || metadata.uid() != nix::unistd::geteuid().as_raw() || metadata.mode() & 0o077 != 0 {
        return Err(Error::UnsafePath(path.into()));
    }
    Ok(())
}

struct Limited {
    bytes: Vec<u8>,
    exceeded: bool,
}
impl io::Write for Limited {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > MAX_BYTES.saturating_sub(self.bytes.len()) {
            self.exceeded = true;
            return Err(io::Error::other("state record exceeds limit"));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};
    use std::os::unix::fs::{PermissionsExt as _, symlink};
    use std::time::{SystemTime, UNIX_EPOCH};

    struct Scratch(PathBuf);
    impl Scratch {
        fn new() -> Self {
            let nonce = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let path =
                std::env::temp_dir().join(format!("herdrkit-state-{}-{nonce}", std::process::id()));
            fs::DirBuilder::new().mode(0o700).create(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn lock_is_exclusive_and_record_survives_reopening() {
        let root = Scratch::new();
        let path = root.0.join("slot");
        let mut slot = StateFile::lock(&path).unwrap();
        assert!(matches!(StateFile::lock(&path), Err(Error::Busy)));
        assert_eq!(slot.read::<Value>().unwrap(), None);
        slot.write(&json!({"generation":1})).unwrap();
        drop(slot);
        let slot = StateFile::lock(&path).unwrap();
        assert_eq!(slot.read::<Value>().unwrap(), Some(json!({"generation":1})));
        assert_eq!(
            fs::metadata(path.join(RECORD)).unwrap().mode() & 0o777,
            0o600
        );
    }

    #[test]
    fn interrupted_writes_preserve_old_or_publish_whole_new_record() {
        for stage in [Stage::Written, Stage::Synced, Stage::Renamed] {
            let root = Scratch::new();
            let mut slot = StateFile::lock(&root.0.join("slot")).unwrap();
            slot.write(&json!({"old":true})).unwrap();
            let result = slot.write_with(&json!({"new":true}), |at| {
                if at == stage {
                    Err(io::Error::other("injected interruption"))
                } else {
                    Ok(())
                }
            });
            assert!(result.is_err());
            assert_eq!(
                matches!(result, Err(Error::CommitUncertain(_))),
                stage == Stage::Renamed
            );
            drop(slot);
            let mut slot = StateFile::lock(&root.0.join("slot")).unwrap();
            let expected = if stage == Stage::Renamed {
                json!({"new":true})
            } else {
                json!({"old":true})
            };
            assert_eq!(slot.read::<Value>().unwrap(), Some(expected));
            slot.write(&json!({"retry":true})).unwrap();
            assert_eq!(slot.read::<Value>().unwrap(), Some(json!({"retry":true})));
        }
    }

    #[test]
    fn bounds_reads_and_serialization_without_overwriting_good_state() {
        let root = Scratch::new();
        let mut slot = StateFile::lock(&root.0.join("slot")).unwrap();
        slot.write(&json!("good")).unwrap();
        assert!(matches!(
            slot.write(&"x".repeat(MAX_BYTES + 1)),
            Err(Error::TooLarge)
        ));
        assert_eq!(slot.read::<Value>().unwrap(), Some(json!("good")));
        fs::write(slot.path.join(RECORD), vec![b'x'; MAX_BYTES + 1]).unwrap();
        assert!(matches!(slot.read::<Value>(), Err(Error::TooLarge)));
        fs::write(slot.path.join(RECORD), "not JSON").unwrap();
        assert!(matches!(slot.read::<Value>(), Err(Error::Json(_))));
    }

    #[test]
    fn rejects_unsafe_paths_without_changing_permissions_or_targets() {
        let root = Scratch::new();
        let public = root.0.join("public");
        fs::create_dir(&public).unwrap();
        fs::set_permissions(&public, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(matches!(
            StateFile::lock(&public),
            Err(Error::UnsafePath(_))
        ));
        assert_eq!(fs::metadata(&public).unwrap().mode() & 0o777, 0o755);
        let link = root.0.join("linked");
        symlink(&root.0, &link).unwrap();
        assert!(StateFile::lock(&link).is_err());
        let mut slot = StateFile::lock(&root.0.join("slot")).unwrap();
        let other = root.0.join("other");
        fs::write(&other, "\"untouched\"").unwrap();
        fs::set_permissions(&other, fs::Permissions::from_mode(0o600)).unwrap();
        for name in [RECORD, TEMPORARY] {
            let path = slot.path.join(name);
            symlink(&other, &path).unwrap();
            assert!(slot.write(&json!("bad")).is_err());
            if name == RECORD {
                assert!(slot.read::<Value>().is_err());
            }
            fs::remove_file(path).unwrap();
        }
        assert_eq!(fs::read_to_string(other).unwrap(), "\"untouched\"");
    }

    #[test]
    fn locks_exclude_other_processes_and_survive_abrupt_holder_exit() {
        let root = Scratch::new();
        let path = root.0.join("slot");
        let slot = StateFile::lock(&path).unwrap();
        let probe = || {
            let mut command = std::process::Command::new("python3");
            command.args(["-I", "-c", "import fcntl, os, sys\nf = open(sys.argv[1], 'r+')\ntry:\n fcntl.flock(f, fcntl.LOCK_EX | fcntl.LOCK_NB)\nexcept BlockingIOError:\n os._exit(3)\nos._exit(0)"]).arg(path.join("record.lock"));
            crate::process::output(command, std::time::Duration::from_secs(3), 4096)
                .unwrap()
                .status
                .code()
        };
        assert_eq!(probe(), Some(3));
        drop(slot);
        // os._exit bypasses language finalizers. Kernel lock release, rather
        // than a cleanup callback or a saved PID, allows the next acquisition.
        assert_eq!(probe(), Some(0));
        StateFile::lock(&path).unwrap();
    }

    #[test]
    fn non_regular_records_are_rejected_without_waiting_for_a_writer() {
        let root = Scratch::new();
        let mut slot = StateFile::lock(&root.0.join("slot")).unwrap();
        let mut command = std::process::Command::new("mkfifo");
        command.args(["-m", "600"]).arg(slot.path.join(RECORD));
        assert!(
            crate::process::output(command, std::time::Duration::from_secs(2), 4096)
                .unwrap()
                .status
                .success()
        );
        assert!(matches!(slot.read::<Value>(), Err(Error::UnsafePath(_))));
        assert!(matches!(
            slot.write(&json!("no")),
            Err(Error::UnsafePath(_))
        ));
    }

    #[test]
    fn rejects_shared_lock_files_and_hardlinked_records() {
        let root = Scratch::new();
        let path = root.0.join("slot");
        fs::DirBuilder::new().mode(0o700).create(&path).unwrap();
        let lock = path.join("record.lock");
        let other = root.0.join("other-lock");
        fs::write(&other, "untouched").unwrap();
        fs::set_permissions(&other, fs::Permissions::from_mode(0o600)).unwrap();
        symlink(&other, &lock).unwrap();
        assert!(StateFile::lock(&path).is_err());
        assert_eq!(fs::read_to_string(&other).unwrap(), "untouched");
        fs::remove_file(&lock).unwrap();
        fs::write(&lock, "").unwrap();
        fs::set_permissions(&lock, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(matches!(StateFile::lock(&path), Err(Error::UnsafePath(_))));
        fs::set_permissions(&lock, fs::Permissions::from_mode(0o600)).unwrap();
        let mut slot = StateFile::lock(&path).unwrap();
        slot.write(&json!("private")).unwrap();
        fs::hard_link(path.join(RECORD), root.0.join("alias")).unwrap();
        assert!(matches!(slot.read::<Value>(), Err(Error::UnsafePath(_))));
        assert!(matches!(
            slot.write(&json!("new")),
            Err(Error::UnsafePath(_))
        ));
    }
}
