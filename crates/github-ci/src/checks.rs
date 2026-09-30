//! One traversal obtains status and requiredness together. No moving-status cache.
use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::status::CheckBucket;
use crate::{Error, Provider, PullRequest, Result};

pub(crate) const QUERY: &str = r"query($owner: String!, $repo: String!, $number: Int!, $endCursor: String) {
  repository(owner: $owner, name: $repo) {
    pullRequest(number: $number) {
      url number state headRefOid
      commits(last: 1) { nodes { commit {
        oid
        statusCheckRollup { contexts(first: 100, after: $endCursor) {
          pageInfo { hasNextPage endCursor }
          nodes {
            __typename
            ... on CheckRun {
              databaseId name status conclusion detailsUrl startedAt
              isRequired(pullRequestNumber: $number)
              checkSuite { workflowRun { event workflow { name } } }
            }
            ... on StatusContext {
              context state targetUrl createdAt
              isRequired(pullRequestNumber: $number)
            }
          }
        } }
      } } }
    }
  }
}";

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct Row {
    pub name: String,
    pub workflow: String,
    pub bucket: CheckBucket,
    pub link: String,
    pub required: Option<bool>,
    #[serde(default)]
    pub check_id: Option<u64>,
    #[serde(default)]
    pub job: Option<crate::evidence::Job>,
    #[serde(default)]
    pub status: Option<String>,
}

