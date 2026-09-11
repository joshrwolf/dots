use std::collections::BTreeSet;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant};

#[cfg(unix)]
use nix::sys::signal::{Signal, killpg};
#[cfg(unix)]
use nix::unistd::Pid;

use crate::{
    AnchorLocation, CapturedComparison, CapturedEndpoint, ComparisonSpec, DiffEndpoint, DiffSide,
    Error, ObservationId, Result,
};

const GIT_TIMEOUT: Duration = Duration::from_secs(5);
const GIT_CAPTURE_TIMEOUT: Duration = Duration::from_secs(2 * 60);
const STATE_DIRECTORY: &str = "herdr-review";
const DATABASE_FILE: &str = "review.sqlite3";
const CHECKOUT_TOKEN_FILE: &str = "herdr-review-checkout-id";
const MAX_ANCHOR_SOURCE_BYTES: usize = 8 * 1024 * 1024;
const MAX_GIT_OUTPUT_BYTES: usize = 16 * 1024 * 1024;
static NEXT_TEMP_INDEX: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Repository {
    checkout_root: PathBuf,
    worktree_git_dir: PathBuf,
    common_git_dir: PathBuf,
    database_path: PathBuf,
    checkout_token_path: PathBuf,
}

impl Repository {
    pub fn discover(checkout: &Path) -> Result<Self> {
        let checkout_root = git_path(checkout, "checkout root", &["--show-toplevel"])?;
        let worktree_git_dir =
            git_path(checkout, "worktree Git directory", &["--absolute-git-dir"])?;
        let common_git_dir = git_path(
            checkout,
            "common Git directory",
            &["--path-format=absolute", "--git-common-dir"],
        )?;
        let database_path = common_git_dir.join(STATE_DIRECTORY).join(DATABASE_FILE);
        let checkout_token_path = worktree_git_dir.join(CHECKOUT_TOKEN_FILE);
        Ok(Self {
            checkout_root,
            worktree_git_dir,
            common_git_dir,
            database_path,
            checkout_token_path,
        })
    }

    pub fn checkout_root(&self) -> &Path {
        &self.checkout_root
    }
    pub fn worktree_git_dir(&self) -> &Path {
        &self.worktree_git_dir
    }
    pub fn common_git_dir(&self) -> &Path {
        &self.common_git_dir
    }
    pub fn database_path(&self) -> &Path {
        &self.database_path
    }
    pub fn checkout_token_path(&self) -> &Path {
        &self.checkout_token_path
    }
    pub fn is_linked_worktree(&self) -> bool {
        self.worktree_git_dir != self.common_git_dir
    }

    pub fn comparison_against_working_tree(
        &self,
        base_oid: impl Into<String>,
    ) -> Result<ComparisonSpec> {
        ComparisonSpec::new(
            DiffEndpoint::Commit {
                oid: base_oid.into(),
            },
            DiffEndpoint::WorkingTree,
        )
    }

    /// Builds an operational comparison from `HEAD` to the current working tree.
    pub fn working_tree_comparison(&self) -> Result<ComparisonSpec> {
        let head = self.head_oid()?;
        self.comparison_against_working_tree(head)
    }

    /// Resolves the checkout's current `HEAD` to an immutable commit ID.
    pub fn head_oid(&self) -> Result<String> {
        self.git_text(&["rev-parse", "--verify", "HEAD^{commit}"])
    }

    pub(crate) fn capture_comparison(&self, spec: &ComparisonSpec) -> Result<CapturedComparison> {
        spec.validate()?;
        let base = self.capture_endpoint(&spec.base)?;
        let target = self.capture_endpoint(&spec.target)?;
        let comparison = CapturedComparison { base, target };
        comparison.validate()?;
        Ok(comparison)
    }

    pub(crate) fn retain_comparison(
        &self,
        observation: &ObservationId,
        comparison: &CapturedComparison,
    ) -> Result<()> {
        self.retain_snapshot(observation, DiffSide::Base, &comparison.base.oid)?;
        if let Err(error) =
            self.retain_snapshot(observation, DiffSide::Target, &comparison.target.oid)
        {
            self.release_observation(observation);
            return Err(error);
        }
        Ok(())
    }

    pub(crate) fn release_observation(&self, observation: &ObservationId) {
        let _ = self.delete_observation_refs(observation);
    }

