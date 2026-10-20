//! Scoped Markdown storage with optimistic writes and bounded literal retrieval.
//! Files remain authoritative. Locks coordinate cooperating writers, not hostile processes.
use std::fs::{self, File, OpenOptions};
use std::io::{Read as _, Write as _};
use std::path::{Component, Path, PathBuf};

use serde::Serialize;
use sha2::{Digest as _, Sha256};
use unicode_segmentation::UnicodeSegmentation as _;

#[cfg(test)]
mod tests;

pub const MAX_FILE_BYTES: usize = 256 * 1024;
const MAX_FILES: usize = 1000;
const MAX_OUTPUT_BYTES: usize = 16 * 1024;

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
}

#[derive(Debug, Serialize)]
pub struct Document {
    pub path: PathBuf,
    pub revision: String,
    pub exists: bool,
    pub content: String,
    pub start: usize,
    pub total_lines: usize,
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
    pub next_offset: Option<usize>,
}

impl Store {
    pub fn new(root: PathBuf) -> Result<Self> {
        if !root.is_absolute() {
            return Err(invalid("memory scope must be absolute"));
        }
        Ok(Self { root })
    }

    fn path(&self, relative: &Path) -> Result<PathBuf> {
        if relative.as_os_str().is_empty() || relative.extension().is_none_or(|ext| ext != "md") {
            return Err(invalid("memory paths must name Markdown (.md) files"));
        }
        for component in relative.components() {
            let Component::Normal(name) = component else {
                return Err(invalid(
                    "memory path must be scope-relative without traversal",
                ));
            };
            if name.to_str().is_none_or(|name| name.starts_with('.')) {
                return Err(invalid(
                    "hidden and non-Unicode memory paths are not supported",
                ));
            }
        }
        let mut path = self.root.clone();
        reject_symlink(&path)?;
        for component in relative.components() {
            path.push(component);
            reject_symlink(&path)?;
        }
        Ok(path)
    }

