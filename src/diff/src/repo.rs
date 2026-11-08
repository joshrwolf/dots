//! Read-only Git access through gix: worktrees, the review base, and the
//! files that differ between a base tree and a working tree.

use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result};
use gix::bstr::ByteSlice as _;

/// One checkout of the repository.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Worktree {
    pub(crate) path: PathBuf,
    /// The checked-out branch, or a short commit id when detached.
    pub(crate) branch: String,
    pub(crate) main: bool,
    /// When the worktree's Git state last changed: a commit, checkout or
    /// staging.
    pub(crate) touched: Option<std::time::SystemTime>,
}

#[derive(Debug, Clone)]
pub(crate) struct Repo {
    pub(crate) worktrees: Vec<Worktree>,
}

impl Repo {
    /// The index of the worktree containing `path`, if any.
    pub(crate) fn index_of(&self, path: &Path) -> Option<usize> {
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
pub(crate) fn discover(path: &Path) -> Result<Repo> {
    let repo =
        gix::discover(path).with_context(|| format!("no Git repository at {}", path.display()))?;
    let main = repo.main_repo().context("opening the main worktree")?;
    let mut worktrees = Vec::new();
    if let Some(dir) = main.workdir() {
        worktrees.push(Worktree {
            path: dir.canonicalize().unwrap_or_else(|_| dir.to_owned()),
            branch: branch(&main),
            main: true,
            touched: touched(main.git_dir()),
        });
    }
    for proxy in main.worktrees().context("listing worktrees")? {
        let Ok(base) = proxy.base() else { continue };
        // Pruned or unmounted worktrees have nothing to show.
        let Ok(base) = base.canonicalize() else {
            continue;
        };
        let Ok(wt) = proxy.into_repo() else { continue };
        worktrees.push(Worktree {
            path: base,
            branch: branch(&wt),
            main: false,
            touched: touched(wt.git_dir()),
        });
    }
    let skip = usize::from(worktrees.first().is_some_and(|w| w.main));
    worktrees[skip..].sort_by(|a, b| a.branch.cmp(&b.branch));
    Ok(Repo { worktrees })
}

/// The newest change to a worktree's HEAD reflog or index.
fn touched(git_dir: &Path) -> Option<std::time::SystemTime> {
    ["logs/HEAD", "index", "HEAD"]
        .iter()
        .filter_map(|f| std::fs::metadata(git_dir.join(f)).ok()?.modified().ok())
        .max()
}

fn branch(repo: &gix::Repository) -> String {
    match repo.head_name() {
        Ok(Some(name)) => name.shorten().to_string(),
        _ => repo.head_id().map_or_else(
            |_| "(unborn)".to_owned(),
            |id| id.shorten_or_id().to_string(),
        ),
    }
}

/// What a worktree is compared against.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mode {
    /// The merge base of HEAD with the default branch: everything the
    /// worktree's branch adds, committed or not.
    Base,
    /// HEAD: uncommitted changes only.
    Head,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Status {
    Added,
    Deleted,
    Modified,
}

/// The new side of a change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Side {
    /// The working tree matches this blob, so it is read from Git.
    Blob(gix::ObjectId),
    /// The working-tree file differs from every blob; read it from disk.
    Worktree(Stamp),
    /// Deleted.
    Missing,
}

#[derive(Debug, Clone)]
pub(crate) struct FileChange {
    pub(crate) path: String,
    pub(crate) status: Status,
    /// The base side's blob.
    pub(crate) old: Option<gix::ObjectId>,
    pub(crate) new: Side,
}

/// A working-tree file's size and modification time, to tell whether it
/// changed since it was read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct Stamp {
    pub(crate) modified: Option<std::time::SystemTime>,
    pub(crate) len: u64,
}