    pub(crate) fn retained_observations(&self) -> Result<Vec<ObservationId>> {
        const PREFIX: &str = "refs/herdr-review/observations/";
        let mut command = self.git_command(&[
            "for-each-ref",
            "--format=%(refname)",
            "refs/herdr-review/observations",
        ]);
        let output = output_with_timeout(&mut command, GIT_TIMEOUT, &self.checkout_root)?;
        if !output.status.success() {
            return Err(self.git_failure(&output));
        }
        let refs = std::str::from_utf8(&output.stdout).map_err(|_| Error::NonUtf8GitPath {
            checkout: self.checkout_root.clone(),
        })?;
        let mut observations = BTreeSet::new();
        for reference in refs.lines() {
            let Some(suffix) = reference.strip_prefix(PREFIX) else {
                continue;
            };
            let mut parts = suffix.split('/');
            let (Some(id), Some(side), None) = (parts.next(), parts.next(), parts.next()) else {
                continue;
            };
            if !matches!(side, "base" | "target") {
                continue;
            }
            if let Ok(id) = ObservationId::parse(id) {
                observations.insert(id);
            }
        }
        Ok(observations.into_iter().collect())
    }

    pub(crate) fn delete_observation_refs(&self, observation: &ObservationId) -> Result<()> {
        for side in ["base", "target"] {
            let mut command = self.git_command(&[
                "update-ref",
                "-d",
                &format!("refs/herdr-review/observations/{observation}/{side}"),
            ]);
            self.success(&mut command)?;
        }
        Ok(())
    }

    fn capture_endpoint(&self, endpoint: &DiffEndpoint) -> Result<CapturedEndpoint> {
        let oid = match endpoint {
            DiffEndpoint::Commit { oid } => {
                self.git_text(&["rev-parse", "--verify", &format!("{oid}^{{commit}}")])?
            }
            DiffEndpoint::Index => {
                let mut write_tree = self.git_command(&["write-tree"]);
                let tree = self.command_text_with_timeout(&mut write_tree, GIT_CAPTURE_TIMEOUT)?;
                self.snapshot_commit(&tree)?
            }
            DiffEndpoint::WorkingTree => self.capture_working_tree()?,
        };
        Ok(CapturedEndpoint {
            source: endpoint.clone(),
            oid,
        })
    }

    fn capture_working_tree(&self) -> Result<String> {
        std::fs::create_dir_all(self.common_git_dir.join(STATE_DIRECTORY)).map_err(|source| {
            Error::CreateStateDirectory {
                path: self.common_git_dir.join(STATE_DIRECTORY),
                source,
            }
        })?;
        let sequence = NEXT_TEMP_INDEX.fetch_add(1, Ordering::Relaxed);
        let index = self
            .common_git_dir
            .join(STATE_DIRECTORY)
            .join(format!("capture-index-{}-{sequence}", std::process::id()));
        let result = (|| {
            let live_index = self.worktree_git_dir.join("index");
            if live_index.exists() {
                std::fs::copy(&live_index, &index).map_err(|source| Error::CaptureIndex {
                    live_index,
                    target: index.clone(),
                    source_error: source,
                })?;
            } else {
                let mut read_tree = self.git_command(&["read-tree", "--empty"]);
                read_tree.env("GIT_INDEX_FILE", &index);
                self.success_with_timeout(&mut read_tree, GIT_CAPTURE_TIMEOUT)?;
            }
            let mut add = self.git_command(&["add", "-A", "--"]);
            add.env("GIT_INDEX_FILE", &index);
            self.success_with_timeout(&mut add, GIT_CAPTURE_TIMEOUT)?;
            let mut write_tree = self.git_command(&["write-tree"]);
            write_tree.env("GIT_INDEX_FILE", &index);
            let tree = self.command_text_with_timeout(&mut write_tree, GIT_CAPTURE_TIMEOUT)?;
            self.snapshot_commit(&tree)
        })();
        let _ = std::fs::remove_file(&index);
        result
    }

    fn snapshot_commit(&self, tree: &str) -> Result<String> {
        let mut command = self.git_command(&["commit-tree", tree, "-m", "Herdr review snapshot"]);
        command
            .env("GIT_AUTHOR_NAME", "Herdr Review")
            .env("GIT_AUTHOR_EMAIL", "review@herdr.invalid")
            .env("GIT_AUTHOR_DATE", "1970-01-01T00:00:00Z")
            .env("GIT_COMMITTER_NAME", "Herdr Review")
            .env("GIT_COMMITTER_EMAIL", "review@herdr.invalid")
            .env("GIT_COMMITTER_DATE", "1970-01-01T00:00:00Z");
        self.command_text_with_timeout(&mut command, GIT_CAPTURE_TIMEOUT)
    }

