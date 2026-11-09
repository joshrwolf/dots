//! The Git queries reviews depend on, each bounded in time and output.
use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use crate::{Error, Result};

const DEADLINE: Duration = Duration::from_secs(60);
const SMALL: usize = 1024 * 1024;
/// Diffs larger than this are refused rather than reviewed partially.
pub(crate) const DIFF_LIMIT: usize = 32 * 1024 * 1024;
/// Most bytes of paths passed to one Git command as a pathspec.
const PATHSPEC_BYTES: usize = 256 * 1024;

/// One checkout: the top level of its working tree and its own Git directory.
#[derive(Debug)]
pub(crate) struct Git {
    pub(crate) toplevel: PathBuf,
    pub(crate) own_dir: PathBuf,
    /// Shared by every worktree of the repository.
    pub(crate) shared_dir: PathBuf,
}

impl Git {
    pub(crate) fn discover(directory: &Path) -> Result<Self> {
        let output = run(
            directory,
            &[
                "rev-parse",
                "--path-format=absolute",
                "--show-toplevel",
                "--git-dir",
                "--git-common-dir",
            ],
            &[],
            SMALL,
            "resolve checkout",
        )?;
        let text = utf8(output, "checkout paths")?;
        let mut lines = text.lines();
        match (lines.next(), lines.next(), lines.next()) {
            (Some(toplevel), Some(own_dir), Some(common_dir)) => Ok(Self {
                toplevel: PathBuf::from(toplevel),
                own_dir: PathBuf::from(own_dir),
                shared_dir: PathBuf::from(common_dir),
            }),
            _ => Err(Error::Git {
                operation: "resolve checkout",
                message: "rev-parse returned no working tree; reviews need a checkout".into(),
            }),
        }
    }

    /// The canonical common Git directory of the repository containing
    /// `directory`, which may be a Git directory itself.
    pub(crate) fn common_dir(directory: &Path) -> Result<PathBuf> {
        let output = run(
            directory,
            &["rev-parse", "--path-format=absolute", "--git-common-dir"],
            &[],
            SMALL,
            "resolve repository",
        )?;
        let common = PathBuf::from(utf8(output, "repository path")?.trim_end_matches('\n'));
        std::fs::canonicalize(&common).map_err(|source| Error::Io {
            operation: "resolve repository",
            path: common,
            source,
        })
    }

    /// The worktrees of the repository whose common Git directory is
    /// `common`, the primary one first; bare and prunable entries are left
    /// out.
    pub(crate) fn worktrees(common: &Path) -> Result<Vec<Listed>> {
        let output = run(
            common,
            &["worktree", "list", "--porcelain", "-z"],
            &[],
            DIFF_LIMIT,
            "list worktrees",
        )?;
        let text = utf8(output, "worktree list")?;
        let mut listed = Vec::new();
        let mut current: Option<(Listed, bool)> = None;
        for field in text.split('\0') {
            if field.is_empty() {
                if let Some((entry, true)) = current.take() {
                    listed.push(entry);
                }
                continue;
            }
            let (key, value) = field.split_once(' ').unwrap_or((field, ""));
            if key == "worktree" {
                if let Some((entry, true)) = current.take() {
                    listed.push(entry);
                }
                let entry = Listed {
                    path: PathBuf::from(value),
                    head: None,
                    branch: None,
                };
                current = Some((entry, true));
                continue;
            }
            let Some((entry, usable)) = current.as_mut() else {
                continue;
            };
            match key {
                // Git's all-zero id: a branch with no commits yet.
                "HEAD" if !value.bytes().all(|byte| byte == b'0') => {
                    entry.head = Some(value.to_owned());
                }
                "branch" => {
                    entry.branch = Some(
                        value
                            .strip_prefix("refs/heads/")
                            .unwrap_or(value)
                            .to_owned(),
                    );
                }
                "bare" | "prunable" => *usable = false,
                _ => {}
            }
        }
        if let Some((entry, true)) = current {
            listed.push(entry);
        }
        Ok(listed)
    }

