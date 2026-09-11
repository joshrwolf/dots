use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result};
use herdrkit::api::Snapshot;
use review_core::Repository;

use crate::catalog::RepositoryCatalog;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LiveWorkspace {
    pub id: herdrkit::WorkspaceId,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct KnownRepository {
    pub repository: Repository,
    pub name: String,
    pub workspaces: Vec<LiveWorkspace>,
}

impl KnownRepository {
    pub(crate) fn display_name(&self) -> String {
        let root = self.repository.checkout_root();
        let Some(parent) = root
            .parent()
            .and_then(Path::file_name)
            .and_then(|name| name.to_str())
        else {
            return self.name.clone();
        };
        format!("{parent}/{}", self.name)
    }
}

pub(crate) fn collect(
    snapshot: &Snapshot,
    current_dir: Option<&Path>,
    catalog: &RepositoryCatalog,
) -> Result<Vec<KnownRepository>> {
    let mut repositories = BTreeMap::<PathBuf, KnownRepository>::new();

    if let Some(current_dir) = current_dir {
        discover_into(&mut repositories, current_dir, None, None);
    }
    for root in snapshot.repository_roots() {
        discover_into(&mut repositories, root, None, None);
    }
    for workspace in &snapshot.workspaces {
        if let Some(directory) = snapshot.effective_workspace_dir(&workspace.workspace_id) {
            discover_into(
                &mut repositories,
                directory,
                workspace
                    .worktree
                    .as_ref()
                    .map(|worktree| worktree.repository.name.as_str()),
                Some(LiveWorkspace {
                    id: workspace.workspace_id.clone(),
                    label: workspace.label.clone(),
                }),
            );
        }
    }
    for entry in catalog
        .entries()
        .context("reading known review repositories")?
    {
        discover_into(
            &mut repositories,
            &entry.checkout_root,
            Some(&entry.name),
            None,
        );
    }

    let repositories = repositories.into_values().collect::<Vec<_>>();
    for known in &repositories {
        catalog
            .remember(&known.repository, &known.name)
            .with_context(|| format!("remembering repository {}", known.name))?;
    }
    Ok(repositories)
}

fn discover_into(
    repositories: &mut BTreeMap<PathBuf, KnownRepository>,
    checkout: &Path,
    name: Option<&str>,
    workspace: Option<LiveWorkspace>,
) {
    let repository = match discover_candidate(checkout) {
        Ok(Some(repository)) => repository,
        Ok(None) => return,
        Err(error) => {
            eprintln!(
                "review: leaving out repository candidate {}: {error}",
                checkout.display()
            );
            return;
        }
    };
    let key = repository.common_git_dir().to_path_buf();
    let fallback_name = repository
        .checkout_root()
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("repository");
    let entry = repositories.entry(key).or_insert_with(|| KnownRepository {
        repository: repository.clone(),
        name: name.unwrap_or(fallback_name).to_owned(),
        workspaces: Vec::new(),
    });
    if entry.repository.is_linked_worktree() && !repository.is_linked_worktree() {
        entry.repository = repository;
    }
    if let Some(name) = name.filter(|name| !name.is_empty()) {
        name.clone_into(&mut entry.name);
    }
    if let Some(workspace) = workspace
        && !entry
            .workspaces
            .iter()
            .any(|known| known.id == workspace.id)
    {
        entry.workspaces.push(workspace);
    }
}

/// Workspace and catalog paths are discovery hints, not ownership of a checkout.
/// A removed worktree is normal; other inspection failures remain diagnostics.
fn discover_candidate(checkout: &Path) -> Result<Option<Repository>> {
    if !checkout.try_exists()? {
        return Ok(None);
    }
    match Repository::discover(checkout) {
        Ok(repository) => Ok(Some(repository)),
        Err(_) if !checkout.try_exists()? => Ok(None),
        Err(error) => Err(error.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn removed_checkout_is_not_a_discovery_error() {
        let root = std::env::temp_dir().join(format!("review-inventory-{}", std::process::id()));
        std::fs::create_dir(&root).unwrap();
        let checkout = root.join("removed-review");
        std::fs::create_dir(&checkout).unwrap();
        std::fs::remove_dir(&checkout).unwrap();
        assert!(discover_candidate(&checkout).unwrap().is_none());

        // An existing but invalid candidate must not be silently discarded.
        assert!(discover_candidate(&root).is_err());
        std::fs::remove_dir(&root).unwrap();
    }
}
