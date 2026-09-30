//! Status collection for explicit commits and run attempts.
use crate::{
    CheckRef, Error, InspectOptions, Inspection, Provider, Repository, Result, Subject, Target,
};
use crate::{
    checks::Row,
    provider::checked,
    snapshot::Snapshot,
    target::{self, Resource},
};
use serde::Deserialize;
use std::collections::HashSet;

pub fn inspect_target(
    provider: &impl Provider,
    target: Target<'_>,
    repository: Option<&Repository>,
    options: InspectOptions<'_>,
) -> Result<Inspection> {
    if options.limit == 0 {
        return Err(Error::InvalidResponse(
            "check limit must be positive".into(),
        ));
    }
    let (subject, checks, warnings) = match target {
        Target::PullRequest(value) => {
            let explicit = if let Some(value) = value {
                if value.starts_with("https://") {
                    let (repo, Resource::Pr(_)) = Resource::parse(value)? else {
                        return Err(target::invalid());
                    };
                    target::repository(provider, repository, Some(repo))?;
                    Some(value.to_owned())
                } else if let Some(repo) = repository {
                    Some(format!("{}/pull/{}", repo.url(), target::id(value)?))
                } else {
                    Some(value.into())
                }
            } else {
                None
            };
            if explicit.is_none() && repository.is_some() {
                return Err(Error::InvalidReference(
                    "--repo requires an explicit PR, ref, SHA or run".into(),
                ));
            }
            return crate::inspect_with_options(provider, explicit.as_deref(), options);
        }
        Target::Ref(value) => {
            if value.is_empty() || value.len() > 1024 || value.chars().any(char::is_control) {
                return Err(Error::InvalidReference(
                    "ref must be a nonempty remote branch or tag name".into(),
                ));
            }
            let repo = target::repository(provider, repository, None)?;
            // Resolve remote branch/tag explicitly, detecting ambiguous short names.
            let head = resolve_ref(provider, &repo, value)?;
            let checks = crate::checks::fetch_commit(provider, &repo, &head)?;
            (
                Subject::Commit {
                    repository: repo,
                    sha: head,
                    requested_ref: Some(value.into()),
                },
                checks,
                vec![],
            )
        }
        Target::Commit(value) => {
            let head = target::sha(value)?;
            let repo = target::repository(provider, repository, None)?;
            let checks = crate::checks::fetch_commit(provider, &repo, &head)?;
            (
                Subject::Commit {
                    repository: repo,
                    sha: head,
                    requested_ref: None,
                },
                checks,
                vec![],
            )
        }
        Target::Run { value, attempt } => run(provider, repository, value, attempt)?,
    };
    let inspection = crate::inspect::empty();
    let snapshot = Snapshot {
        subject,
        checks,
        warnings,
        observed_at: inspection.observed_at,
    };
    Ok(crate::inspect::complete(
        provider, inspection, snapshot, options,
    ))
}
fn api(provider: &impl Provider, repo: &Repository, path: &str, paged: bool) -> Result<Vec<u8>> {
    let endpoint = repo.endpoint(path);
    let mut args = vec!["api", "--hostname", &repo.host, &endpoint];
    if paged {
        args.extend(["--paginate", "--slurp"]);
    }
    checked(provider, &args)
}
fn resolve_ref(provider: &impl Provider, repo: &Repository, value: &str) -> Result<String> {
    // GitHub's commit resolver accepts fully qualified refs; use matching-refs to
    // reject branch/tag ambiguity without silently treating a branch as a SHA.
    #[derive(Deserialize)]
    struct GitRef {
        #[serde(rename = "ref")]
        name: String,
        object: Object,
    }
    #[derive(Deserialize)]
    struct Object {
        sha: String,
        #[serde(rename = "type")]
        kind: String,
    }
    #[derive(Deserialize)]
    struct Tag {
        sha: String,
        object: Object,
    }
    let names = if value.starts_with("refs/heads/") || value.starts_with("refs/tags/") {
        vec![value.to_owned()]
    } else {
        vec![format!("refs/heads/{value}"), format!("refs/tags/{value}")]
    };
    let mut matches = Vec::new();
    for name in names {
        let suffix = name.strip_prefix("refs/").unwrap_or(&name);
        let bytes = api(
            provider,
            repo,
            &format!("git/matching-refs/{}", target::encode(suffix)),
            true,
        )?;
        let pages: Vec<Vec<GitRef>> = serde_json::from_slice(&bytes)?;
        matches.extend(pages.into_iter().flatten().filter(|item| item.name == name));
    }
    let [resolved] = matches.as_slice() else {
        return Err(Error::InvalidReference(
            "remote ref is missing or ambiguous; use refs/heads/NAME or refs/tags/NAME".into(),
        ));
    };
    let mut object = Object {
        sha: resolved.object.sha.clone(),
        kind: resolved.object.kind.clone(),
    };
    for _ in 0..8 {
        let head = target::sha(&object.sha)?;
        if object.kind == "commit" {
            return Ok(head);
        }
        if object.kind != "tag" {
            return Err(Error::InvalidResponse(
                "ref does not point to a commit or annotated tag".into(),
            ));
        }
        let tag: Tag =
            serde_json::from_slice(&api(provider, repo, &format!("git/tags/{head}"), false)?)?;
        if tag.sha != head {
            return Err(Error::InvalidResponse(
                "tag object identity mismatch".into(),
            ));
        }
        object = tag.object;
    }
    Err(Error::InvalidResponse(
        "annotated tag chain exceeds eight objects".into(),
    ))
}
#[derive(Deserialize)]
struct RunMetadata {
    id: u64,
    run_attempt: u64,
    head_sha: String,
    name: Option<String>,
    event: String,
    head_branch: Option<String>,
    status: String,
    conclusion: Option<String>,
}
#[derive(Deserialize)]
struct Jobs {
    total_count: usize,
    jobs: Vec<RunJob>,
}
#[derive(Deserialize)]
struct RunJob {
    #[serde(flatten)]
    metadata: crate::evidence::Job,
    check_run_url: Option<String>,
}
fn run(
    provider: &impl Provider,
    repository: Option<&Repository>,
    value: &str,
    attempt: Option<u64>,
) -> Result<(Subject, Vec<Row>, Vec<String>)> {
    let (repo, id, attempt) = if value.starts_with("https://") {
        let (
            repo,
            Resource::Run {
                id,
                attempt: url_attempt,
            },
        ) = Resource::parse(value)?
        else {
            return Err(target::invalid());
        };
        if url_attempt.is_some() && attempt.is_some() && url_attempt != attempt {
            return Err(Error::InvalidReference(
                "URL attempt conflicts with --attempt".into(),
            ));
        }
        (
            target::repository(provider, repository, Some(repo))?,
            id,
            attempt.or(url_attempt),
        )
    } else {
        (
            target::repository(provider, repository, None)?,
            target::id(value)?,
            attempt,
        )
    };
    if attempt == Some(0) {
        return Err(target::invalid());
    }
    let endpoint = attempt.map_or_else(
        || format!("actions/runs/{id}"),
        |a| format!("actions/runs/{id}/attempts/{a}"),
    );
    let wire: RunMetadata = serde_json::from_slice(&api(provider, &repo, &endpoint, false)?)?;
    if wire.id != id || wire.run_attempt == 0 || attempt.is_some_and(|a| a != wire.run_attempt) {
        return Err(Error::InvalidResponse(
            "workflow run identity/attempt mismatch".into(),
        ));
    }
    let head = target::sha(&wire.head_sha)?;
    let pages: Vec<Jobs> = serde_json::from_slice(&api(
        provider,
        &repo,
        &format!(
            "actions/runs/{id}/attempts/{}/jobs?per_page=100",
            wire.run_attempt
        ),
        true,
    )?)?;
    let checks = run_rows(pages, &repo, &wire, &head)?;
    let mut warnings = Vec::new();
    if checks.is_empty() {
        warnings.push("No jobs reported for this attempt; inspect run status and any check output. This does not establish success.".into());
    }
    Ok((
        Subject::Run {
            repository: repo,
            id,
            attempt: wire.run_attempt,
            sha: head,
            name: wire.name.unwrap_or_default(),
            event: wire.event,
            branch: wire.head_branch,
            status: wire.status,
            conclusion: wire.conclusion,
        },
        checks,
        warnings,
    ))
}

