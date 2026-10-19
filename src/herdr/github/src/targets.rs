//! Resolve the association before any CI workflow; never fall back to a branch.
use anyhow::{Context as _, Result, bail};
use herdr_prs::{PullRequestUrl, Registry, Scope};
use herdrkit::picker::{Cell, Column, Entry, Group, Picker};
use herdrkit::{Invocation, Theme};

struct Choice(PullRequestUrl);
impl Entry for Choice {
    type Group = ();
    fn group(&self) {}
    fn cells(&self, theme: &Theme) -> Vec<Cell> {
        vec![Cell::new(self.0.as_str(), theme.strong)]
    }
}
pub fn select(invocation: &Invocation, interactive: bool) -> Result<Option<PullRequestUrl>> {
    let id = invocation.require_workspace_id()?;
    let snapshot = invocation.client().snapshot()?;
    let identity = snapshot
        .workspace(id)
        .and_then(|w| w.identity.clone())
        .context("Herdr persistent workspace identity required")?;
    let scope = Scope::new(
        invocation
            .client()
            .endpoint()
            .to_string_lossy()
            .into_owned(),
        identity,
    )?;
    let path = herdrkit::user_state_directory()?.join(herdr_prs::REGISTRY_PATH);
    anyhow::ensure!(
        path.exists(),
        "register a PR with dots herdr prs register URL first"
    );
    let mut registry = Registry::open(&path)?;
    let associations = registry.list(&scope)?;
    let mut choices: Vec<_> = associations
        .pulls
        .into_iter()
        .map(|pull| Choice(pull.url))
        .collect();
    if choices.is_empty() {
        bail!("register a PR with dots herdr prs register URL first");
    }
    let chosen = if choices.len() == 1 {
        choices.pop()
    } else if interactive {
        Picker::new(choices)
            .groups(vec![Group::new((), "Registered PRs", vec![Column::fill()])])
            .prompt("PR › ")
            .run()?
    } else {
        bail!("multiple registered PRs: open the CI watcher pane to select an explicit target");
    };
    let Some(Choice(chosen)) = chosen else {
        return Ok(None);
    };
    anyhow::ensure!(
        registry
            .list(&scope)?
            .pulls
            .iter()
            .any(|pull| pull.url == chosen),
        "PR was unregistered during selection"
    );
    Ok(Some(chosen))
}
