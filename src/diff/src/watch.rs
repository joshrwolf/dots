//! Change notification for a repository's worktrees: their files, each
//! worktree's Git state (HEAD, index) and the shared refs that decide what
//! a worktree is compared against. Bursts of events are coalesced into
//! path-limited refreshes, or one full scan when they grow too large.

use std::collections::BTreeSet;
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{Context as _, Result};
use notify::{RecursiveMode, Watcher as _};

/// Keeps the watch alive; dropping it stops notifications.
pub(crate) struct Watch {
    _watcher: notify::RecommendedWatcher,
}

impl std::fmt::Debug for Watch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Watch").finish_non_exhaustive()
    }
}

/// What an event touched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Touched {
    /// Files of one worktree, relative to its root.
    Files { worktree: usize, paths: Vec<String> },
    /// One worktree's HEAD or index.
    GitState(usize),
    /// Shared refs, which can move every worktree's base.
    Refs,
}

/// Watches the worktrees' files, each worktree's own Git state and the
/// shared refs. With `all_files` false (Linux, where every directory costs
/// an inotify watch) only `selected` has its files watched; the others
/// report Git state alone.
pub(crate) fn watch(
    worktrees: &[PathBuf],
    selected: usize,
    all_files: bool,
    changed: impl Fn(Touched) + Send + 'static,
) -> Result<Watch> {
    // (path, worktree, is the worktree root)
    let mut roots: Vec<(PathBuf, usize, bool)> = Vec::new();
    let mut common = None;
    for (i, root) in worktrees.iter().enumerate() {
        let repo = gix::open(root).with_context(|| format!("opening {}", root.display()))?;
        let git_dir = repo.git_dir().to_owned();
        if all_files || i == selected {
            roots.push((root.clone(), i, true));
        }
        if !git_dir.starts_with(root) || !(all_files || i == selected) {
            roots.push((git_dir, i, false));
        }
        common.get_or_insert_with(|| repo.common_dir().to_owned());
    }
    // Longest prefix first, so nested worktrees win over their parents.
    roots.sort_by_key(|(p, ..)| std::cmp::Reverse(p.as_os_str().len()));
    let common_refs = common.as_ref().map(|c| c.join("refs"));
    let owners = roots.clone();
    let refs = common_refs.clone();
    let mut watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
        let Ok(event) = event else { return };
        if matches!(event.kind, notify::EventKind::Access(_)) {
            return;
        }
        let mut files: Vec<(usize, String)> = Vec::new();
        for path in &event.paths {
            if refs.as_ref().is_some_and(|r| path.starts_with(r))
                || path.file_name().is_some_and(|n| n == "packed-refs")
            {
                changed(Touched::Refs);
                continue;
            }
            let Some((root, i, is_root)) = owners.iter().find(|(root, ..)| path.starts_with(root))
            else {
                continue;
            };
            match classify(path, root, *is_root) {
                Some(Event::File(rel)) => files.push((*i, rel)),
                Some(Event::GitState) => changed(Touched::GitState(*i)),
                None => {}
            }
        }
        files.sort();
        for chunk in files.chunk_by(|a, b| a.0 == b.0) {
            changed(Touched::Files {
                worktree: chunk[0].0,
                paths: chunk.iter().map(|(_, p)| p.clone()).collect(),
            });
        }
    })?;
    for (path, _, is_root) in &roots {
        let mode = if *is_root {
            RecursiveMode::Recursive
        } else {
            RecursiveMode::NonRecursive
        };
        watcher.watch(path, mode)?;
    }
    if let Some(refs) = common_refs.filter(|r| r.is_dir()) {
        watcher.watch(&refs, RecursiveMode::Recursive)?;
    }
    Ok(Watch { _watcher: watcher })
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Event {
    /// A working-tree path, relative to the root.
    File(String),
    GitState,
}