fn run_rows(
    pages: Vec<Jobs>,
    repo: &Repository,
    wire: &RunMetadata,
    head: &str,
) -> Result<Vec<Row>> {
    let id = wire.id;
    let total = pages
        .first()
        .ok_or_else(|| Error::InvalidResponse("missing job response pages".into()))?
        .total_count;
    let mut seen = HashSet::new();
    let mut checks = Vec::new();
    for page in pages {
        if page.total_count != total {
            return Err(Error::InvalidResponse(
                "job collection changed during pagination; refresh".into(),
            ));
        }
        for wire_job in page.jobs {
            let job = wire_job.metadata;
            if job.run_id != id
                || job.run_attempt != wire.run_attempt
                || job.head_sha != head
                || !seen.insert(job.id)
                || job.id == 0
            {
                return Err(Error::InvalidResponse(
                    "job identity/revision/attempt mismatch".into(),
                ));
            }
            let reference = CheckRef {
                repository: repo.clone(),
                pr: None,
                head: head.into(),
                run: id,
                attempt: Some(wire.run_attempt),
                job: job.id,
            };
            let state = job.conclusion.as_deref().unwrap_or(&job.status);
            checks.push(Row {
                name: job.name.clone(),
                workflow: wire.name.clone().unwrap_or_default(),
                bucket: crate::checks::bucket(&state.to_ascii_uppercase()),
                status: Some(state.into()),
                link: reference.job_url(),
                required: None,
                check_id: wire_job
                    .check_run_url
                    .and_then(|url| url.rsplit('/').next()?.parse().ok()),
                job: Some(job),
            });
        }
    }
    if checks.len() != total {
        return Err(Error::InvalidResponse(
            "incomplete job traversal; no complete summary available".into(),
        ));
    }
    Ok(checks)
}
