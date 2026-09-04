use std::path::Path;
use std::time::Duration;

use anyhow::{Context as _, Result};
use herdrkit::{Client, MetadataReporter, Tokens, WorkspaceId};

use crate::ci::status;
use crate::github::{self, Lookup};

const TTL: Duration = Duration::from_secs(180);
const SOURCE: &str = "herdr-github";
const TOKEN: &str = "ghci";

/// Refreshes CI metadata for exactly the workspace that invoked the action.
///
/// This deliberately performs one bounded reconciliation and exits. Herdr's
/// startup hooks are not supervisors, so a permanent watcher would outlive the
/// lifecycle Herdr promises for a plugin command.
pub fn run(client: &Client, workspace: &WorkspaceId, checkout: &Path) -> Result<()> {
    let reporter = MetadataReporter::new(client, SOURCE, TTL)?;
    let tokens = match github::lookup(checkout)? {
        Lookup::Found(state) => Tokens::new().set(TOKEN, status::token(&state))?,
        Lookup::NoPullRequest => clear()?,
    };
    reporter
        .report_workspace(workspace, &tokens)
        .with_context(|| format!("reporting GitHub CI for {workspace}"))
}

fn clear() -> Result<Tokens> {
    Tokens::new().clear(TOKEN).map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clearing_is_explicit_and_uses_the_configured_name() {
        let tokens = clear().unwrap();
        assert_eq!(serde_json::to_string(&tokens).unwrap(), r#"{"ghci":null}"#);
    }

    #[test]
    fn manual_status_expires_instead_of_becoming_permanently_stale() {
        assert!(TTL >= Duration::from_secs(60));
        assert!(TTL <= Duration::from_secs(300));
    }
}