    fn retain_snapshot(
        &self,
        observation: &ObservationId,
        side: DiffSide,
        oid: &str,
    ) -> Result<()> {
        let side = match side {
            DiffSide::Base => "base",
            DiffSide::Target => "target",
        };
        let mut command = self.git_command(&[
            "update-ref",
            &format!("refs/herdr-review/observations/{observation}/{side}"),
            oid,
        ]);
        self.success(&mut command)
    }

    pub fn anchor_source(
        &self,
        comparison: &CapturedComparison,
        location: &AnchorLocation,
    ) -> Result<Option<String>> {
        self.read_snapshot(endpoint_for_side(comparison, location.side), &location.path)
    }

    pub fn relocated_anchor_source(
        &self,
        comparison: &CapturedComparison,
        location: &AnchorLocation,
    ) -> Result<Option<(String, String)>> {
        if let Some(source) =
            self.read_snapshot(endpoint_for_side(comparison, location.side), &location.path)?
        {
            return Ok(Some((location.path.clone(), source)));
        }
        let Some(path) = self.renamed_path(comparison, location.side, &location.path)? else {
            return Ok(None);
        };
        Ok(self
            .read_snapshot(endpoint_for_side(comparison, location.side), &path)?
            .map(|source| (path, source)))
    }

    pub fn endpoint_description(comparison: &CapturedComparison, side: DiffSide) -> String {
        let endpoint = endpoint_for_side(comparison, side);
        match &endpoint.source {
            DiffEndpoint::Commit { .. } => format!("commit {}", endpoint.oid),
            DiffEndpoint::Index => format!("captured index {}", endpoint.oid),
            DiffEndpoint::WorkingTree => format!("captured working tree {}", endpoint.oid),
        }
    }

    fn read_snapshot(&self, endpoint: &CapturedEndpoint, path: &str) -> Result<Option<String>> {
        validate_repo_path(path)?;
        self.read_git_object(&format!("{}:{path}", endpoint.oid), path)
    }

    fn read_git_object(&self, spec: &str, path: &str) -> Result<Option<String>> {
        let mut command = self.git_command(&["show", "--no-textconv", spec]);
        let output = output_with_timeout(&mut command, GIT_TIMEOUT, &self.checkout_root)?;
        if !output.status.success() {
            return Ok(None);
        }
        decode_source(path, output.stdout)
    }

    fn renamed_path(
        &self,
        comparison: &CapturedComparison,
        side: DiffSide,
        original: &str,
    ) -> Result<Option<String>> {
        if side == DiffSide::Base {
            return Ok(None);
        }
        let mut command = self.git_command(&[
            "diff",
            "--name-status",
            "-z",
            "-M",
            &comparison.base.oid,
            &comparison.target.oid,
            "--",
        ]);
        let output = output_with_timeout(&mut command, GIT_TIMEOUT, &self.checkout_root)?;
        if !output.status.success() {
            return Err(self.git_failure(&output));
        }
        parse_rename(&output.stdout, original, &self.checkout_root)
    }

    fn git_command(&self, args: &[&str]) -> Command {
        let mut command = Command::new("git");
        command
            .arg("-C")
            .arg(&self.checkout_root)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        command
    }

    /// Porcelain status of the actual checkout, independent of captured review commits.
    pub fn checkout_status(&self) -> Result<String> {
        let mut command =
            self.git_command(&["status", "--porcelain=v1", "--untracked-files=normal"]);
        let output = output_with_timeout(&mut command, GIT_TIMEOUT, &self.checkout_root)?;
        if !output.status.success() {
            return Err(self.git_failure(&output));
        }
        String::from_utf8(output.stdout).map_err(|_| Error::NonUtf8GitPath {
            checkout: self.checkout_root.clone(),
        })
    }

    fn git_text(&self, args: &[&str]) -> Result<String> {
        let mut command = self.git_command(args);
        self.command_text(&mut command)
    }

    fn command_text(&self, command: &mut Command) -> Result<String> {
        self.command_text_with_timeout(command, GIT_TIMEOUT)
    }

    fn command_text_with_timeout(
        &self,
        command: &mut Command,
        timeout: Duration,
    ) -> Result<String> {
        let output = output_with_timeout(command, timeout, &self.checkout_root)?;
        if !output.status.success() {
            return Err(self.git_failure(&output));
        }
        let value = std::str::from_utf8(&output.stdout)
            .map_err(|_| Error::NonUtf8GitPath {
                checkout: self.checkout_root.clone(),
            })?
            .trim();
        if value.is_empty() {
            return Err(Error::EmptyGitPath {
                checkout: self.checkout_root.clone(),
                field: "Git object id",
            });
        }
        Ok(value.to_owned())
    }

