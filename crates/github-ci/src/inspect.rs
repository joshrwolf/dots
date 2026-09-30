use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::status::CheckBucket;
use crate::{CheckRef, Error, Evidence, Provider, Result, Subject};

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PullRequest {
    pub url: String,
    pub number: u64,
    pub head_ref_oid: String,
    pub state: String,
}

#[derive(Debug, Serialize)]
pub struct Inspection {
    pub source: ObservationSource,
    pub snapshot: Option<crate::SnapshotRef>,
    pub observed_at: u64,
    pub subject: Option<Subject>,
    pub local_head: Option<String>,
    pub checks: Vec<Check>,
    pub summary: Summary,
    pub total_checks: usize,
    pub matching_checks: usize,
    pub hidden_checks: usize,
    pub all_checks_command: Option<String>,
    pub offset: usize,
    pub omitted_checks: usize,
    pub next_command: Option<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, serde::Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ObservationSource {
    Network,
    Snapshot,
}

#[derive(Debug, Default, Serialize)]
pub struct Summary {
    pub passed: usize,
    pub failed: usize,
    pub pending: usize,
    pub cancelled: usize,
    pub skipped: usize,
    pub unknown: usize,
}

impl Summary {
    fn observe(&mut self, bucket: CheckBucket) {
        let count = match bucket {
            CheckBucket::Pass => &mut self.passed,
            CheckBucket::Fail => &mut self.failed,
            CheckBucket::Pending => &mut self.pending,
            CheckBucket::Cancel => &mut self.cancelled,
            CheckBucket::Skipping => &mut self.skipped,
            CheckBucket::Unknown => &mut self.unknown,
        };
        *count += 1;
    }
}

#[derive(Debug, Serialize)]
pub struct Check {
    pub name: String,
    pub workflow: String,
    pub bucket: CheckBucket,
    pub status: Option<String>,
    pub link: String,
    /// None means classification was unavailable, not that a check is optional.
    pub required: Option<bool>,
    pub reference: Option<CheckRef>,
    pub next_command: Option<String>,
    pub evidence: Option<Evidence>,
    pub evidence_note: Option<String>,
    pub output_reference: Option<crate::OutputRef>,
    pub output_command: Option<String>,
    pub output_evidence: Option<crate::CheckOutput>,
}

/// Inspect one PR. Provider calls remain separate from rendering and harnesses.
/// The default includes at most two small job excerpts; every check has a link.
pub fn inspect(
    provider: &impl Provider,
    target: Option<&str>,
    include_evidence: bool,
) -> Result<Inspection> {
    inspect_with_options(
        provider,
        target,
        InspectOptions {
            include_evidence,
            offset: 0,
            limit: 20,
            show_all: false,
            name: None,
        },
    )
}

#[derive(Debug, Clone, Copy)]
pub struct InspectOptions<'a> {
    pub include_evidence: bool,
    pub offset: usize,
    pub limit: usize,
    pub show_all: bool,
    pub name: Option<&'a str>,
}

pub fn inspect_with_options(
    provider: &impl Provider,
    target: Option<&str>,
    options: InspectOptions<'_>,
) -> Result<Inspection> {
    if options.limit == 0 {
        return Err(Error::InvalidResponse(
            "check limit must be positive".into(),
        ));
    }
    let explicit = target.map(github_client::Target::parse).transpose()?;
    let inspection = empty();
    let (pr, all, warnings) = if let Some(github_client::Target::Url(url)) = explicit {
        crate::checks::fetch(provider, &url, None)?
    } else {
        let mut args = vec!["pr", "view"];
        if let Some(target) = target {
            args.push(target);
        }
        args.extend(["--json", "url,number,headRefOid,state"]);
        let result = provider.gh(&args)?;
        if !result.status.success() {
            let message = String::from_utf8_lossy(&result.stderr).trim().to_owned();
            if target.is_none() && message.to_lowercase().contains("no pull requests found") {
                return Ok(inspection);
            }
            return Err(Error::Query {
                operation: "resolve pull request".into(),
                message,
            });
        }
        let resolved: PullRequest = serde_json::from_slice(&result.stdout)?;
        crate::checks::fetch(provider, &resolved.url, Some(&resolved.head_ref_oid))?
    };
    let repository = crate::Repository::from_url(&pr.url)?.0;
    let snapshot = crate::snapshot::Snapshot {
        subject: Subject::PullRequest { repository, pr },
        checks: all,
        warnings,
        observed_at: inspection.observed_at,
    };
    Ok(complete(provider, inspection, snapshot, options))
}

