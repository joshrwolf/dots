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

/// Stamps of the files whose change means a worktree's HEAD, index or the
/// refs its base is resolved from moved. Restatting them is cheap, so the
/// periodic check opens the repository only when they differ.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GitState(Vec<(PathBuf, Option<Stamp>)>);

impl GitState {
    pub(crate) fn of(repo: &gix::Repository) -> Self {
        let common = repo.common_dir();
        let mut paths = vec![
            repo.git_dir().join("HEAD"),
            repo.index_path(),
            common.join("packed-refs"),
            // Updating a loose ref renames a new file into its directory.
            common.join("refs/heads"),
        ];
        // Each remote's HEAD and branches are candidates for the base.
        if let Ok(remotes) = std::fs::read_dir(common.join("refs/remotes")) {
            paths.extend(remotes.flatten().map(|e| e.path()));
        }
        if let Ok(Some(name)) = repo.head_name() {
            paths.push(common.join(name.as_bstr().to_string()));
        }
        Self(paths.into_iter().map(|p| (p, None)).collect()).restat()
    }

    /// The same files, statted again.
    pub(crate) fn restat(&self) -> Self {
        Self(
            self.0
                .iter()
                .map(|(path, _)| (path.clone(), stamp(path)))
                .collect(),
        )
    }
}

fn stamp(path: &Path) -> Option<Stamp> {
    let m = std::fs::symlink_metadata(path).ok()?;
    Some(Stamp {
        modified: m.modified().ok(),
        len: m.len(),
    })
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

/// Whether two paths lie in worktrees of one repository.
pub(crate) fn same_repository(a: &Path, b: &Path) -> bool {
    let common = |p: &Path| {
        let dir = p.ancestors().find(|d| d.is_dir())?;
        let repo = gix::discover(dir).ok()?;
        repo.common_dir().canonicalize().ok()
    };
    match (common(a), common(b)) {
        (Some(a), Some(b)) => a == b,
        _ => false,
    }
}

/// The repository containing `path` and all its worktrees, main first.
/// `path` with symlinks resolved, or as given when it cannot be.
pub(crate) fn canonical(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_owned())
}

