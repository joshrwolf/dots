//! Keeping the stream current: loading worktrees, rescanning paths after
//! file events, diffing, searching for moved code, the safety-net check
//! and the switcher counts.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Condvar, Mutex};
use std::time::Instant;

use crate::app;
use crate::moves;
use crate::repo::{self, FileChange, GitState, Reader, Side, Stamp};
use crate::review;
use crate::stats::{self, Stats};
use crate::watch::{self, Burst};

use super::{Msg, RECONCILE, Runtime, WATCH_ALL_FILES, load};

pub(super) struct DiffJob {
    pub(super) cancel: Arc<AtomicBool>,
    pub(super) focus: Arc<AtomicUsize>,
}

/// Keeping the stream current: loads, path-limited rescans after file
/// events, diffs and the safety-net check.
pub(super) struct Pipeline {
    /// Numbers loads, so results of an older one are dropped.
    pub(super) generation: u64,
    /// Numbers file lists, so diffs of an older list are dropped.
    pub(super) epoch: u64,
    pub(super) diffs: Option<DiffJob>,
    /// File events waiting to be coalesced into one refresh.
    pub(super) burst: Burst,
    /// A path-limited rescan is running; later bursts wait for it.
    pub(super) partial_busy: bool,
    pub(super) next_reconcile: Instant,
    /// What the last full scan saw of HEAD, the index and the base.
    pub(super) state: Option<GitState>,
    /// Where the current stream's contents are read from.
    pub(super) source: Option<(gix::ThreadSafeRepository, PathBuf)>,
    /// The current worktree is loading, until its first full frame.
    pub(super) loading: bool,
}

impl Pipeline {
    pub(super) fn new() -> Self {
        Self {
            generation: 0,
            epoch: 0,
            diffs: None,
            burst: Burst::default(),
            partial_busy: false,
            next_reconcile: Instant::now() + RECONCILE,
            state: None,
            source: None,
            loading: false,
        }
    }

    /// Stops the diffs running for the current file list.
    pub(super) fn cancel_diffs(&mut self) {
        if let Some(job) = self.diffs.take() {
            job.cancel.store(true, Ordering::Relaxed);
        }
    }
}

/// Background threads computing switcher counts, one worktree at a time each.
pub(super) struct SummaryQueue {
    pub(super) queue: Queue,
    pub(super) stats: SharedStats,
}

impl SummaryQueue {
    pub(super) fn new(threads: usize, tx: &Sender<Msg>) -> Self {
        let queue: Queue = Arc::default();
        for _ in 0..threads {
            let (queue, tx) = (Arc::clone(&queue), tx.clone());
            std::thread::spawn(move || {
                loop {
                    let (path, shared) = {
                        let (lock, cv) = &*queue;
                        let Ok(mut q) = lock.lock() else { return };
                        loop {
                            if let Some(p) = q.pop_front() {
                                break p;
                            }
                            let Ok(next) = cv.wait(q) else { return };
                            q = next;
                        }
                    };
                    let stats = shared.get_or_init(|| Stats::open(&path));
                    let summary = stats::summarize(stats, &path).map_err(|e| format!("{e:#}"));
                    if tx.send(Msg::Summary { path, summary }).is_err() {
                        return;
                    }
                }
            });
        }
        Self {
            queue,
            stats: SharedStats::default(),
        }
    }

    /// Starts over with another repository's caches.
    pub(super) fn reset(&mut self) {
        if let Ok(mut q) = self.queue.0.lock() {
            q.clear();
        }
        self.stats = SharedStats::default();
    }

    /// Queues `path` unless it is already waiting; `urgent` puts it first.
    pub(super) fn push(&self, path: PathBuf, urgent: bool) {
        let (lock, cv) = &*self.queue;
        let Ok(mut q) = lock.lock() else { return };
        if let Some(i) = q.iter().position(|(p, _)| *p == path) {
            if !urgent {
                return;
            }
            q.remove(i);
        }
        let item = (path, Arc::clone(&self.stats));
        if urgent {
            q.push_front(item);
        } else {
            q.push_back(item);
        }
        cv.notify_one();
    }
}

/// The undone index nearest to `focus`, looking forward first.
pub(super) fn nearest<T>(todo: &[Option<T>], focus: usize) -> Option<usize> {
    let n = todo.len();
    let focus = focus.min(n.saturating_sub(1));
    (0..n).find_map(|d| {
        let ahead = focus + d;
        if ahead < n && todo[ahead].is_some() {
            return Some(ahead);
        }
        focus.checked_sub(d).filter(|&i| i < n && todo[i].is_some())
    })
}