    fn contents(&self, relative: &Path) -> Result<Option<String>> {
        let path = self.path(relative)?;
        match fs::symlink_metadata(&path) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(io("inspect memory file", &path, error)),
            Ok(metadata) if !metadata.is_file() => {
                return Err(invalid("memory path is not a regular file"));
            }
            Ok(_) => {}
        }
        let file = File::open(&path).map_err(|error| io("open memory file", &path, error))?;
        let mut content = String::new();
        file.take((MAX_FILE_BYTES + 1) as u64)
            .read_to_string(&mut content)
            .map_err(|error| io("read UTF-8 memory file", &path, error))?;
        if content.len() > MAX_FILE_BYTES {
            return Err(invalid("memory file exceeds 256 KiB; split the topic"));
        }
        Ok(Some(content))
    }

    pub fn read(&self, relative: &Path, start: usize, lines: usize) -> Result<Document> {
        if start == 0 || !(1..=200).contains(&lines) {
            return Err(invalid("start must be positive; lines must be 1..200"));
        }
        let content = self.contents(relative)?;
        let revision = revision(content.as_deref());
        let exists = content.is_some();
        let content = content.unwrap_or_default();
        let total_lines = content.lines().count();
        let mut selected = String::new();
        let mut count = 0;
        for line in content.lines().skip(start - 1).take(lines) {
            if selected.len() + line.len() + 1 > MAX_OUTPUT_BYTES {
                if count == 0 {
                    return Err(invalid(
                        "memory line exceeds read budget; use an ordinary file read and shorten the line",
                    ));
                }
                break;
            }
            selected.push_str(line);
            selected.push('\n');
            count += 1;
        }
        Ok(Document {
            path: self.path(relative)?,
            revision,
            exists,
            content: selected,
            start,
            total_lines,
            next_line: (start.saturating_add(count) <= total_lines)
                .then_some(start.saturating_add(count)),
        })
    }

    /// Replace one file atomically, or remove it. No multi-file transaction is implied.
    /// `missing` is the expected revision for a new file. A short lock covers the check and mutation.
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
        fs::create_dir_all(&self.root)
            .map_err(|error| io("create memory scope", &self.root, error))?;
        let lock = Lock::acquire(self.root.join(".write-lock"))?;
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
            fs::create_dir_all(parent)
                .map_err(|error| io("create memory topic directory", parent, error))?;
            // Recheck after parent creation; no symlinks within the scope are accepted.
            self.path(relative)?;
            let staged = lock.path.join("content");
            let mut options = OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt as _;
                options.mode(0o600);
            }
            let mut file = options
                .open(&staged)
                .map_err(|error| io("stage memory file", &staged, error))?;
            file.write_all(content.as_bytes())
                .map_err(|error| io("write staged memory file", &staged, error))?;
            file.sync_all()
                .map_err(|error| io("sync staged memory file", &staged, error))?;
            fs::rename(&staged, &path).map_err(|error| io("publish memory file", &path, error))?;
        } else if actual != "missing" {
            fs::remove_file(&path).map_err(|error| io("remove memory file", &path, error))?;
        }
        Ok(Published {
            path,
            revision: revision(content),
        })
    }

    /// Case-insensitive literal matching, deterministic path/line order, no ranking or index.
    /// Sources and the change log are excluded unless explicitly requested.
    pub fn search(
        &self,
        query: &str,
        sources: bool,
        offset: usize,
        limit: usize,
    ) -> Result<Search> {
        if query.trim().is_empty() || query.len() > 1024 || !(1..=20).contains(&limit) {
            return Err(invalid(
                "query must contain 1..1024 bytes; limit must be 1..20",
            ));
        }
        reject_symlink(&self.root)?;
        let mut files = Vec::new();
        collect(&self.root, &self.root, sources, &mut files, 0)?;
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
                let excerpt = bounded(text, 600);
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

fn bounded(text: &str, bytes: usize) -> String {
    let mut output = String::new();
    for grapheme in text.graphemes(true) {
        if output.len() + grapheme.len() > bytes {
            break;
        }
        output.push_str(grapheme);
    }
    output
}
fn revision(content: Option<&str>) -> String {
    content.map_or_else(
        || "missing".into(),
        |content| format!("sha256:{:x}", Sha256::digest(content.as_bytes())),
    )
}
fn reject_symlink(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(invalid(format!(
            "symlink memory path refused: {}",
            path.display()
        ))),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(io("inspect memory path", path, error)),
    }
}
fn collect(
    root: &Path,
    directory: &Path,
    sources: bool,
    files: &mut Vec<PathBuf>,
    depth: usize,
) -> Result<()> {
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
        if name.starts_with('.') || (!sources && (name == "sources" || name == "log.md")) {
            continue;
        }
        let path = entry.path();
        reject_symlink(&path)?;
        let kind = entry
            .file_type()
            .map_err(|error| io("inspect memory entry", &path, error))?;
        if kind.is_dir() {
            collect(root, &path, sources, files, depth + 1)?;
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

#[derive(Debug)]
struct Lock {
    path: PathBuf,
}
impl Lock {
    fn acquire(path: PathBuf) -> Result<Self> {
        fs::create_dir(&path).map_err(|error| {
            io(
                "acquire memory lock (if held, coordinate with its owner; never remove it blindly)",
                &path,
                error,
            )
        })?;
        let lock = Self { path };
        let owner = lock.path.join("owner");
        fs::write(&owner, format!("pid={}\n", std::process::id()))
            .map_err(|error| io("write lock owner", &owner, error))?;
        Ok(lock)
    }
}
impl Drop for Lock {
    fn drop(&mut self) {
        let _ = fs::remove_file(self.path.join("content"));
        let _ = fs::remove_file(self.path.join("owner"));
        let _ = fs::remove_dir(&self.path);
    }
}

/// Match the existing Pi checkout-key convention; checkout identity is machine-local.
pub fn project_key(checkout: &str) -> String {
    let mut key = String::new();
    let mut separator = false;
    for c in checkout.trim_start_matches('/').chars() {
        if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
            key.push(c);
            separator = false;
        } else if !separator {
            key.push('-');
            separator = true;
        }
    }
    key
}
