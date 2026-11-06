//! Shared private staging for document and catalog publication.
use crate::{Result, directories, invalid, io};
use std::fs::{self, File, OpenOptions};
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub struct StagedFile {
    path: PathBuf,
    file: File,
}

impl StagedFile {
    /// The private staging path, usable by a native writer such as SQLite.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The staging file handle; callers must finish external writers before syncing.
    pub fn file(&self) -> &File {
        &self.file
    }

    /// Sync complete bytes and atomically link them at an unused destination.
    /// Existing destinations are never replaced. The caller holds the mutation lock.
    pub fn publish_new(self, destination: &Path) -> Result<()> {
        self.file
            .sync_all()
            .map_err(|error| io("sync staged document", &self.path, error))?;
        fs::hard_link(&self.path, destination)
            .map_err(|error| io("publish new document", destination, error))?;
        fs::remove_file(&self.path)
            .map_err(|error| io("remove staging link", &self.path, error))?;
        directories::sync(
            destination
                .parent()
                .ok_or_else(|| invalid("destination has no parent"))?,
        )?;
        Ok(())
    }

    /// Sync and atomically replace one owned file. The caller holds its mutation lock.
    pub fn publish_replace(self, destination: &Path) -> Result<()> {
        self.file
            .sync_all()
            .map_err(|error| io("sync staged document", &self.path, error))?;
        fs::rename(&self.path, destination)
            .map_err(|error| io("replace document", destination, error))?;
        directories::sync(
            destination
                .parent()
                .ok_or_else(|| invalid("destination has no parent"))?,
        )?;
        Ok(())
    }

    /// Allocate a private file in an existing directory using exclusive creation.
    pub fn create(parent: &Path) -> Result<Self> {
        for _ in 0..128 {
            let mut nonce = [0; 16];
            getrandom::fill(&mut nonce).map_err(|error| {
                io(
                    "allocate staging name",
                    parent,
                    std::io::Error::other(error),
                )
            })?;
            let path = parent.join(format!(".staged-{:032x}", u128::from_ne_bytes(nonce)));
            let mut options = OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt as _;
                options.mode(0o600);
            }
            match options.open(&path) {
                Ok(file) => return Ok(Self { path, file }),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(io("stage document file", &path, error)),
            }
        }
        Err(invalid("cannot allocate a document staging file"))
    }
}

impl Drop for StagedFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}
