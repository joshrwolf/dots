//! The Git queries reviews depend on, each bounded in time and output.
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use crate::{Error, Result};

const DEADLINE: Duration = Duration::from_secs(60);
const SMALL: usize = 1024 * 1024;
/// Diffs larger than this are refused rather than reviewed partially.
pub(crate) const DIFF_LIMIT: usize = 32 * 1024 * 1024;

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
        head_commit: Option<&str>,
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
        head_commit: Option<&str>,
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
        head_commit: Option<&str>,
    ) -> Result<String> {
        let mut args = vec!["commit-tree", "--no-gpg-sign", head];
        for parent in previous.into_iter().chain([base]).chain(head_commit) {
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

fn utf8(bytes: Vec<u8>, what: &str) -> Result<String> {
    String::from_utf8(bytes).map_err(|_| Error::Git {
        operation: "read output",
        message: format!("{what} is not UTF-8"),
    })
}

pub(crate) fn random() -> Result<u64> {
    getrandom::u64().map_err(|error| Error::Invalid(format!("no randomness available: {error}")))
}