pub(crate) fn complete(
    provider: &impl Provider,
    mut inspection: Inspection,
    mut snapshot: crate::snapshot::Snapshot,
    options: InspectOptions<'_>,
) -> Inspection {
    snapshot.checks.sort_by_key(|check| {
        (
            severity(check.bucket),
            check.required != Some(true),
            check.name.clone(),
            check.link.clone(),
        )
    });
    if snapshot.checks.len() > options.offset.saturating_add(options.limit)
        || !options.show_all
        || options.name.is_some()
    {
        inspection.snapshot = crate::snapshot::save(provider, &snapshot);
    }
    finish(provider, inspection, snapshot, options)
}

pub(crate) fn empty() -> Inspection {
    Inspection {
        source: ObservationSource::Network,
        snapshot: None,
        observed_at: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
        subject: None,
        local_head: None,
        checks: Vec::new(),
        summary: Summary::default(),
        total_checks: 0,
        matching_checks: 0,
        hidden_checks: 0,
        all_checks_command: None,
        offset: 0,
        omitted_checks: 0,
        next_command: None,
        warnings: Vec::new(),
    }
}

pub fn inspect_snapshot(
    provider: &impl Provider,
    reference: &crate::SnapshotRef,
    offset: usize,
    limit: usize,
) -> Result<Inspection> {
    inspect_snapshot_filtered(
        provider,
        reference,
        InspectOptions {
            include_evidence: false,
            offset,
            limit,
            show_all: false,
            name: None,
        },
    )
}

pub fn inspect_snapshot_filtered(
    provider: &impl Provider,
    reference: &crate::SnapshotRef,
    mut options: InspectOptions<'_>,
) -> Result<Inspection> {
    options.include_evidence = false;
    let limit = options.limit;
    if limit == 0 {
        return Err(Error::InvalidResponse(
            "check limit must be positive".into(),
        ));
    }
    let snapshot = crate::snapshot::load(provider, reference)?;
    let inspection = Inspection {
        observed_at: snapshot.observed_at,
        source: ObservationSource::Snapshot,
        snapshot: Some(reference.clone()),
        subject: None,
        local_head: None,
        checks: Vec::new(),
        summary: Summary::default(),
        total_checks: 0,
        matching_checks: 0,
        hidden_checks: 0,
        all_checks_command: None,
        offset: 0,
        omitted_checks: 0,
        next_command: None,
        warnings: Vec::new(),
    };
    Ok(finish(provider, inspection, snapshot, options))
}

fn finish(
    provider: &impl Provider,
    mut inspection: Inspection,
    snapshot: crate::snapshot::Snapshot,
    options: InspectOptions<'_>,
) -> Inspection {
    let crate::snapshot::Snapshot {
        subject,
        checks: all,
        warnings,
        ..
    } = snapshot;
    match provider.local_head_for(&subject.url()) {
        Ok(head) => inspection.local_head = head,
        Err(error) => inspection.warnings.push(format!(
            "Local comparison unavailable: {}",
            error.diagnostic()
        )),
    }

    inspection.warnings.extend(warnings);
    inspection.total_checks = all.len();
    for check in &all {
        inspection.summary.observe(check.bucket);
    }
    let filtered: Vec<_> = all
        .into_iter()
        .filter(|check| {
            options.name.map_or_else(
                || {
                    options.show_all
                        || !matches!(check.bucket, CheckBucket::Pass | CheckBucket::Skipping)
                },
                |name| check.name.to_lowercase().contains(&name.to_lowercase()),
            )
        })
        .collect();
    inspection.matching_checks = filtered.len();
    inspection.hidden_checks = inspection.total_checks - filtered.len();
    inspection.offset = options.offset.min(filtered.len());
    let selected: Vec<_> = filtered
        .into_iter()
        .skip(options.offset)
        .take(options.limit)
        .collect();
    inspection.omitted_checks = inspection.matching_checks - selected.len();
    let target = inspection.snapshot.as_ref().map_or_else(
        || {
            subject
                .command()
                .trim_start_matches("dots github ci ")
                .to_owned()
        },
        |reference| format!("--snapshot {reference}"),
    );
    inspection.all_checks_command = Some(format!("dots github ci {target} --all --no-logs"));
    let end = inspection.offset + selected.len();
    if end < inspection.matching_checks {
        let filter = options.name.map_or_else(
            || {
                if options.show_all {
                    " --all".into()
                } else {
                    String::new()
                }
            },
            |name| format!(" --name '{}'", name.replace('\'', "'\"'\"'")),
        );
        inspection.next_command = Some(format!(
            "dots github ci {target}{filter} --offset {end} --limit {} --no-logs",
            options.limit
        ));
    }
    inspection.checks = project_checks(provider, &subject, selected, options.include_evidence);
    inspection.subject = Some(subject);
    inspection
}