    /// The checkout whose working tree is `toplevel`, without asking Git: its
    /// own Git directory is `toplevel/.git`, or the one a linked worktree's
    /// `.git` file names.
    pub(crate) fn checkout(toplevel: &Path, shared_dir: &Path) -> Result<Self> {
        let dot_git = toplevel.join(".git");
        let own_dir = if dot_git.is_dir() {
            dot_git
        } else {
            let text = std::fs::read_to_string(&dot_git).map_err(|source| Error::Io {
                operation: "read worktree link",
                path: dot_git.clone(),
                source,
            })?;
            let named = text
                .strip_prefix("gitdir: ")
                .map(str::trim_end)
                .ok_or_else(|| Error::Git {
                    operation: "read worktree link",
                    message: format!("{} does not name a Git directory", dot_git.display()),
                })?;
            toplevel.join(named)
        };
        Ok(Self {
            toplevel: toplevel.to_owned(),
            own_dir,
            shared_dir: shared_dir.to_owned(),
        })
    }

    /// The object id `spec` names, peeled to `kind` (`commit` or `tree`).
    pub(crate) fn object(&self, spec: &str, kind: &str) -> Result<String> {
        if spec.starts_with('-') {
            return Err(Error::Invalid(format!(
                "revision {spec:?} must not start with '-'"
            )));
        }
        let output = self.git(
            &[
                "rev-parse",
                "--verify",
                "--quiet",
                "--end-of-options",
                &format!("{spec}^{{{kind}}}"),
            ],
            SMALL,
            "resolve revision",
        );
        match output {
            Ok(bytes) => Ok(utf8(bytes, "revision")?.trim().to_owned()),
            Err(Error::Git { .. }) => Err(Error::Invalid(format!(
                "revision {spec:?} does not name a {kind} in this repository"
            ))),
            Err(error) => Err(error),
        }
    }

    pub(crate) fn merge_base(&self, left: &str, right: &str) -> Result<String> {
        let output = self.git(&["merge-base", left, right], SMALL, "find merge base")?;
        Ok(utf8(output, "merge base")?.trim().to_owned())
    }

    /// The merge base of two commits, or `None` when they share no history.
    pub(crate) fn shared_base(&self, left: &str, right: &str) -> Result<Option<String>> {
        match self.merge_base(left, right) {
            Ok(base) => Ok(Some(base)),
            // Exit status 1 without a message: no common ancestor.
            Err(Error::Git { message, .. }) if message.is_empty() => Ok(None),
            Err(error) => Err(error),
        }
    }

    /// The commit HEAD names, or `None` on an unborn branch.
    pub(crate) fn head_commit(&self) -> Result<Option<String>> {
        match self.object("HEAD", "commit") {
            Ok(commit) => Ok(Some(commit)),
            Err(Error::Invalid(_)) => Ok(None),
            Err(error) => Err(error),
        }
    }

    /// The branch HEAD names, or `None` when HEAD is detached.
    pub(crate) fn branch(&self) -> Result<Option<String>> {
        match self.git(
            &["symbolic-ref", "--quiet", "--short", "HEAD"],
            SMALL,
            "read branch",
        ) {
            Ok(bytes) => Ok(Some(utf8(bytes, "branch")?.trim().to_owned())),
            // --quiet: a detached HEAD fails without a message.
            Err(Error::Git { message, .. }) if message.is_empty() => Ok(None),
            Err(error) => Err(error),
        }
    }

    /// Each local branch's upstream and each remote's default branch: one
    /// line per ref of `refname NUL symref NUL objectname NUL upstream NUL
    /// upstream:remoteref`.
    pub(crate) fn base_refs(&self) -> Result<String> {
        let output = self.git(
            &[
                "for-each-ref",
                "--format=%(refname)%00%(symref)%00%(objectname)%00%(upstream)%00%(upstream:remoteref)",
                "refs/heads/",
                "refs/remotes/*/HEAD",
            ],
            DIFF_LIMIT,
            "read branches",
        )?;
        utf8(output, "branches")
    }