#[derive(Deserialize)]
struct Envelope {
    data: Option<serde_json::Value>,
    #[serde(default)]
    errors: Vec<GraphError>,
}
#[derive(Deserialize)]
struct GraphError {
    message: String,
    #[serde(default)]
    path: Vec<serde_json::Value>,
}
#[derive(Deserialize)]
struct Data {
    repository: Option<Repository>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Repository {
    pull_request: Option<Node>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Node {
    #[serde(flatten)]
    pr: PullRequest,
    commits: Commits,
}
#[derive(Deserialize)]
struct Commits {
    nodes: Vec<CommitNode>,
}
#[derive(Deserialize)]
struct CommitNode {
    commit: Commit,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Commit {
    oid: String,
    status_check_rollup: Option<Rollup>,
}
#[derive(Deserialize)]
struct Rollup {
    contexts: Contexts,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Contexts {
    page_info: PageInfo,
    nodes: Vec<Context>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PageInfo {
    has_next_page: bool,
    end_cursor: Option<String>,
}
#[derive(Deserialize)]
#[serde(tag = "__typename")]
enum Context {
    CheckRun {
        #[serde(rename = "databaseId", default)]
        database_id: Option<serde_json::Value>,
        name: String,
        status: String,
        conclusion: Option<String>,
        #[serde(rename = "detailsUrl")]
        details_url: Option<String>,
        #[serde(rename = "startedAt")]
        started_at: Option<String>,
        #[serde(rename = "isRequired")]
        is_required: Option<bool>,
        #[serde(rename = "checkSuite")]
        check_suite: Option<Suite>,
    },
    StatusContext {
        context: String,
        state: String,
        #[serde(rename = "targetUrl")]
        target_url: Option<String>,
        #[serde(rename = "createdAt")]
        created_at: String,
        #[serde(rename = "isRequired")]
        is_required: Option<bool>,
    },
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Suite {
    workflow_run: Option<WorkflowRun>,
}
#[derive(Deserialize)]
struct WorkflowRun {
    event: String,
    workflow: Option<Workflow>,
}
#[derive(Deserialize)]
struct Workflow {
    name: String,
}

pub(crate) fn fetch(
    provider: &impl Provider,
    url: &str,
    expected_head: Option<&str>,
) -> Result<(PullRequest, Vec<Row>, Vec<String>)> {
    let identity = identity_from_url(url)?;
    let output = request(provider, &identity)?;
    let pages: Vec<Envelope> = match serde_json::from_slice(&output.stdout) {
        Ok(pages) => pages,
        Err(_) if !output.status.success() => {
            return Err(Error::Query {
                operation: "fetch pull-request checks".into(),
                message: String::from_utf8_lossy(&output.stderr).trim().into(),
            });
        }
        Err(error) => return Err(error.into()),
    };
    let mut observed: Option<PullRequest> = None;
    let mut contexts = Vec::new();
    let mut warnings = Vec::new();
    let mut seen = HashSet::new();
    let mut has_next_page = true;
    for envelope in pages {
        if !has_next_page {
            return Err(Error::InvalidResponse(
                "GitHub returned an unexpected checks page".into(),
            ));
        }
        let node = decode_node(envelope, &mut warnings)?;
        let returned = identity_from_url(&node.pr.url)?;
        if !identity.0.eq_ignore_ascii_case(&returned.0)
            || identity.1 != returned.1
            || identity.1 != node.pr.number
        {
            return Err(Error::InvalidResponse(
                "GitHub returned a different pull-request identity".into(),
            ));
        }
        let commit = node
            .commits
            .nodes
            .into_iter()
            .next()
            .ok_or_else(|| {
                Error::InvalidResponse("GitHub returned no pull-request head commit".into())
            })?
            .commit;
        let expected = observed
            .as_ref()
            .map(|pr| pr.head_ref_oid.as_str())
            .or(expected_head);
        if expected.is_some_and(|head| head != node.pr.head_ref_oid)
            || commit.oid != node.pr.head_ref_oid
        {
            return Err(Error::InvalidResponse(
                "PR head changed during inspection; run dots github ci again".into(),
            ));
        }
        github_client::Target::parse(&node.pr.url)?;
        observed = Some(node.pr);
        let Some(rollup) = commit.status_check_rollup else {
            has_next_page = false;
            continue;
        };
        contexts.extend(rollup.contexts.nodes);
        let page = rollup.contexts.page_info;
        has_next_page = page.has_next_page;
        if has_next_page {
            let next = page
                .end_cursor
                .filter(|cursor| !cursor.is_empty())
                .ok_or_else(|| {
                    Error::InvalidResponse("GitHub checks page has no continuation cursor".into())
                })?;
            if !seen.insert(next) {
                return Err(Error::InvalidResponse(
                    "GitHub checks pagination repeated a cursor".into(),
                ));
            }
        }
    }
    if !output.status.success() && warnings.is_empty() {
        return Err(Error::Query {
            operation: "fetch pull-request checks".into(),
            message: String::from_utf8_lossy(&output.stderr).trim().into(),
        });
    }
    if has_next_page {
        return Err(Error::InvalidResponse(
            "GitHub checks pagination ended before all checks were read".into(),
        ));
    }
    let pr =
        observed.ok_or_else(|| Error::InvalidResponse("GitHub returned no checks pages".into()))?;
    warnings.sort();
    warnings.dedup();
    Ok((pr, deduplicate(contexts), warnings))
}

fn identity_from_url(url: &str) -> Result<(String, u64)> {
    let repository = github_client::Target::parse(url)?
        .repository()?
        .ok_or_else(|| Error::InvalidReference(url.into()))?;
    let path = url.split(['?', '#']).next().unwrap_or(url);
    let number = path
        .split('/')
        .nth(6)
        .and_then(|part| part.parse().ok())
        .ok_or_else(|| Error::InvalidReference(url.into()))?;
    Ok((repository.display(), number))
}

fn request(provider: &impl Provider, identity: &(String, u64)) -> Result<std::process::Output> {
    let mut parts = identity.0.split('/');
    let host = parts
        .next()
        .ok_or_else(|| Error::InvalidReference(identity.0.clone()))?;
    let owner = format!(
        "owner={}",
        parts
            .next()
            .ok_or_else(|| Error::InvalidReference(identity.0.clone()))?
    );
    let repo = format!(
        "repo={}",
        parts
            .next()
            .ok_or_else(|| Error::InvalidReference(identity.0.clone()))?
    );
    let number = format!("number={}", identity.1);
    let query = format!("query={QUERY}");
    // gh reuses its HTTP connection between pages. Its paginator follows the
    // endCursor variable; slurp makes page boundaries explicit for validation.
    provider.gh(&[
        "api",
        "graphql",
        "--hostname",
        host,
        "--paginate",
        "--slurp",
        "-f",
        &query,
        "-f",
        &owner,
        "-f",
        &repo,
        "-F",
        &number,
    ])
}

fn decode_node(envelope: Envelope, warnings: &mut Vec<String>) -> Result<Node> {
    let mut page_warnings = Vec::new();
    for error in envelope.errors {
        if error.path.last().and_then(serde_json::Value::as_str) != Some("isRequired") {
            return Err(Error::Query {
                operation: "fetch pull-request checks".into(),
                message: error.message,
            });
        }
        page_warnings.push(format!(
            "Required-check classification unavailable: {}",
            error.message
        ));
    }
    let data: Data = serde_json::from_value(envelope.data.unwrap_or(serde_json::Value::Null))
        .map_err(|error| {
            if page_warnings.is_empty() {
                Error::Decode(error)
            } else {
                Error::InvalidResponse(format!(
                    "GitHub omitted check status after a requiredness error: {}",
                    page_warnings.join("; ")
                ))
            }
        })?;
    warnings.extend(page_warnings);
    data.repository
        .and_then(|repo| repo.pull_request)
        .ok_or_else(|| {
            Error::InvalidResponse(
                "GitHub returned no pull request for the requested identity".into(),
            )
        })
}

pub(crate) fn bucket(state: &str) -> CheckBucket {
    match state {
        "SUCCESS" => CheckBucket::Pass,
        "SKIPPED" | "NEUTRAL" => CheckBucket::Skipping,
        "ERROR" | "FAILURE" | "TIMED_OUT" | "ACTION_REQUIRED" | "STARTUP_FAILURE" => {
            CheckBucket::Fail
        }
        "CANCELLED" => CheckBucket::Cancel,
        "EXPECTED" | "REQUESTED" | "WAITING" | "QUEUED" | "PENDING" | "IN_PROGRESS" | "STALE" => {
            CheckBucket::Pending
        }
        _ => CheckBucket::Unknown,
    }
}

fn deduplicate(contexts: Vec<Context>) -> Vec<Row> {
    // Match gh's latest-run grouping without constructing ambiguous slash keys.
    // Context and CheckRun namespaces stay separate. All timestamps are GitHub UTC.
    let mut rows = HashMap::<(bool, String, String, String), (String, Row)>::new();
    for context in contexts {
        let (key, started, row) = project(context);
        match rows.entry(key) {
            std::collections::hash_map::Entry::Vacant(entry) => {
                entry.insert((started, row));
            }
            std::collections::hash_map::Entry::Occupied(mut entry) => {
                if started > entry.get().0 {
                    entry.insert((started, row));
                }
            }
        }
    }
    rows.into_values().map(|(_, row)| row).collect()
}

type GroupKey = (bool, String, String, String);
fn project(context: Context) -> (GroupKey, String, Row) {
    match context {
        Context::CheckRun {
            database_id,
            name,
            status,
            conclusion,
            details_url,
            started_at,
            is_required,
            check_suite,
        } => {
            let run = check_suite.and_then(|suite| suite.workflow_run);
            let event = run
                .as_ref()
                .map(|run| run.event.clone())
                .unwrap_or_default();
            let workflow = run
                .and_then(|run| run.workflow)
                .map(|workflow| workflow.name)
                .unwrap_or_default();
            let bucket = bucket(if status == "COMPLETED" {
                conclusion.as_deref().unwrap_or_default()
            } else {
                &status
            });
            (
                (true, name.clone(), workflow.clone(), event),
                started_at.unwrap_or_default(),
                Row {
                    name,
                    workflow,
                    bucket,
                    link: details_url.unwrap_or_default(),
                    required: is_required,
                    job: None,
                    status: Some(conclusion.unwrap_or(status)),
                    check_id: database_id
                        .and_then(|value| value.as_u64().or_else(|| value.as_str()?.parse().ok()))
                        .filter(|id| *id > 0),
                },
            )
        }
        Context::StatusContext {
            context,
            state,
            target_url,
            created_at,
            is_required,
        } => (
            (false, context.clone(), String::new(), String::new()),
            created_at,
            Row {
                name: context,
                workflow: String::new(),
                bucket: bucket(&state),
                link: target_url.unwrap_or_default(),
                required: is_required,
                job: None,
                status: Some(state),
                check_id: None,
            },
        ),
    }
}

pub(crate) const COMMIT_QUERY: &str = r"query($owner:String!, $repo:String!, $head:String!, $endCursor:String) {
  repository(owner:$owner,name:$repo) { object(expression:$head) { ... on Commit {
    oid statusCheckRollup { contexts(first:100,after:$endCursor) {
      pageInfo { hasNextPage endCursor }
      nodes { __typename
        ... on CheckRun { databaseId name status conclusion detailsUrl startedAt
          checkSuite { workflowRun { event workflow { name } } } }
        ... on StatusContext { context state targetUrl createdAt }
      }
    } }
  } } }
}";

#[derive(Deserialize)]
struct CommitData {
    repository: Option<CommitRepository>,
}
#[derive(Deserialize)]
struct CommitRepository {
    object: Option<Commit>,
}

pub(crate) fn fetch_commit(
    provider: &impl Provider,
    repo: &crate::Repository,
    head: &str,
) -> Result<Vec<Row>> {
    let query = format!("query={COMMIT_QUERY}");
    let owner = format!("owner={}", repo.owner);
    let name = format!("repo={}", repo.repo);
    let revision = format!("head={head}");
    let bytes = crate::provider::checked(
        provider,
        &[
            "api",
            "graphql",
            "--hostname",
            &repo.host,
            "--paginate",
            "--slurp",
            "-f",
            &query,
            "-f",
            &owner,
            "-f",
            &name,
            "-f",
            &revision,
        ],
    )?;
    let pages: Vec<Envelope> = serde_json::from_slice(&bytes)?;
    let mut next = true;
    let mut cursors = HashSet::new();
    let mut ids = HashSet::new();
    let mut checks = Vec::new();
    let mut statuses = HashMap::<String, (String, Row)>::new();
    for page in pages {
        if !next || !page.errors.is_empty() {
            return Err(Error::InvalidResponse(
                "commit check query returned errors or unexpected pages".into(),
            ));
        }
        let data: CommitData = serde_json::from_value(page.data.ok_or_else(|| {
            Error::InvalidResponse("commit check query returned no data".into())
        })?)?;
        let commit = data
            .repository
            .and_then(|repo| repo.object)
            .ok_or_else(|| {
                Error::InvalidResponse("commit was not found in the selected repository".into())
            })?;
        if commit.oid != head {
            return Err(Error::InvalidResponse(
                "commit check query returned a different revision".into(),
            ));
        }
        let Some(rollup) = commit.status_check_rollup else {
            next = false;
            continue;
        };
        for context in rollup.contexts.nodes {
            let (key, created, row) = project(context);
            if key.0 {
                if row.check_id.is_some_and(|id| !ids.insert(id)) {
                    return Err(Error::InvalidResponse(
                        "duplicate check identity during pagination".into(),
                    ));
                }
                checks.push(row);
            } else {
                match statuses.entry(row.name.clone()) {
                    std::collections::hash_map::Entry::Vacant(entry) => {
                        entry.insert((created, row));
                    }
                    std::collections::hash_map::Entry::Occupied(mut entry) => {
                        if created >= entry.get().0 {
                            entry.insert((created, row));
                        }
                    }
                }
            }
        }
        let info = rollup.contexts.page_info;
        next = info.has_next_page;
        if next
            && !info
                .end_cursor
                .filter(|cursor| !cursor.is_empty())
                .is_some_and(|cursor| cursors.insert(cursor))
        {
            return Err(Error::InvalidResponse(
                "commit check pagination has no new cursor".into(),
            ));
        }
    }
    if next {
        return Err(Error::InvalidResponse(
            "incomplete commit check traversal".into(),
        ));
    }
    checks.extend(statuses.into_values().map(|(_, row)| row));
    Ok(checks)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn run(started: Option<&str>, state: &str, event: &str) -> Context {
        serde_json::from_value(json!({
            "__typename":"CheckRun", "name":"tests", "status":"COMPLETED", "conclusion":state,
            "startedAt":started,"detailsUrl":null,"isRequired":true,
            "checkSuite":{"workflowRun":{"event":event,"workflow":{"name":"CI"}}}
        }))
        .unwrap()
    }

    #[test]
    fn reruns_follow_gh_latest_started_grouping_but_distinct_events_survive() {
        let rows = deduplicate(vec![
            run(Some("2026-01-01T00:00:00Z"), "FAILURE", "push"),
            run(Some("2026-01-02T00:00:00Z"), "SUCCESS", "push"),
            run(None, "PENDING", "push"),
            run(Some("2026-01-02T00:00:00Z"), "FAILURE", "pull_request"),
        ]);
        assert_eq!(rows.len(), 2);
        assert!(rows.iter().any(|row| row.bucket == CheckBucket::Pass));
        assert!(rows.iter().any(|row| row.bucket == CheckBucket::Fail));
    }

    #[test]
    fn legacy_status_contexts_deduplicate_and_unknown_states_fail_closed() {
        let context = |date, state| {
            serde_json::from_value(json!({
                "__typename":"StatusContext","context":"tests","state":state,
                "createdAt":date,"targetUrl":null,"isRequired":false
            }))
            .unwrap()
        };
        let rows = deduplicate(vec![
            context("2026-01-01T00:00:00Z", "SUCCESS"),
            context("2026-01-02T00:00:00Z", "NEW_STATE"),
            run(Some("2026-01-02T00:00:00Z"), "NEUTRAL", "push"),
        ]);
        assert_eq!(rows.len(), 2);
        assert!(
            rows.iter()
                .any(|row| row.bucket == CheckBucket::Unknown && row.required == Some(false))
        );
        assert!(rows.iter().any(|row| row.bucket == CheckBucket::Skipping));
    }
}