#[derive(Debug, Clone)]
pub(crate) struct Changes {
    /// What the worktree is compared against, such as `main 1a2b3c4`.
    pub(crate) base: String,
    pub(crate) target: Target,
    /// Sorted by path.
    pub(crate) files: Vec<FileChange>,
    /// For reading contents on other threads.
    pub(crate) repo: gix::ThreadSafeRepository,
    pub(crate) root: PathBuf,
}

/// The trees a worktree is compared through: the base and HEAD.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct Target {
    pub(crate) base_tree: gix::ObjectId,
    pub(crate) head_tree: gix::ObjectId,
}

/// Files above this size are listed but not read.
pub(crate) const MAX_FILE: u64 = 16 * 1024 * 1024;

/// Whether `git status` may use the repository's fsmonitor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Fsmonitor {
    /// As configured, starting the daemon if git wants to: the viewed worktree.
    AsConfigured,
    /// Only when a daemon already runs for the worktree: background stats
    /// must not start daemons.
    IfRunning,
}

/// Opens `worktree` and resolves what `mode` compares it against.
pub(crate) fn open(worktree: &Path, mode: Mode) -> Result<(gix::Repository, String, Target)> {
    let mut repo =
        gix::open(worktree).with_context(|| format!("opening {}", worktree.display()))?;
    repo.object_cache_size_if_unset(32 * 1024 * 1024);
    let head = repo.head_id().ok().map(gix::Id::detach);
    let (base_commit, label) = match (mode, head) {
        (Mode::Head, Some(head)) => (Some(head), format!("HEAD {}", short(head))),
        (Mode::Base, Some(head)) => match resolve_base(&repo, head)? {
            Some((name, commit)) => (Some(commit), format!("{name} {}", short(commit))),
            None => (Some(head), format!("HEAD {}", short(head))),
        },
        (_, None) => (None, "empty tree".to_owned()),
    };
    let tree_of = |commit: Option<gix::ObjectId>| -> Result<gix::ObjectId> {
        Ok(match commit {
            Some(c) => repo.find_commit(c)?.tree_id()?.detach(),
            None => repo.empty_tree().id,
        })
    };
    let target = Target {
        base_tree: tree_of(base_commit)?,
        head_tree: tree_of(head)?,
    };
    Ok((repo, label, target))
}

/// Every file that differs between the base tree and the working tree,
/// including untracked files that are not ignored.
pub(crate) fn changes(worktree: &Path, mode: Mode, fsmonitor: Fsmonitor) -> Result<Changes> {
    let (repo, base, target) = open(worktree, mode)?;
    let status = status(&repo, worktree, fsmonitor, &[])?;
    let tree = if target.base_tree == target.head_tree {
        Vec::new()
    } else {
        tree_changes(&repo, target.base_tree, target.head_tree)?
    };
    let root = workdir(&repo)?;
    let files = stat_worktree(&root, assemble(tree, status))?;
    Ok(Changes {
        base,
        target,
        files,
        repo: repo.into_sync(),
        root,
    })
}

/// The changes among `paths` alone, for a worktree whose base and HEAD are
/// still `target`: a pathspec-limited status and per-path tree lookups.
/// A path missing from the result has no change.
pub(crate) fn changes_for(
    repo: &gix::Repository,
    target: Target,
    paths: &[String],
) -> Result<Vec<FileChange>> {
    let root = workdir(repo)?;
    let status = status(repo, &root, Fsmonitor::AsConfigured, paths)?;
    let mut tree = Vec::new();
    if target.base_tree != target.head_tree {
        let base = repo.find_tree(target.base_tree)?;
        let head = repo.find_tree(target.head_tree)?;
        for path in paths {
            let (old, new) = (blob_at(&base, path)?, blob_at(&head, path)?);
            if old != new {
                tree.push(TreeChange {
                    path: path.clone(),
                    old,
                    new,
                });
            }
        }
    }
    stat_worktree(&root, assemble(tree, status))
}

fn workdir(repo: &gix::Repository) -> Result<PathBuf> {
    Ok(repo
        .workdir()
        .context("bare repositories have no working tree")?
        .to_owned())
}