    /// Whether commit `ancestor` is `descendant` or one of its ancestors.
    pub(crate) fn is_ancestor(&self, ancestor: &str, descendant: &str) -> Result<bool> {
        match self.git(
            &["merge-base", "--is-ancestor", ancestor, descendant],
            SMALL,
            "compare commits",
        ) {
            Ok(_) => Ok(true),
            // Exit status 1 without a message: not an ancestor.
            Err(Error::Git { message, .. }) if message.is_empty() => Ok(false),
            Err(error) => Err(error),
        }
    }

    /// How many commits `to` reaches that `from` does not, or `None` when
    /// that is more than `at_most`.
    pub(crate) fn commits_between(
        &self,
        from: &str,
        to: &str,
        at_most: Option<u64>,
    ) -> Result<Option<u64>> {
        let range = format!("{from}..{to}");
        let limit = at_most.map(|m| format!("--max-count={}", m.saturating_add(1)));
        let mut args = vec!["rev-list", "--count"];
        args.extend(limit.as_deref());
        args.push(&range);
        let output = self.git(&args, SMALL, "count commits")?;
        let n: u64 = utf8(output, "commit count")?
            .trim()
            .parse()
            .map_err(|_| Error::Git {
                operation: "count commits",
                message: "rev-list printed no count".into(),
            })?;
        Ok(at_most.is_none_or(|m| n <= m).then_some(n))
    }

    /// `git diff --numstat -z` from `commit` to the working tree's tracked
    /// files among `among`, renames off, refreshing the index's stat cache.
    pub(crate) fn numstat(&self, commit: &str, among: &[String]) -> Result<Vec<u8>> {
        self.diff_among(
            &[
                "diff",
                "--numstat",
                "-z",
                "--no-renames",
                "--no-ext-diff",
                commit,
            ],
            among,
            "measure changes",
        )
    }

    /// Tracked paths among `among` whose working-tree content differs from
    /// `commit`'s. A path of `commit` that is untracked in the working tree
    /// counts as removed.
    pub(crate) fn changed_since(&self, commit: &str, among: &[String]) -> Result<Vec<String>> {
        let output = self.diff_among(
            &[
                "diff",
                "--name-only",
                "-z",
                "--no-renames",
                "--no-ext-diff",
                commit,
            ],
            among,
            "compare with the working tree",
        )?;
        paths(&output)
    }

    /// Run working-tree diff `args` limited to the literal paths `among`:
    /// without a limit, comparing a commit with the working tree walks the
    /// whole tree, which takes about a second in a large repository.
    fn diff_among(
        &self,
        args: &[&str],
        among: &[String],
        operation: &'static str,
    ) -> Result<Vec<u8>> {
        if among.is_empty() {
            return Ok(Vec::new());
        }
        let mut full = vec!["--literal-pathspecs"];
        full.extend_from_slice(args);
        full.push("--");
        // Past this size the paths could exceed the argument limit; the
        // whole tree is then compared.
        if among.iter().map(String::len).sum::<usize>() <= PATHSPEC_BYTES {
            full.extend(among.iter().map(String::as_str));
        }
        self.git_refreshing(&full, DIFF_LIMIT, operation)
    }