fn severity(bucket: CheckBucket) -> u8 {
    match bucket {
        CheckBucket::Fail => 0,
        CheckBucket::Cancel => 1,
        CheckBucket::Unknown => 2,
        CheckBucket::Pending => 3,
        CheckBucket::Pass => 4,
        CheckBucket::Skipping => 5,
    }
}

fn project_checks(
    provider: &impl Provider,
    subject: &Subject,
    all: Vec<crate::checks::Row>,
    include_evidence: bool,
) -> Vec<Check> {
    let mut result = Vec::new();
    let mut remaining = if include_evidence { 2 } else { 0 };
    let mut attempts = 0;
    let mut downloads = 0;
    for mut check in all {
        let observed_job = check.job.take();
        let mut item = project_check(subject, check);
        if matches!(item.bucket, CheckBucket::Fail | CheckBucket::Cancel) {
            if let Some(reference) = &item.reference {
                if remaining > 0 && attempts < 10 && downloads < 4 {
                    attempts += 1;
                    match crate::evidence::logs_from_observation(
                        provider,
                        reference,
                        subject.head(),
                        observed_job,
                    ) {
                        Ok(evidence) => {
                            if !evidence.dependency_failure || evidence.log.is_some() {
                                downloads += 1;
                            }
                            if evidence.dependency_failure {
                                item.evidence_note = Some("Dependency-failure markers found; inspect other failed jobs. This is a heuristic, not a dependency graph.".into());
                            } else {
                                remaining -= 1;
                            }
                            item.next_command = Some(evidence.command());
                            item.reference = Some(evidence.reference.clone());
                            item.evidence = Some(evidence);
                        }
                        Err(error) => {
                            downloads += 1;
                            remaining -= 1;
                            item.evidence_note =
                                Some(format!("Evidence unavailable: {}", error.diagnostic()));
                        }
                    }
                } else {
                    item.evidence_note = Some(if include_evidence {
                        "Evidence omitted by query budget; use the More command.".into()
                    } else {
                        "Automatic logs disabled; use the More command.".into()
                    });
                }
            } else if let Some(reference) = &item.output_reference {
                if remaining > 0 && attempts < 10 && downloads < 4 {
                    attempts += 1;
                    downloads += 1;
                    remaining -= 1;
                    match crate::output::output_observed(
                        provider,
                        reference,
                        crate::OutputOptions::default(),
                        Some(subject.head()),
                    ) {
                        Ok(output) => {
                            item.next_command = Some(output.command());
                            item.output_evidence = Some(output);
                        }
                        Err(error) => {
                            item.evidence_note = Some(format!(
                                "Check output unavailable: {}; use the More command.",
                                error.diagnostic()
                            ));
                        }
                    }
                } else {
                    item.evidence_note = Some(
                        "Automatic evidence disabled or budget exhausted; use the More command."
                            .into(),
                    );
                }
            } else {
                item.evidence_note = Some("No exact Actions job or GitHub check-run ID; follow the check link. Legacy statuses have no check output.".into());
            }
        }
        result.push(item);
    }
    result
}

fn project_check(subject: &Subject, check: crate::checks::Row) -> Check {
    let reference = CheckRef::from_subject(subject, &check.link);
    let required = check.required;
    let output_reference = check
        .check_id
        .and_then(|id| crate::OutputRef::create(&subject.url(), subject.head(), id));
    let output_command = output_reference.as_ref().map(crate::OutputRef::command);
    let next_command = reference
        .as_ref()
        .map(CheckRef::command)
        .or_else(|| output_command.clone());
    Check {
        name: check.name,
        workflow: check.workflow,
        bucket: check.bucket,
        status: check.status,
        link: check.link,
        required,
        reference,
        next_command,
        evidence: None,
        evidence_note: None,
        output_reference,
        output_command,
        output_evidence: None,
    }
}
