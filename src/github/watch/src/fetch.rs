//! One aliased GraphQL request per chunk of watched PRs, through `gh`.
use std::fmt::Write as _;
use std::process::Output;
use std::time::Duration;

use serde::Deserialize;
use serde_json::{Map, Value};

use crate::status::{Ci, Review, State, Status};
use crate::store::Claimed;

/// The `gh` executable, or a test double answering with fixture output.
pub trait Gh {
    /// Run `gh` with `args`, bounded by `timeout`.
    fn gh(&self, args: &[&str], timeout: Duration) -> github_client::Result<Output>;
}

const TIMEOUT: Duration = Duration::from_secs(30);
const FIELDS: &str = "title body state isDraft reviewDecision headRefName commits(last: 1) { nodes { commit { statusCheckRollup { state } } } }";

/// One result per row, in order. A transport or authentication failure
/// becomes the error of every row; a GraphQL error within one PR's alias,
/// such as a missing PR, is an error of its row only.
pub(crate) fn fetch(gh: &dyn Gh, rows: &[Claimed]) -> Vec<Result<Status, String>> {
    let (query, variables) = request(rows);
    let mut args = vec!["api", "graphql", "--hostname", "github.com", "-f", &query];
    for (flag, variable) in &variables {
        args.extend([*flag, variable.as_str()]);
    }
    match gh.gh(&args, TIMEOUT) {
        Ok(output) => parse(&output, rows.len()),
        Err(error) => vec![Err(chain(&error)); rows.len()],
    }
}