    /// The working tree's state from one `git status`, refreshing the
    /// index's stat cache: tracked paths whose index or working-tree content
    /// differs from HEAD, and untracked paths that are not ignored, every
    /// file listed. Status names untracked directories whole and quickly, and
    /// only those are listed file by file.
    pub(crate) fn status(&self) -> Result<Status> {
        let status = self.git_refreshing(
            &[
                "status",
                "--porcelain=v2",
                "-z",
                "--no-renames",
                "--untracked-files=normal",
                "--ignore-submodules=all",
            ],
            DIFF_LIMIT,
            "read working tree status",
        )?;
        let utf8_path = |path: &[u8]| {
            String::from_utf8(path.to_vec()).map_err(|_| Error::Git {
                operation: "read working tree status",
                message: "path is not UTF-8".into(),
            })
        };
        let mut changed = Vec::new();
        let mut untracked = Vec::new();
        let mut directories = Vec::new();
        for entry in status.split(|byte| *byte == 0) {
            // Fields before the path: 8 for an ordinary change, 10 for an
            // unmerged one.
            let fields = match entry.first() {
                Some(b'1') => 8,
                Some(b'u') => 10,
                Some(b'?') => 1,
                _ => continue,
            };
            let Some(path) = entry.splitn(fields + 1, |byte| *byte == b' ').nth(fields) else {
                continue;
            };
            let path = utf8_path(path)?;
            if fields != 1 {
                changed.push(path);
            } else if path.ends_with('/') {
                directories.push(path);
            } else {
                untracked.push(path);
            }
        }
        if !directories.is_empty() {
            let mut args = vec!["ls-files", "--others", "--exclude-standard", "-z", "--"];
            args.extend(directories.iter().map(String::as_str));
            let listed = self.git(&args, DIFF_LIMIT, "list untracked files")?;
            untracked.extend(paths(&listed)?);
        }
        changed.sort();
        untracked.sort();
        Ok(Status { changed, untracked })
    }

    /// Paths that differ between two trees.
    pub(crate) fn changed_between(&self, from: &str, to: &str) -> Result<Vec<String>> {
        let output = self.git(
            &[
                "diff",
                "--name-only",
                "-z",
                "--no-renames",
                "--no-ext-diff",
                from,
                to,
                "--",
            ],
            DIFF_LIMIT,
            "diff revisions",
        )?;
        paths(&output)
    }

    /// The blob of each of `files` in `tree`, by path; absent paths are
    /// missing from the map.
    pub(crate) fn blobs(&self, tree: &str, files: &[String]) -> Result<BTreeMap<String, Blob>> {
        let mut blobs = BTreeMap::new();
        for chunk in files.chunks(256) {
            let mut args = vec!["ls-tree", "-r", "-z", "--full-tree", tree, "--"];
            args.extend(chunk.iter().map(String::as_str));
            let output = self.git(&args, DIFF_LIMIT, "read tree")?;
            for entry in output.split(|byte| *byte == 0) {
                let Some(tab) = entry.iter().position(|byte| *byte == b'\t') else {
                    continue;
                };
                let (Ok(info), Ok(path)) = (
                    std::str::from_utf8(&entry[..tab]),
                    std::str::from_utf8(&entry[tab + 1..]),
                ) else {
                    continue;
                };
                let mut fields = info.split(' ');
                if let (Some(mode), Some("blob"), Some(id)) =
                    (fields.next(), fields.next(), fields.next())
                {
                    blobs.insert(
                        path.to_owned(),
                        Blob {
                            mode: mode.to_owned(),
                            id: id.to_owned(),
                        },
                    );
                }
            }
        }
        Ok(blobs)
    }