impl Runtime {
    pub(super) fn start_load(&mut self) {
        self.pipeline.generation += 1;
        if let Ok(mut status) = self.ctl_status.lock() {
            status.worktree.clone_from(self.app.worktree_path());
        }
        self.pipeline.cancel_diffs();
        self.pipeline.burst = Burst::default();
        self.pipeline.state = None;
        let generation = self.pipeline.generation;
        let worktree = self.app.worktree_path().clone();
        let mode = self.app.settings.mode;
        let since = self.app.settings.since;
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let (result, review) = load(&worktree, mode, since);
            let _ = tx.send(Msg::Loaded {
                generation,
                worktree,
                mode,
                result,
                review,
            });
        });
    }

    /// Rescans `paths` alone, off the event loop.
    pub(super) fn start_partial(&mut self, paths: Vec<String>) {
        let (Some((repo, root)), Some(stream)) = (self.pipeline.source.clone(), &self.app.stream)
        else {
            return;
        };
        let listed: Vec<&str> = stream
            .files
            .iter()
            .map(|f| f.change.path.as_str())
            .collect();
        if watch::needs_full_scan(&root, &paths, &listed) {
            self.start_load();
            return;
        }
        let target = stream.target;
        let renames: std::collections::HashMap<String, String> = stream
            .files
            .iter()
            .filter_map(|f| Some((f.change.path.clone(), f.change.from.clone()?)))
            .filter(|(path, _)| paths.contains(path))
            .collect();
        let generation = self.pipeline.generation;
        let tx = self.tx.clone();
        let seen = self
            .app
            .settings
            .since
            .then(|| self.app.session.state().seen.clone());
        self.pipeline.partial_busy = true;
        std::thread::spawn(move || {
            let repo = repo.to_thread_local();
            let result = repo::changes_for(&repo, target, &paths, &renames)
                .map(|files| match &seen {
                    Some(seen) => review::since_sent(
                        files,
                        seen,
                        |p| paths.iter().any(|q| q == p),
                        target,
                        &mut Reader::new(repo, root),
                    ),
                    None => files,
                })
                .map_err(|e| format!("{e:#}"));
            let _ = tx.send(Msg::Partial {
                generation,
                paths,
                result,
            });
        });
    }

    /// The safety net behind the watcher: compares HEAD, the index and the
    /// base with the last check, and restats working-tree files.
    pub(super) fn spawn_reconcile(&self) {
        let Some(stream) = &self.app.stream else {
            return;
        };
        let worktree = stream.worktree.clone();
        let state = self.pipeline.state.clone();
        let files: Vec<(String, Stamp)> = stream
            .files
            .iter()
            .filter_map(|f| match f.change.new {
                Side::Worktree(stamp) => Some((f.change.path.clone(), stamp)),
                _ => None,
            })
            .collect();
        let generation = self.pipeline.generation;
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let state = state.map(|s| s.restat());
            let moved = files
                .into_iter()
                .filter(|(path, stamp)| {
                    let now = std::fs::symlink_metadata(worktree.join(path))
                        .ok()
                        .map(|m| Stamp {
                            modified: m.modified().ok(),
                            len: m.len(),
                        });
                    now != Some(*stamp)
                })
                .map(|(path, _)| path)
                .collect();
            let _ = tx.send(Msg::Reconciled {
                generation,
                state,
                moved,
            });
        });
    }

    /// Starts watching the worktrees; setting up takes a moment, so it
    /// happens off the event loop.
    pub(super) fn spawn_watch(&mut self) {
        if self.watched == Some(self.app.current) {
            return;
        }
        self.watched = Some(self.app.current);
        self.watcher = None;
        let roots: Vec<PathBuf> = self
            .app
            .repo
            .worktrees
            .iter()
            .map(|w| w.path.clone())
            .collect();
        let selected = self.app.current;
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let events = Mutex::new(tx.clone());
            let result = watch::watch(&roots, selected, WATCH_ALL_FILES, move |t| {
                if let Ok(tx) = events.lock() {
                    let _ = tx.send(Msg::Touched(t));
                }
            })
            .map_err(|e| format!("{e:#}"));
            let _ = tx.send(Msg::Watching(result));
        });
    }

    /// Starts a moved-code search when moves are shown and the diffs
    /// differ from those the last search was started for.
    pub(super) fn refresh_moves(&mut self) {
        if !self.app.show_moves {
            return;
        }
        let Some(print) = self.app.move_fingerprint() else {
            return;
        };
        if self.moves.started_for == Some(print) {
            return;
        }
        let (Some(inputs), Some((repo, root))) =
            (self.app.move_inputs(), self.pipeline.source.clone())
        else {
            return;
        };
        self.moves.started_for = Some(print);
        self.moves.seq += 1;
        let (tx, epoch, seq) = (self.tx.clone(), self.pipeline.epoch, self.moves.seq);
        std::thread::spawn(move || {
            let mut reader = Reader::new(repo.to_thread_local(), root);
            let moves = Arc::new(moves::search(&mut reader, &inputs));
            let _ = tx.send(Msg::Moves { epoch, seq, moves });
        });
    }

    /// Diffs pending files on several threads, nearest to the view first.
    pub(super) fn spawn_diffs(&mut self) {
        let pending = self.app.pending();
        if pending.is_empty() {
            return;
        }
        self.pipeline.cancel_diffs();
        let Some(stream) = &self.app.stream else {
            return;
        };
        let Some((repo, root)) = self.pipeline.source.clone() else {
            return;
        };
        let todo: Vec<Option<FileChange>> = (0..stream.files.len())
            .map(|i| {
                pending
                    .binary_search(&i)
                    .ok()
                    .map(|_| stream.files[i].change.clone())
            })
            .collect();
        let todo = Arc::new(Mutex::new(todo));
        let job = DiffJob {
            cancel: Arc::new(AtomicBool::new(false)),
            focus: Arc::new(AtomicUsize::new(self.app.top.file)),
        };
        let threads = std::thread::available_parallelism().map_or(2, |n| n.get().clamp(1, 4));
        for _ in 0..threads {
            let (repo, root) = (repo.clone(), root.clone());
            let (todo, cancel, focus, tx, epoch) = (
                Arc::clone(&todo),
                Arc::clone(&job.cancel),
                Arc::clone(&job.focus),
                self.tx.clone(),
                self.pipeline.epoch,
            );
            std::thread::spawn(move || {
                let mut reader = Reader::new(repo.to_thread_local(), root);
                while !cancel.load(Ordering::Relaxed) {
                    let next = {
                        let Ok(mut todo) = todo.lock() else { return };
                        nearest(&todo, focus.load(Ordering::Relaxed))
                            .and_then(|i| todo[i].take().map(|c| (i, c)))
                    };
                    let Some((file, change)) = next else { return };
                    let (body, texts) = app::compute_body(&mut reader, &change);
                    if tx
                        .send(Msg::Diffed {
                            epoch,
                            file,
                            body,
                            texts,
                        })
                        .is_err()
                    {
                        return;
                    }
                }
            });
        }
        self.pipeline.diffs = Some(job);
    }

    /// Queues every worktree's switcher counts, those listed after the
    /// current one first.
    pub(super) fn spawn_summaries(&self) {
        let n = self.app.repo.worktrees.len();
        let at = self.app.current.min(n.saturating_sub(1));
        let order = (0..n).map(|d| (at + d) % n);
        for i in order {
            self.summaries
                .push(self.app.repo.worktrees[i].path.clone(), false);
        }
    }

    /// The selected worktree's switcher entry follows its own stream once complete.
    pub(super) fn update_own_summary(&mut self) {
        let Some(stream) = &self.app.stream else {
            return;
        };
        if stream.mode != repo::Mode::Base {
            return;
        }
        let (added, removed, done) = stream.counts();
        if done {
            // The content name and times come from the summary threads.
            let summary = self
                .app
                .summaries
                .entry(stream.worktree.clone())
                .or_default();
            summary.added = added;
            summary.removed = removed;
            summary.files = stream.files.len();
        }
    }
}

/// The repository's line-count caches, opened by the first summary thread
/// that needs them so loading never delays the first frame.
pub(super) type SharedStats = Arc<std::sync::OnceLock<Stats>>;

pub(super) type Queue = Arc<(Mutex<VecDeque<(PathBuf, SharedStats)>>, Condvar)>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diffs_files_nearest_the_view_first() {
        let todo = [Some(()), None, None, Some(()), Some(())];
        assert_eq!(nearest(&todo, 2), Some(3));
        assert_eq!(nearest(&todo, 1), Some(0));
        assert_eq!(nearest(&[None::<()>, None], 0), None);
        assert_eq!(nearest(&todo, 9), Some(4));
    }
}
