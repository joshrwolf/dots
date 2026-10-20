//! Resolve the association before any CI workflow; never fall back to a branch.
use anyhow::{Result, bail};
use herdr_prs::{PullRequestUrl, Registry, Scope};
use herdrkit::picker::{Cell, Column, Entry, Group, Picker};
use herdrkit::{Invocation, Theme};

struct Choice {
    url: PullRequestUrl,
    status: String,
}
impl Entry for Choice {
    type Group = ();
    fn group(&self) {}
    fn cells(&self, theme: &Theme) -> Vec<Cell> {
        let name = self
            .url
            .as_str()
            .trim_start_matches("https://github.com/")
            .replacen("/pull/", "#", 1);
        vec![
            Cell::new(name, theme.strong),
            Cell::new(self.status.as_str(), theme.muted),
        ]
    }
}

const UNREGISTERED: &str = "register a PR with dots herdr prs register URL --workspace ID first";

/// Choose one registered PR. With several, `interactive` shows a picker;
/// otherwise the caller must use a surface that can.
pub fn select(invocation: &Invocation, interactive: bool) -> Result<Option<PullRequestUrl>> {
    let id = invocation.require_workspace_id()?;
    anyhow::ensure!(
        invocation.client().snapshot()?.workspace(id).is_some(),
        "workspace {id} is no longer open"
    );
    let scope = Scope::at(invocation.client().endpoint(), id.as_str())?;
    let path = herdrkit::user_state_directory()?.join(herdr_prs::REGISTRY_PATH);
    anyhow::ensure!(path.exists(), UNREGISTERED);
    let mut registry = Registry::open(&path)?;
    let urls: Vec<_> = registry
        .list(&scope)?
        .pulls
        .into_iter()
        .map(|pull| pull.url)
        .collect();
    let described = crate::ci::sidebar::describe(
        invocation.state_dir(),
        &urls.iter().map(PullRequestUrl::as_str).collect::<Vec<_>>(),
    );
    let mut choices: Vec<_> = urls
        .into_iter()
        .map(|url| Choice {
            status: described
                .get(url.as_str())
                .cloned()
                .unwrap_or_else(|| "Waiting for GitHub".to_owned()),
            url,
        })
        .collect();
    if choices.is_empty() {
        bail!(UNREGISTERED);
    }
    let chosen = if choices.len() == 1 {
        choices.pop()
    } else if interactive {
        Picker::new(choices)
            .groups(vec![Group::new(
                (),
                "Registered PRs",
                vec![Column::content(48), Column::fill()],
            )])
            .prompt("PR › ")
            .run()?
    } else {
        bail!("multiple registered PRs: open the CI watcher pane to select an explicit target");
    };
    let Some(Choice { url: chosen, .. }) = chosen else {
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