    fn success(&self, command: &mut Command) -> Result<()> {
        self.success_with_timeout(command, GIT_TIMEOUT)
    }

    fn success_with_timeout(&self, command: &mut Command, timeout: Duration) -> Result<()> {
        let output = output_with_timeout(command, timeout, &self.checkout_root)?;
        if output.status.success() {
            Ok(())
        } else {
            Err(self.git_failure(&output))
        }
    }

    fn git_failure(&self, output: &Output) -> Error {
        Error::GitFailed {
            checkout: self.checkout_root.clone(),
            status: output.status,
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        }
    }
}

fn endpoint_for_side(comparison: &CapturedComparison, side: DiffSide) -> &CapturedEndpoint {
    match side {
        DiffSide::Base => &comparison.base,
        DiffSide::Target => &comparison.target,
    }
}

fn validate_repo_path(path: &str) -> Result<()> {
    let candidate = Path::new(path);
    let valid = !path.is_empty()
        && !candidate.is_absolute()
        && candidate
            .components()
            .all(|component| matches!(component, Component::Normal(_)));
    if valid {
        Ok(())
    } else {
        Err(Error::InvalidAnchorPath {
            path: path.to_owned(),
        })
    }
}

fn decode_source(path: &str, bytes: Vec<u8>) -> Result<Option<String>> {
    if bytes.len() > MAX_ANCHOR_SOURCE_BYTES {
        return Err(Error::AnchorSourceTooLarge {
            path: path.to_owned(),
            limit: MAX_ANCHOR_SOURCE_BYTES,
        });
    }
    String::from_utf8(bytes)
        .map(Some)
        .map_err(|_| Error::NonUtf8AnchorSource {
            path: path.to_owned(),
        })
}

fn parse_rename(output: &[u8], original: &str, checkout: &Path) -> Result<Option<String>> {
    let mut fields = output
        .split(|byte| *byte == 0)
        .filter(|field| !field.is_empty());
    let mut found = None;
    while let Some(status) = fields.next() {
        let Some(path) = fields.next() else {
            break;
        };
        if status.first() != Some(&b'R') && status.first() != Some(&b'C') {
            continue;
        }
        let Some(new_path) = fields.next() else {
            break;
        };
        if path == original.as_bytes() {
            let value =
                String::from_utf8(new_path.to_vec()).map_err(|_| Error::NonUtf8GitPath {
                    checkout: checkout.to_path_buf(),
                })?;
            if found.replace(value).is_some() {
                return Ok(None);
            }
        }
    }
    Ok(found)
}

fn git_path(checkout: &Path, field: &'static str, args: &[&str]) -> Result<PathBuf> {
    let mut command = Command::new("git");
    command
        .arg("-C")
        .arg(checkout)
        .arg("rev-parse")
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let output = output_with_timeout(&mut command, GIT_TIMEOUT, checkout)?;
    if !output.status.success() {
        return Err(Error::GitFailed {
            checkout: checkout.to_path_buf(),
            status: output.status,
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        });
    }
    let raw = String::from_utf8(output.stdout).map_err(|_| Error::NonUtf8GitPath {
        checkout: checkout.to_path_buf(),
    })?;
    let path = raw.trim_end_matches(['\n', '\r']);
    if path.is_empty() {
        return Err(Error::EmptyGitPath {
            checkout: checkout.to_path_buf(),
            field,
        });
    }
    std::fs::canonicalize(path).map_err(|source| Error::Canonicalize {
        field,
        path: PathBuf::from(path),
        source,
    })
}

fn output_with_timeout(
    command: &mut Command,
    timeout: Duration,
    checkout: &Path,
) -> Result<Output> {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt as _;
        command.process_group(0);
    }
    let mut child = command.spawn().map_err(|source| Error::RunGit {
        checkout: checkout.to_path_buf(),
        source,
    })?;
    let stdout = child.stdout.take().map(drain);
    let stderr = child.stderr.take().map(drain);
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                kill_process_group(child.id());
                let stdout = finish_drain(stdout, checkout)?;
                let stderr = finish_drain(stderr, checkout)?;
                if stdout.truncated || stderr.truncated {
                    return Err(Error::GitOutputTooLarge {
                        checkout: checkout.to_path_buf(),
                        limit: MAX_GIT_OUTPUT_BYTES,
                    });
                }
                return Ok(Output {
                    status,
                    stdout: stdout.bytes,
                    stderr: stderr.bytes,
                });
            }
            Ok(None) => {}
            Err(source) => {
                kill_process_group(child.id());
                let _ = child.kill();
                let _ = child.wait();
                return Err(Error::RunGit {
                    checkout: checkout.to_path_buf(),
                    source,
                });
            }
        }
        if Instant::now() >= deadline {
            kill_process_group(child.id());
            let _ = child.kill();
            let _ = child.wait();
            return Err(Error::GitTimedOut {
                checkout: checkout.to_path_buf(),
            });
        }
        thread::sleep(Duration::from_millis(10));
    }
}

