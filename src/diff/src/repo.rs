//! Read-only Git access through gix: worktrees, the review base, and the
//! files that differ between a base tree and a working tree.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context as _, Result};
use gix::bstr::ByteSlice as _;

/// One checkout of the repository.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Worktree {
    pub path: PathBuf,
    /// The checked-out branch, or a short commit id when detached.
    pub branch: String,
    pub main: bool,
}

#[derive(Debug, Clone)]
pub struct Repo {
    /// The main worktree's directory name.
    pub name: String,
    pub worktrees: Vec<Worktree>,
}

impl Repo {
    /// The index of the worktree containing `path`, if any.
    pub fn index_of(&self, path: &Path) -> Option<usize> {
        let path = path.canonicalize().ok()?;
        self.worktrees
            .iter()
            .enumerate()
            .filter(|(_, w)| path.starts_with(&w.path))
            .max_by_key(|(_, w)| w.path.as_os_str().len())
            .map(|(i, _)| i)
    }
}

/// The repository containing `path` and all its worktrees, main first.
pub fn discover(path: &Path) -> Result<Repo> {
    let repo = gix::discover(path).with_context(|| format!("no Git repository at {}", path.display()))?;
    let main = repo.main_repo().context("opening the main worktree")?;
    let mut worktrees = Vec::new();
    if let Some(dir) = main.workdir() {
        worktrees.push(Worktree {
            path: dir.canonicalize().unwrap_or_else(|_| dir.to_owned()),
            branch: branch(&main),
            main: true,
        });
    }
    for proxy in main.worktrees().context("listing worktrees")? {
        let Ok(base) = proxy.base() else { continue };
        // Pruned or unmounted worktrees have nothing to show.
        let Ok(base) = base.canonicalize() else { continue };
        let Ok(wt) = proxy.into_repo() else { continue };
        worktrees.push(Worktree {
            path: base,
            branch: branch(&wt),
            main: false,
        });
    }
    let skip = usize::from(worktrees.first().is_some_and(|w| w.main));
    worktrees[skip..]
        .sort_by(|a, b| a.branch.cmp(&b.branch));
    let name = worktrees
        .first()
        .and_then(|w| w.path.file_name())
        .map_or_else(|| "repo".to_owned(), |n| n.to_string_lossy().into_owned());
    Ok(Repo { name, worktrees })
}

fn branch(repo: &gix::Repository) -> String {
    match repo.head_name() {
        Ok(Some(name)) => name.shorten().to_string(),
        _ => repo
            .head_id()
            .map_or_else(|_| "(unborn)".to_owned(), |id| id.shorten_or_id().to_string()),
    }
}

/// What a worktree is compared against.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// The merge base of HEAD with the default branch: everything the
    /// worktree's branch adds, committed or not.
    Base,
    /// HEAD: uncommitted changes only.
    Head,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Status {
    Added,
    Deleted,
    Modified,
}

impl Status {
    pub fn letter(self) -> char {
        match self {
            Status::Added => 'A',
            Status::Deleted => 'D',
            Status::Modified => 'M',
        }
    }
}

#[derive(Debug, Clone)]
pub struct FileChange {
    pub path: String,
    pub status: Status,
    pub old: Option<Arc<[u8]>>,
    pub new: Option<Arc<[u8]>>,
}

#[derive(Debug, Clone)]
pub struct Changes {
    /// What the worktree is compared against, such as `main 1a2b3c4`.
    pub base: String,
    /// Sorted by path.
    pub files: Vec<FileChange>,
}

/// Files above this size are listed but not read.
pub const MAX_FILE: u64 = 16 * 1024 * 1024;