pub(crate) fn discover(path: &Path) -> Result<Repo> {
    let repo =
        gix::discover(path).with_context(|| format!("no Git repository at {}", path.display()))?;
    let main = repo.main_repo().context("opening the main worktree")?;
    let mut worktrees = Vec::new();
    if let Some(dir) = main.workdir() {
        worktrees.push(Worktree {
            path: canonical(dir),
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
    /// The base `dots_base` chooses for HEAD: everything the worktree's
    /// branch adds, committed or not.
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
    /// The base path, when the branch renamed the file.
    pub(crate) from: Option<String>,
    /// The file's modes, base and new, when they differ.
    pub(crate) modes: Option<(u32, u32)>,
}

impl FileChange {
    /// A mode change as git writes modes, such as `100644 → 100755`.
    pub(crate) fn mode_note(&self) -> Option<String> {
        self.modes.map(|(old, new)| format!("{old:o} → {new:o}"))
    }
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
    /// Taken before the scan, so later changes show as a difference.
    pub(crate) state: GitState,
}

/// The trees a worktree is compared through: the base and HEAD.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct Target {
    pub(crate) base_tree: gix::ObjectId,
    pub(crate) head_tree: gix::ObjectId,
}

/// Files above this size are listed but not read.
pub(crate) const MAX_FILE: u64 = 16 * 1024 * 1024;

/// Opens `worktree` and resolves what `mode` compares it against.
pub(crate) fn open(worktree: &Path, mode: Mode) -> Result<(gix::Repository, String, Target)> {
    let mut repo =
        gix::open(worktree).with_context(|| format!("opening {}", worktree.display()))?;
    repo.object_cache_size_if_unset(32 * 1024 * 1024);
    let head = repo.head_id().ok().map(gix::Id::detach);
    let (base_commit, label) = match (mode, head) {
        (Mode::Head, Some(head)) => (Some(head), format!("HEAD {}", short(head))),
        (Mode::Base, Some(head)) => {
            let (label, commit) = resolve_base(&repo, head)?;
            (Some(commit), label)
        }
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
pub(crate) fn changes(worktree: &Path, mode: Mode) -> Result<Changes> {
    let (repo, base, target) = open(worktree, mode)?;
    let state = GitState::of(&repo);
    let status = status(&repo, worktree, &[])?;
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
        state,
    })
}

/// The changes among `paths` alone, for a worktree whose base and HEAD are
/// still `target`: a pathspec-limited status and per-path tree lookups.
/// A path missing from the result has no change.
pub(crate) fn changes_for(
    repo: &gix::Repository,
    target: Target,
    paths: &[String],
    renames: &std::collections::HashMap<String, String>,
) -> Result<Vec<FileChange>> {
    let root = workdir(repo)?;
    let status = status(repo, &root, paths)?;
    let mut tree = Vec::new();
    if target.base_tree != target.head_tree {
        let base = repo.find_tree(target.base_tree)?;
        let head = repo.find_tree(target.head_tree)?;
        for path in paths {
            // A renamed file's base side is at its old path.
            let from = renames.get(path).cloned();
            let old = entry_at(&base, from.as_deref().unwrap_or(path))?;
            let new = entry_at(&head, path)?;
            if old != new || from.is_some() {
                let mode = |e: Option<(gix::ObjectId, u32)>| e.map_or(0, |(_, m)| m);
                tree.push(TreeChange {
                    path: path.clone(),
                    from,
                    old: old.map(|(id, _)| id),
                    new: new.map(|(id, _)| id),
                    modes: (mode(old), mode(new)),
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
    Ok(entry_at(tree, path)?.map(|(id, _)| id))
}

/// The blob at `path` and its mode, if it is a file or symlink.
fn entry_at(tree: &gix::Tree<'_>, path: &str) -> Result<Option<(gix::ObjectId, u32)>> {
    Ok(match tree.lookup_entry_by_path(path)? {
        Some(entry) if entry.mode().is_blob_or_symlink() => {
            Some((entry.object_id(), u32::from(entry.mode().value())))
        }
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
    /// File modes in HEAD and the working tree, 0 where absent or unknown.
    pub(crate) modes: (u32, u32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StatusNew {
    /// The worktree file matches this index blob.
    Blob(gix::ObjectId),
    /// The worktree file differs from the index, or is untracked.
    Worktree,
    Missing,
}

/// A path whose blob or mode differs between the base and HEAD trees.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct TreeChange {
    pub(crate) path: String,
    /// The base path of a renamed file.
    pub(crate) from: Option<String>,
    pub(crate) old: Option<gix::ObjectId>,
    pub(crate) new: Option<gix::ObjectId>,
    /// File modes in the base and HEAD, 0 where absent.
    pub(crate) modes: (u32, u32),
}

/// A base→worktree change before the working tree is checked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Assembled {
    pub(crate) path: String,
    pub(crate) from: Option<String>,
    pub(crate) old: Option<gix::ObjectId>,
    pub(crate) new: Planned,
    /// Base and new modes, 0 where absent or unknown.
    pub(crate) modes: (u32, u32),
}

impl Assembled {
    /// The modes, when both are known and differ.
    pub(crate) fn mode_change(&self) -> Option<(u32, u32)> {
        let (old, new) = self.modes;
        (old != 0 && new != 0 && old != new).then_some(self.modes)
    }
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
pub(crate) fn assemble(tree: Vec<TreeChange>, status: Vec<StatusEntry>) -> Vec<Assembled> {
    let mut tree: std::collections::HashMap<String, TreeChange> =
        tree.into_iter().map(|t| (t.path.clone(), t)).collect();
    let mut out = Vec::with_capacity(tree.len() + status.len());
    for entry in status {
        let committed = tree.remove(&entry.path);
        let new = match entry.new {
            StatusNew::Blob(id) => Planned::Blob(id),
            StatusNew::Worktree => Planned::Worktree,
            StatusNew::Missing => Planned::Missing,
        };
        let (from, old, base_mode) = match committed {
            Some(t) => (t.from, t.old, t.modes.0),
            None => (None, entry.head, entry.modes.0),
        };
        out.push(Assembled {
            path: entry.path,
            from,
            old,
            new,
            modes: (base_mode, entry.modes.1),
        });
    }
    out.extend(tree.into_values().map(|t| Assembled {
        new: t.new.map_or(Planned::Missing, Planned::Blob),
        path: t.path,
        from: t.from,
        old: t.old,
        modes: t.modes,
    }));
    out.retain(|a| {
        a.from.is_some()
            || a.mode_change().is_some()
            || match a.new {
                Planned::Blob(id) => a.old != Some(id),
                Planned::Missing => a.old.is_some(),
                Planned::Worktree => true,
            }
    });
    out.sort_unstable_by(|a, b| a.path.cmp(&b.path));
    out
}

/// Finishes planned changes: stats working-tree files, skipping
/// directories (submodules, nested repositories).
fn stat_worktree(root: &Path, planned: Vec<Assembled>) -> Result<Vec<FileChange>> {
    let mut files = Vec::with_capacity(planned.len());
    for planned in planned {
        let modes = planned.mode_change();
        let Assembled {
            path, from, old, ..
        } = planned;
        let new = match planned.new {
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
            from,
            modes,
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
    /// Texts of working-tree contents sent in a review, which have no blob.
    seen: PathBuf,
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
        let seen = crate::review::seen_dir(repo.git_dir());
        Self {
            repo,
            root,
            seen,
            convert: None,
        }
    }

    /// A blob's data, or the text kept for it when a review was sent.
    pub(crate) fn blob(&self, id: gix::ObjectId) -> Result<Vec<u8>> {
        match self.repo.find_blob(id) {
            Ok(mut blob) => Ok(blob.take_data()),
            Err(e) => std::fs::read(self.seen.join(id.to_string())).map_err(|_| e.into()),
        }
    }

    /// The blob at `path` in `tree`: `None` when there is no file.
    pub(crate) fn blob_in(&self, tree: gix::ObjectId, path: &str) -> Result<Option<gix::ObjectId>> {
        blob_at(&self.repo.find_tree(tree)?, path)
    }

    /// The content id of a change's new side: `None` for no file.
    pub(crate) fn content_id(&mut self, change: &FileChange) -> Result<Option<gix::ObjectId>> {
        Ok(match change.new {
            Side::Blob(id) => Some(id),
            Side::Missing => None,
            Side::Worktree(_) => {
                let data = self.worktree_file(&change.path)?;
                Some(gix::objs::compute_hash(
                    self.repo.object_hash(),
                    gix::object::Kind::Blob,
                    &data,
                )?)
            }
        })
    }

    /// Keeps the working-tree text of `change` for diffing against later,
    /// unless Git has it. Returns its content id.
    pub(crate) fn keep(&mut self, change: &FileChange) -> Result<Option<gix::ObjectId>> {
        let Side::Worktree(_) = change.new else {
            return self.content_id(change);
        };
        let data = self.worktree_file(&change.path)?;
        let id = gix::objs::compute_hash(self.repo.object_hash(), gix::object::Kind::Blob, &data)?;
        let path = self.seen.join(id.to_string());
        if !self.repo.has_object(id) && !path.exists() {
            std::fs::create_dir_all(&self.seen)
                .with_context(|| format!("creating {}", self.seen.display()))?;
            let tmp = path.with_extension("tmp");
            std::fs::write(&tmp, &data).with_context(|| format!("writing {}", tmp.display()))?;
            std::fs::rename(&tmp, &path).with_context(|| format!("writing {}", path.display()))?;
        }
        Ok(Some(id))
    }

    /// Removes kept texts no snapshot refers to.
    pub(crate) fn prune_kept(&self, keep: &std::collections::BTreeMap<String, String>) {
        let wanted: std::collections::HashSet<&str> = keep.values().map(String::as_str).collect();
        let Ok(entries) = std::fs::read_dir(&self.seen) else {
            return;
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            if !wanted.contains(name.to_string_lossy().as_ref()) {
                let _ = std::fs::remove_file(entry.path());
            }
        }
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
/// The fsmonitor stays off: without optional locks Git cannot record what
/// its daemon reported, and the untracked cache it then trusts can miss new
/// files.
pub(crate) fn status(
    repo: &gix::Repository,
    worktree: &Path,
    paths: &[String],
) -> Result<Vec<StatusEntry>> {
    let mut args = vec!["-c", "core.fsmonitor=false"];
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
                    modes: (0, 0),
                }),
        );
    }
    Ok(entries)
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

/// A file mode as git prints it, in octal; 0 when absent.
fn mode(octal: &str) -> u32 {
    u32::from_str_radix(octal, 8).unwrap_or(0)
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
                    modes: (0, 0),
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
                    modes: (mode(fields[3]), mode(fields[5])),
                });
            }
            "u" if fields.len() == 11 => {
                // u XY sub m1 m2 m3 mW h1 h2 h3 path: stage 2 is ours (HEAD).
                entries.push(StatusEntry {
                    path,
                    head: oid(fields[8]),
                    new: StatusNew::Worktree,
                    modes: (mode(fields[4]), mode(fields[6])),
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

/// Blobs and modes that differ between two trees, with renames. Subtrees with equal
/// ids are not entered, so this is cheap for related trees.
pub(crate) fn tree_changes(
    repo: &gix::Repository,
    old: gix::ObjectId,
    new: gix::ObjectId,
) -> Result<Vec<TreeChange>> {
    use gix::object::tree::diff::ChangeDetached as C;
    let (old, new) = (repo.find_tree(old)?, repo.find_tree(new)?);
    let mut options = gix::diff::Options::default();
    options.track_path();
    // Similar-content renames are tried only among a few hundred added and
    // deleted files; past that, git-style pairing costs hundreds of
    // milliseconds and only identical-content renames are found.
    options.track_rewrites(Some(gix::diff::Rewrites {
        limit: 200,
        ..gix::diff::Rewrites::default()
    }));
    let blob = |mode: gix::object::tree::EntryMode, id: gix::ObjectId| {
        mode.is_blob_or_symlink().then_some(id)
    };
    let mode = |mode: gix::object::tree::EntryMode| {
        if mode.is_blob_or_symlink() {
            u32::from(mode.value())
        } else {
            0
        }
    };
    let mut out = Vec::new();
    for change in repo.diff_tree_to_tree(&old, &new, options)? {
        let change = match change {
            C::Addition {
                location,
                entry_mode,
                id,
                ..
            } => TreeChange {
                path: location.to_str_lossy().into_owned(),
                new: blob(entry_mode, id),
                modes: (0, mode(entry_mode)),
                ..TreeChange::default()
            },
            C::Deletion {
                location,
                entry_mode,
                id,
                ..
            } => TreeChange {
                path: location.to_str_lossy().into_owned(),
                old: blob(entry_mode, id),
                modes: (mode(entry_mode), 0),
                ..TreeChange::default()
            },
            C::Modification {
                location,
                previous_entry_mode,
                previous_id,
                entry_mode,
                id,
            } => TreeChange {
                path: location.to_str_lossy().into_owned(),
                old: blob(previous_entry_mode, previous_id),
                new: blob(entry_mode, id),
                modes: (mode(previous_entry_mode), mode(entry_mode)),
                ..TreeChange::default()
            },
            C::Rewrite {
                source_location,
                source_entry_mode,
                source_id,
                entry_mode,
                id,
                location,
                copy,
                ..
            } => TreeChange {
                path: location.to_str_lossy().into_owned(),
                // A copy leaves its source in place: the new path is added.
                from: (!copy).then(|| source_location.to_str_lossy().into_owned()),
                old: (!copy)
                    .then(|| blob(source_entry_mode, source_id))
                    .flatten(),
                new: blob(entry_mode, id),
                modes: (
                    if copy { 0 } else { mode(source_entry_mode) },
                    mode(entry_mode),
                ),
            },
        };
        if change.old.is_some() || change.new.is_some() {
            out.push(change);
        }
    }
    Ok(out)
}

fn short(id: gix::ObjectId) -> String {
    id.to_hex_with_len(7).to_string()
}

/// The base `head` compares against by the rule `dots review` shares: its
/// commit and a label naming it, such as `origin/main 1a2b3c4`.
fn resolve_base(repo: &gix::Repository, head: gix::ObjectId) -> Result<(String, gix::ObjectId)> {
    let branch = repo.head_name().ok().flatten();
    let refs = dots_base::Refs::new(base_records(repo, branch.as_ref())?);
    let short_branch = branch.as_ref().map(|b| b.shorten().to_string());
    let chosen = refs.choose(
        &History(repo),
        &head.to_string(),
        short_branch.as_deref(),
        dots_base::Measure::Nearest,
    )?;
    let base = gix::ObjectId::from_hex(chosen.base.as_bytes())?;
    let label = match (chosen.reason, chosen.candidates.first()) {
        (dots_base::Reason::NoSharedHistory, Some(nearest)) => format!(
            "HEAD {}, no history shared with {}",
            short(base),
            nearest.reference
        ),
        _ => format!("{} {}", chosen.reference, short(base)),
    };
    Ok((label, base))
}

/// The local `main` and `master`, each remote's `HEAD`, and the checked-out
/// `branch`: every ref the base rule reads for it, each local branch with
/// its upstream.
fn base_records(
    repo: &gix::Repository,
    branch: Option<&gix::refs::FullName>,
) -> Result<Vec<dots_base::Record>> {
    let text = |n: &gix::refs::FullNameRef| n.as_bstr().to_str_lossy().into_owned();
    let commit = |r: &mut gix::Reference<'_>| r.peel_to_id().ok().map(|id| id.to_string());
    let mut records = Vec::new();
    let mut locals: Vec<gix::refs::FullName> = Vec::new();
    for name in ["refs/heads/main", "refs/heads/master"] {
        if let Some(r) = repo.try_find_reference(name)? {
            locals.push(r.name().to_owned());
        }
    }
    if let Some(branch) = branch
        && !locals.contains(branch)
    {
        locals.push(branch.clone());
    }
    for branch in &locals {
        let remote_ref = repo
            .branch_remote_ref_name(branch.as_ref(), gix::remote::Direction::Fetch)
            .and_then(Result::ok);
        let local = repo
            .config_snapshot()
            .string(format!("branch.{}.remote", branch.shorten()).as_str())
            .is_some_and(|r| r.as_slice() == b".");
        let upstream = if local {
            remote_ref.clone()
        } else {
            repo.branch_remote_tracking_ref_name(branch.as_ref(), gix::remote::Direction::Fetch)
                .and_then(Result::ok)
        };
        records.push(dots_base::Record {
            name: text(branch.as_ref()),
            commit: repo
                .try_find_reference(branch.as_ref())?
                .and_then(|mut r| commit(&mut r)),
            upstream: upstream.as_ref().map(|n| text(n.as_ref())),
            remote_ref: remote_ref.as_ref().map(|n| text(n.as_ref())),
            ..dots_base::Record::default()
        });
    }
    for remote in repo.remote_names() {
        let name = format!("refs/remotes/{remote}/HEAD");
        let Some(mut r) = repo.try_find_reference(name.as_str())? else {
            continue;
        };
        let symref = r.target().try_name().map(text);
        records.push(dots_base::Record {
            name,
            symref,
            commit: commit(&mut r),
            ..dots_base::Record::default()
        });
    }
    Ok(records)
}

/// The base rule's questions, answered by gix.
struct History<'r>(&'r gix::Repository);

impl History<'_> {
    fn id(hex: &str) -> Result<gix::ObjectId> {
        Ok(gix::ObjectId::from_hex(hex.as_bytes())?)
    }
}

/// How many commits `to` reaches that `from` does not, or `None` past
/// `at_most`. Both sides are walked together, newest first, so a count
/// gives up after `at_most` commits only `to` reaches, however long the
/// history behind either side; gix's hidden walks paint the whole
/// difference before yielding anything.
fn count_beyond(
    repo: &gix::Repository,
    from: gix::ObjectId,
    to: gix::ObjectId,
    at_most: Option<u64>,
) -> Result<Option<u64>> {
    let cache = repo.commit_graph_if_enabled()?;
    let mut walk = Paint {
        graph: repo.revision_graph(cache.as_ref()),
        queue: std::collections::BinaryHeap::new(),
        queued: 0,
        visible_only: 0,
        counted: std::collections::BTreeMap::new(),
        total: 0,
    };
    walk.mark(to, Paint::VISIBLE)?;
    walk.mark(from, Paint::HIDDEN)?;
    // Once only commits both sides reach are queued, the hidden side may
    // still reach commits counted before it: walk on while any queued
    // commit could be their descendant.
    while let Some(&(generation, time, _, id)) = walk.queue.peek() {
        let top = (generation, time);
        if walk.visible_only == 0 && walk.counted.first_key_value().is_none_or(|(o, _)| top < *o) {
            break;
        }
        // A counted commit newer than every queued one keeps its count.
        if let Some(m) = at_most
            && walk.total > m
            && walk.settled(top) > m
        {
            return Ok(None);
        }
        walk.queue.pop();
        let commit = walk
            .graph
            .get_mut(&id)
            .context("a queued commit is in the graph")?;
        commit.data &= !Paint::QUEUED;
        let flags = commit.data & Paint::SIDES;
        if flags == Paint::VISIBLE {
            walk.visible_only -= 1;
            if commit.data & Paint::COUNTED == 0 {
                commit.data |= Paint::COUNTED;
                let key = Paint::key(commit);
                *walk.counted.entry(key).or_default() += 1;
                walk.total += 1;
            }
        }
        let parents = commit.parents.clone();
        for parent in parents {
            walk.mark(parent, flags)?;
        }
    }
    if at_most.is_some_and(|m| walk.total > m) {
        return Ok(None);
    }
    Ok(Some(walk.total))
}

/// Commits painted by the sides that reach them, queued newest first: by
/// generation, which commits outside the commit-graph lack and none of
/// their ancestors in it exceeds, then commit time, then first queued, so
/// children usually come before parents of the same second. A walked
/// commit that a side reaches later is walked again.
struct Paint<'r, 'c> {
    graph: gix::revwalk::Graph<'r, 'c, gix::revwalk::graph::Commit<u8>>,
    queue: std::collections::BinaryHeap<(u32, i64, std::cmp::Reverse<u64>, gix::ObjectId)>,
    /// Commits queued so far, to order commits of the same second.
    queued: u64,
    /// Queued commits only `to` reaches; with none left the count is done.
    visible_only: u64,
    /// Walked commits only `to` reaches, by queue key.
    counted: std::collections::BTreeMap<(u32, i64), u64>,
    /// How many there are.
    total: u64,
}

impl Paint<'_, '_> {
    const VISIBLE: u8 = 1;
    const HIDDEN: u8 = 2;
    const SIDES: u8 = Self::VISIBLE | Self::HIDDEN;
    const QUEUED: u8 = 4;
    const COUNTED: u8 = 8;

    fn key(commit: &gix::revwalk::graph::Commit<u8>) -> (u32, i64) {
        (commit.generation.unwrap_or(u32::MAX), commit.commit_time)
    }

    /// Counted commits newer than `top`, which no queued commit reaches.
    fn settled(&self, top: (u32, i64)) -> u64 {
        use std::ops::Bound::{Excluded, Unbounded};
        self.counted
            .range((Excluded(top), Unbounded))
            .map(|(_, n)| n)
            .sum()
    }

    /// Paints `id` with the sides in `flags`, queueing it when it gains one.
    fn mark(&mut self, id: gix::ObjectId, flags: u8) -> Result<()> {
        let before = self.graph.get(&id).map(|c| c.data);
        if before.is_some_and(|b| b & flags == flags) {
            return Ok(());
        }
        let before = before.unwrap_or(0);
        let commit = self
            .graph
            .get_or_insert_full_commit(id, |c| c.data |= flags)?
            .with_context(|| format!("commit {id} is missing"))?;
        if commit.data & Self::HIDDEN != 0 && commit.data & Self::COUNTED != 0 {
            commit.data &= !Self::COUNTED;
            let key = Self::key(commit);
            if let Some(n) = self.counted.get_mut(&key) {
                *n -= 1;
                if *n == 0 {
                    self.counted.remove(&key);
                }
            }
            self.total -= 1;
        }
        let only = |data: u8| data & Self::SIDES == Self::VISIBLE;
        if before & Self::QUEUED == 0 {
            commit.data |= Self::QUEUED;
            let generation = commit.generation.unwrap_or(u32::MAX);
            self.queued += 1;
            let order = std::cmp::Reverse(self.queued);
            self.queue.push((generation, commit.commit_time, order, id));
            if only(commit.data) {
                self.visible_only += 1;
            }
        } else if only(before) && !only(commit.data) {
            self.visible_only -= 1;
        }
        Ok(())
    }
}

impl dots_base::History for History<'_> {
    type Error = anyhow::Error;

    fn is_ancestor(&self, ancestor: &str, descendant: &str) -> Result<bool> {
        let (a, d) = (Self::id(ancestor)?, Self::id(descendant)?);
        Ok(a == d || self.0.merge_base(a, d)?.is_some_and(|m| m.detach() == a))
    }

    fn commits_between(&self, from: &str, to: &str, at_most: Option<u64>) -> Result<Option<u64>> {
        count_beyond(self.0, Self::id(from)?, Self::id(to)?, at_most)
    }

    fn merge_base(&self, left: &str, right: &str) -> Result<Option<String>> {
        let base = self.0.merge_base(Self::id(left)?, Self::id(right)?)?;
        Ok(base.map(|b| b.to_string()))
    }

    fn commit(&self, reference: &str) -> Result<Option<String>> {
        Ok(self
            .0
            .try_find_reference(reference)?
            .and_then(|mut r| r.peel_to_id().ok())
            .map(|id| id.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Runs Git in `dir` without the user's configuration; its output.
    fn git_in(dir: &Path, args: &[&str]) -> String {
        let out = std::process::Command::new("git")
            .args(["-c", "user.name=t", "-c", "user.email=t@t"])
            .args(["-c", "commit.gpgsign=false"])
            .args(args)
            .current_dir(dir)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .expect("git");
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_owned()
    }

    /// The base `dir`'s HEAD compares against: its name and commit.
    fn base_of(dir: &Path) -> (String, String) {
        let repo = gix::open(dir).expect("open");
        let head = repo.head_id().expect("head").detach();
        let (label, commit) = resolve_base(&repo, head).expect("base");
        let name = label.split(' ').next().unwrap_or_default().to_owned();
        (name, commit.to_string())
    }

    #[test]
    fn a_default_named_branch_compares_with_its_differently_named_upstream() {
        let dir = std::env::temp_dir().join(format!("dots-diff-trunk-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("dir");
        let git = |args: &[&str]| git_in(&dir, args);
        git(&["init", "-q", "-b", "master"]);
        git(&["commit", "-q", "--allow-empty", "-m", "m1"]);
        let m1 = git(&["rev-parse", "HEAD"]);
        git(&["remote", "add", "origin", "https://example.invalid/r.git"]);
        git(&["update-ref", "refs/remotes/origin/main", "HEAD"]);
        git(&[
            "symbolic-ref",
            "refs/remotes/origin/HEAD",
            "refs/remotes/origin/main",
        ]);
        git(&["branch", "-q", "--set-upstream-to", "origin/main"]);
        // Local master is one commit ahead of what it tracks.
        git(&["commit", "-q", "--allow-empty", "-m", "m2"]);
        assert_eq!(base_of(&dir), ("origin/main".to_owned(), m1));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_orphan_branch_loads_against_head_and_says_why() {
        let dir = std::env::temp_dir().join(format!("dots-diff-orphan-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("dir");
        let git = |args: &[&str]| git_in(&dir, args);
        git(&["init", "-q", "-b", "main"]);
        git(&["commit", "-q", "--allow-empty", "-m", "m1"]);
        git(&["switch", "-q", "--orphan", "pages"]);
        std::fs::write(dir.join("index.html"), "one\n").expect("write");
        git(&["add", "."]);
        git(&["commit", "-q", "-m", "p1"]);
        std::fs::write(dir.join("index.html"), "two\n").expect("write");
        let changes = changes(&dir, Mode::Base).expect("an orphan branch loads");
        let paths: Vec<&str> = changes.files.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(paths, ["index.html"], "its uncommitted changes");
        assert!(
            changes.base.contains("no history shared with main"),
            "{}",
            changes.base
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn counting_commits_beyond_a_base_matches_git_and_stops_at_its_limit() {
        let dir = std::env::temp_dir().join(format!("dots-diff-count-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("dir");
        let git = |args: &[&str]| git_in(&dir, args);
        git(&["init", "-q", "-b", "main"]);
        for i in 0..5 {
            git(&["commit", "-q", "--allow-empty", "-m", &format!("m{i}")]);
        }
        // side forks from main~3 and merges main back in; main moves on.
        git(&["switch", "-q", "-c", "side", "main~3"]);
        git(&["commit", "-q", "--allow-empty", "-m", "s1"]);
        git(&["merge", "-q", "--no-ff", "-m", "merge", "main"]);
        git(&["commit", "-q", "--allow-empty", "-m", "s2"]);
        git(&["switch", "-q", "main"]);
        git(&["commit", "-q", "--allow-empty", "-m", "m5"]);
        git(&["switch", "-q", "--orphan", "pages"]);
        git(&["commit", "-q", "--allow-empty", "-m", "p1"]);
        let repo = gix::open(&dir).expect("open");
        let id = |r: &str| gix::ObjectId::from_hex(git(&["rev-parse", r]).as_bytes()).expect("id");
        for (from, to) in [
            ("main", "side"),
            ("side", "main"),
            ("main~3", "side"),
            ("side", "side"),
            ("pages", "side"),
            ("main", "pages"),
        ] {
            let want: u64 = git(&["rev-list", "--count", &format!("{from}..{to}")])
                .parse()
                .expect("count");
            let got = count_beyond(&repo, id(from), id(to), None).expect("count");
            assert_eq!(got, Some(want), "{from}..{to}");
            let limited = count_beyond(&repo, id(from), id(to), Some(want)).expect("count");
            assert_eq!(limited, Some(want), "{from}..{to} within its limit");
            if want > 0 {
                let under = count_beyond(&repo, id(from), id(to), Some(want - 1)).expect("count");
                assert_eq!(under, None, "{from}..{to} past its limit");
            }
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_base_follows_the_shared_rule() {
        let dir = std::env::temp_dir().join(format!("dots-diff-base-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("dir");
        let git = |args: &[&str]| git_in(&dir, args);
        git(&["init", "-q", "-b", "main"]);
        git(&["commit", "-q", "--allow-empty", "-m", "m1"]);
        // origin/main moves ahead of the local main, which stays behind.
        git(&["commit", "-q", "--allow-empty", "-m", "m2"]);
        let m2 = git(&["rev-parse", "HEAD"]);
        git(&["update-ref", "refs/remotes/origin/main", "HEAD"]);
        git(&[
            "symbolic-ref",
            "refs/remotes/origin/HEAD",
            "refs/remotes/origin/main",
        ]);
        git(&["remote", "add", "origin", "https://example.invalid/r.git"]);
        git(&["reset", "-q", "--hard", "HEAD~1"]);
        git(&["switch", "-q", "-c", "feature", "origin/main"]);
        git(&["commit", "-q", "--allow-empty", "-m", "f1"]);
        let f1 = git(&["rev-parse", "HEAD"]);
        let base = base_of;
        assert_eq!(
            base(&dir),
            ("origin/main".to_owned(), m2),
            "the nearer default"
        );
        // A branch stacked on feature compares with it.
        git(&["switch", "-q", "-c", "stacked", "--track", "feature"]);
        git(&["commit", "-q", "--allow-empty", "-m", "s1"]);
        assert_eq!(base(&dir), ("feature".to_owned(), f1));
        // On the default branch: HEAD itself.
        git(&["switch", "-q", "main"]);
        let head = git(&["rev-parse", "HEAD"]);
        assert_eq!(base(&dir), ("HEAD".to_owned(), head));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn worktree_files_are_compared_in_git_form() {
        let dir = std::env::temp_dir().join(format!("dots-diff-eol-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let repo = gix::init(&dir).expect("init");
        std::fs::write(
            dir.join(".gitattributes"),
            "*.txt text eol=crlf\n*.bin -text\n",
        )
        .expect("write");
        std::fs::write(dir.join("a.txt"), "one\r\ntwo\r\n").expect("write");
        std::fs::write(dir.join("b.bin"), "one\r\n").expect("write");
        let mut reader = Reader::new(repo, dir.clone());
        // Checked-out CRLF text is LF in the repository, so it matches the blob.
        assert_eq!(reader.worktree_file("a.txt").expect("read"), b"one\ntwo\n");
        // Unconverted files keep their bytes.
        assert_eq!(reader.worktree_file("b.bin").expect("read"), b"one\r\n");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn parses_porcelain_v2_status() {
        let (a, b, c) = ("a".repeat(40), "b".repeat(40), "c".repeat(40));
        let zero = "0".repeat(40);
        let out = format!(
            "1 .M N... 100644 100644 100644 {a} {a} src/a b.rs\0\
             1 M. N... 100644 100644 100755 {a} {b} staged.rs\0\
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
            modes: (0o100_644, 0o100_644),
        };
        assert_eq!(
            entries,
            vec![
                entry("src/a b.rs", &a, StatusNew::Worktree),
                StatusEntry {
                    modes: (0o100_644, 0o100_755),
                    ..entry("staged.rs", &a, StatusNew::Blob(id(&b).expect("hex")))
                },
                StatusEntry {
                    modes: (0o100_644, 0),
                    ..entry("gone.rs", &a, StatusNew::Missing)
                },
                entry("conflict.txt", &b, StatusNew::Worktree),
                StatusEntry {
                    modes: (0, 0),
                    ..entry("new.txt", "", StatusNew::Worktree)
                },
            ]
        );
        assert_eq!(dirs, vec!["fresh/dir"]);
    }

    #[test]
    fn assembles_base_to_worktree_changes() {
        const FILE: u32 = 0o100_644;
        const EXEC: u32 = 0o100_755;
        let id = |n: u8| gix::ObjectId::from_bytes_or_panic(&[n; 20]);
        let tree = vec![
            // Committed on the branch, then reverted in the working tree.
            TreeChange {
                path: "reverted".into(),
                old: Some(id(1)),
                new: Some(id(2)),
                modes: (FILE, FILE),
                ..TreeChange::default()
            },
            // Committed only.
            TreeChange {
                path: "committed".into(),
                old: Some(id(3)),
                new: Some(id(4)),
                modes: (FILE, FILE),
                ..TreeChange::default()
            },
            // Renamed without changes.
            TreeChange {
                path: "moved".into(),
                from: Some("origin".into()),
                old: Some(id(6)),
                new: Some(id(6)),
                modes: (FILE, FILE),
            },
            // Made executable on the branch.
            TreeChange {
                path: "script".into(),
                old: Some(id(7)),
                new: Some(id(7)),
                modes: (FILE, EXEC),
                ..TreeChange::default()
            },
        ];
        let status = vec![
            StatusEntry {
                path: "reverted".into(),
                head: Some(id(2)),
                new: StatusNew::Blob(id(1)),
                modes: (FILE, FILE),
            },
            StatusEntry {
                path: "edited".into(),
                head: Some(id(5)),
                new: StatusNew::Worktree,
                modes: (FILE, FILE),
            },
            // Made executable in the working tree alone.
            StatusEntry {
                path: "tool".into(),
                head: Some(id(8)),
                new: StatusNew::Blob(id(8)),
                modes: (FILE, EXEC),
            },
        ];
        let change = |path: &str, from: Option<&str>, old: u8, new, modes| Assembled {
            path: path.to_owned(),
            from: from.map(str::to_owned),
            old: Some(id(old)),
            new,
            modes,
        };
        assert_eq!(
            assemble(tree, status),
            vec![
                change("committed", None, 3, Planned::Blob(id(4)), (FILE, FILE)),
                change("edited", None, 5, Planned::Worktree, (FILE, FILE)),
                change(
                    "moved",
                    Some("origin"),
                    6,
                    Planned::Blob(id(6)),
                    (FILE, FILE)
                ),
                change("script", None, 7, Planned::Blob(id(7)), (FILE, EXEC)),
                change("tool", None, 8, Planned::Blob(id(8)), (FILE, EXEC)),
            ]
        );
    }

    #[test]
    fn tree_changes_report_renames_and_modes() {
        use gix::object::tree::EntryKind;
        let dir = std::env::temp_dir().join(format!("dots-diff-renames-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let repo = gix::init_bare(&dir).expect("init");
        let text = "one\ntwo\nthree\nfour\nfive\n";
        let notes = repo.write_blob(text).expect("blob").detach();
        let script = repo.write_blob("echo hi\n").expect("blob").detach();
        let empty = repo.empty_tree().id;
        let mut base = repo.edit_tree(empty).expect("editor");
        base.upsert("notes.md", EntryKind::Blob, notes)
            .expect("upsert");
        base.upsert("build.sh", EntryKind::Blob, script)
            .expect("upsert");
        let base = base.write().expect("write").detach();
        let mut head = repo.edit_tree(empty).expect("editor");
        head.upsert("docs/notes.md", EntryKind::Blob, notes)
            .expect("upsert");
        head.upsert("build.sh", EntryKind::BlobExecutable, script)
            .expect("upsert");
        let head = head.write().expect("write").detach();
        let mut changes = tree_changes(&repo, base, head).expect("diff");
        changes.sort_by(|a, b| a.path.cmp(&b.path));
        assert_eq!(
            changes,
            vec![
                TreeChange {
                    path: "build.sh".into(),
                    from: None,
                    old: Some(script),
                    new: Some(script),
                    modes: (0o100_644, 0o100_755),
                },
                TreeChange {
                    path: "docs/notes.md".into(),
                    from: Some("notes.md".into()),
                    old: Some(notes),
                    new: Some(notes),
                    modes: (0o100_644, 0o100_644),
                },
            ]
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