pub(crate) fn blob_at(tree: &gix::Tree<'_>, path: &str) -> Result<Option<gix::ObjectId>> {
    Ok(match tree.lookup_entry_by_path(path)? {
        Some(entry) if entry.mode().is_blob_or_symlink() => Some(entry.object_id()),
        _ => None,
    })
}

/// A path's place in `git status`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StatusEntry {
    pub(crate) path: String,
    /// The HEAD blob, if the path is tracked there.
    pub(crate) head: Option<gix::ObjectId>,
    pub(crate) new: StatusNew,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StatusNew {
    /// The worktree file matches this index blob.
    Blob(gix::ObjectId),
    /// The worktree file differs from the index, or is untracked.
    Worktree,
    Missing,
}

/// A path whose blob differs between the base and HEAD trees.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TreeChange {
    pub(crate) path: String,
    pub(crate) old: Option<gix::ObjectId>,
    pub(crate) new: Option<gix::ObjectId>,
}

/// What a change's new side will be once the working tree is checked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Planned {
    Blob(gix::ObjectId),
    Worktree,
    Missing,
}

/// Joins the base→HEAD tree diff with the HEAD→worktree status into
/// base→worktree changes, sorted by path. A path in the tree diff takes its
/// old side from there; otherwise the base and HEAD agree on it and git's
/// HEAD blob is the old side. Paths that end where they started vanish.
pub(crate) fn assemble(
    tree: Vec<TreeChange>,
    status: Vec<StatusEntry>,
) -> Vec<(String, Option<gix::ObjectId>, Planned)> {
    let mut tree: std::collections::HashMap<String, TreeChange> =
        tree.into_iter().map(|t| (t.path.clone(), t)).collect();
    let mut out = Vec::with_capacity(tree.len() + status.len());
    for entry in status {
        let old = tree.remove(&entry.path).map_or(entry.head, |t| t.old);
        let new = match entry.new {
            StatusNew::Blob(id) => Planned::Blob(id),
            StatusNew::Worktree => Planned::Worktree,
            StatusNew::Missing => Planned::Missing,
        };
        out.push((entry.path, old, new));
    }
    out.extend(tree.into_values().map(|t| {
        let new = t.new.map_or(Planned::Missing, Planned::Blob);
        (t.path, t.old, new)
    }));
    out.retain(|(_, old, new)| match new {
        Planned::Blob(id) => *old != Some(*id),
        Planned::Missing => old.is_some(),
        Planned::Worktree => true,
    });
    out.sort_unstable_by(|a, b| a.0.cmp(&b.0));
    out
}

/// Finishes planned changes: stats working-tree files, skipping
/// directories (submodules, nested repositories).
fn stat_worktree(
    root: &Path,
    planned: Vec<(String, Option<gix::ObjectId>, Planned)>,
) -> Result<Vec<FileChange>> {
    let mut files = Vec::with_capacity(planned.len());
    for (path, old, planned) in planned {
        let new = match planned {
            Planned::Blob(id) => Side::Blob(id),
            Planned::Missing => Side::Missing,
            Planned::Worktree => match std::fs::symlink_metadata(root.join(&path)) {
                Ok(meta) if meta.is_file() || meta.file_type().is_symlink() => {
                    Side::Worktree(Stamp {
                        modified: meta.modified().ok(),
                        len: meta.len(),
                    })
                }
                Ok(_) => continue,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Side::Missing,
                Err(e) => return Err(e).with_context(|| format!("reading {path}")),
            },
        };
        let status = match (old, new) {
            (None, Side::Missing) => continue,
            (None, _) => Status::Added,
            (Some(_), Side::Missing) => Status::Deleted,
            (Some(_), _) => Status::Modified,
        };
        files.push(FileChange {
            path,
            status,
            old,
            new,
        });
    }
    Ok(files)
}

