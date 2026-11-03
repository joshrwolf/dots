//! Project history as other processes commit it, for long-lived followers.
//!
//! A follower can be limited to one task's subtree. Membership is judged
//! against the tree at each read, so tasks created in or moved into the
//! subtree count from then on, with no parentage kept between reads.
//!
//! Change detection polls the open connection's `PRAGMA data_version` every
//! [`FOLLOW_INTERVAL`]. SQLite changes that value whenever another connection
//! commits, in this process or another, and leaves it alone otherwise. Under
//! WAL, reading it takes a brief read transaction on the shared-memory
//! wal-index, so an idle tick costs one `stat` and one header read; history
//! queries run only after a commit. Checkpoints move pages without changing content, so they never hide a
//! commit. A follower holds no transaction between ticks, so it never blocks
//! checkpoints or writers.
//!
//! Filesystem notifications were rejected: commits surface as writes to the
//! `-wal` file, but resets, checkpoints and catalog replacement each need
//! their own handling (directory watches, re-registration), the APIs differ
//! between kqueue and inotify, and a timed backstop would still be needed for
//! missed events. A sub-second poll alone meets the latency target.
//!
//! The connection stays bound to the file it opened, so each tick also
//! compares the catalog path's device and inode with that file's and reopens
//! a replacement, reading it from the cursor; while the path is absent the
//! follower waits on the file it has. A follower is an open connection, so
//! SQLite's rule for live databases applies: replace the catalog together
//! with its `-wal` and `-shm` files, never by renaming over it alone. See
//! "Unlinking or renaming a database file while in use" at
//! <https://www.sqlite.org/howtocorrupt.html>.
use std::os::unix::fs::MetadataExt as _;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::store::{Error, Event, Project, Store};

/// How often a follower checks for commits: new events surface within this.
pub const FOLLOW_INTERVAL: Duration = Duration::from_millis(250);

/// Events read per query, bounding memory while a long backlog drains.
const BATCH: usize = 500;

/// Reads one project's history after a cursor, oldest first, as it commits.
#[derive(Debug)]
pub struct Follower {
    path: PathBuf,
    store: Store,
    /// Device and inode of the file `store` opened.
    identity: (u64, u64),
    project: Project,
    /// Only events on this task and its descendants.
    under: Option<String>,
    /// `seq` of the last event returned, or the starting cursor.
    cursor: i64,
    /// Every project's history up to this `seq` has been read.
    scanned: i64,
    /// `data_version` at which `scanned` last reached the newest event.
    caught_up: Option<i64>,
}

impl Follower {
    /// Follow `project` in the catalog at `path` from the event after `since`,
    /// only on `under` and its descendants when given.
    ///
    /// An `under` outside the project fails the first [`Self::poll`], and
    /// any later one after a replacement catalog lacks it.
    pub fn open(
        path: &Path,
        project: Project,
        under: Option<String>,
        since: i64,
    ) -> Result<Self, Error> {
        // Identify the file before opening it: a replacement in between is
        // then seen as a change on the first tick rather than never.
        let identity = identity(path).map_err(|source| Error::CatalogPath {
            path: path.to_owned(),
            source,
        })?;
        Ok(Self {
            path: path.to_owned(),
            store: Store::open(path)?,
            identity,
            project,
            under,
            cursor: since,
            scanned: since,
            caught_up: None,
        })
    }

    /// The next committed events after the last one returned, oldest first.
    ///
    /// Never blocks: empty means nothing new has committed. Call again at
    /// once after a nonempty result, since a backlog arrives in batches, and
    /// otherwise after [`FOLLOW_INTERVAL`].
    pub fn poll(&mut self) -> Result<Vec<Event>, Error> {
        let current = match identity(&self.path) {
            Ok(current) => Some(current),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(source) => {
                return Err(Error::CatalogPath {
                    path: self.path.clone(),
                    source,
                });
            }
        };
        if let Some(current) = current
            && current != self.identity
        {
            self.store = Store::open(&self.path)?;
            self.identity = current;
            self.scanned = self.cursor;
            self.caught_up = None;
        }
        // Read the version first: a commit after it changes the version the
        // next tick reads, even if this page already includes it.
        let version: i64 =
            self.store
                .connection
                .pragma_query_value(None, "data_version", |row| row.get(0))?;
        if self.caught_up == Some(version) {
            return Ok(Vec::new());
        }
        let page =
            self.store
                .event_page(&self.project, self.under.as_deref(), self.scanned, BATCH)?;
        self.scanned = page.through;
        if let Some(last) = page.events.last() {
            self.cursor = last.seq;
        }
        self.caught_up = (page.events.len() < BATCH).then_some(version);
        Ok(page.events)
    }
}

/// Device and inode of the file at `path`.
fn identity(path: &Path) -> std::io::Result<(u64, u64)> {
    let metadata = std::fs::metadata(path)?;
    Ok((metadata.dev(), metadata.ino()))
}

#[cfg(test)]
mod tests;
