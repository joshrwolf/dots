//! Review files: one JSON document per review, replaced atomically under a
//! per-directory lock so parallel reviewers can write to one review safely.
use std::collections::HashSet;
use std::fs::{self, File, OpenOptions};
use std::io::{ErrorKind, Write as _};
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::model::{Review, SCHEMA};
use crate::{Error, Result, git};

#[derive(Debug)]
pub(crate) struct Store {
    directory: PathBuf,
}

impl Store {
    pub(crate) fn new(directory: PathBuf) -> Self {
        Self { directory }
    }

    pub(crate) fn directory(&self) -> &Path {
        &self.directory
    }

    pub(crate) fn prepare(&self) -> Result<()> {
        fs::create_dir_all(&self.directory).map_err(|source| Error::Io {
            operation: "create review directory",
            path: self.directory.clone(),
            source,
        })
    }

    /// Write `review` under a fresh id, replacing whatever id it carries, as
    /// this directory's most recent change.
    /// `pin` publishes the review's content under its id, under the same lock
    /// that `remove` takes, and records the pinned revision in the review; it
    /// returns false when the id is taken elsewhere.
    pub(crate) fn create(
        &self,
        mut review: Review,
        mut pin: impl FnMut(&mut Review) -> Result<bool>,
    ) -> Result<Review> {
        let _lock = self.lock()?;
        review.sequence = self.next_sequence()?;
        for _ in 0..8 {
            let id = format!("r-{:08x}", git::random()? >> 32);
            let path = self.path(&id);
            match OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(_) => {
                    review.id = id;
                    let pinned = pin(&mut review);
                    if !matches!(pinned, Ok(true)) {
                        let _ = fs::remove_file(&path);
                        pinned?;
                        continue;
                    }
                    if let Err(error) = self.save(&review) {
                        let _ = fs::remove_file(&path);
                        return Err(error);
                    }
                    return Ok(review);
                }
                Err(error) if error.kind() == ErrorKind::AlreadyExists => {}
                Err(source) => {
                    return Err(Error::Io {
                        operation: "create review",
                        path,
                        source,
                    });
                }
            }
        }
        Err(Error::Invalid("could not allocate a review id".into()))
    }

    pub(crate) fn load(&self, id: &str) -> Result<Review> {
        check_id(id)?;
        let path = self.path(id);
        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == ErrorKind::NotFound => {
                return Err(Error::NotFound(id.to_owned()));
            }
            Err(source) => {
                return Err(Error::Io {
                    operation: "read review",
                    path,
                    source,
                });
            }
        };
        let corrupt = |reason: String| Error::Corrupt {
            path: path.clone(),
            reason,
        };
        // The schema is read on its own first, so a file of another schema is
        // refused by name rather than by whichever field changed.
        let version: Version =
            serde_json::from_slice(&bytes).map_err(|error| corrupt(error.to_string()))?;
        if version.schema != SCHEMA {
            return Err(corrupt(format!(
                "schema {} is not the supported schema {SCHEMA}; remove it with dots review remove {id}",
                version.schema
            )));
        }
        let review: Review =
            serde_json::from_slice(&bytes).map_err(|error| corrupt(error.to_string()))?;
        if review.id != id {
            return Err(corrupt(format!("names review {}", review.id)));
        }
        if review.revisions.is_empty() {
            return Err(corrupt("has no revisions".into()));
        }
        if review
            .threads
            .iter()
            .any(|thread| thread.revision >= review.revisions.len())
        {
            return Err(corrupt("a thread names a revision the review lacks".into()));
        }
        if review
            .threads
            .iter()
            .any(|thread| thread.comments.is_empty())
        {
            return Err(corrupt("a thread has no comments".into()));
        }
        let revisions = review.revisions.len();
        let has_thread = |id: &str| review.threads.iter().any(|thread| thread.id == id);
        if review.drafts.iter().any(|draft| {
            draft.revision >= revisions
                || draft
                    .thread
                    .as_deref()
                    .is_some_and(|thread| !has_thread(thread))
        }) {
            return Err(corrupt(
                "a draft names a revision or thread the review lacks".into(),
            ));
        }
        if review
            .looked
            .as_ref()
            .is_some_and(|looked| looked.revision >= revisions)
        {
            return Err(corrupt("last looked at a revision the review lacks".into()));
        }
        if review.submissions.iter().any(|submission| {
            submission.comments.iter().any(|published| {
                review
                    .threads
                    .iter()
                    .find(|thread| thread.id == published.thread)
                    .is_none_or(|thread| published.comment >= thread.comments.len())
            })
        }) {
            return Err(corrupt(
                "a submission names a comment the review lacks".into(),
            ));
        }
        Ok(review)
    }

    pub(crate) fn all(&self) -> Result<Vec<Review>> {
        let entries = match fs::read_dir(&self.directory) {
            Ok(entries) => entries,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
            Err(source) => {
                return Err(Error::Io {
                    operation: "list reviews",
                    path: self.directory.clone(),
                    source,
                });
            }
        };
        let mut reviews = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|source| Error::Io {
                operation: "list reviews",
                path: self.directory.clone(),
                source,
            })?;
            let name = entry.file_name();
            let Some(id) = name.to_str().and_then(|name| name.strip_suffix(".json")) else {
                continue;
            };
            if check_id(id).is_ok() {
                match self.load(id) {
                    // Created but not yet written by a concurrent start.
                    Err(Error::Corrupt { .. }) if is_empty(&entry.path()) => {}
                    result => reviews.push(result?),
                }
            }
        }
        Ok(reviews)
    }

    /// Load, change and replace a review while holding the directory lock.
    /// `change` receives the sequence its change takes; see [`Review::touch`].
    pub(crate) fn update<T>(
        &self,
        id: &str,
        change: impl FnOnce(&mut Review, u64) -> Result<T>,
    ) -> Result<T> {
        check_id(id)?;
        let _lock = self.lock()?;
        let sequence = self.next_sequence()?;
        let mut review = self.load(id)?;
        let result = change(&mut review, sequence)?;
        self.save(&review)?;
        Ok(result)
    }

    /// Delete a review under the directory lock, after `release` succeeds.
    pub(crate) fn remove(&self, id: &str, release: impl FnOnce() -> Result<()>) -> Result<()> {
        check_id(id)?;
        let _lock = self.lock()?;
        let path = self.path(id);
        if !path.exists() {
            return Err(Error::NotFound(id.to_owned()));
        }
        release()?;
        fs::remove_file(&path).map_err(|source| Error::Io {
            operation: "remove review",
            path,
            source,
        })
    }

    /// One past the highest sequence of this directory's reviews. Callers hold
    /// the lock, so no other change can take the same sequence.
    fn next_sequence(&self) -> Result<u64> {
        let last = self.all()?.iter().map(|review| review.sequence).max();
        Ok(last.map_or(1, |last| last + 1))
    }

    /// One stable lock file for every mutation in this directory; it is
    /// never removed, so every writer locks the same inode.
    fn lock(&self) -> Result<File> {
        self.prepare()?;
        let path = self.directory.join(".lock");
        let file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)
            .map_err(|source| Error::Io {
                operation: "open review lock",
                path: path.clone(),
                source,
            })?;
        file.lock().map_err(|source| Error::Io {
            operation: "lock reviews",
            path,
            source,
        })?;
        Ok(file)
    }

    fn save(&self, review: &Review) -> Result<()> {
        let path = self.path(&review.id);
        let temporary = self
            .directory
            .join(format!(".{}.{:016x}.tmp", review.id, git::random()?));
        let io = |operation, source| Error::Io {
            operation,
            path: temporary.clone(),
            source,
        };
        let mut bytes = serde_json::to_vec_pretty(review)
            .map_err(|error| Error::Invalid(format!("serialize review: {error}")))?;
        bytes.push(b'\n');
        let result = (|| {
            let mut file = File::create(&temporary).map_err(|source| io("write review", source))?;
            file.write_all(&bytes)
                .and_then(|()| file.sync_all())
                .map_err(|source| io("write review", source))?;
            fs::rename(&temporary, &path).map_err(|source| io("replace review", source))
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        result
    }

    /// The file that holds review `id`, whether or not it exists.
    pub(crate) fn path(&self, id: &str) -> PathBuf {
        self.directory.join(format!("{id}.json"))
    }
}

#[derive(Deserialize)]
struct Version {
    schema: u32,
}

/// The review directory of every worktree of the repository whose common Git
/// directory is `common`, the primary worktree's first.
pub(crate) fn directories(common: &Path) -> Result<Vec<PathBuf>> {
    let mut directories = vec![common.join("dots/reviews")];
    match fs::read_dir(common.join("worktrees")) {
        Ok(entries) => {
            for entry in entries {
                let entry = entry.map_err(|source| Error::Io {
                    operation: "list worktrees",
                    path: common.join("worktrees"),
                    source,
                })?;
                directories.push(entry.path().join("dots/reviews"));
            }
        }
        Err(error) if error.kind() == ErrorKind::NotFound => {}
        Err(source) => {
            return Err(Error::Io {
                operation: "list worktrees",
                path: common.join("worktrees"),
                source,
            });
        }
    }
    Ok(directories)
}

/// Ids of every review file in any worktree of the repository whose common
/// Git directory is `common`.
pub(crate) fn live_ids(common: &Path) -> Result<HashSet<String>> {
    let mut ids = HashSet::new();
    for directory in directories(common)? {
        let entries = match fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(error) if error.kind() == ErrorKind::NotFound => continue,
            Err(source) => {
                return Err(Error::Io {
                    operation: "list reviews",
                    path: directory,
                    source,
                });
            }
        };
        for entry in entries {
            let entry = entry.map_err(|source| Error::Io {
                operation: "list reviews",
                path: directory.clone(),
                source,
            })?;
            if let Some(id) = entry
                .file_name()
                .to_str()
                .and_then(|name| name.strip_suffix(".json"))
                .filter(|id| check_id(id).is_ok())
            {
                ids.insert(id.to_owned());
            }
        }
    }
    Ok(ids)
}

fn is_empty(path: &Path) -> bool {
    fs::metadata(path).is_ok_and(|metadata| metadata.len() == 0)
}

/// `r-` and eight lowercase hex digits.
pub(crate) fn check_id(id: &str) -> Result<()> {
    let valid = id.strip_prefix("r-").is_some_and(|hex| {
        hex.len() == 8
            && hex
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    });
    if valid {
        Ok(())
    } else {
        Err(Error::Invalid(format!(
            "review id {id:?} must look like r-1a2b3c4d; dots review list --all shows this repository's reviews"
        )))
    }
}
