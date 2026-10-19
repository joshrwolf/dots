use std::io::{self, Write as _};
use std::path::Path;

use anyhow::{Context as _, Result};
use github_client::{Client as GitHub, GitHubRepository, PullRequest, Target as GitHubTarget};
use review_core::{ComparisonSpec, DiffEndpoint, ExternalReference};
use sha2::{Digest as _, Sha256};

const GITHUB_PULL_REQUEST: &str = "github.pull_request";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SourceAction {
    CurrentBranchPullRequest,
    ExplicitPullRequest,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ManagedWorktree {
    pub branch: String,
    pub commit: String,
    pub label: String,
}

/// Everything the materializer needs, without exposing provider concepts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SourcePlan {
    pub external_reference: ExternalReference,
    pub label: String,
    pub comparison: ComparisonSpec,
    pub checkout: ManagedWorktree,
}

pub(crate) fn target_for_action(action: SourceAction) -> Result<Option<GitHubTarget>> {
    let target = match action {
        SourceAction::CurrentBranchPullRequest => GitHubTarget::CurrentBranch,
        SourceAction::ExplicitPullRequest => {
            let Some(target) = prompt_pull_request()? else {
                return Ok(None);
            };
            target
        }
    };
    Ok(Some(target))
}

pub(crate) fn parse_target(value: &str) -> Result<Option<GitHubTarget>> {
    if value.is_empty() {
        return Ok(None);
    }
    GitHubTarget::parse(value).map(Some).map_err(Into::into)
}

pub(crate) fn target_repository(target: &GitHubTarget) -> Result<Option<GitHubRepository>> {
    target.repository().map_err(Into::into)
}

pub(crate) fn resolve(checkout: &Path, target: &GitHubTarget) -> Result<SourcePlan> {
    resolve_github(checkout, target)
}

/// Refreshes a context only when one of its references has a registered source.
/// No source is queried while the picker is populated.
pub(crate) fn refresh(
    checkout: &Path,
    references: &[ExternalReference],
) -> Result<Option<SourcePlan>> {
    let Some(reference) = references
        .iter()
        .find(|reference| reference.kind == GITHUB_PULL_REQUEST)
    else {
        return Ok(None);
    };
    let target = GitHubTarget::parse(&reference.locator)?;
    resolve_github(checkout, &target).map(Some)
}

fn resolve_github(checkout: &Path, target: &GitHubTarget) -> Result<SourcePlan> {
    let github = GitHub;
    let pull_request = github
        .resolve(checkout, target)
        .context("resolving the GitHub pull request")?;
    let prepared = github
        .prepare_comparison(checkout, &pull_request)
        .context("preparing the pull request commits")?;
    let external_reference = github_reference(&pull_request)?;
    let branch = managed_worktree_branch(
        pull_request.number,
        &external_reference,
        &prepared.base_oid,
        &prepared.head_oid,
    );
    let comparison = ComparisonSpec::new(
        DiffEndpoint::Commit {
            oid: prepared.base_oid,
        },
        DiffEndpoint::Commit {
            oid: prepared.head_oid.clone(),
        },
    )?;
    let label = pull_request.title.clone();
    Ok(SourcePlan {
        external_reference,
        label,
        comparison,
        checkout: ManagedWorktree {
            branch,
            commit: prepared.head_oid,
            label: format!("PR #{}", pull_request.number),
        },
    })
}

fn github_reference(pull_request: &PullRequest) -> Result<ExternalReference> {
    ExternalReference::new(GITHUB_PULL_REQUEST, pull_request.url.clone()).map_err(Into::into)
}

fn managed_worktree_branch(
    number: u64,
    reference: &ExternalReference,
    base: &str,
    target: &str,
) -> String {
    let mut digest = Sha256::new();
    for value in [&reference.kind, &reference.locator, base, target] {
        digest.update(value.as_bytes());
        digest.update([0]);
    }
    // Herdr derives the checkout directory from this branch. Keep it readable
    // while allowing old and new PR observations to coexist without resets.
    let identity = format!("{:x}", digest.finalize());
    format!("review-pr-{number}-{}", identity.split_at(12).0)
}

fn prompt_pull_request() -> Result<Option<GitHubTarget>> {
    print!("Pull request number or URL: ");
    io::stdout()
        .flush()
        .context("drawing the pull request prompt")?;
    let mut value = String::new();
    io::stdin()
        .read_line(&mut value)
        .context("reading the pull request target")?;
    parse_target(value.trim())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_pull_request_accepts_number_hash_or_url() {
        assert!(matches!(
            parse_target("42").unwrap(),
            Some(GitHubTarget::Number(42))
        ));
        assert!(matches!(
            parse_target("#42").unwrap(),
            Some(GitHubTarget::Number(42))
        ));
        assert!(matches!(
            parse_target("https://github.com/o/r/pull/42").unwrap(),
            Some(GitHubTarget::Url(url)) if url.ends_with("/pull/42")
        ));
        assert!(parse_target("").unwrap().is_none());
        assert!(parse_target("0").is_err());
        assert!(parse_target("nope").is_err());
    }

    #[test]
    fn github_reference_uses_the_provider_canonical_url() {
        let pull_request = PullRequest {
            host: "GitHub.COM".to_owned(),
            owner: "Owner".to_owned(),
            repository: "Repo".to_owned(),
            number: 42,
            title: "Review me".to_owned(),
            url: "https://github.com/owner/repo/pull/42".to_owned(),
            repository_url: "https://github.com/Owner/Repo.git".to_owned(),
            base_ref: "main".to_owned(),
            head_ref: "feature".to_owned(),
            head_oid: "b".repeat(40),
        };

        let reference = github_reference(&pull_request).unwrap();
        assert_eq!(reference.kind, GITHUB_PULL_REQUEST);
        assert_eq!(reference.locator, "https://github.com/owner/repo/pull/42");
    }

    #[test]
    fn managed_worktree_identity_is_stable_and_observation_specific() {
        let reference = ExternalReference::new(
            GITHUB_PULL_REQUEST,
            "https://github.com/example/repo/pull/42",
        )
        .unwrap();
        let first = managed_worktree_branch(42, &reference, &"a".repeat(40), &"b".repeat(40));
        let same = managed_worktree_branch(42, &reference, &"a".repeat(40), &"b".repeat(40));
        let changed = managed_worktree_branch(42, &reference, &"a".repeat(40), &"c".repeat(40));
        let changed_base =
            managed_worktree_branch(42, &reference, &"c".repeat(40), &"b".repeat(40));
        let other_reference = ExternalReference::new(
            GITHUB_PULL_REQUEST,
            "https://github.com/example/other/pull/42",
        )
        .unwrap();
        let other_repository =
            managed_worktree_branch(42, &other_reference, &"a".repeat(40), &"b".repeat(40));

        assert_eq!(first, same);
        assert_ne!(first, changed);
        assert_ne!(first, changed_base);
        assert_ne!(first, other_repository);
        let suffix = first.strip_prefix("review-pr-42-").unwrap();
        assert_eq!(suffix.len(), 12);
        assert!(suffix.bytes().all(|byte| byte.is_ascii_hexdigit()));
    }
}