/// Every file that differs between the base tree and the working tree,
/// including untracked files that are not ignored.
pub fn changes(worktree: &Path, mode: Mode) -> Result<Changes> {
    let mut repo = gix::open(worktree).with_context(|| format!("opening {}", worktree.display()))?;
    repo.object_cache_size_if_unset(32 * 1024 * 1024);
    let head = repo.head_id().ok().map(gix::Id::detach);
    let (base_commit, base_label) = match (mode, head) {
        (Mode::Head, Some(head)) => (Some(head), format!("HEAD {}", short(head))),
        (Mode::Base, Some(head)) => match resolve_base(&repo, head)? {
            Some((name, commit)) => (Some(commit), format!("{name} {}", short(commit))),
            None => (Some(head), format!("HEAD {}", short(head))),
        },
        (_, None) => (None, "empty tree".to_owned()),
    };
    let base_tree = match base_commit {
        Some(commit) => repo.find_commit(commit)?.tree()?,
        None => repo.empty_tree(),
    };
    let head_tree = match head {
        Some(head) => repo.find_commit(head)?.tree_id()?.detach(),
        None => repo.empty_tree().id,
    };
    let mut paths = match git_status(&repo, worktree) {
        Ok((mut paths, untracked_dirs)) => {
            if !untracked_dirs.is_empty() {
                paths.extend(untracked_files(&repo, untracked_dirs)?);
            }
            if base_tree.id != head_tree {
                paths.extend(tree_paths(&repo, base_tree.id, head_tree)?);
            }
            paths
        }
        // Without a usable git, gix compares the base tree with the index and
        // walks the whole tree for untracked files.
        Err(_) => gix_status(&repo, base_tree.id)?,
    };
    paths.retain(|p| !p.is_empty());
    let root = repo.workdir().context("bare repositories have no working tree")?.to_owned();
    let mut files = Vec::with_capacity(paths.len());
    for path in paths {
        let old = match base_tree.lookup_entry_by_path(&path)? {
            Some(entry) if entry.mode().is_blob_or_symlink() => {
                Some(Arc::from(repo.find_blob(entry.object_id())?.take_data()))
            }
            _ => None,
        };
        let Some(new) = read_worktree_file(&root.join(&path))? else {
            continue;
        };
        let status = match (&old, &new) {
            (None, None) => continue,
            (None, Some(_)) => Status::Added,
            (Some(_), None) => Status::Deleted,
            (Some(a), Some(b)) if a == b => continue,
            (Some(_), Some(_)) => Status::Modified,
        };
        files.push(FileChange {
            path,
            status,
            old,
            new,
        });
    }
    Ok(Changes {
        base: base_label,
        files,
    })
}

/// A working-tree file's content: `Some(None)` when it is gone, `None` for
/// paths that are not files (submodules, directories).
fn read_worktree_file(path: &Path) -> Result<Option<Option<Arc<[u8]>>>> {
    let meta = match std::fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Some(None)),
        Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
    };
    if meta.file_type().is_symlink() {
        let target = std::fs::read_link(path)?;
        return Ok(Some(Some(Arc::from(
            target.as_os_str().as_encoded_bytes(),
        ))));
    }
    if !meta.is_file() {
        return Ok(None);
    }
    if meta.len() > MAX_FILE {
        return Ok(Some(Some(Arc::from(&b"\0(file too large to show)"[..]))));
    }
    let bytes = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    Ok(Some(Some(Arc::from(bytes))))
}

/// Paths that may differ from HEAD, from `git status`, and untracked
/// directories to expand. Git's fsmonitor and untracked cache make this an
/// order of magnitude faster than walking a large tree, which gix cannot yet
/// avoid. Read-only: no optional locks, so the index is never rewritten, and
/// fsmonitor is used only when its daemon already runs, so none is started.
fn git_status(repo: &gix::Repository, worktree: &Path) -> Result<(BTreeSet<String>, Vec<String>)> {
    let fsmonitor = repo.config_snapshot().boolean("core.fsmonitor") == Some(true)
        && git(worktree, &["fsmonitor--daemon", "status"]).is_ok();
    let setting = format!("core.fsmonitor={fsmonitor}");
    let out = git(
        worktree,
        &[
            "-c",
            &setting,
            "status",
            "--porcelain=v2",
            "-z",
            "--untracked-files=normal",
            "--ignore-submodules=all",
            "--no-renames",
        ],
    )?;
    let (paths, dirs) = parse_status(&out);
    Ok((paths, dirs))
}

fn git(worktree: &Path, args: &[&str]) -> Result<Vec<u8>> {
    let mut command = std::process::Command::new("git");
    command.arg("-C").arg(worktree).arg("--no-optional-locks").args(args);
    command.env("GIT_OPTIONAL_LOCKS", "0");
    let output = dots_process::output(command, std::time::Duration::from_secs(60), 256 * 1024 * 1024)?;
    if !output.status.success() {
        anyhow::bail!("git {} failed: {}", args.join(" "), String::from_utf8_lossy(&output.stderr).trim());
    }
    Ok(output.stdout)
}

