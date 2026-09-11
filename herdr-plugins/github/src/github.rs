use std::collections::HashSet;
use std::path::Path;
use std::process::{Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context as _, Result, bail};
use serde::Deserialize;

const COMMAND_DEADLINE: Duration = Duration::from_secs(30);
const COMMAND_POLL_INTERVAL: Duration = Duration::from_millis(20);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CheckBucket {
    Pass,
    Fail,
    Pending,
    Skipping,
    Cancel,
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum PullRequestState {
    Open,
    Closed,
    Merged,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PullRequestWire {
    number: u64,
    is_draft: bool,
    state: PullRequestState,
    #[serde(default)]
    review_decision: Option<ReviewDecisionWire>,
    #[serde(default)]
    review_requests: Vec<serde_json::Value>,
    head_ref_oid: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum ReviewDecisionWire {
    Approved,
    ChangesRequested,
    ReviewRequired,
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReviewStatus {
    Approved,
    ChangesRequested,
    Needed,
    Requested,
    NotReviewed,
    Unknown,
}

impl ReviewStatus {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Approved => "approved",
            Self::ChangesRequested => "changes requested",
            Self::Needed => "review needed",
            Self::Requested => "review requested",
            Self::NotReviewed => "not reviewed",
            Self::Unknown => "review unknown",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PullRequest {
    pub(crate) number: u64,
    pub(crate) draft: bool,
    pub(crate) state: PullRequestState,
    pub(crate) review: ReviewStatus,
    pub(crate) head_oid: String,
}

impl From<PullRequestWire> for PullRequest {
    fn from(value: PullRequestWire) -> Self {
        Self {
            number: value.number,
            draft: value.is_draft,
            state: value.state,
            review: match value.review_decision {
                Some(ReviewDecisionWire::Approved) => ReviewStatus::Approved,
                Some(ReviewDecisionWire::ChangesRequested) => ReviewStatus::ChangesRequested,
                Some(ReviewDecisionWire::ReviewRequired) => ReviewStatus::Needed,
                Some(ReviewDecisionWire::Unknown) => ReviewStatus::Unknown,
                None if value.review_requests.is_empty() => ReviewStatus::NotReviewed,
                None => ReviewStatus::Requested,
            },
            head_oid: value.head_ref_oid,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
pub struct Check {
    pub bucket: CheckBucket,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub link: String,
    #[serde(default)]
    pub workflow: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CiVerdict {
    Green,
    Failed,
    Pending,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CiState {
    pub number: u64,
    pub draft: bool,
    pub pull_request_state: PullRequestState,
    pub review: ReviewStatus,
    pub verdict: CiVerdict,
    pub checks: Vec<Check>,
    pub(crate) required_check_names: HashSet<String>,
    pub local_branch: String,
    pub local_head_oid: String,
    pub remote_head_oid: String,
}

impl CiState {
    #[must_use]
    pub fn total_checks(&self) -> usize {
        self.checks.len()
    }

    #[must_use]
    pub fn failed_checks(&self) -> usize {
        self.checks
            .iter()
            .filter(|check| matches!(check.bucket, CheckBucket::Fail | CheckBucket::Cancel))
            .count()
    }

    #[must_use]
    pub fn pending_checks(&self) -> usize {
        self.checks
            .iter()
            .filter(|check| check.bucket == CheckBucket::Pending)
            .count()
    }

    #[must_use]
    pub fn has_required_checks(&self) -> bool {
        !self.required_check_names.is_empty()
    }

    #[must_use]
    pub fn is_required(&self, check_name: &str) -> bool {
        self.required_check_names.contains(check_name)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Lookup {
    NoPullRequest,
    Found(CiState),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ChecksOutcome {
    Found(Vec<Check>),
    NoChecks,
}

pub fn lookup(dir: &Path) -> Result<Lookup> {
    lookup_with(&SystemCommandRunner, dir)
}

/// Local checkout identity; no worktree scan or network request.
pub(crate) fn checkout_identity(dir: &Path) -> Result<String> {
    let mut head = git_with(
        &SystemCommandRunner,
        dir,
        &["rev-parse", "--abbrev-ref", "HEAD"],
    )?;
    if head.trim() == "HEAD" {
        head = git_with(&SystemCommandRunner, dir, &["rev-parse", "HEAD"])?;
    }
    let remote = git_with(
        &SystemCommandRunner,
        dir,
        &["config", "--get", "remote.origin.url"],
    )?;
    Ok(format!("{remote}\n{head}"))
}

/// Resolve a branch once, independently of periodically fetching its PR status.
pub(crate) fn discover(dir: &Path) -> Result<Option<String>> {
    #[derive(Deserialize)]
    struct Located {
        url: String,
    }
    let output = SystemCommandRunner.output(dir, "gh", &["pr", "view", "--json", "url"])?;
    if !output.status.success() {
        let error = String::from_utf8_lossy(&output.stderr);
        if error
            .to_ascii_lowercase()
            .contains("no pull requests found")
        {
            return Ok(None);
        }
        bail!("discovering pull request: {}", error.trim());
    }
    let located: Located = serde_json::from_slice(&output.stdout)?;
    anyhow::ensure!(
        located.url.starts_with("https://") && located.url.contains("/pull/"),
        "invalid pull request locator"
    );
    Ok(Some(located.url))
}

/// Sidebar reads omit required-check classification and local revision details.
/// Both requests name the PR; changing branches cannot redirect them.
pub(crate) fn sidebar(dir: &Path, url: &str) -> Result<SidebarSnapshot> {
    sidebar_with(&SystemCommandRunner, dir, url)
}

fn sidebar_with(runner: &impl CommandRunner, dir: &Path, url: &str) -> Result<SidebarSnapshot> {
    let output = runner.output(
        dir,
        "gh",
        &[
            "pr",
            "view",
            url,
            "--json",
            "number,isDraft,state,reviewDecision,reviewRequests,headRefOid",
        ],
    )?;
    anyhow::ensure!(
        output.status.success(),
        "reading PR status: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let pr = PullRequest::from(serde_json::from_slice::<PullRequestWire>(&output.stdout)?);
    let checks = if pr.state == PullRequestState::Open {
        let output = runner.output(dir, "gh", &["pr", "checks", url, "--json", "bucket"])?;
        decode_checks(&output)?.into_checks()
    } else {
        Vec::new()
    };
    validate_buckets(&checks)?;
    Ok(SidebarSnapshot {
        pull_request: pr,
        checks,
    })
}

#[derive(Debug)]
pub(crate) struct SidebarSnapshot {
    pub pull_request: PullRequest,
    pub checks: Vec<Check>,
}

fn lookup_with(runner: &impl CommandRunner, dir: &Path) -> Result<Lookup> {
    let Some(pr) = pull_request_with(runner, dir)? else {
        return Ok(Lookup::NoPullRequest);
    };
    let mut all = checks_with(runner, dir, false, "name,bucket,link,workflow")?.into_checks();
    all.sort();
    let required = checks_with(runner, dir, true, "name,bucket")?.into_checks();
    validate_buckets(&all)?;
    validate_buckets(&required)?;
    let verdict = gating_verdict(&all, &required)?;
    let local_branch = git_with(runner, dir, &["rev-parse", "--abbrev-ref", "HEAD"])?;
    let local_head_oid = git_with(runner, dir, &["rev-parse", "HEAD"])?;
    Ok(Lookup::Found(CiState {
        number: pr.number,
        draft: pr.draft,
        pull_request_state: pr.state,
        review: pr.review,
        verdict,
        checks: all,
        required_check_names: required.into_iter().map(|check| check.name).collect(),
        local_branch,
        local_head_oid,
        remote_head_oid: pr.head_oid,
    }))
}

pub(crate) fn pull_request(dir: &Path) -> Result<Option<PullRequest>> {
    pull_request_with(&SystemCommandRunner, dir)
}

fn pull_request_with(runner: &impl CommandRunner, dir: &Path) -> Result<Option<PullRequest>> {
    let output = runner.output(
        dir,
        "gh",
        &[
            "pr",
            "view",
            "--json",
            "number,isDraft,state,reviewDecision,reviewRequests,headRefOid",
        ],
    )?;
    if !output.status.success() {
        let error = String::from_utf8_lossy(&output.stderr);
        if error
            .to_ascii_lowercase()
            .contains("no pull requests found")
        {
            return Ok(None);
        }
        bail!("gh pr view failed: {}", error.trim());
    }
    if output.stdout.is_empty() {
        return Ok(None);
    }
    let pull_request: PullRequestWire = serde_json::from_slice(&output.stdout)
        .context("decoding the pull request returned by gh")?;
    Ok(Some(pull_request.into()))
}

fn checks_with(
    runner: &impl CommandRunner,
    dir: &Path,
    required: bool,
    fields: &str,
) -> Result<ChecksOutcome> {
    let mut args = vec!["pr", "checks"];
    if required {
        args.push("--required");
    }
    args.extend(["--json", fields]);
    decode_checks(&runner.output(dir, "gh", &args)?)
}

fn decode_checks(output: &Output) -> Result<ChecksOutcome> {
    let stderr = String::from_utf8_lossy(&output.stderr);
    if output.stdout.is_empty() {
        if output.status.success() || is_no_checks_message(&stderr) {
            return Ok(ChecksOutcome::NoChecks);
        }
        bail!(
            "gh pr checks returned no JSON (exit {:?}): {}",
            output.status.code(),
            stderr.trim()
        );
    }
    if !matches!(output.status.code(), Some(0 | 1 | 8)) {
        bail!(
            "gh pr checks failed with {:?}: {}",
            output.status.code(),
            stderr.trim()
        );
    }
    serde_json::from_slice(&output.stdout)
        .context("decoding checks returned by gh")
        .map(ChecksOutcome::Found)
}

fn is_no_checks_message(stderr: &str) -> bool {
    let stderr = stderr.to_ascii_lowercase();
    stderr.contains("no checks reported") || stderr.contains("no required checks reported")
}

impl ChecksOutcome {
    fn into_checks(self) -> Vec<Check> {
        match self {
            Self::Found(checks) => checks,
            Self::NoChecks => Vec::new(),
        }
    }
}

pub(crate) fn wait_for_checks(dir: &Path, interval: Duration, required_only: bool) -> Result<()> {
    let interval = interval.as_secs().max(1).to_string();
    let mut args = vec!["pr", "checks"];
    if required_only {
        args.push("--required");
    }
    args.extend(["--watch", "--interval", &interval]);
    let status = Command::new("gh")
        .args(args)
        .current_dir(dir)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GH_HTTP_TIMEOUT", "10")
        .status()
        .context("running gh pr checks --watch")?;
    if !matches!(status.code(), Some(0 | 1 | 8)) {
        bail!("gh pr checks --watch exited with {status}");
    }
    Ok(())
}

pub(crate) fn run_log(dir: &Path, owner_repo: &str, run: &str) -> Result<String> {
    let output = SystemCommandRunner.output(
        dir,
        "gh",
        &["run", "view", run, "-R", owner_repo, "--log-failed"],
    )?;
    if !output.status.success() {
        bail!(
            "gh run view failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn verdict(checks: &[Check]) -> Result<CiVerdict> {
    validate_buckets(checks)?;
    if checks
        .iter()
        .any(|check| matches!(check.bucket, CheckBucket::Fail | CheckBucket::Cancel))
    {
        return Ok(CiVerdict::Failed);
    }
    if checks
        .iter()
        .any(|check| check.bucket == CheckBucket::Pending)
    {
        return Ok(CiVerdict::Pending);
    }
    Ok(CiVerdict::Green)
}

fn gating_verdict(all: &[Check], required: &[Check]) -> Result<CiVerdict> {
    verdict(if required.is_empty() { all } else { required })
}

fn validate_buckets(checks: &[Check]) -> Result<()> {
    if checks
        .iter()
        .any(|check| check.bucket == CheckBucket::Unknown)
    {
        bail!("GitHub returned an unknown check bucket");
    }
    Ok(())
}

trait CommandRunner {
    fn output(&self, dir: &Path, program: &str, args: &[&str]) -> Result<Output>;
}

#[derive(Debug, Clone, Copy)]
struct SystemCommandRunner;

impl CommandRunner for SystemCommandRunner {
    fn output(&self, dir: &Path, program: &str, args: &[&str]) -> Result<Output> {
        command_with_deadline(dir, program, args, COMMAND_DEADLINE)
    }
}

fn command_with_deadline(
    dir: &Path,
    program: &str,
    args: &[&str],
    deadline: Duration,
) -> Result<Output> {
    let mut child = Command::new(program)
        .args(args)
        .current_dir(dir)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GH_HTTP_TIMEOUT", "10")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("starting {program} in {}", dir.display()))?;
    let stdout = child.stdout.take().context("capturing command stdout")?;
    let stderr = child.stderr.take().context("capturing command stderr")?;
    let stdout = thread::spawn(move || read_all(stdout));
    let stderr = thread::spawn(move || read_all(stderr));
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() < deadline => thread::sleep(COMMAND_POLL_INTERVAL),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = stdout.join();
                let _ = stderr.join();
                bail!(
                    "{program} {} exceeded its {deadline:?} deadline",
                    args.join(" ")
                );
            }
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = stdout.join();
                let _ = stderr.join();
                return Err(error).context("checking command status");
            }
        }
    };
    Ok(Output {
        status,
        stdout: join_reader(stdout, "stdout")?,
        stderr: join_reader(stderr, "stderr")?,
    })
}

fn read_all(mut reader: impl std::io::Read) -> std::io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    std::io::Read::read_to_end(&mut reader, &mut bytes)?;
    Ok(bytes)
}

fn join_reader(
    reader: thread::JoinHandle<std::io::Result<Vec<u8>>>,
    stream: &str,
) -> Result<Vec<u8>> {
    reader
        .join()
        .map_err(|_| anyhow::anyhow!("command {stream} reader panicked"))?
        .with_context(|| format!("reading command {stream}"))
}

fn git_with(runner: &impl CommandRunner, dir: &Path, args: &[&str]) -> Result<String> {
    git_optional_with(runner, dir, args)?
        .with_context(|| format!("git {} returned no output", args.join(" ")))
}

fn git_optional_with(
    runner: &impl CommandRunner,
    dir: &Path,
    args: &[&str],
) -> Result<Option<String>> {
    let output = runner.output(dir, "git", args)?;
    if !output.status.success() {
        if output.status.code() == Some(1) {
            return Ok(None);
        }
        bail!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    let value = String::from_utf8(output.stdout)
        .with_context(|| format!("git {} returned non-UTF-8 output", args.join(" ")))?
        .trim()
        .to_owned();
    Ok((!value.is_empty()).then_some(value))
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::os::unix::process::ExitStatusExt as _;
    use std::sync::Mutex;

    use super::*;

    #[derive(Debug)]
    struct FakeRunner {
        outputs: Mutex<VecDeque<Output>>,
        calls: Mutex<Vec<(String, Vec<String>)>>,
    }

    impl FakeRunner {
        fn new(outputs: Vec<Output>) -> Self {
            Self {
                outputs: Mutex::new(outputs.into()),
                calls: Mutex::new(Vec::new()),
            }
        }
    }

    impl CommandRunner for FakeRunner {
        fn output(&self, _dir: &Path, program: &str, args: &[&str]) -> Result<Output> {
            self.calls
                .lock()
                .map_err(|_| anyhow::anyhow!("call lock poisoned"))?
                .push((
                    program.to_owned(),
                    args.iter().map(ToString::to_string).collect(),
                ));
            self.outputs
                .lock()
                .map_err(|_| anyhow::anyhow!("output lock poisoned"))?
                .pop_front()
                .context("fake command output exhausted")
        }
    }

    fn output(code: i32, stdout: &str, stderr: &str) -> Output {
        use std::process::ExitStatus;
        Output {
            status: ExitStatus::from_raw(code << 8),
            stdout: stdout.as_bytes().to_vec(),
            stderr: stderr.as_bytes().to_vec(),
        }
    }

    fn check(bucket: CheckBucket) -> Check {
        Check {
            bucket,
            name: String::new(),
            link: String::new(),
            workflow: String::new(),
        }
    }

    #[test]
    fn sidebar_fetch_is_pinned_and_omits_autofix_details() {
        let runner = FakeRunner::new(vec![
            output(
                0,
                r#"{"number":7,"isDraft":false,"state":"OPEN","reviewDecision":"APPROVED","headRefOid":"abc"}"#,
                "",
            ),
            output(8, r#"[{"bucket":"pending"},{"bucket":"fail"}]"#, ""),
        ]);
        let url = "https://github.com/example/project/pull/7";
        let snapshot = sidebar_with(&runner, Path::new("/unused"), url).unwrap();
        assert_eq!(snapshot.checks.len(), 2);
        assert_eq!(snapshot.pull_request.review, ReviewStatus::Approved);
        let calls = runner.calls.lock().unwrap();
        assert_eq!(calls.len(), 2);
        assert!(
            calls
                .iter()
                .all(|(program, args)| program == "gh"
                    && args.get(2).map(String::as_str) == Some(url))
        );
        assert!(!calls.iter().any(|(_, args)| {
            args.iter()
                .any(|arg| arg.contains("required") || arg.contains("workflow"))
        }));
    }

    #[test]
    fn terminal_sidebar_fetch_does_not_query_checks() {
        let runner = FakeRunner::new(vec![output(
            0,
            r#"{"number":7,"isDraft":false,"state":"MERGED","headRefOid":"abc"}"#,
            "",
        )]);
        let snapshot = sidebar_with(
            &runner,
            Path::new("/unused"),
            "https://github.com/example/project/pull/7",
        )
        .unwrap();
        assert_eq!(snapshot.pull_request.state, PullRequestState::Merged);
        assert_eq!(runner.calls.lock().unwrap().len(), 1);
    }

    #[test]
    fn required_failures_win_over_pending_checks() {
        assert_eq!(
            verdict(&[check(CheckBucket::Pending), check(CheckBucket::Cancel)]).unwrap(),
            CiVerdict::Failed
        );
    }

    #[test]
    fn no_required_checks_falls_back_to_all_checks() {
        assert_eq!(
            gating_verdict(&[check(CheckBucket::Fail)], &[]).unwrap(),
            CiVerdict::Failed
        );
        assert!(matches!(
            decode_checks(&output(
                1,
                "",
                "no required checks reported on the 'main' branch"
            ))
            .unwrap(),
            ChecksOutcome::NoChecks
        ));
    }

    #[test]
    fn no_checks_is_typed_but_unrelated_empty_failures_are_errors() {
        assert!(matches!(
            decode_checks(&output(0, "", "")).unwrap(),
            ChecksOutcome::NoChecks
        ));
        assert!(decode_checks(&output(1, "", "authentication failed")).is_err());
    }

    #[test]
    fn lookup_uses_required_fallback_and_exact_command_boundaries() {
        let runner = FakeRunner::new(vec![
            output(
                0,
                r#"{"number":42,"isDraft":false,"state":"OPEN","reviewDecision":null,"headRefOid":"abc"}"#,
                "",
            ),
            output(1, r#"[{"name":"test","bucket":"fail"}]"#, ""),
            output(1, "", "no required checks reported on the 'main' branch"),
            output(0, "main\n", ""),
            output(0, "abc\n", ""),
        ]);
        let Lookup::Found(state) = lookup_with(&runner, Path::new("/repo")).unwrap() else {
            unreachable!();
        };
        assert_eq!(state.verdict, CiVerdict::Failed);
        assert_eq!(state.total_checks(), 1);
        assert!(!state.has_required_checks());
        let calls = runner.calls.lock().unwrap();
        assert_eq!(calls.len(), 5);
        assert_eq!(
            calls.first(),
            Some(&(
                "gh".to_owned(),
                vec![
                    "pr".to_owned(),
                    "view".to_owned(),
                    "--json".to_owned(),
                    "number,isDraft,state,reviewDecision,reviewRequests,headRefOid".to_owned()
                ]
            ))
        );
        assert_eq!(
            calls.get(1),
            Some(&(
                "gh".to_owned(),
                vec![
                    "pr".to_owned(),
                    "checks".to_owned(),
                    "--json".to_owned(),
                    "name,bucket,link,workflow".to_owned()
                ]
            ))
        );
        assert_eq!(
            calls.get(2),
            Some(&(
                "gh".to_owned(),
                vec![
                    "pr".to_owned(),
                    "checks".to_owned(),
                    "--required".to_owned(),
                    "--json".to_owned(),
                    "name,bucket".to_owned()
                ]
            ))
        );
    }

    #[test]
    fn unknown_wire_values_are_rejected_or_fail_closed() {
        assert!(
            serde_json::from_str::<PullRequestWire>(
                r#"{"number":42,"isDraft":false,"state":"QUEUED","headRefOid":"abc"}"#
            )
            .is_err()
        );
        assert!(verdict(&[check(CheckBucket::Unknown)]).is_err());
    }

    #[test]
    fn review_decisions_and_pending_requests_are_distinct() {
        let review = |decision: &str, requests: &str| {
            let wire: PullRequestWire = serde_json::from_str(&format!(
                r#"{{"number":42,"isDraft":false,"state":"OPEN","reviewDecision":{decision},"reviewRequests":{requests},"headRefOid":"abc"}}"#
            ))
            .unwrap();
            PullRequest::from(wire).review
        };

        assert_eq!(review(r#""APPROVED""#, "[]"), ReviewStatus::Approved);
        assert_eq!(
            review(r#""CHANGES_REQUESTED""#, "[]"),
            ReviewStatus::ChangesRequested
        );
        assert_eq!(review(r#""REVIEW_REQUIRED""#, "[{}]"), ReviewStatus::Needed);
        assert_eq!(review("null", "[{}]"), ReviewStatus::Requested);
        assert_eq!(review("null", "[]"), ReviewStatus::NotReviewed);
        assert_eq!(review(r#""FUTURE_STATE""#, "[]"), ReviewStatus::Unknown);
    }

    #[test]
    fn a_command_deadline_kills_and_reaps_the_child() {
        let error = command_with_deadline(
            Path::new("/tmp"),
            "sh",
            &["-c", "sleep 2"],
            Duration::from_millis(20),
        )
        .unwrap_err();
        assert!(error.to_string().contains("exceeded"));
    }
}
