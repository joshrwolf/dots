//! `dots project share`: report and set up whether a checkout's project
//! shares across machines, in one command.
use std::fmt::Write as _;
use std::path::Path;
use std::time::SystemTime;

use anyhow::{Context as _, Result, bail, ensure};
use dots_memory::{Landing, ProjectId};
use dots_tasks::Project;
use serde_json::json;

use crate::command::ProjectShare;
use crate::memory::Sharing;

const OPTIONS: &str = "dots project share --bind SHARED_ID (an identity above that is this same repository), --create NAME (a new identity), or --decline (stop asking on this machine)";

pub(crate) fn run(directory: &Path, share: &ProjectShare, as_json: bool) -> Result<String> {
    let common = crate::project::common_directory(directory)?
        .context("project share requires a Git checkout")?;
    let mut memory = crate::memory::open()?;
    let registered = || -> Result<Option<(dots_tasks::Store, Project)>> {
        if !crate::project::catalog_path()?.try_exists()? {
            return Ok(None);
        }
        let catalog = crate::project::open()?;
        Ok(catalog
            .project_for_common_directory(&common)?
            .map(|project| (catalog, project)))
    };
    // Registers the checkout under `name` unless it already is.
    let register = |name: &str| -> Result<(dots_tasks::Store, Project)> {
        ensure!(
            crate::project::catalog_path()?.try_exists()?,
            "this machine has no workspace catalog; create it once with dots project init NAME in this checkout, then rerun"
        );
        let (catalog, project, _) = crate::project::initialize(&common, name, false)?;
        Ok((catalog, project))
    };
    let bound = |memory: &dots_memory::Store| -> Result<Option<ProjectId>> {
        match registered()? {
            Some((_, project)) => crate::memory::binding(memory, &project),
            None => Ok(None),
        }
    };
    let now = SystemTime::now();
    if let Some(name) = &share.create {
        if let Some(id) = bound(&memory)? {
            bail!("this checkout is already shared as {id}; nothing to create");
        }
        let head = memory.head()?;
        let registration = memory.register_project(&head.snapshot, name, now)?;
        let id = registration.project;
        if let Landing::Pending { reason, .. } = registration.publication.landing {
            bail!(
                "shared identity {id} ({name}) is retained on this machine until it publishes ({reason}); once dots memory project list shows it, run dots project share --bind {id}"
            );
        }
        let (mut catalog, project) = register(name)?;
        crate::memory::bind(&memory, &mut catalog, &project, &id)?;
    } else if let Some(id) = &share.bind {
        let head = memory.head()?;
        let view = memory.view(&head, None)?;
        let known = &view.manifest().projects;
        let name = known
            .get(id)
            .with_context(|| {
                format!(
                    "unknown shared identity {id}; known identities: {}; or create one with --create NAME",
                    crate::memory::identities(known)
                )
            })?
            .clone();
        let (mut catalog, project) = register(&name)?;
        crate::memory::bind(&memory, &mut catalog, &project, id)?;
    } else if share.decline {
        if let Some(id) = bound(&memory)? {
            bail!(
                "this checkout is shared as {id}; remove that binding with dots memory project unbind --expected {id} before declining"
            );
        }
        let (mut catalog, project) = register(&checkout_name(&common))?;
        catalog.decline_share(&project, unix(now))?;
        // Agents watching knowledge context reread it.
        memory.substrate().notify()?;
    }
    status(&memory, &common, registered()?, as_json)
}

fn status(
    memory: &dots_memory::Store,
    common: &Path,
    registered: Option<(dots_tasks::Store, Project)>,
    as_json: bool,
) -> Result<String> {
    let head = memory.head()?;
    let view = memory.view(&head, None)?;
    let known = &view.manifest().projects;
    let (project, shared, sharing, machine) = match registered {
        None => {
            let machine = crate::project::catalog_path()?.try_exists()?
                && crate::project::open()?
                    .sharing()?
                    .is_some_and(|sharing| sharing.enabled);
            (None, None, Sharing::Unshared, machine)
        }
        Some((mut catalog, project)) => {
            let shared = crate::memory::binding(memory, &project)?;
            let sharing = if shared.is_some() {
                Sharing::Bound
            } else if catalog.share_declined(&project)? {
                Sharing::Declined
            } else {
                Sharing::Unshared
            };
            let machine = catalog.sharing()?.is_some_and(|sharing| sharing.enabled);
            (
                Some(catalog.registered_project(&project)?),
                shared,
                sharing,
                machine,
            )
        }
    };
    let identity = |id: &ProjectId| json!({"id": id, "name": known.get(id)});
    if as_json {
        return Ok(crate::json::render(json!({
            "checkout": common,
            "project": project,
            "sharing": sharing,
            "shared": shared.as_ref().map(identity),
            "machine_sharing": machine,
            "identities": known.keys().map(identity).collect::<Vec<_>>(),
            "snapshot": head.snapshot,
        })));
    }
    let mut out = String::new();
    let local = project.as_ref().map_or_else(
        || "is not registered".to_owned(),
        |project| {
            format!(
                "is local project {} ({})",
                project.id.as_str(),
                project.name
            )
        },
    );
    let _ = write!(out, "Checkout {} {local}", common.display());
    match (&shared, sharing) {
        (Some(id), _) => {
            let name = known.get(id).map_or("unavailable", String::as_str);
            let _ = writeln!(out, ", shared across machines as {id} ({name}).");
        }
        (None, Sharing::Declined) => {
            out.push_str("; the user declined sharing it from this machine.\n");
        }
        (None, _) => out.push_str(", not shared across machines.\n"),
    }
    out.push_str(if machine {
        "Machine sharing is on: bound projects replicate goals and tasks.\n"
    } else {
        "Machine sharing is off: bound projects share knowledge only; dots task share enable replicates goals and tasks.\n"
    });
    out.push_str("Shared identities (names never establish identity):\n");
    if known.is_empty() {
        out.push_str("  none\n");
    }
    for (id, name) in known {
        let _ = writeln!(out, "  {id} {name}");
    }
    if shared.is_none() {
        let _ = writeln!(out, "Set up: {OPTIONS}.");
    }
    Ok(out)
}

/// A display name for a checkout: its repository directory.
fn checkout_name(common: &Path) -> String {
    let repository = if common.file_name().is_some_and(|name| name == ".git") {
        common.parent().unwrap_or(common)
    } else {
        common
    };
    repository
        .file_name()
        .and_then(|name| name.to_str())
        .map(|name| name.strip_suffix(".git").unwrap_or(name))
        .filter(|name| !name.is_empty())
        .unwrap_or("checkout")
        .to_owned()
}

fn unix(now: SystemTime) -> i64 {
    i64::try_from(crate::substrate::unix(now)).unwrap_or(i64::MAX)
}