    /// The blob each of `files` in the working tree would get, as capturing
    /// the working tree stores it, without writing it: a symbolic link's
    /// blob holds its target, unfollowed.
    pub(crate) fn hash_worktree(&self, files: &[String]) -> Result<Vec<Blob>> {
        let io = |path: &Path| {
            let path = path.to_owned();
            move |source| Error::Io {
                operation: "hash files",
                path,
                source,
            }
        };
        // Links are hashed by target here; ordinary files by Git, in batches.
        let mut blobs: Vec<Option<Blob>> = Vec::with_capacity(files.len());
        let mut regular = Vec::new();
        for file in files {
            let path = self.toplevel.join(file);
            let metadata = std::fs::symlink_metadata(&path).map_err(io(&path))?;
            if metadata.file_type().is_symlink() {
                let target = std::fs::read_link(&path).map_err(io(&path))?;
                blobs.push(Some(Blob {
                    mode: "120000".into(),
                    id: self.hash_bytes(target.as_os_str().as_encoded_bytes())?,
                }));
            } else {
                regular.push(file.as_str());
                blobs.push(None);
            }
        }
        let mut ids = Vec::with_capacity(regular.len());
        for chunk in regular.chunks(256) {
            let mut args = vec!["hash-object", "--"];
            args.extend(chunk);
            let output = self.git(&args, SMALL, "hash files")?;
            ids.extend(utf8(output, "object ids")?.lines().map(str::to_owned));
        }
        if ids.len() != regular.len() {
            return Err(Error::Git {
                operation: "hash files",
                message: format!(
                    "hash-object printed {} ids for {} files",
                    ids.len(),
                    regular.len()
                ),
            });
        }
        // As `git add` stores a file new to the index.
        let executable_bit = regular.is_empty() || self.trusts_executable_bit()?;
        let mut ids = ids.into_iter();
        files
            .iter()
            .zip(blobs)
            .map(|(file, blob)| {
                if let Some(blob) = blob {
                    return Ok(blob);
                }
                Ok(Blob {
                    mode: file_mode(&self.toplevel.join(file), executable_bit)?.into(),
                    id: ids.next().unwrap_or_default(),
                })
            })
            .collect()
    }

    /// Whether Git records files' executable bit: `core.filemode`, true
    /// unless set false.
    fn trusts_executable_bit(&self) -> Result<bool> {
        match self.git(
            &["config", "--type=bool", "--get", "core.filemode"],
            SMALL,
            "read core.filemode",
        ) {
            Ok(value) => Ok(utf8(value, "core.filemode")?.trim() != "false"),
            // Exit status 1 without a message: not set.
            Err(Error::Git { message, .. }) if message.is_empty() => Ok(true),
            Err(error) => Err(error),
        }
    }

    /// The blob id of `bytes`, unfiltered, without writing it.
    fn hash_bytes(&self, bytes: &[u8]) -> Result<String> {
        with_temporary(bytes, |path| {
            let output = self.git(
                &["hash-object", "--no-filters", "--", &path.to_string_lossy()],
                SMALL,
                "hash files",
            )?;
            Ok(utf8(output, "object id")?.trim().to_owned())
        })
    }

    /// The content of blob `id`.
    pub(crate) fn blob(&self, id: &str) -> Result<Vec<u8>> {
        self.git(&["cat-file", "blob", id], DIFF_LIMIT, "read blob")
    }

    /// `git diff --numstat` from blob `base` to the working-tree file `file`,
    /// both ordinary files.
    pub(crate) fn numstat_against_blob(&self, base: &str, file: &str) -> Result<(u64, u64)> {
        let content = self.blob(base)?;
        with_temporary(&content, |path| {
            let mut command = Command::new("git");
            command
                .args([
                    "diff",
                    "--no-index",
                    "--numstat",
                    "-z",
                    "--no-ext-diff",
                    "--",
                ])
                .arg(path)
                .arg(self.toplevel.join(file))
                .current_dir(&self.toplevel)
                .env("LC_ALL", "C")
                .env("GIT_OPTIONAL_LOCKS", "0");
            let output = dots_process::Runner::new(DEADLINE, SMALL)
                .stdout_limit(SMALL)
                .output(command)
                .map_err(|source| Error::Process {
                    operation: "measure changes",
                    source,
                })?;
            // Without --exit-code, --no-index still exits 1 when the files differ.
            if !matches!(output.status.code(), Some(0 | 1)) {
                return Err(Error::Git {
                    operation: "measure changes",
                    message: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
                });
            }
            let text = utf8(output.stdout, "numstat")?;
            let mut fields = text.split('\t');
            Ok((
                fields.next().and_then(|n| n.parse().ok()).unwrap_or(0),
                fields.next().and_then(|n| n.parse().ok()).unwrap_or(0),
            ))
        })
    }