/// Content of both sides of a change, with the new side in Git's form.
#[derive(Debug)]
pub(crate) struct Contents {
    pub(crate) old: Vec<u8>,
    pub(crate) new: Vec<u8>,
    /// The new side's blob id: given, or hashed from the working-tree file.
    pub(crate) new_id: Option<gix::ObjectId>,
}

/// Reads blobs and working-tree files for one thread, converting files to
/// Git's form (line endings, `ident`, working-tree encodings) the way
/// `git add` would, so they compare with blobs line by line. Filter
/// drivers such as LFS are not run.
pub(crate) struct Reader {
    repo: gix::Repository,
    root: PathBuf,
    convert: Option<(gix::filter::plumbing::Pipeline, gix::worktree::Stack)>,
}

impl std::fmt::Debug for Reader {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Reader")
            .field("root", &self.root)
            .finish_non_exhaustive()
    }
}

impl Reader {
    pub(crate) fn new(repo: gix::Repository, root: PathBuf) -> Self {
        Self {
            repo,
            root,
            convert: None,
        }
    }

    pub(crate) fn blob(&self, id: gix::ObjectId) -> Result<Vec<u8>> {
        Ok(self.repo.find_blob(id)?.take_data())
    }

    pub(crate) fn contents(&mut self, change: &FileChange) -> Result<Contents> {
        let old = match change.old {
            Some(id) => self.blob(id)?,
            None => Vec::new(),
        };
        let (new, new_id) = match change.new {
            Side::Blob(id) => (self.blob(id)?, Some(id)),
            Side::Worktree(_) => {
                let data = self.worktree_file(&change.path)?;
                let id = gix::objs::compute_hash(
                    self.repo.object_hash(),
                    gix::object::Kind::Blob,
                    &data,
                )?;
                (data, Some(id))
            }
            Side::Missing => (Vec::new(), None),
        };
        Ok(Contents { old, new, new_id })
    }

    /// A working-tree file in Git's form, or a symlink's target. Missing
    /// files are empty.
    pub(crate) fn worktree_file(&mut self, path: &str) -> Result<Vec<u8>> {
        let full = self.root.join(path);
        let meta = match std::fs::symlink_metadata(&full) {
            Ok(meta) => meta,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e).with_context(|| format!("reading {}", full.display())),
        };
        if meta.file_type().is_symlink() {
            return Ok(std::fs::read_link(&full)?
                .into_os_string()
                .into_encoded_bytes());
        }
        if meta.len() > MAX_FILE {
            return Ok(b"\0(file too large to show)".to_vec());
        }
        let data = std::fs::read(&full).with_context(|| format!("reading {}", full.display()))?;
        self.git_form(path, data)
    }

    fn git_form(&mut self, path: &str, data: Vec<u8>) -> Result<Vec<u8>> {
        if self.convert.is_none() {
            let (pipeline, _) = self.repo.filter_pipeline(None)?;
            let (mut pipeline, stack) = pipeline.into_parts();
            pipeline.options_mut().drivers.clear();
            self.convert = Some((pipeline, stack));
        }
        let Some((pipeline, stack)) = self.convert.as_mut() else {
            unreachable!("the converter was just created")
        };
        let objects = &self.repo.objects;
        let entry = stack.at_path(Path::new(path), None, objects)?;
        let mut outcome = pipeline.convert_to_git(
            data.as_slice(),
            Path::new(path),
            &mut |_, attrs| {
                entry.matching_attributes(attrs);
            },
            &mut |_| Ok(None),
        )?;
        if !outcome.is_changed() {
            drop(outcome);
            return Ok(data);
        }
        if let Some(bytes) = outcome.as_bytes() {
            return Ok(bytes.to_vec());
        }
        let mut out = Vec::with_capacity(data.len());
        if let Some(read) = outcome.as_read() {
            read.read_to_end(&mut out)?;
        }
        Ok(out)
    }
}

