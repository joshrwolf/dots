use crate::logview::{Navigation, Query, page};
use serde::{Deserialize, Serialize};
use std::io::Seek as _;

use crate::provider::checked;
use crate::{CheckRef, Error, Provider, Result};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Step {
    pub name: String,
    pub number: u64,
    pub status: String,
    pub conclusion: Option<String>,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub(crate) struct Job {
    pub(crate) id: u64,
    pub(crate) run_id: u64,
    pub(crate) run_attempt: u64,
    pub(crate) head_sha: String,
    pub(crate) name: String,
    pub(crate) status: String,
    pub(crate) conclusion: Option<String>,
    pub(crate) steps: Vec<Step>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Evidence {
    pub snapshot: Option<crate::SnapshotRef>,
    pub observed_at: u64,
    pub source: crate::ObservationSource,
    pub reference: CheckRef,
    pub job_name: String,
    pub run_attempt: u64,
    pub job_head: String,
    pub observed_head: String,
    pub observed_pr_head: Option<String>,
    pub current_pr_head: Option<String>,
    pub status: String,
    pub conclusion: Option<String>,
    pub steps: Vec<Step>,
    pub log: Option<LogPage>,
    pub full_log_command: String,
    pub dependency_failure: bool,
    pub warnings: Vec<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct LogPage {
    pub source: LogSource,
    pub fetched_at: u64,
    pub start_line: usize,
    pub total_lines: usize,
    pub lines: Vec<String>,
    pub line_numbers: Vec<usize>,
    pub test: Option<String>,
    pub navigation_note: String,
    pub dependency_marker: bool,
    pub omitted_before: usize,
    pub omitted_after: usize,
    pub next_command: Option<String>,
    pub previous_command: Option<String>,
    pub search: Option<String>,
    pub matches: usize,
    pub eligible_matches: usize,
    pub match_index: Vec<LogMatch>,
    pub next_match_command: Option<String>,
    pub byte_limit_reached: bool,
    pub shortened_lines: usize,
    pub full_log_command: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LogMatch {
    pub line: usize,
    pub preview: String,
    pub command: String,
    pub preview_shortened: bool,
    pub test_name: Option<String>,
    pub test_output_command: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LogSource {
    Network,
    Cache,
}

#[derive(Debug, Clone, Copy)]
pub struct LogOptions<'a> {
    pub snapshot: Option<&'a crate::SnapshotRef>,
    pub start: Option<usize>,
    pub lines: usize,
    pub search: Option<&'a str>,
    pub test: Option<&'a str>,
}

/// Diagnose a direct job URL/ID or an emitted reference without a PR lookup.
pub fn job_logs(
    provider: &impl Provider,
    value: &str,
    repository: Option<&crate::Repository>,
    options: LogOptions<'_>,
) -> Result<Evidence> {
    if options.lines == 0
        || options.lines > 500
        || options.start == Some(0)
        || (options.search.is_some() && options.test.is_some())
    {
        return Err(Error::InvalidResponse(
            "invalid log window/search options".into(),
        ));
    }
    let query = if let Some(search) = options.search {
        if search.is_empty() || search.len() > 1024 || search.contains(['\n', '\r', '\0']) {
            return Err(Error::InvalidResponse(
                "search must be a nonempty single-line literal of at most 1024 bytes".into(),
            ));
        }
        Query::Search(search)
    } else if let Some(test) = options.test {
        if test.is_empty()
            || test.len() > 1024
            || test.chars().any(|c| c.is_whitespace() || c.is_control())
        {
            return Err(Error::InvalidResponse(
                "test must be an exact Go test name of at most 1024 bytes".into(),
            ));
        }
        Query::Test(test)
    } else {
        Query::Window
    };
    if let Some(snapshot) = options.snapshot {
        let reference: CheckRef = value.parse()?;
        crate::target::repository(provider, repository, Some(reference.repository.clone()))?;
        return saved_logs(provider, &reference, snapshot, options, query);
    }
    if value.starts_with("job:") {
        let reference: CheckRef = value.parse()?;
        crate::target::repository(provider, repository, Some(reference.repository.clone()))?;
        return retrieve(
            provider,
            &reference,
            options.start,
            options.lines,
            None,
            query,
        );
    }
    let (repository, id, expected_run) = if value.starts_with("https://") {
        let (repo, crate::target::Resource::Job { id, run }) =
            crate::target::Resource::parse(value)?
        else {
            return Err(crate::target::invalid());
        };
        (
            crate::target::repository(provider, repository, Some(repo))?,
            id,
            run,
        )
    } else {
        (
            crate::target::repository(provider, repository, None)?,
            crate::target::id(value)?,
            None,
        )
    };
    let endpoint = repository.endpoint(&format!("actions/jobs/{id}"));
    let job: Job = serde_json::from_slice(&checked(
        provider,
        &["api", "--hostname", &repository.host, &endpoint],
    )?)?;
    if job.id != id
        || expected_run.is_some_and(|run| run != job.run_id)
        || job.run_id == 0
        || job.run_attempt == 0
    {
        return Err(Error::InvalidResponse(
            "GitHub returned a different job/run identity".into(),
        ));
    }
    let reference = CheckRef {
        repository,
        pr: None,
        head: crate::target::sha(&job.head_sha)?,
        run: job.run_id,
        attempt: Some(job.run_attempt),
        job: id,
    };
    retrieve_job(
        provider,
        &reference,
        (options.start, options.lines),
        None,
        query,
        job,
    )
}

/// Reuse the head observed with the checks when collecting inline evidence.
/// Standalone drill-down still refreshes it to detect an old reference.
pub(crate) fn logs_observed(
    provider: &impl Provider,
    reference: &CheckRef,
    start: Option<usize>,
    count: usize,
    observed_head: Option<&str>,
) -> Result<Evidence> {
    retrieve(
        provider,
        reference,
        start,
        count,
        observed_head,
        Query::Window,
    )
}

pub(crate) fn logs_from_observation(
    provider: &impl Provider,
    reference: &CheckRef,
    head: &str,
    job: Option<Job>,
) -> Result<Evidence> {
    if let Some(job) = job {
        retrieve_job(
            provider,
            reference,
            (None, 40),
            Some(head),
            Query::Window,
            job,
        )
    } else {
        logs_observed(provider, reference, None, 40, Some(head))
    }
}

fn retrieve(
    provider: &impl Provider,
    reference: &CheckRef,
    start: Option<usize>,
    count: usize,
    observed_head: Option<&str>,
    query: Query<'_>,
) -> Result<Evidence> {
    if count == 0 || count > 500 || start == Some(0) {
        return Err(Error::InvalidResponse(
            "log lines must be 1..=500; start is one-based".into(),
        ));
    }
    let endpoint = reference.endpoint();
    let data = checked(
        provider,
        &["api", "--hostname", &reference.repository.host, &endpoint],
    )?;
    let job: Job = serde_json::from_slice(&data)?;
    retrieve_job(
        provider,
        reference,
        (start, count),
        observed_head,
        query,
        job,
    )
}

fn retrieve_job(
    provider: &impl Provider,
    reference: &CheckRef,
    window: (Option<usize>, usize),
    observed_head: Option<&str>,
    query: Query<'_>,
    job: Job,
) -> Result<Evidence> {
    let (start, count) = window;
    let endpoint = reference.endpoint();
    if job.id != reference.job
        || job.run_id != reference.run
        || job.run_attempt == 0
        || reference
            .attempt
            .is_some_and(|attempt| attempt != job.run_attempt)
        || (reference.pr.is_none() && reference.head != job.head_sha)
    {
        return Err(Error::InvalidResponse(
            "GitHub job identity did not match the requested reference".into(),
        ));
    }
    let mut pinned = reference.clone();
    pinned.attempt = Some(job.run_attempt);
    let reference = &pinned;
    let mut evidence = Evidence {
        snapshot: None,
        observed_at: crate::cache::now(),
        source: crate::ObservationSource::Network,
        reference: reference.clone(),
        job_name: job.name,
        run_attempt: job.run_attempt,
        job_head: job.head_sha,
        observed_head: reference.head.clone(),
        observed_pr_head: reference.pr.map(|_| reference.head.clone()),
        current_pr_head: None,
        status: job.status,
        conclusion: job.conclusion,
        steps: job.steps,
        log: None,
        full_log_command: full_log_command(reference),
        dependency_failure: false,
        warnings: Vec::new(),
    };
    observe_head(provider, reference, observed_head, &mut evidence);
    if evidence.status != "completed" {
        evidence
            .warnings
            .push("Job is not complete; logs may not be available yet.".into());
        return Ok(evidence);
    }
    if observed_head.is_some() && dependency_steps(&evidence.steps) {
        evidence.dependency_failure = true;
        evidence.warnings.push("Likely aggregate job based on failed steps; automatic download skipped. Use the exact log command if needed.".into());
        return Ok(evidence);
    }
    if let Some((mut file, fetched_at)) = provider.cached_log_file(reference, evidence.run_attempt)
    {
        let navigation = save_navigation(provider, &mut evidence, true);
        evidence.log = Some(page(
            &mut file,
            &navigation,
            start,
            count,
            LogSource::Cache,
            fetched_at,
            query,
        )?);
        evidence.dependency_failure = dependency_failure(&evidence);
        compact_steps(&mut evidence, query);
        return Ok(evidence);
    }
    let log_endpoint = format!("{endpoint}/logs");
    match fetch_log(provider, &reference.repository.host, &log_endpoint) {
        Ok(mut file) => {
            if file.metadata()?.len() == 0 {
                evidence
                    .warnings
                    .push("GitHub returned an empty job log.".into());
            } else {
                let fetched_at = crate::cache::now();
                let saved =
                    provider.cache_log_file(reference, evidence.run_attempt, &mut file, fetched_at);
                file.rewind()?;
                let navigation = save_navigation(provider, &mut evidence, saved);
                evidence.log = Some(page(
                    &mut file,
                    &navigation,
                    start,
                    count,
                    LogSource::Network,
                    fetched_at,
                    query,
                )?);
                evidence.dependency_failure = dependency_failure(&evidence);
            }
        }
        Err(error) => evidence
            .warnings
            .push(format!("Job logs unavailable: {}", error.diagnostic())),
    }
    compact_steps(&mut evidence, query);
    Ok(evidence)
}

fn observe_head(
    provider: &impl Provider,
    reference: &CheckRef,
    observed_head: Option<&str>,
    evidence: &mut Evidence,
) {
    if reference.pr.is_none() {
        return;
    }
    let head_result = observed_head.map_or_else(
        || current_head(provider, reference),
        |head| Ok(head.to_owned()),
    );
    match head_result {
        Ok(head) => {
            if head != reference.head {
                evidence.warnings.push(
                    "PR head changed since inspection; these logs belong to the original job."
                        .into(),
                );
            }
            evidence.current_pr_head = Some(head);
        }
        Err(error) => evidence.warnings.push(format!(
            "Current PR head unavailable: {}",
            error.diagnostic()
        )),
    }
}

fn compact_steps(evidence: &mut Evidence, query: Query<'_>) {
    if matches!(query, Query::Search(_) | Query::Test(_)) {
        evidence.steps.retain(|step| {
            step.conclusion
                .as_deref()
                .is_some_and(|state| state != "success" && state != "skipped")
        });
    }
}

fn current_head(provider: &impl Provider, reference: &CheckRef) -> Result<String> {
    #[derive(Deserialize)]
    struct Head {
        #[serde(rename = "headRefOid")]
        oid: String,
    }
    let pr = reference
        .pull_request_url()
        .ok_or_else(crate::target::invalid)?;
    let bytes = checked(provider, &["pr", "view", &pr, "--json", "headRefOid"])?;
    Ok(serde_json::from_slice::<Head>(&bytes)?.oid)
}

fn fetch_log(provider: &impl Provider, host: &str, endpoint: &str) -> Result<std::fs::File> {
    let mut file = crate::cache::spool()?;
    // New gh needs this flag for raw logs. Old gh rejects it before HTTP, so
    // compatibility fallback costs a process, never a second log download.
    let args = [
        "api",
        "--hostname",
        host,
        endpoint,
        "--allow-escape-sequences",
    ];
    let result = match checked_log(provider, &args, &mut file) {
        Err(Error::Query { message, .. })
            if message.contains("unknown flag: --allow-escape-sequences") =>
        {
            file.set_len(0)?;
            file.rewind()?;
            checked_log(provider, &["api", "--hostname", host, endpoint], &mut file)
        }
        result => result,
    };
    result?;
    file.rewind()?;
    Ok(file)
}

fn checked_log(provider: &impl Provider, args: &[&str], file: &mut std::fs::File) -> Result<()> {
    let result = provider.gh_log(args, file)?;
    if !result.status.success() {
        return Err(Error::Query {
            operation: args.join(" "),
            message: String::from_utf8_lossy(&result.stderr).trim().into(),
        });
    }
    Ok(())
}

fn full_log_command(reference: &CheckRef) -> String {
    format!(
        "gh run view --repo {}/{}/{} --job {} --log",
        reference.repository.host,
        reference.repository.owner,
        reference.repository.repo,
        reference.job
    )
}

fn dependency_failure(evidence: &Evidence) -> bool {
    if dependency_steps(&evidence.steps) {
        return true;
    }
    evidence.log.as_ref().is_some_and(|log| {
        log.dependency_marker
            || log.lines.iter().any(|line| {
                let line = line.to_ascii_lowercase();
                [
                    "dependency failed",
                    "dependencies failed",
                    "failed dependencies",
                    "needs result: failure",
                    "needs results:",
                    "dependent jobs failed",
                    "dependency failure",
                    "required job failed",
                    "dependencies have failed",
                    "one or more dependent jobs",
                ]
                .iter()
                .any(|marker| line.contains(marker))
            })
    })
}

fn dependency_steps(steps: &[Step]) -> bool {
    let conclusion_action = steps
        .iter()
        .any(|step| step.name.contains("workflow-conclusion-action"));
    if conclusion_action
        && steps
            .iter()
            .filter(|step| step.conclusion.as_deref() == Some("failure"))
            .all(|step| {
                step.name.contains("echo failure")
                    || step.name.to_ascii_lowercase().contains("conclusion")
            })
    {
        return true;
    }
    let failed: Vec<_> = steps
        .iter()
        .filter(|step| step.conclusion.as_deref() == Some("failure"))
        .collect();
    !failed.is_empty()
        && failed.iter().all(|step| {
            let name = step.name.to_ascii_lowercase();
            name.contains("dependenc") || (name.contains("job") && name.contains("result"))
        })
}

impl Evidence {
    /// Navigate the saved observation when available; omit its snapshot to refresh.
    pub fn command(&self) -> String {
        self.snapshot.as_ref().map_or_else(
            || self.reference.command(),
            |snapshot| format!("{} --snapshot {snapshot}", self.reference.command()),
        )
    }

    fn navigation(&self) -> Navigation {
        let mut nav = Navigation::job(&self.reference);
        nav.command = self.command();
        nav.failed_steps = self
            .steps
            .iter()
            .filter(|step| step.conclusion.as_deref() == Some("failure"))
            .filter_map(|step| Some((step.started_at.clone()?, step.completed_at.clone()?)))
            .collect();
        nav
    }
}

fn save_navigation(
    provider: &impl Provider,
    evidence: &mut Evidence,
    log_saved: bool,
) -> Navigation {
    // Only advertise zero-query navigation if both the metadata and log are saved.
    if log_saved
        && let Ok(bytes) = serde_json::to_vec(evidence)
        && let Some(snapshot) = crate::SnapshotRef::create(&evidence.reference.job_url(), &bytes)
        && provider.save_snapshot(&snapshot, &bytes, evidence.observed_at)
    {
        evidence.snapshot = Some(snapshot);
    }
    evidence.navigation()
}

fn saved_logs(
    provider: &impl Provider,
    reference: &CheckRef,
    snapshot: &crate::SnapshotRef,
    options: LogOptions<'_>,
    query: Query<'_>,
) -> Result<Evidence> {
    let invalid = || {
        Error::InvalidResponse(format!(
            "Saved job evidence missing, expired or invalid; refresh with {}",
            reference.command()
        ))
    };
    let cached = provider.read_snapshot(snapshot).map_err(|error| {
        Error::InvalidResponse(format!(
            "{}; refresh with {}",
            error.diagnostic(),
            reference.command()
        ))
    })?;
    if crate::SnapshotRef::create(&reference.job_url(), &cached.bytes)
        .is_none_or(|actual| actual.to_string() != snapshot.to_string())
    {
        return Err(invalid());
    }
    let mut evidence: Evidence = serde_json::from_slice(&cached.bytes).map_err(|_| invalid())?;
    if &evidence.reference != reference
        || evidence.status != "completed"
        || reference.attempt != Some(evidence.run_attempt)
        || crate::cache::now()
            .checked_sub(evidence.observed_at)
            .is_none_or(|age| age >= 300)
    {
        return Err(invalid());
    }
    let (mut file, fetched_at) = provider
        .cached_log_file(reference, evidence.run_attempt)
        .ok_or_else(invalid)?;
    evidence.snapshot = Some(snapshot.clone());
    evidence.source = crate::ObservationSource::Snapshot;
    evidence.log = Some(page(
        &mut file,
        &evidence.navigation(),
        options.start,
        options.lines,
        LogSource::Cache,
        fetched_at,
        query,
    )?);
    evidence.dependency_failure = dependency_failure(&evidence);
    compact_steps(&mut evidence, query);
    Ok(evidence)
}
