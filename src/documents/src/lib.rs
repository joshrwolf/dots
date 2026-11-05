//! Scoped Markdown storage with optimistic writes and bounded literal retrieval.
//! Files remain authoritative. Locks coordinate cooperating writers, not hostile processes.
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
pub mod staged_file;
use mutation_lock::MutationLock;
use staged_file::StagedFile;

#[cfg(test)]
mod tests;

pub const MAX_FILE_BYTES: usize = 256 * 1024;
const MAX_FILES: usize = 1000;

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
        "memory write conflict for {path}; expected {expected}, found {actual}; reread before retrying"
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

/// A caller-selected scope. It never expands a search to sibling projects.
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
#[derive(Debug, Serialize)]
pub struct Hit {
    pub path: PathBuf,
    pub line: usize,
    pub excerpt: String,
    pub truncated: bool,
}
#[derive(Debug, Serialize)]
pub struct Search {
    pub backend: &'static str,
    pub hits: Vec<Hit>,
    pub has_more: bool,
    /// Offset for another page when the result limit truncated the matches.
    pub next_offset: Option<usize>,
}

impl Store {
    /// Validate the absolute root of this caller-selected memory scope.
    pub fn try_new(root: PathBuf, lock_path: PathBuf) -> Result<Self> {
        if !root.is_absolute() || !lock_path.is_absolute() {
            return Err(invalid(
                "memory scope and mutation lock paths must be absolute",
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
            return Err(invalid("memory file exceeds 256 KiB"));
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
                .ok_or_else(|| invalid("memory file has no parent"))?;
            directories::create_within(parent, storage_root)?;
            // Recheck after parent creation; no symlinks within the scope are accepted.
            self.path(relative)?;
            let staged = StagedFile::create(parent)?;
            staged
                .file()
                .write_all(content.as_bytes())
                .map_err(|error| io("write staged memory file", staged.path(), error))?;
            staged
                .file()
                .sync_all()
                .map_err(|error| io("sync staged memory file", staged.path(), error))?;
            fs::rename(staged.path(), &path)
                .map_err(|error| io("publish memory file", &path, error))?;
        } else if actual != "missing" {
            fs::remove_file(&path).map_err(|error| io("remove memory file", &path, error))?;
        }
        if let Some(parent) = path.parent().filter(|parent| parent.exists()) {
            directories::sync(parent)?;
        }
        Ok(Published {
            path,
            revision: revision(content),
        })
    }

    /// Case-insensitive literal matching, deterministic path/line order, no ranking or index.
    /// The change log is excluded; retained outputs belong outside this knowledge root.
    /// Pages contain up to 20 hits and scan at most 1000 Markdown files.
    pub fn search(&self, query: &str, offset: usize, limit: usize) -> Result<Search> {
        if query.trim().is_empty() || query.len() > 1024 || !(1..=20).contains(&limit) {
            return Err(invalid(
                "query must contain 1..1024 bytes; limit must be 1..20",
            ));
        }
        self.checked_root()?;
        let mut files = Vec::new();
        collect(&self.root, &self.root, &mut files, 0)?;
        files.sort();
        let query = query.to_lowercase();
        let mut hits = Vec::new();
        let mut skipped = 0;
        for relative in files {
            let Some(content) = self.contents(&relative)? else {
                continue;
            };
            for (line, text) in content.lines().enumerate() {
                if !text.to_lowercase().contains(&query) {
                    continue;
                }
                if skipped < offset {
                    skipped += 1;
                    continue;
                }
                if hits.len() == limit {
                    return Ok(Search {
                        backend: "literal",
                        hits,
                        has_more: true,
                        next_offset: Some(offset + limit),
                    });
                }
                let excerpt = text::excerpt(text, 600);
                let truncated = excerpt.len() < text.len();
                hits.push(Hit {
                    path: self.path(&relative)?,
                    line: line + 1,
                    excerpt,
                    truncated,
                });
            }
        }
        Ok(Search {
            backend: "literal",
            hits,
            has_more: false,
            next_offset: None,
        })
    }
}

fn collect(root: &Path, directory: &Path, files: &mut Vec<PathBuf>, depth: usize) -> Result<()> {
    if depth > 8 {
        return Err(invalid("memory directory depth exceeds 8"));
    }
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound && directory == root => {
            return Ok(());
        }
        Err(error) => return Err(io("list memory directory", directory, error)),
    };
    for entry in entries {
        let entry = entry.map_err(|error| io("read memory directory entry", directory, error))?;
        let name = entry.file_name();
        let name = name
            .to_str()
            .ok_or_else(|| invalid("non-Unicode memory filename"))?;
        if name.starts_with('.') || name == "log.md" {
            continue;
        }
        let path = entry.path();
        paths::reject_symlink(&path)?;
        let kind = entry
            .file_type()
            .map_err(|error| io("inspect memory entry", &path, error))?;
        if kind.is_dir() {
            collect(root, &path, files, depth + 1)?;
        } else if kind.is_file() && path.extension().is_some_and(|ext| ext == "md") {
            if files.len() == MAX_FILES {
                return Err(invalid(
                    "memory search exceeds 1000 files; narrow the corpus or use a configured search backend",
                ));
            }
            files.push(
                path.strip_prefix(root)
                    .map_err(|_| invalid("memory path escaped scope"))?
                    .to_owned(),
            );
        }
    }
    Ok(())
}
