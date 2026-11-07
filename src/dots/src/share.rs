//! `dots task share`: cross-machine replication of task trees, and the
//! export, import and pass hooks other commands and the sync agent use.
use std::fmt::Write as _;
use std::time::SystemTime;

use anyhow::{Context as _, Result};
use dots_substrate::{Pass, Substrate, Trigger};
use dots_tasks::share::{self, Binding, Context, Exported, Imported, SharedProject};
use dots_tasks::{Project, Store};
use serde_json::json;

use crate::command::ShareCommand;

/// This machine's project bindings to shared identities.
fn bindings() -> Result<Vec<Binding>> {
    crate::memory::shared_bindings()?
        .into_iter()
        .map(|(local, shared)| {
            Ok(Binding {
                local: Project::try_new(local.as_str())
                    .with_context(|| format!("knowledge binding names local project {local}"))?,
                shared: SharedProject::try_new(shared.as_str())
                    .with_context(|| format!("knowledge binding names shared project {shared}"))?,
            })
        })
        .collect()
}

/// The substrate and bindings, when this machine has a substrate.
fn parts() -> Result<Option<(Substrate, Vec<Binding>)>> {
    if !crate::substrate::configured()? {
        return Ok(None);
    }
    Ok(Some((crate::substrate::open()?, bindings()?)))
}

pub(crate) fn run(command: ShareCommand, json: bool) -> Result<String> {
    let mut store = crate::project::open()?;
    let now = SystemTime::now();
    let unix = crate::substrate::unix(now);
    let unix = i64::try_from(unix).unwrap_or(i64::MAX);
    match command {
        ShareCommand::Enable => {
            let (substrate, bindings) = parts()?.context(
                "no substrate is configured; run dots substrate init --remote URL first",
            )?;
            let queued = store.enable_sharing(substrate.machine(), &bindings, unix)?;
            let context = Context {
                substrate: &substrate,
                bindings: &bindings,
            };
            let exported = share::export(&mut store, context, now)?;
            Ok(if json {
                crate::json::render(json!({
                    "enabled": true,
                    "machine": substrate.machine(),
                    "projects": bindings.iter().map(|b| json!({"local": b.local, "shared": b.shared})).collect::<Vec<_>>(),
                    "queued": queued,
                    "exported": exported,
                }))
            } else {
                format!(
                    "Sharing enabled for machine {} over {} bound projects; {} trees wait for the sync agent to push them.\n",
                    substrate.machine(),
                    bindings.len(),
                    exported.trees.len()
                )
            })
        }
        ShareCommand::Disable => {
            let changed = store.disable_sharing(unix)?;
            Ok(if json {
                crate::json::render(json!({"enabled": false, "changed": changed}))
            } else if changed {
                "Sharing disabled; imported trees stay read-only as last seen.\n".into()
            } else {
                "Sharing was already off.\n".into()
            })
        }
        ShareCommand::Status => status(&mut store, json),
        ShareCommand::Take { root } => {
            let (substrate, bindings) = parts()?.context(
                "no substrate is configured; run dots substrate init --remote URL first",
            )?;
            let context = Context {
                substrate: &substrate,
                bindings: &bindings,
            };
            let taken = share::take(&mut store, context, &root, now)?;
            Ok(if json {
                crate::json::render(json!({ "taken": taken }))
            } else {
                format!(
                    "This machine now homes {root} (previously {}). Coordinate it with dots task goal coordinate {root} --project {} before changing it.\n",
                    taken.previous_home, taken.project
                )
            })
        }
    }
}