/// What a changed path under a watched root means, if anything. Build
/// output, dependencies, Git's object store, lock files and the fsmonitor
/// daemon's cookies change constantly and never decide what the diff shows.
pub(crate) fn classify(path: &Path, root: &Path, is_worktree_root: bool) -> Option<Event> {
    let rel = path.strip_prefix(root).unwrap_or(path);
    let parts: Vec<&str> = rel
        .components()
        .filter_map(|c| match c {
            Component::Normal(s) => s.to_str(),
            _ => None,
        })
        .collect();
    let in_git = !is_worktree_root || parts.first() == Some(&".git");
    if in_git {
        let name = parts.last().copied().unwrap_or("");
        let churn = parts
            .iter()
            .any(|p| matches!(*p, "objects" | "logs" | "fsmonitor--daemon" | "refs"))
            || Path::new(name).extension().is_some_and(|e| e == "lock")
            || name.starts_with(".watchman-cookie");
        // Inside a worktree's Git directory, only HEAD and the index matter;
        // shared refs are watched on their own.
        let state = matches!(name, "HEAD" | "index" | "MERGE_HEAD" | "ORIG_HEAD");
        return (!churn && state).then_some(Event::GitState);
    }
    if parts.is_empty()
        || parts
            .iter()
            .any(|p| matches!(*p, "target" | "node_modules"))
        || parts
            .last()
            .is_some_and(|n| n.starts_with(".watchman-cookie"))
    {
        return None;
    }
    Some(Event::File(parts.join("/")))
}

/// Whether a path-limited rescan of `paths` would go wrong: a path that is a
/// directory on disk, or that holds files `listed` (sorted) shows. Renaming
/// or removing a directory reports the directory alone, and status and the
/// per-path tree lookups do not expand it the way the stream lists files.
pub(crate) fn needs_full_scan(root: &Path, paths: &[String], listed: &[&str]) -> bool {
    paths.iter().any(|p| {
        let dir = std::fs::symlink_metadata(root.join(p)).is_ok_and(|m| m.is_dir());
        let prefix = format!("{p}/");
        let at = listed.partition_point(|l| *l < prefix.as_str());
        dir || listed.get(at).is_some_and(|l| l.starts_with(&prefix))
    })
}

/// A burst of file events, held until it goes quiet.
#[derive(Debug, Default)]
pub(crate) struct Burst {
    paths: BTreeSet<String>,
    first: Option<Instant>,
    last: Option<Instant>,
    full: bool,
}

/// What a settled burst asks for.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Refresh {
    Paths(Vec<String>),
    Full,
}

/// Quiet time that ends a burst: editors write a file in several steps.
pub(crate) const QUIET: Duration = Duration::from_millis(20);
/// The longest a burst is held while events keep coming.
pub(crate) const MAX_WAIT: Duration = Duration::from_millis(150);
/// Above this many paths one full scan is cheaper than a path-limited one.
pub(crate) const MAX_PATHS: usize = 256;

impl Burst {
    pub(crate) fn push(&mut self, now: Instant, paths: impl IntoIterator<Item = String>) {
        self.first.get_or_insert(now);
        self.last = Some(now);
        if !self.full {
            self.paths.extend(paths);
            if self.paths.len() > MAX_PATHS {
                self.full = true;
                self.paths.clear();
            }
        }
    }

    /// Asks for a full scan, as Git state changes do.
    pub(crate) fn push_full(&mut self, now: Instant) {
        self.push(now, []);
        self.full = true;
        self.paths.clear();
    }

    /// When the burst is due: quiet for [`QUIET`], or held [`MAX_WAIT`].
    pub(crate) fn due(&self) -> Option<Instant> {
        Some((self.last? + QUIET).min(self.first? + MAX_WAIT))
    }