/// Reads `git status --porcelain=v2 -z`: changed paths, and untracked
/// directories (listed with a trailing slash) separately.
pub fn parse_status(out: &[u8]) -> (BTreeSet<String>, Vec<String>) {
    let mut paths = BTreeSet::new();
    let mut dirs = Vec::new();
    for record in out.split(|&b| b == 0).filter(|r| !r.is_empty()) {
        let text = String::from_utf8_lossy(record);
        let fields = match text.as_bytes().first() {
            Some(b'1') => 8,
            Some(b'u') => 10,
            Some(b'?') => 1,
            _ => continue,
        };
        let Some(path) = text.splitn(fields + 1, ' ').nth(fields) else { continue };
        match path.strip_suffix('/') {
            Some(dir) if text.starts_with('?') => dirs.push(dir.to_owned()),
            _ => {
                paths.insert(path.to_owned());
            }
        }
    }
    (paths, dirs)
}

/// Untracked, not ignored files under `dirs`.
fn untracked_files(repo: &gix::Repository, dirs: Vec<String>) -> Result<Vec<String>> {
    let iter = repo
        .status(gix::progress::Discard)?
        .untracked_files(gix::status::UntrackedFiles::Files)
        .index_worktree_submodules(None)
        .into_index_worktree_iter(dirs.into_iter().map(gix::bstr::BString::from))?;
    let mut out = Vec::new();
    for item in iter {
        let item = item?;
        if let gix::status::index_worktree::Item::DirectoryContents { entry, .. } = &item
            && matches!(entry.status, gix::dir::entry::Status::Untracked)
        {
            out.push(entry.rela_path.to_str_lossy().into_owned());
        }
    }
    Ok(out)
}

/// Paths that differ between two trees.
fn tree_paths(repo: &gix::Repository, old: gix::ObjectId, new: gix::ObjectId) -> Result<Vec<String>> {
    let (old, new) = (repo.find_tree(old)?, repo.find_tree(new)?);
    let mut options = gix::diff::Options::default();
    options.track_path();
    options.track_rewrites(None);
    let changes = repo.diff_tree_to_tree(&old, &new, options)?;
    Ok(changes
        .into_iter()
        .filter(|c| !c.entry_mode().is_tree())
        .map(|c| c.location().to_str_lossy().into_owned())
        .collect())
}

/// Every path differing between `base` and the working tree, through gix
/// alone: base tree against the index, the index against files, and a
/// walk for untracked files.
fn gix_status(repo: &gix::Repository, base: gix::ObjectId) -> Result<BTreeSet<String>> {
    let mut paths = BTreeSet::new();
    let status = repo
        .status(gix::progress::Discard)?
        .head_tree(base)
        .untracked_files(gix::status::UntrackedFiles::Files)
        .index_worktree_submodules(None);
    for item in status.into_iter(None)? {
        let item = item?;
        if let gix::status::Item::IndexWorktree(change) = &item
            && change.summary().is_none()
        {
            continue;
        }
        paths.insert(item.location().to_str_lossy().into_owned());
    }
    Ok(paths)
}

fn short(id: gix::ObjectId) -> String {
    id.to_hex_with_len(7).to_string()
}

/// The default branch's merge base with `head`, and the branch's name.
fn resolve_base(repo: &gix::Repository, head: gix::ObjectId) -> Result<Option<(String, gix::ObjectId)>> {
    let origin_head = repo
        .try_find_reference("refs/remotes/origin/HEAD")?
        .and_then(|r| r.target().try_name().map(|n| n.as_bstr().to_str_lossy().into_owned()));
    let exists = |name: &str| repo.try_find_reference(name).ok().flatten().is_some();
    let mut bases = Vec::new();
    for name in base_candidates(origin_head.as_deref(), exists) {
        let Ok(mut reference) = repo.find_reference(name.as_str()) else {
            continue;
        };
        let Ok(tip) = reference.peel_to_id() else { continue };
        if let Some(base) = repo.merge_base(head, tip.detach())? {
            bases.push((short_ref(&name), base.detach()));
        }
    }
    let is_ancestor = |a: gix::ObjectId, b: gix::ObjectId| {
        a == b || repo.merge_base(a, b).ok().flatten().is_some_and(|m| m.detach() == a)
    };
    Ok(newest(bases, is_ancestor))
}

