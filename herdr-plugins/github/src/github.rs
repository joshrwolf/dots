//! Herdr-specific managed-review resolution over shared GitHub CI operations.

use anyhow::Result;
pub use github_ci::status::*;
use std::path::Path;

/// Local inputs to gh's repository selection plus any explicitly bound review.
/// Remote names are unrestricted; gh's set-default marker is included without
/// making a network request during sidebar inventory or publication.
pub(crate) fn checkout_identity(
    dir: &Path,
    server: &str,
    workspace: &herdrkit::WorkspaceId,
) -> Result<String> {
    let local = github_ci::status::local_identity(dir)?;
    Ok(serde_json::to_string(&(
        local,
        review_url(dir, server, workspace)?,
    ))?)
}

fn review_url(
    dir: &Path,
    server: &str,
    workspace: &herdrkit::WorkspaceId,
) -> Result<Option<String>> {
    let repository = review_core::Repository::discover(dir)?;
    let url = repository.managed_runtime_reference(server, workspace, "github.pull_request")?;
    if let Some(url) = &url {
        anyhow::ensure!(
            matches!(
                github_client::Target::parse(url)?,
                github_client::Target::Url(_)
            ),
            "review reference is not a PR URL"
        );
    }
    Ok(url)
}

/// Prefer the saved URL for a managed review. Ordinary checkouts use gh's
/// current-branch lookup and configured default repository, including forks.
pub(crate) fn discover(
    dir: &Path,
    server: &str,
    workspace: &herdrkit::WorkspaceId,
) -> Result<Option<String>> {
    if let Some(url) = review_url(dir, server, workspace)? {
        return Ok(Some(url));
    }
    discover_with(&SystemCommandRunner, dir)
}
