//! Filesystem documents, private staging, optimistic writes and bounded text inspection.
//! Locks coordinate cooperating writers, not hostile processes.
use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use serde::Serialize;

pub mod directories;
pub mod immutable;
pub mod inventory;
pub mod text;
pub mod verified_text;
use text::revision;
pub mod mutation_lock;
pub mod paths;
pub mod staged_directory;
pub mod staged_file;
use mutation_lock::MutationLock;
use staged_file::StagedFile;

#[cfg(test)]
mod tests;

pub const MAX_FILE_BYTES: usize = 256 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{operation}: {path}")]
    Io {
        operation: &'static str,
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("{0}")]
    Invalid(String),
    #[error("document mutation is busy; retry after the writer finishes")]
    Busy,
    #[error("text file {path} exceeds {maximum} bytes")]
    TooLarge { path: PathBuf, maximum: usize },
    #[error(
        "document write conflict for {path}; expected {expected}, found {actual}; reread before retrying"
    )]
    Conflict {
        path: PathBuf,
        expected: String,
        actual: String,
    },
}
type Result<T> = std::result::Result<T, Error>;

fn io(operation: &'static str, path: &Path, source: std::io::Error) -> Error {
    Error::Io {
        operation,
        path: path.to_owned(),
        source,
    }
}
fn invalid(message: impl Into<String>) -> Error {
    Error::Invalid(message.into())
}

/// A caller-selected filesystem document root.
#[derive(Debug)]
pub struct Store {
    root: PathBuf,
    lock_path: PathBuf,
}

#[derive(Debug, Serialize)]
pub struct Document {
    pub path: PathBuf,
    /// Content hash, or `missing` when the file does not exist; supply it to `publish`.
    pub revision: String,
    /// Distinguishes an absent document from a present empty document.
    pub exists: bool,
    pub content: String,
    pub start: usize,
    pub total_lines: usize,
    /// The next one-based line to request, if this page did not reach the end.
    pub next_line: Option<usize>,
}
#[derive(Debug, Serialize)]
pub struct Published {
    pub path: PathBuf,
    pub revision: String,
}
impl Store {
    /// Validate the absolute root of this caller-selected document store.
    pub fn try_new(root: PathBuf, lock_path: PathBuf) -> Result<Self> {
        if !root.is_absolute() || !lock_path.is_absolute() {
            return Err(invalid(
                "document root and mutation lock paths must be absolute",
            ));
        }
        Ok(Self { root, lock_path })
    }

    fn checked_root(&self) -> Result<PathBuf> {
        let storage_root = self
            .lock_path
            .parent()
            .ok_or_else(|| invalid("lock path has no parent"))?;
        let relative = self.root.strip_prefix(storage_root).map_err(|_| {
            invalid("document scope must be within the mutation lock's storage root")
        })?;
        if relative.as_os_str().is_empty() {
            paths::reject_symlink(storage_root)?;
            Ok(storage_root.to_owned())
        } else {
            paths::resolve(storage_root, relative)
        }
    }

    fn path(&self, relative: &Path) -> Result<PathBuf> {
        paths::validate_markdown(relative)?;
        paths::resolve(&self.checked_root()?, relative)
    }

    fn contents(&self, relative: &Path) -> Result<Option<String>> {
        text::read(&self.path(relative)?, MAX_FILE_BYTES)
    }

    /// Read a one-based bounded line window with a full-file content revision.
    pub fn read(&self, relative: &Path, start: usize, lines: usize) -> Result<Document> {
        let content = self.contents(relative)?;
        text::page(&self.path(relative)?, content.as_deref(), start, lines)
    }

    /// Replace one file atomically, or remove it. No multi-file transaction is implied.
    /// `missing` is the expected revision for a new file. A short lock covers the check and mutation.
    /// Locks coordinate cooperating writers; path checks do not defend against hostile path swaps.
    pub fn publish(
        &self,
        relative: &Path,
        expected: &str,
        content: Option<&str>,
    ) -> Result<Published> {
        let path = self.path(relative)?;
        if content.is_some_and(|text| text.len() > MAX_FILE_BYTES) {
            return Err(invalid("document exceeds 256 KiB"));
        }
        let _lock = MutationLock::acquire(&self.lock_path)?;
        let storage_root = self
            .lock_path
            .parent()
            .ok_or_else(|| invalid("lock path has no parent"))?;
        directories::create_within(&self.root, storage_root)?;
        let actual = revision(self.contents(relative)?.as_deref());
        if expected != actual {
            return Err(Error::Conflict {
                path,
                expected: expected.to_owned(),
                actual,
            });
        }
        if let Some(content) = content {
            let parent = path
                .parent()
                .ok_or_else(|| invalid("document has no parent"))?;
            directories::create_within(parent, storage_root)?;
            // Recheck after parent creation; no symlinks within the scope are accepted.
            self.path(relative)?;
            let staged = StagedFile::create(parent)?;
            staged
                .file()
                .write_all(content.as_bytes())
                .map_err(|error| io("write staged document", staged.path(), error))?;
            staged.publish_replace(&path)?;
        } else if actual != "missing" {
            fs::remove_file(&path).map_err(|error| io("remove document", &path, error))?;
        }
        if content.is_none()
            && let Some(parent) = path.parent().filter(|parent| parent.exists())
        {
            directories::sync(parent)?;
        }
        Ok(Published {
            path,
            revision: revision(content),
        })
    }
}