    /// The working tree, tracked and untracked files that are not ignored, as
    /// a tree object. Uses a copy of the checkout's index so its stat cache
    /// spares rehashing unchanged files; the checkout's own index is untouched.
    pub(crate) fn capture_worktree(&self, scratch: &Path) -> Result<String> {
        let index = self.git(&["rev-parse", "--git-path", "index"], SMALL, "locate index")?;
        let index = self.toplevel.join(utf8(index, "index path")?.trim());
        let temporary = scratch.join(format!(".index-{:016x}", random()?));
        let result = (|| {
            match std::fs::copy(&index, &temporary) {
                Ok(_) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(source) => {
                    return Err(Error::Io {
                        operation: "copy index",
                        path: index.clone(),
                        source,
                    });
                }
            }
            let env = [("GIT_INDEX_FILE", temporary.as_os_str())];
            run(
                &self.toplevel,
                &["add", "--all", "--", "."],
                &env,
                SMALL,
                "capture working tree",
            )?;
            let tree = run(
                &self.toplevel,
                &["write-tree"],
                &env,
                SMALL,
                "capture working tree",
            )?;
            Ok(utf8(tree, "tree")?.trim().to_owned())
        })();
        match std::fs::remove_file(&temporary) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(source) => {
                return Err(Error::Io {
                    operation: "remove temporary index",
                    path: temporary,
                    source,
                });
            }
        }
        result
    }

    /// Start the pin of new review `id`: keep its first revision's `head`,
    /// `base` and `head_commit` reachable under the shared ref
    /// `refs/dots/reviews/ID`, so garbage collection in any worktree never
    /// prunes reviewed content. Returns the pin commit, whose tree is `head`,
    /// or `None` when another review owns the ref. Per-worktree refs would not
    /// do: Git's reachability ignores other worktrees' `refs/worktree/`
    /// namespaces.
    pub(crate) fn pin_new(
        &self,
        id: &str,
        base: &str,
        head: &str,
        head_commit: &str,
    ) -> Result<Option<String>> {
        if self.object(&pin_ref(id), "commit").is_ok() {
            return Ok(None);
        }
        self.commit_pin(id, None, base, head, head_commit).map(Some)
    }

    /// Pin a later revision of review `id`, chained to the revisions it
    /// already pins. Returns the pin commit, whose tree is `head`.
    pub(crate) fn pin_next(
        &self,
        id: &str,
        base: &str,
        head: &str,
        head_commit: &str,
    ) -> Result<String> {
        let previous = self.object(&pin_ref(id), "commit").ok();
        self.commit_pin(id, previous.as_deref(), base, head, head_commit)
    }

    fn commit_pin(
        &self,
        id: &str,
        previous: Option<&str>,
        base: &str,
        head: &str,
        head_commit: &str,
    ) -> Result<String> {
        let mut args = vec!["commit-tree", "--no-gpg-sign", head];
        for parent in previous.into_iter().chain([base, head_commit]) {
            args.extend(["-p", parent]);
        }
        // A nonce keeps every pin commit distinct, so a conditional release
        // never matches a newer pin of a reused id.
        let message = format!("dots review {id} {:016x}", random()?);
        args.extend(["-m", &message]);
        let identity = [
            ("GIT_AUTHOR_NAME", OsStr::new("dots review")),
            ("GIT_AUTHOR_EMAIL", OsStr::new("dots-review@localhost")),
            ("GIT_COMMITTER_NAME", OsStr::new("dots review")),
            ("GIT_COMMITTER_EMAIL", OsStr::new("dots-review@localhost")),
        ];
        let commit = run(
            &self.toplevel,
            &args,
            &identity,
            SMALL,
            "pin review content",
        )?;
        let commit = utf8(commit, "pin commit")?.trim().to_owned();
        self.git(
            &[
                "update-ref",
                &pin_ref(id),
                &commit,
                previous.unwrap_or_default(),
            ],
            SMALL,
            "pin review content",
        )?;
        Ok(commit)
    }

    /// Every pinned review in the repository: its id and pin commit.
    pub(crate) fn pins(&self) -> Result<Vec<(String, String)>> {
        let output = self.git(
            &[
                "for-each-ref",
                "--format=%(refname:lstrip=3) %(objectname)",
                PIN_NAMESPACE,
            ],
            DIFF_LIMIT,
            "list review pins",
        )?;
        Ok(utf8(output, "review pins")?
            .lines()
            .filter_map(|line| line.split_once(' '))
            .map(|(id, commit)| (id.to_owned(), commit.to_owned()))
            .collect())
    }

    /// Release what [`Git::pin_new`] and [`Git::pin_next`] kept for review `id`.
    /// With `expected`, only while the ref still points there.
    pub(crate) fn unpin(&self, id: &str, expected: Option<&str>) -> Result<()> {
        let reference = pin_ref(id);
        let Ok(current) = self.object(&reference, "commit") else {
            return Ok(());
        };
        if expected.is_some_and(|expected| expected != current) {
            return Ok(());
        }
        match self.git(
            &["update-ref", "-d", &reference, &current],
            SMALL,
            "release review content",
        ) {
            // Moved since it was read: a newer pin owns it now.
            Err(Error::Git { .. })
                if self.object(&reference, "commit").ok().as_deref() != Some(&current) => {}
            result => {
                result?;
            }
        }
        Ok(())
    }

    /// The patch between two trees with `context` lines, renames off. Like
    /// every diff of reviewed content, it is a function of the objects alone:
    /// attributes come from `to`, never from the calling worktree's files.
    pub(crate) fn diff(&self, from: &str, to: &str, context: u8) -> Result<Vec<u8>> {
        self.git(
            &[
                &format!("--attr-source={to}"),
                "-c",
                "core.quotePath=false",
                "diff",
                "--no-color",
                "--no-ext-diff",
                "--no-textconv",
                "--no-renames",
                "--src-prefix=a/",
                "--dst-prefix=b/",
                &format!("--unified={context}"),
                from,
                to,
                "--",
            ],
            DIFF_LIMIT,
            "diff revisions",
        )
    }

    /// Which paths changed between two trees, how, and between which
    /// objects: `git diff --raw -z`, renames off, full object ids.
    pub(crate) fn raw(&self, from: &str, to: &str) -> Result<Vec<u8>> {
        self.git(
            &[
                "diff",
                "--raw",
                "-z",
                "--no-abbrev",
                "--no-renames",
                "--no-ext-diff",
                from,
                to,
                "--",
            ],
            DIFF_LIMIT,
            "diff revisions",
        )
    }

    /// A command that prints the patch [`Git::diff`] reads, from any worktree.
    pub(crate) fn diff_command(from: &str, to: &str) -> String {
        format!("git --attr-source={to} diff --no-renames {from} {to}")
    }

    /// The content of `path` in `tree`, or `None` when it has no such file.
    pub(crate) fn file(&self, tree: &str, path: &str) -> Result<Option<Vec<u8>>> {
        match self.git(
            &["cat-file", "blob", &format!("{tree}:{path}")],
            DIFF_LIMIT,
            "read file",
        ) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(Error::Git { .. }) => Ok(None),
            Err(error) => Err(error),
        }
    }

    fn git(&self, args: &[&str], limit: usize, operation: &'static str) -> Result<Vec<u8>> {
        run(&self.toplevel, args, &[], limit, operation)
    }

    /// As [`Self::git`], letting Git refresh the index's cached file stats
    /// on disk as a plain `git status` does (skipped while another process
    /// holds the index lock). Without it, a worktree whose cache is stale is
    /// rehashed on every read.
    fn git_refreshing(
        &self,
        args: &[&str],
        limit: usize,
        operation: &'static str,
    ) -> Result<Vec<u8>> {
        run(
            &self.toplevel,
            args,
            &[("GIT_OPTIONAL_LOCKS", OsStr::new("1"))],
            limit,
            operation,
        )
    }
}

