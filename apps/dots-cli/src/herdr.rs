use anyhow::{Context as _, Result, bail};
use herdr_prs::{PullRequestUrl, Registry, Scope, Source};
use herdrkit::{Client, WorkspaceId};

use crate::command::{HerdrCommand, PrAssociationCommand};

pub(crate) fn run(command: HerdrCommand, json: bool) -> Result<String> {
    let HerdrCommand::Prs { workspace, command } = command;
    let workspace = workspace.context("--workspace is required; verify the intended workspace from the task and live inventory, not inherited Herdr IDs or UI focus")?;
    let url = match &command {
        PrAssociationCommand::Register { url } | PrAssociationCommand::Unregister { url } => {
            Some(PullRequestUrl::parse(url)?)
        }
        PrAssociationCommand::List => None,
    };
    // Only the endpoint comes from the environment. Pane/workspace variables can
    // belong to the shared daemon's launcher rather than this agent session.
    let client = Client::from_env()?;
    let snapshot = client.snapshot()?;
    let workspace_id = WorkspaceId::new(workspace);
    let workspace = snapshot
        .workspace(&workspace_id)
        .context("explicit workspace no longer exists")?;
    let identity = workspace.identity.clone().context("Herdr does not expose persistent workspace identities; build the required host changes first")?;
    let endpoint = client
        .endpoint()
        .to_str()
        .context("Herdr endpoint path is not Unicode")?
        .to_owned();
    let scope = Scope::new(endpoint, identity)?;
    let path = herdrkit::user_state_directory()?.join(herdr_prs::REGISTRY_PATH);
    let directory = path.parent().context("PR registry path has no parent")?;
    std::fs::create_dir_all(directory).context("create PR registry directory")?;
    let mut registry = Registry::open(&path)?;
    let changed = match (&command, url.as_ref()) {
        (PrAssociationCommand::Register { .. }, Some(url)) => Some(registry.register(
            &scope,
            url,
            Source::Agent,
            time::OffsetDateTime::now_utc().unix_timestamp(),
        )?),
        (PrAssociationCommand::Unregister { .. }, Some(url)) => {
            Some(registry.unregister(&scope, url)?)
        }
        (PrAssociationCommand::List, None) => None,
        _ => bail!("invalid PR command target"),
    };
    let associations = registry.list(&scope)?;
    let (observed, watcher_error) = match client.workspace_resources(&workspace_id) {
        Ok(resources) if resources.identity == scope.identity() => (
            resources
                .collections
                .get("herdr-github")
                .map(|collection| collection.resources.clone())
                .unwrap_or_default(),
            None,
        ),
        Ok(_) => (Vec::new(), Some("workspace lifetime changed".to_owned())),
        Err(error) => (Vec::new(), Some(error.to_string())),
    };
    let observations: Vec<_> = associations.pulls.iter().map(|pull| {
        let resource = observed.iter().find(|resource| resource.id == pull.url.as_str());
        serde_json::json!({"url": pull.url, "status": resource.map_or("Awaiting watcher projection", |resource| resource.status.as_str()), "projection": resource})
    }).collect();
    if json {
        Ok(crate::json::render(
            serde_json::json!({"workspace_id": workspace_id, "scope": scope, "changed": changed, "associations": associations, "observations": observations, "watcher_error": watcher_error}),
        ))
    } else if associations.pulls.is_empty() {
        Ok(format!("{workspace_id}: no registered PRs\n"))
    } else {
        let mut output = String::new();
        for pull in &associations.pulls {
            output.push_str(pull.url.as_str());
            if let Some(resource) = observed
                .iter()
                .find(|resource| resource.id == pull.url.as_str())
            {
                output.push_str("  ");
                output.push_str(&resource.status);
            } else {
                output.push_str("  Awaiting watcher projection");
            }
            output.push('\n');
        }
        if let Some(error) = watcher_error {
            output.push_str("Watcher unavailable: ");
            output.push_str(&error);
            output.push('\n');
        }
        Ok(output)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn checkout_flags_cannot_silently_select_a_different_workspace() {
        for arguments in [
            vec!["herdr", "prs", "list", "--directory", "/elsewhere"],
            vec!["herdr", "prs", "list", "--repo", "example/project"],
            vec!["herdr", "prs", "list", "--no-cache"],
        ] {
            let error =
                crate::run(arguments.into_iter().map(std::ffi::OsString::from)).unwrap_err();
            assert!(error.to_string().contains("do not apply"));
        }
    }
}