#[derive(Debug)]
struct Captured {
    bytes: Vec<u8>,
    truncated: bool,
}

fn drain(mut stream: impl io::Read + Send + 'static) -> thread::JoinHandle<io::Result<Captured>> {
    thread::spawn(move || {
        let mut bytes = Vec::new();
        let mut truncated = false;
        let mut buffer = [0_u8; 16 * 1024];
        loop {
            let count = stream.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            let remaining = MAX_GIT_OUTPUT_BYTES.saturating_sub(bytes.len());
            let retained = remaining.min(count);
            let retained_bytes = buffer
                .get(..retained)
                .ok_or_else(|| io::Error::other("Git output buffer accounting failed"))?;
            bytes.extend_from_slice(retained_bytes);
            truncated |= retained < count;
        }
        Ok(Captured { bytes, truncated })
    })
}

fn finish_drain(
    worker: Option<thread::JoinHandle<io::Result<Captured>>>,
    checkout: &Path,
) -> Result<Captured> {
    worker
        .map_or_else(
            || {
                Ok(Captured {
                    bytes: Vec::new(),
                    truncated: false,
                })
            },
            |worker| {
                worker
                    .join()
                    .map_err(|_| io::Error::other("Git output reader thread panicked"))?
            },
        )
        .map_err(|source| Error::RunGit {
            checkout: checkout.to_path_buf(),
            source,
        })
}

#[cfg(unix)]
fn kill_process_group(id: u32) {
    if let Ok(id) = i32::try_from(id) {
        let _ = killpg(Pid::from_raw(id), Signal::SIGKILL);
    }
}
#[cfg(not(unix))]
fn kill_process_group(_id: u32) {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn git(cwd: &Path, args: &[&str]) -> String {
        let output = Command::new("git")
            .arg("-C")
            .arg(cwd)
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap().trim().to_owned()
    }

    #[test]
    fn captures_mutable_endpoints_without_changing_the_real_index() {
        let base = std::env::temp_dir().join(format!(
            "review-capture-{}-{}",
            std::process::id(),
            NEXT_TEMP_INDEX.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();
        git(&base, &["init", "--initial-branch=main"]);
        git(&base, &["config", "user.name", "Test"]);
        git(&base, &["config", "user.email", "test@example.invalid"]);
        fs::write(base.join("a"), "one\n").unwrap();
        git(&base, &["add", "a"]);
        git(&base, &["commit", "-m", "one"]);
        let repository = Repository::discover(&base).unwrap();
        assert_eq!(repository.checkout_status().unwrap(), "");
        let head = git(&base, &["rev-parse", "HEAD"]);
        fs::write(base.join("a"), "two\n").unwrap();
        fs::write(base.join("b"), "new\n").unwrap();
        assert_eq!(repository.checkout_status().unwrap(), " M a\n?? b\n");
        let before = git(&base, &["write-tree"]);
        let repository = Repository::discover(&base).unwrap();
        let id = ObservationId::parse("11111111111111111111111111111111").unwrap();
        let captured = repository
            .capture_comparison(
                &ComparisonSpec::new(
                    DiffEndpoint::Commit { oid: head },
                    DiffEndpoint::WorkingTree,
                )
                .unwrap(),
            )
            .unwrap();
        repository.retain_comparison(&id, &captured).unwrap();
        assert_ne!(captured.base.oid, captured.target.oid);
        assert_eq!(git(&base, &["write-tree"]), before);
        assert_eq!(
            git(&base, &["show", &format!("{}:b", captured.target.oid)]),
            "new"
        );
        assert_eq!(
            git(
                &base,
                &[
                    "rev-parse",
                    &format!("refs/herdr-review/observations/{id}/target")
                ]
            ),
            captured.target.oid
        );
        let _ = fs::remove_dir_all(base);
    }
}