/// `git status` entries and untracked directories, expanded through gix.
/// Read-only: no optional locks, so the index is never rewritten. Git's
/// untracked cache makes this an order of magnitude faster than a gix walk.
pub(crate) fn status(
    repo: &gix::Repository,
    worktree: &Path,
    fsmonitor: Fsmonitor,
    paths: &[String],
) -> Result<Vec<StatusEntry>> {
    let mut args: Vec<&str> = Vec::new();
    if fsmonitor == Fsmonitor::IfRunning && !fsmonitor_running(worktree) {
        args.extend(["-c", "core.fsmonitor=false"]);
    }
    args.extend([
        "status",
        "--porcelain=v2",
        "-z",
        "--untracked-files=normal",
        "--ignore-submodules=all",
        "--no-renames",
    ]);
    if !paths.is_empty() {
        args.push("--");
        args.extend(paths.iter().map(String::as_str));
    }
    let out = git(worktree, &args)?;
    let (mut entries, dirs) = parse_status(&out);
    if !dirs.is_empty() {
        entries.extend(
            untracked_files(repo, dirs)?
                .into_iter()
                .map(|path| StatusEntry {
                    path,
                    head: None,
                    new: StatusNew::Worktree,
                }),
        );
    }
    Ok(entries)
}

/// Whether an fsmonitor daemon already serves `worktree`. Asking does not
/// start one.
pub(crate) fn fsmonitor_running(worktree: &Path) -> bool {
    git(worktree, &["fsmonitor--daemon", "status"]).is_ok()
}

