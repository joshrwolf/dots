//! One host resolver for the catalog and canonical Git common directories.
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use anyhow::{Context as _, Result, bail, ensure};
use dots_documents::mutation_lock::MutationLock;
use dots_tasks::{Project, Store};
use serde_json::json;

use crate::command::ProjectCommand;

pub(crate) fn data_root() -> Result<PathBuf> {
    Ok(crate::xdg::base("XDG_DATA_HOME", ".local/share")?.join("dots"))
}

/// The project's machine-local documents: briefs/, outputs/ and scratch/.
pub(crate) fn workspace(project: &Project) -> Result<PathBuf> {
    Ok(data_root()?.join("workspaces").join(project.as_str()))
}

pub(crate) fn catalog_path() -> Result<PathBuf> {
    Ok(crate::xdg::base("XDG_DATA_HOME", ".local/share")?.join(dots_tasks::STORE_PATH))
}

pub(crate) const UNBOUND: &str = "Git common directory is unbound. For a moved checkout: dots project list, then dots project rebind ID --from OLD. For a new checkout: dots project init or dots project bind.";

pub(crate) fn open() -> Result<Store> {
    let path = catalog_path()?;
    ensure!(
        path.try_exists()?,
        "workspace absent; use dots project init to initialize explicitly"
    );
    Store::open(&path).with_context(|| {
        format!(
            "catalog at {} is invalid, incompatible or unavailable; init will not replace it",
            path.display()
        )
    })
}

pub(crate) fn common_directory(directory: &Path) -> Result<Option<PathBuf>> {
    let mut command = Command::new("git");
    command
        .args(["rev-parse", "--path-format=absolute", "--git-common-dir"])
        .env("LC_ALL", "C")
        .current_dir(directory);
    let output = dots_process::output(command, Duration::from_secs(10), 64 * 1024)
        .context("resolve Git common directory")?;
    if !output.status.success() {
        let message = String::from_utf8_lossy(&output.stderr);
        if message
            .starts_with("fatal: not a git repository (or any of the parent directories): .git")
        {
            return Ok(None);
        }
        bail!("cannot resolve Git common directory: {message}");
    }
    let text = String::from_utf8(output.stdout).context("Git common directory is not UTF-8")?;
    Ok(Some(
        Path::new(text.trim_end_matches('\n'))
            .canonicalize()
            .context("canonicalize Git common directory")?,
    ))
}

pub(crate) fn resolve(
    store: &mut Store,
    directory: &Path,
    explicit: Option<&str>,
) -> Result<Project> {
    match resolve_optional(store, directory, explicit)? {
        Resolution::Bound(project) => Ok(project),
        unavailable => bail!("{}", unavailable.message()),
    }
}

pub(crate) enum Resolution {
    Bound(Project),
    OutsideGit,
    /// A Git checkout no registered project is bound to.
    Unbound,
}

impl Resolution {
    /// Why no project resolved, naming the commands that bind one.
    pub(crate) fn message(&self) -> &'static str {
        match self {
            Self::Bound(_) => "resolved a bound project",
            Self::OutsideGit => {
                "outside Git: pass --project ID for a registered project or select --scope global"
            }
            Self::Unbound => UNBOUND,
        }
    }
}

pub(crate) fn resolve_optional(
    store: &mut Store,
    directory: &Path,
    explicit: Option<&str>,
) -> Result<Resolution> {
    if let Some(id) = explicit {
        let project = Project::try_new(id)?;
        return Ok(Resolution::Bound(store.registered_project(&project)?.id));
    }
    let Some(common) = common_directory(directory)? else {
        return Ok(Resolution::OutsideGit);
    };
    Ok(match store.project_for_common_directory(&common)? {
        Some(project) => Resolution::Bound(project),
        None => Resolution::Unbound,
    })
}

/// Register `name` and bind the checkout's Git common directory `common`,
/// creating the catalog if absent; an existing binding is returned as is.
/// Returns the catalog, the project and whether it was created.
pub(crate) fn initialize(common: &Path, name: &str, fresh: bool) -> Result<(Store, Project, bool)> {
    let name = dots_tasks::validate_project_name(name)?;
    let root = data_root()?;
    let _lock = MutationLock::acquire(&root.join("mutation.lock"))?;
    let path = catalog_path()?;
    let mut store = if path.try_exists()? {
        open()?
    } else {
        let old_tasks =
            crate::xdg::base("XDG_STATE_HOME", ".local/state")?.join("dots/tasks/tasks.sqlite3");
        ensure!(
            fresh
                || !(old_tasks.try_exists()?
                    || root.join("memory").try_exists()?
                    || root.join("goals").try_exists()?),
            "existing data needs a reviewed cutover; --fresh explicitly starts an empty catalog without deleting it"
        );
        Store::create(&path)?
    };
    let (project, created) = store.initialize_project(name, common)?;
    Ok((store, project, created))
}

pub(crate) fn run(command: ProjectCommand, directory: &Path, as_json: bool) -> Result<String> {
    let value = match command {
        ProjectCommand::Init { name, fresh } => {
            let common = common_directory(directory)?.context(
                "project init requires a Git checkout; use project register for an unbound project",
            )?;
            let (mut store, project, created) = initialize(&common, &name, fresh)?;
            json!({
                "project": store.registered_project(&project)?,
                "workspace": workspace(&project)?,
                "created": created,
            })
        }
        ProjectCommand::Share(share) => {
            return crate::project_share::run(directory, &share, as_json);
        }
        ProjectCommand::Register { name } => {
            let mut store = open()?;
            let project = store.register_project(&name)?;
            json!({"project": store.registered_project(&project)?})
        }
        ProjectCommand::List => json!({"projects": open()?.registered_projects()?}),
        ProjectCommand::Resolve => {
            let mut store = open()?;
            let project = resolve(&mut store, directory, None)?;
            json!({"project": store.registered_project(&project)?, "workspace": workspace(&project)?})
        }
        ProjectCommand::Bind { id } => {
            let mut store = open()?;
            let project = Project::try_new(id)?;
            let common =
                common_directory(directory)?.context("binding requires a Git common directory")?;
            store.bind_project(&project, &common)?;
            json!({"project": store.registered_project(&project)?})
        }
        ProjectCommand::Unbind { id, path } => {
            let mut store = open()?;
            let project = Project::try_new(id)?;
            let removed = store.unbind_project(&project, &path)?;
            json!({"project": store.registered_project(&project)?, "removed": removed})
        }
        ProjectCommand::Rebind { id, from } => {
            let mut store = open()?;
            let project = Project::try_new(id)?;
            let common = common_directory(directory)?
                .context("rebinding requires a Git common directory")?;
            store.rebind_project(&project, &from, &common)?;
            json!({"project": store.registered_project(&project)?})
        }
    };
    Ok(crate::json::render(value))
}