/// A file as Git stores it in a tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Blob {
    /// Such as `100644`, `100755` or `120000`.
    pub(crate) mode: String,
    pub(crate) id: String,
}

/// What `git status` reports of a working tree against HEAD.
#[derive(Debug)]
pub(crate) struct Status {
    /// Tracked paths changed in the index or the working tree.
    pub(crate) changed: Vec<String>,
    /// Untracked files that are not ignored.
    pub(crate) untracked: Vec<String>,
}

/// One worktree as `git worktree list` names it.
#[derive(Debug)]
pub(crate) struct Listed {
    pub(crate) path: PathBuf,
    /// Its HEAD commit; `None` before the first commit.
    pub(crate) head: Option<String>,
    /// `None` when HEAD is detached.
    pub(crate) branch: Option<String>,
}

const PIN_NAMESPACE: &str = "refs/dots/reviews/";

fn pin_ref(id: &str) -> String {
    format!("{PIN_NAMESPACE}{id}")
}

fn run(
    directory: &Path,
    args: &[&str],
    env: &[(&str, &OsStr)],
    limit: usize,
    operation: &'static str,
) -> Result<Vec<u8>> {
    let mut command = Command::new("git");
    command
        .args(args)
        .current_dir(directory)
        .env("LC_ALL", "C")
        .env("GIT_OPTIONAL_LOCKS", "0");
    for (key, value) in env {
        command.env(key, value);
    }
    let output = dots_process::Runner::new(DEADLINE, SMALL)
        .stdout_limit(limit)
        .output(command)
        .map_err(|source| Error::Process { operation, source })?;
    if !output.status.success() {
        return Err(Error::Git {
            operation,
            message: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        });
    }
    Ok(output.stdout)
}