fn git(worktree: &Path, args: &[&str]) -> Result<Vec<u8>> {
    let mut command = std::process::Command::new("git");
    command
        .arg("-C")
        .arg(worktree)
        .arg("--no-optional-locks")
        .args(args);
    command.env("GIT_OPTIONAL_LOCKS", "0");
    command.env("GIT_LITERAL_PATHSPECS", "1");
    let output = dots_process::output(
        command,
        std::time::Duration::from_secs(60),
        256 * 1024 * 1024,
    )?;
    if !output.status.success() {
        anyhow::bail!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(output.stdout)
}

fn oid(hex: &str) -> Option<gix::ObjectId> {
    gix::ObjectId::from_hex(hex.as_bytes())
        .ok()
        .filter(|id| !id.is_null())
}

/// Reads `git status --porcelain=v2 -z`: entries with the blob ids git
/// prints, and untracked directories (listed with a trailing slash).
/// Submodules are skipped.
pub(crate) fn parse_status(out: &[u8]) -> (Vec<StatusEntry>, Vec<String>) {
    let mut entries = Vec::new();
    let mut dirs = Vec::new();
    for record in out.split(|&b| b == 0).filter(|r| !r.is_empty()) {
        let text = String::from_utf8_lossy(record);
        let fields: Vec<&str> = match text.as_bytes().first() {
            Some(b'1') => text.splitn(9, ' ').collect(),
            Some(b'u') => text.splitn(11, ' ').collect(),
            Some(b'?') => text.splitn(2, ' ').collect(),
            _ => continue,
        };
        let Some(&path) = fields.last() else { continue };
        let path = path.to_owned();
        match fields[0] {
            "?" => match path.strip_suffix('/') {
                Some(dir) => dirs.push(dir.to_owned()),
                None => entries.push(StatusEntry {
                    path,
                    head: None,
                    new: StatusNew::Worktree,
                }),
            },
            "1" if fields.len() == 9 => {
                // 1 XY sub mH mI mW hH hI path
                let (xy, sub) = (fields[1].as_bytes(), fields[2]);
                if !sub.starts_with('N') || fields[5] == "160000" {
                    continue;
                }
                let new = match xy.get(1) {
                    Some(b'.') => oid(fields[7]).map_or(StatusNew::Missing, StatusNew::Blob),
                    Some(b'D') => StatusNew::Missing,
                    _ => StatusNew::Worktree,
                };
                entries.push(StatusEntry {
                    path,
                    head: oid(fields[6]),
                    new,
                });
            }
            "u" if fields.len() == 11 => {
                // u XY sub m1 m2 m3 mW h1 h2 h3 path: stage 2 is ours (HEAD).
                entries.push(StatusEntry {
                    path,
                    head: oid(fields[8]),
                    new: StatusNew::Worktree,
                });
            }
            _ => {}
        }
    }
    (entries, dirs)
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

/// Blobs that differ between two trees. Subtrees with equal ids are not
/// entered, so this is cheap for related trees.
pub(crate) fn tree_changes(
    repo: &gix::Repository,
    old: gix::ObjectId,
    new: gix::ObjectId,
) -> Result<Vec<TreeChange>> {
    use gix::object::tree::diff::ChangeDetached as C;
    let (old, new) = (repo.find_tree(old)?, repo.find_tree(new)?);
    let mut options = gix::diff::Options::default();
    options.track_path();
    options.track_rewrites(None);
    let blob = |mode: gix::object::tree::EntryMode, id: gix::ObjectId| {
        mode.is_blob_or_symlink().then_some(id)
    };
    let mut out = Vec::new();
    for change in repo.diff_tree_to_tree(&old, &new, options)? {
        let (location, old, new) = match change {
            C::Addition {
                location,
                entry_mode,
                id,
                ..
            } => (location, None, blob(entry_mode, id)),
            C::Deletion {
                location,
                entry_mode,
                id,
                ..
            } => (location, blob(entry_mode, id), None),
            C::Modification {
                location,
                previous_entry_mode,
                previous_id,
                entry_mode,
                id,
            } => (
                location,
                blob(previous_entry_mode, previous_id),
                blob(entry_mode, id),
            ),
            C::Rewrite { .. } => continue,
        };
        if old.is_some() || new.is_some() {
            out.push(TreeChange {
                path: location.to_str_lossy().into_owned(),
                old,
                new,
            });
        }
    }
    Ok(out)
}

fn short(id: gix::ObjectId) -> String {
    id.to_hex_with_len(7).to_string()
}

/// The default branch's merge base with `head`, and the branch's name.
fn resolve_base(
    repo: &gix::Repository,
    head: gix::ObjectId,
) -> Result<Option<(String, gix::ObjectId)>> {
    let origin_head = repo
        .try_find_reference("refs/remotes/origin/HEAD")?
        .and_then(|r| {
            r.target()
                .try_name()
                .map(|n| n.as_bstr().to_str_lossy().into_owned())
        });
    let exists = |name: &str| repo.try_find_reference(name).ok().flatten().is_some();
    let mut bases = Vec::new();
    for name in base_candidates(origin_head.as_deref(), exists) {
        let Ok(mut reference) = repo.find_reference(name.as_str()) else {
            continue;
        };
        let Ok(tip) = reference.peel_to_id() else {
            continue;
        };
        if let Some(base) = repo.merge_base(head, tip.detach())? {
            bases.push((short_ref(&name), base.detach()));
        }
    }
    let is_ancestor = |a: gix::ObjectId, b: gix::ObjectId| {
        a == b
            || repo
                .merge_base(a, b)
                .ok()
                .flatten()
                .is_some_and(|m| m.detach() == a)
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
pub(crate) fn base_candidates(
    origin_head: Option<&str>,
    exists: impl Fn(&str) -> bool,
) -> Vec<String> {
    let names: Vec<String> = match origin_head.and_then(|t| t.strip_prefix("refs/remotes/origin/"))
    {
        Some(branch) => vec![
            format!("refs/heads/{branch}"),
            format!("refs/remotes/origin/{branch}"),
        ],
        None => ["main", "master"]
            .iter()
            .flat_map(|b| {
                [
                    format!("refs/heads/{b}"),
                    format!("refs/remotes/origin/{b}"),
                ]
            })
            .collect(),
    };
    let mut names: Vec<String> = names.into_iter().filter(|n| exists(n)).collect();
    if origin_head.is_none() {
        // Without origin/HEAD, the first branch name that exists decides.
        let branch = names
            .first()
            .map(|n| n.rsplit('/').next().unwrap_or("").to_owned());
        names.retain(|n| Some(n.rsplit('/').next().unwrap_or("").to_owned()) == branch);
    }
    names
}

/// The most recent of several merge bases: the one every other is an
/// ancestor of. A local branch behind its remote, or ahead of it, then
/// yields the fork point closest to the worktree's branch.
pub(crate) fn newest<T: Copy>(
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
        assert_eq!(
            base_candidates(None, |_| false),
            [] as [std::string::String; 0]
        );
    }

    #[test]
    fn picks_the_newest_merge_base() {
        // History 1 <- 2 <- 3: a larger number descends from a smaller one.
        let is_ancestor = |a: u32, b: u32| a <= b;
        let bases = vec![("main".to_owned(), 2), ("origin/main".to_owned(), 3)];
        assert_eq!(
            newest(bases, is_ancestor),
            Some(("origin/main".to_owned(), 3))
        );
        let bases = vec![("main".to_owned(), 3), ("origin/main".to_owned(), 1)];
        assert_eq!(newest(bases, is_ancestor), Some(("main".to_owned(), 3)));
        assert_eq!(newest::<u32>(vec![], is_ancestor), None);
    }

    #[test]
    fn parses_porcelain_v2_status() {
        let (a, b, c) = ("a".repeat(40), "b".repeat(40), "c".repeat(40));
        let zero = "0".repeat(40);
        let out = format!(
            "1 .M N... 100644 100644 100644 {a} {a} src/a b.rs\0\
             1 M. N... 100644 100644 100644 {a} {b} staged.rs\0\
             1 D. N... 100644 000000 000000 {a} {zero} gone.rs\0\
             1 .M S... 160000 160000 160000 {a} {a} sub\0\
             u UU N... 100644 100644 100644 100644 {a} {b} {c} conflict.txt\0\
             ? new.txt\0? fresh/dir/\0"
        );
        let (entries, dirs) = parse_status(out.as_bytes());
        let id = |hex: &str| gix::ObjectId::from_hex(hex.as_bytes()).ok();
        let entry = |path: &str, head: &str, new| StatusEntry {
            path: path.to_owned(),
            head: id(head),
            new,
        };
        assert_eq!(
            entries,
            vec![
                entry("src/a b.rs", &a, StatusNew::Worktree),
                entry("staged.rs", &a, StatusNew::Blob(id(&b).expect("hex"))),
                entry("gone.rs", &a, StatusNew::Missing),
                entry("conflict.txt", &b, StatusNew::Worktree),
                entry("new.txt", "", StatusNew::Worktree),
            ]
        );
        assert_eq!(dirs, vec!["fresh/dir"]);
    }

    #[test]
    fn assembles_base_to_worktree_changes() {
        let id = |n: u8| gix::ObjectId::from_bytes_or_panic(&[n; 20]);
        let tree = vec![
            // Committed on the branch, then reverted in the working tree.
            TreeChange {
                path: "reverted".into(),
                old: Some(id(1)),
                new: Some(id(2)),
            },
            // Committed only.
            TreeChange {
                path: "committed".into(),
                old: Some(id(3)),
                new: Some(id(4)),
            },
        ];
        let status = vec![
            StatusEntry {
                path: "reverted".into(),
                head: Some(id(2)),
                new: StatusNew::Blob(id(1)),
            },
            StatusEntry {
                path: "edited".into(),
                head: Some(id(5)),
                new: StatusNew::Worktree,
            },
        ];
        assert_eq!(
            assemble(tree, status),
            vec![
                ("committed".to_owned(), Some(id(3)), Planned::Blob(id(4))),
                ("edited".to_owned(), Some(id(5)), Planned::Worktree),
            ]
        );
    }
}