/// Owners, names and numbers travel as GraphQL variables, never as query text.
fn request(rows: &[Claimed]) -> (String, Vec<(&'static str, String)>) {
    let mut parameters = Vec::new();
    let mut selections = String::new();
    let mut variables = Vec::new();
    for (index, row) in rows.iter().enumerate() {
        parameters.push(format!(
            "$o{index}: String!, $r{index}: String!, $n{index}: Int!"
        ));
        let _ = write!(
            selections,
            " p{index}: repository(owner: $o{index}, name: $r{index}) {{ pullRequest(number: $n{index}) {{ {FIELDS} }} }}"
        );
        variables.push(("-f", format!("o{index}={}", row.owner)));
        variables.push(("-f", format!("r{index}={}", row.repo)));
        variables.push(("-F", format!("n{index}={}", row.number)));
    }
    let query = format!("query=query({}) {{{selections} }}", parameters.join(", "));
    (query, variables)
}

#[derive(Debug, Deserialize)]
struct Response {
    #[serde(default)]
    data: Option<Map<String, Value>>,
    #[serde(default)]
    errors: Vec<GraphQlError>,
}

#[derive(Debug, Deserialize)]
struct GraphQlError {
    #[serde(default)]
    message: String,
    #[serde(default)]
    path: Vec<Value>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Repository {
    pull_request: Option<PullRequest>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PullRequest {
    title: String,
    body: String,
    state: String,
    is_draft: bool,
    review_decision: Option<String>,
    head_ref_name: String,
    commits: Commits,
}

#[derive(Debug, Deserialize)]
struct Commits {
    nodes: Vec<CommitNode>,
}

#[derive(Debug, Deserialize)]
struct CommitNode {
    commit: Commit,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Commit {
    status_check_rollup: Option<Rollup>,
}

#[derive(Debug, Deserialize)]
struct Rollup {
    state: String,
}

/// `gh api graphql` prints the response body even when it exits nonzero for
/// GraphQL errors, so per-alias data wins over the exit status.
///
/// GraphQL nulls a failed field and reports the error by path, so an error
/// anywhere under an alias makes that row's data untrustworthy; other aliases
/// still succeed. An error outside every alias, or a failed exit with no
/// error to explain it, fails every row.
fn parse(output: &Output, count: usize) -> Vec<Result<Status, String>> {
    let response = serde_json::from_slice::<Response>(&output.stdout).ok();
    let Some(Response {
        data: Some(data),
        errors,
    }) = response
    else {
        let reason = match response {
            Some(Response { errors, .. }) if !errors.is_empty() => messages(errors.iter()),
            _ => exit_reason(output),
        };
        return vec![Err(reason); count];
    };
    let mut by_alias: Vec<Vec<&GraphQlError>> = vec![Vec::new(); count];
    let mut unattributed = Vec::new();
    for error in &errors {
        match alias_index(error, count) {
            Some(index) => by_alias[index].push(error),
            None => unattributed.push(error),
        }
    }
    if !unattributed.is_empty() {
        let reason = format!(
            "GitHub reported an error for the whole request: {}",
            messages(unattributed.into_iter())
        );
        return vec![Err(reason); count];
    }
    if errors.is_empty() && !output.status.success() {
        return vec![Err(exit_reason(output)); count];
    }
    by_alias
        .into_iter()
        .enumerate()
        .map(|(index, alias_errors)| {
            if !alias_errors.is_empty() {
                return Err(messages(alias_errors.into_iter()));
            }
            let repository = data.get(&format!("p{index}")).cloned();
            match serde_json::from_value::<Option<Repository>>(repository.unwrap_or(Value::Null)) {
                Ok(None) => Err("repository not found or not accessible".into()),
                Ok(Some(Repository { pull_request: None })) => {
                    Err("pull request not found or not accessible".into())
                }
                Ok(Some(Repository {
                    pull_request: Some(pull_request),
                })) => status(pull_request),
                Err(error) => Err(format!(
                    "unreadable pull request in GitHub response: {error}"
                )),
            }
        })
        .collect()
}

/// The row a GraphQL error belongs to: the index of the alias its path starts
/// with, when that alias was requested.
fn alias_index(error: &GraphQlError, count: usize) -> Option<usize> {
    let alias = error.path.first()?.as_str()?;
    let index: usize = alias.strip_prefix('p')?.parse().ok()?;
    (index < count && alias == format!("p{index}")).then_some(index)
}

fn messages<'a>(errors: impl Iterator<Item = &'a GraphQlError>) -> String {
    errors
        .map(|error| error.message.as_str())
        .collect::<Vec<_>>()
        .join("; ")
}

fn exit_reason(output: &Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stderr = stderr.trim();
    if stderr.is_empty() {
        format!("gh api graphql returned no data ({})", output.status)
    } else {
        format!("gh api graphql failed: {stderr}")
    }
}

/// A PR always has a head commit; one missing from the response says nothing
/// about its checks.
fn status(pull_request: PullRequest) -> Result<Status, String> {
    let Some(head) = pull_request.commits.nodes.last() else {
        return Err("GitHub returned the pull request without its head commit".into());
    };
    let rollup = head
        .commit
        .status_check_rollup
        .as_ref()
        .map(|rollup| rollup.state.as_str());
    Ok(Status {
        state: state(&pull_request.state, pull_request.is_draft)
            .ok_or_else(|| format!("unrecognized pull request state {:?}", pull_request.state))?,
        review: review(pull_request.review_decision.as_deref()),
        ci: ci(rollup),
        title: pull_request.title,
        body: pull_request.body,
        head_ref: pull_request.head_ref_name,
    })
}

fn state(state: &str, draft: bool) -> Option<State> {
    match state {
        "MERGED" => Some(State::Merged),
        "CLOSED" => Some(State::Closed),
        "OPEN" if draft => Some(State::Draft),
        "OPEN" => Some(State::Open),
        _ => None,
    }
}

fn review(decision: Option<&str>) -> Review {
    match decision {
        Some("APPROVED") => Review::Approved,
        Some("CHANGES_REQUESTED") => Review::ChangesRequested,
        Some("REVIEW_REQUIRED") => Review::ReviewRequired,
        None => Review::None,
        Some(_) => Review::Unknown,
    }
}

fn ci(rollup: Option<&str>) -> Ci {
    match rollup {
        Some("SUCCESS") => Ci::Success,
        Some("FAILURE" | "ERROR") => Ci::Failure,
        Some("PENDING" | "EXPECTED") => Ci::Pending,
        None => Ci::None,
        Some(_) => Ci::Unknown,
    }
}

/// The error and its sources, each once.
fn chain(error: &dyn std::error::Error) -> String {
    let mut out = error.to_string();
    let mut source = error.source();
    while let Some(cause) = source {
        let _ = write!(out, ": {cause}");
        source = cause.source();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_graphql_enum_maps_including_nulls() {
        for (wire, draft, expected) in [
            ("OPEN", false, Some(State::Open)),
            ("OPEN", true, Some(State::Draft)),
            ("MERGED", false, Some(State::Merged)),
            ("CLOSED", true, Some(State::Closed)),
            ("LOCKED", false, None),
        ] {
            assert_eq!(state(wire, draft), expected, "{wire} draft={draft}");
        }
        for (wire, expected) in [
            (Some("APPROVED"), Review::Approved),
            (Some("CHANGES_REQUESTED"), Review::ChangesRequested),
            (Some("REVIEW_REQUIRED"), Review::ReviewRequired),
            (None, Review::None),
            (Some("NEW_DECISION"), Review::Unknown),
        ] {
            assert_eq!(review(wire), expected, "{wire:?}");
        }
        for (wire, expected) in [
            (Some("SUCCESS"), Ci::Success),
            (Some("FAILURE"), Ci::Failure),
            (Some("ERROR"), Ci::Failure),
            (Some("PENDING"), Ci::Pending),
            (Some("EXPECTED"), Ci::Pending),
            (None, Ci::None),
            (Some("NEW_STATE"), Ci::Unknown),
        ] {
            assert_eq!(ci(wire), expected, "{wire:?}");
        }
    }

    #[test]
    fn identities_travel_as_variables() {
        let rows = [
            Claimed {
                url: "https://github.com/example/project/pull/7".into(),
                lease: 1,
                owner: "example".into(),
                repo: "project".into(),
                number: 7,
            },
            Claimed {
                url: "https://github.com/example/other.repo/pull/9".into(),
                lease: 1,
                owner: "example".into(),
                repo: "other.repo".into(),
                number: 9,
            },
        ];
        let (query, variables) = request(&rows);
        assert!(query.starts_with(
            "query=query($o0: String!, $r0: String!, $n0: Int!, $o1: String!, $r1: String!, $n1: Int!) {"
        ));
        assert!(
            query.contains("p1: repository(owner: $o1, name: $r1) { pullRequest(number: $n1) {")
        );
        assert!(!query.contains("example"), "{query}");
        assert_eq!(
            variables,
            [
                ("-f", "o0=example".to_owned()),
                ("-f", "r0=project".to_owned()),
                ("-F", "n0=7".to_owned()),
                ("-f", "o1=example".to_owned()),
                ("-f", "r1=other.repo".to_owned()),
                ("-F", "n1=9".to_owned()),
            ]
        );
    }
}