    /// Ends the burst if it is due at `now`.
    pub(crate) fn take(&mut self, now: Instant) -> Option<Refresh> {
        if self.due()? > now {
            return None;
        }
        let refresh = if self.full {
            Refresh::Full
        } else {
            Refresh::Paths(std::mem::take(&mut self.paths).into_iter().collect())
        };
        *self = Self::default();
        Some(refresh)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(rel: &str) -> Event {
        Event::File(rel.to_owned())
    }

    #[test]
    fn classifies_files_git_state_and_churn() {
        let root = Path::new("/r");
        let at = |p: &str| classify(Path::new(p), root, true);
        assert_eq!(at("/r/src/main.rs"), Some(file("src/main.rs")));
        assert_eq!(at("/r/Cargo.lock"), Some(file("Cargo.lock")));
        assert_eq!(at("/r/.git/HEAD"), Some(Event::GitState));
        assert_eq!(at("/r/.git/index"), Some(Event::GitState));
        assert_eq!(at("/r/target/debug/x"), None);
        assert_eq!(at("/r/web/node_modules/a.js"), None);
        assert_eq!(at("/r/.git/objects/ab/cdef"), None);
        assert_eq!(at("/r/.git/index.lock"), None);
        assert_eq!(at("/r/.git/fsmonitor--daemon/cookies/1-2"), None);
        assert_eq!(at("/r"), None);
        // A linked worktree's Git directory, watched on its own.
        let git = Path::new("/c/.git/worktrees/w");
        assert_eq!(
            classify(Path::new("/c/.git/worktrees/w/HEAD"), git, false),
            Some(Event::GitState)
        );
        assert_eq!(
            classify(Path::new("/c/.git/worktrees/w/logs/HEAD"), git, false),
            None
        );
    }

    #[test]
    fn directories_and_paths_holding_listed_files_need_a_full_scan() {
        let root = std::env::temp_dir().join(format!("dots-diff-dirs-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("src/dir2")).expect("dir");
        std::fs::write(root.join("src/a.rs"), "").expect("file");
        let listed = ["src/a.rs", "src/dir/a.rs", "src/dirt.rs"];
        let full = |paths: &[&str]| {
            let paths: Vec<String> = paths.iter().map(|p| (*p).to_owned()).collect();
            needs_full_scan(&root, &paths, &listed)
        };
        assert!(!full(&["src/a.rs", "src/gone.rs"]));
        // `mv src/dir src/dir2` reports the two directories alone.
        assert!(full(&["src/dir2"]), "a directory on disk");
        assert!(
            full(&["src/dir"]),
            "a removed directory holding listed files"
        );
        assert!(!full(&["src/di"]), "a name prefix is not a directory");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn bursts_coalesce_until_quiet_or_held_too_long() {
        let t0 = Instant::now();
        let ms = |n| t0 + Duration::from_millis(n);
        let mut b = Burst::default();
        assert_eq!(b.take(t0), None);
        b.push(t0, ["a".to_owned()]);
        b.push(ms(10), ["b".to_owned(), "a".to_owned()]);
        assert_eq!(b.take(ms(20)), None, "still within the quiet time");
        assert_eq!(b.due(), Some(ms(30)));
        assert_eq!(
            b.take(ms(30)),
            Some(Refresh::Paths(vec!["a".to_owned(), "b".to_owned()]))
        );
        assert_eq!(b.due(), None);
        // Steady events are cut off after the longest wait.
        for n in (0..200).step_by(10) {
            b.push(ms(n), [format!("f{n}")]);
        }
        assert_eq!(b.due(), Some(ms(150)));
    }

    #[test]
    fn large_bursts_and_git_state_fall_back_to_a_full_scan() {
        let t0 = Instant::now();
        let mut b = Burst::default();
        b.push(t0, (0..=MAX_PATHS).map(|i| i.to_string()));
        assert_eq!(b.take(t0 + MAX_WAIT), Some(Refresh::Full));
        b.push(t0, ["a".to_owned()]);
        b.push_full(t0);
        b.push(t0, ["b".to_owned()]);
        assert_eq!(b.take(t0 + MAX_WAIT), Some(Refresh::Full));
    }
}
