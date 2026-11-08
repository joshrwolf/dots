//! Change notification for one worktree: its files and the Git state that
//! decides what it is compared against (HEAD, index, refs).

use std::path::{Component, Path, PathBuf};

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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Touched {
    /// Files or Git state of one worktree.
    Worktree(usize),
    /// Shared refs, which can move every worktree's base.
    Refs,
}

/// Watches every worktree's files, each worktree's own Git state (HEAD,
/// index) and the shared refs, reporting which worktree changed.
pub(crate) fn watch(
    worktrees: &[PathBuf],
    changed: impl Fn(Touched) + Send + 'static,
) -> Result<Watch> {
    let mut roots: Vec<(PathBuf, usize)> = Vec::new();
    let mut common = None;
    for (i, root) in worktrees.iter().enumerate() {
        roots.push((root.clone(), i));
        let repo = gix::open(root).with_context(|| format!("opening {}", root.display()))?;
        if !repo.git_dir().starts_with(root) {
            roots.push((repo.git_dir().to_owned(), i));
        }
        common.get_or_insert_with(|| repo.common_dir().to_owned());
    }
    // Longest prefix first, so nested worktrees win over their parents.
    roots.sort_by_key(|(p, _)| std::cmp::Reverse(p.as_os_str().len()));
    let common_refs = common.as_ref().map(|c| c.join("refs"));
    let owners = roots.clone();
    let refs = common_refs.clone();
    let mut watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
        let Ok(event) = event else { return };
        if matches!(event.kind, notify::EventKind::Access(_)) {
            return;
        }
        for path in &event.paths {
            if refs.as_ref().is_some_and(|r| path.starts_with(r))
                || path.file_name().is_some_and(|n| n == "packed-refs")
            {
                changed(Touched::Refs);
                continue;
            }
            if let Some((root, i)) = owners.iter().find(|(root, _)| path.starts_with(root))
                && relevant(path, root)
            {
                changed(Touched::Worktree(*i));
            }
        }
    })?;
    for (path, i) in &roots {
        let mode = if worktrees.get(*i) == Some(path) {
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

/// Build output, dependencies and Git's object store change constantly and
/// never decide what the diff shows.
pub(crate) fn relevant(path: &Path, root: &Path) -> bool {
    let rel = path.strip_prefix(root).unwrap_or(path);
    let parts: Vec<&std::ffi::OsStr> = rel
        .components()
        .filter_map(|c| match c {
            Component::Normal(s) => Some(s),
            _ => None,
        })
        .collect();
    if parts.iter().any(|p| *p == "target" || *p == "node_modules") {
        return false;
    }
    let in_git = parts.iter().any(|p| *p == ".git");
    let churn = parts.iter().any(|p| *p == "objects" || *p == "logs")
        || path.extension().is_some_and(|e| e == "lock");
    !(in_git && churn)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ignores_build_output_and_git_internals() {
        let root = Path::new("/r");
        assert!(relevant(Path::new("/r/src/main.rs"), root));
        assert!(relevant(Path::new("/r/Cargo.lock"), root));
        assert!(relevant(Path::new("/r/.git/HEAD"), root));
        assert!(!relevant(Path::new("/r/target/debug/x"), root));
        assert!(!relevant(Path::new("/r/web/node_modules/a.js"), root));
        assert!(!relevant(Path::new("/r/.git/objects/ab/cdef"), root));
        assert!(!relevant(Path::new("/r/.git/index.lock"), root));
    }
}