/// NUL-separated UTF-8 paths.
fn paths(bytes: &[u8]) -> Result<Vec<String>> {
    bytes
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
        .map(|path| {
            String::from_utf8(path.to_vec()).map_err(|_| Error::Git {
                operation: "read paths",
                message: "path is not UTF-8".into(),
            })
        })
        .collect()
}

fn utf8(bytes: Vec<u8>, what: &str) -> Result<String> {
    String::from_utf8(bytes).map_err(|_| Error::Git {
        operation: "read output",
        message: format!("{what} is not UTF-8"),
    })
}

pub(crate) fn random() -> Result<u64> {
    getrandom::u64().map_err(|error| Error::Invalid(format!("no randomness available: {error}")))
}

/// The mode Git records for the ordinary file at `path`, new to the index:
/// 100644 unless `executable_bit` is honoured and set, as `git add` does.
fn file_mode(path: &Path, executable_bit: bool) -> Result<&'static str> {
    use std::os::unix::fs::PermissionsExt as _;
    if !executable_bit {
        return Ok("100644");
    }
    let metadata = std::fs::metadata(path).map_err(|source| Error::Io {
        operation: "hash files",
        path: path.to_owned(),
        source,
    })?;
    Ok(if metadata.permissions().mode() & 0o100 == 0 {
        "100644"
    } else {
        "100755"
    })
}

/// Run `work` on a temporary file holding `bytes`, outside any repository,
/// and remove it afterwards.
fn with_temporary<T>(bytes: &[u8], work: impl FnOnce(&Path) -> Result<T>) -> Result<T> {
    let path = std::env::temp_dir().join(format!("dots-review-{:016x}", random()?));
    std::fs::write(&path, bytes).map_err(|source| Error::Io {
        operation: "write temporary file",
        path: path.clone(),
        source,
    })?;
    let result = work(&path);
    let _ = std::fs::remove_file(&path);
    result
}