/// `share status`: the switch, trees and pending replication.
fn status(store: &mut Store, json: bool) -> Result<String> {
    let status = store.share_status()?;
    let pending = match parts()? {
        Some((substrate, _)) => substrate
            .status()?
            .pending
            .iter()
            .filter(|pending| {
                let name = pending.name.as_str();
                name.starts_with("tasks/") || name.starts_with("answers/")
            })
            .map(|pending| pending.name.to_string())
            .collect(),
        None => Vec::new(),
    };
    if json {
        return Ok(crate::json::render(json!({
            "sharing": status.sharing,
            "home": status.home,
            "foreign": status.foreign,
            "dirty": status.dirty,
            "sent_answers": status.sent_answers,
            "pending": pending,
            "bindings": bindings()?.iter().map(|b| json!({"local": b.local, "shared": b.shared})).collect::<Vec<_>>(),
        })));
    }
    let mut out = String::new();
    match &status.sharing {
        Some(sharing) if sharing.enabled => {
            let _ = writeln!(out, "Sharing on as machine {}.", sharing.machine);
        }
        Some(sharing) => {
            let _ = writeln!(out, "Sharing off (machine {}).", sharing.machine);
        }
        None => out.push_str("Sharing off; dots task share enable turns it on.\n"),
    }
    let _ = writeln!(
        out,
        "{} trees homed here, {} imported read-only, {} local changes and {} answers waiting, {} updates pending push.",
        status.home,
        status.foreign.len(),
        status.dirty,
        status.sent_answers,
        pending.len()
    );
    for tree in &status.foreign {
        let _ = writeln!(
            out,
            "  {} {} from {}{}",
            tree.project,
            tree.root,
            tree.home,
            if tree.complete { "" } else { " (incomplete)" }
        );
    }
    Ok(out)
}

/// After a local write: defer the trees it changed to the outbox, where the
/// sync agent pushes them. The write already committed, so a failure here
/// only delays replication; the agent retries it on its next pass.
pub(crate) fn after_write(store: &mut Store) {
    if let Err(error) = try_after_write(store) {
        eprintln!("dots: sharing deferred: {error:#}");
    }
}

fn try_after_write(store: &mut Store) -> Result<()> {
    if !store.share_pending()? {
        return Ok(());
    }
    let Some((substrate, bindings)) = parts()? else {
        return Ok(());
    };
    let context = Context {
        substrate: &substrate,
        bindings: &bindings,
    };
    share::export(store, context, SystemTime::now())?;
    Ok(())
}

/// One explicit pass before a read: export, sync and import, so the catalog
/// shows other machines' current trees.
pub(crate) fn fresh(store: &mut Store) -> Result<()> {
    let Some((substrate, bindings)) = parts()? else {
        return Ok(());
    };
    let context = Context {
        substrate: &substrate,
        bindings: &bindings,
    };
    let (pass, shared) = share::pass(store, context, Trigger::Explicit, SystemTime::now())?;
    if let Pass::Done(report) = &pass
        && let Some(reason) = &report.offline
    {
        eprintln!("dots: remote unavailable; showing the local copy: {reason}");
    }
    if shared
        .as_ref()
        .is_some_and(|(_, imported)| changed(imported))
    {
        substrate.notify()?;
    }
    Ok(())
}

/// A sync pass with replication when this machine has a catalog: export
/// local changes, sync, import foreign trees and answers.
pub(crate) fn pass(
    substrate: &Substrate,
    trigger: Trigger,
    now: SystemTime,
) -> Result<(Pass, Option<(Exported, Imported)>)> {
    let path = crate::project::catalog_path()?;
    if !path.try_exists()? {
        return Ok((substrate.sync(trigger, now)?, None));
    }
    let mut store = crate::project::open()?;
    let bindings = bindings()?;
    let context = Context {
        substrate,
        bindings: &bindings,
    };
    let (pass, shared) = share::pass(&mut store, context, trigger, now)?;
    if shared
        .as_ref()
        .is_some_and(|(_, imported)| changed(imported))
    {
        substrate.notify()?;
    }
    Ok((pass, shared))
}

/// Whether an import changed what local views show.
fn changed(imported: &Imported) -> bool {
    !imported.trees.is_empty() || !imported.answers.is_empty() || !imported.lost.is_empty()
}

/// The JSON form of what a pass replicated.
pub(crate) fn report(shared: Option<&(Exported, Imported)>) -> serde_json::Value {
    match shared {
        None => json!(null),
        Some((exported, imported)) => json!({
            "exported": exported,
            "imported": imported,
        }),
    }
}