fn short_ref(name: &str) -> String {
    name.strip_prefix("refs/heads/")
        .or_else(|| name.strip_prefix("refs/remotes/"))
        .unwrap_or(name)
        .to_owned()
}

/// Refs that may name the default branch, best first, keeping those that
/// exist. With `origin/HEAD` set, its target and the local branch of the same
/// name; otherwise `main` and `master`, local before remote.
pub fn base_candidates(origin_head: Option<&str>, exists: impl Fn(&str) -> bool) -> Vec<String> {
    let names: Vec<String> = match origin_head.and_then(|t| t.strip_prefix("refs/remotes/origin/")) {
        Some(branch) => vec![
            format!("refs/heads/{branch}"),
            format!("refs/remotes/origin/{branch}"),
        ],
        None => ["main", "master"]
            .iter()
            .flat_map(|b| [format!("refs/heads/{b}"), format!("refs/remotes/origin/{b}")])
            .collect(),
    };
    let mut names: Vec<String> = names.into_iter().filter(|n| exists(n)).collect();
    if origin_head.is_none() {
        // Without origin/HEAD, the first branch name that exists decides.
        let branch = names.first().map(|n| n.rsplit('/').next().unwrap_or("").to_owned());
        names.retain(|n| Some(n.rsplit('/').next().unwrap_or("").to_owned()) == branch);
    }
    names
}

/// The most recent of several merge bases: the one every other is an
/// ancestor of. A local branch behind its remote, or ahead of it, then
/// yields the fork point closest to the worktree's branch.
pub fn newest<T: Copy>(
    bases: Vec<(String, T)>,
    is_ancestor: impl Fn(T, T) -> bool,
) -> Option<(String, T)> {
    let mut best: Option<(String, T)> = None;
    for (name, base) in bases {
        best = match best {
            Some((_, b)) if is_ancestor(b, base) => Some((name, base)),
            Some(kept) => Some(kept),
            None => Some((name, base)),
        };
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn candidates_follow_origin_head_and_prefer_the_local_branch() {
        let all = |_: &str| true;
        assert_eq!(
            base_candidates(Some("refs/remotes/origin/trunk"), all),
            vec!["refs/heads/trunk", "refs/remotes/origin/trunk"]
        );
        let remote_only = |n: &str| n.starts_with("refs/remotes/");
        assert_eq!(
            base_candidates(Some("refs/remotes/origin/main"), remote_only),
            vec!["refs/remotes/origin/main"]
        );
    }

    #[test]
    fn candidates_fall_back_to_main_then_master() {
        let master_only = |n: &str| n.ends_with("/master");
        assert_eq!(
            base_candidates(None, master_only),
            vec!["refs/heads/master", "refs/remotes/origin/master"]
        );
        let both = |n: &str| n == "refs/heads/main" || n == "refs/heads/master";
        assert_eq!(base_candidates(None, both), vec!["refs/heads/main"]);
        assert!(base_candidates(None, |_| false).is_empty());
    }

    #[test]
    fn picks_the_newest_merge_base() {
        // History 1 <- 2 <- 3: a larger number descends from a smaller one.
        let is_ancestor = |a: u32, b: u32| a <= b;
        let bases = vec![("main".to_owned(), 2), ("origin/main".to_owned(), 3)];
        assert_eq!(newest(bases, is_ancestor), Some(("origin/main".to_owned(), 3)));
        let bases = vec![("main".to_owned(), 3), ("origin/main".to_owned(), 1)];
        assert_eq!(newest(bases, is_ancestor), Some(("main".to_owned(), 3)));
        assert_eq!(newest::<u32>(vec![], is_ancestor), None);
    }

    #[test]
    fn parses_porcelain_v2_status() {
        let out = b"1 .M N... 100644 100644 100644 abc abc src/a b.rs\0u UU N... 100644 100644 100644 100644 a b c conflict.txt\0? new.txt\0? fresh/dir/\0";
        let (paths, dirs) = parse_status(out);
        assert_eq!(
            paths.into_iter().collect::<Vec<_>>(),
            vec!["conflict.txt", "new.txt", "src/a b.rs"]
        );
        assert_eq!(dirs, vec!["fresh/dir"]);
    }
}
