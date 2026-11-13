//! Task trees replicated across machines on the substrate.
//!
//! Only projects bound to a shared identity replicate, and only while this
//! machine's switch is on. The unit is a tree: a top-level task (a goal or a
//! loose task) with its descendants, at ref `tasks/<shared project>/<root>`.
//! One machine, the tree's home, writes it. Its local writes commit at once;
//! [`export`] turns trees that changed into deferred substrate updates, which
//! the sync agent flushes within the push budget. Every other machine
//! [`import`]s the tree into its catalog as read-only rows, and every
//! mutation of them refuses with [`Error::Foreign`]. [`take`] moves a tree's
//! home here by compare-and-swap from the version this machine last saw.
//!
//! An update the old home deferred before a takeover reached it is refused
//! by the remote and kept as a stranded substrate entry; [`stranded`] lists
//! what it held that never published, and the next import makes the tree
//! read-only here.
mod export;
mod format;
mod import;
mod stranded;

#[cfg(test)]
mod tests;

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use dots_substrate::{MachineId, Outcome, RefName, Substrate, Trigger, Update};
use rusqlite::{OptionalExtension as _, params};
use serde::Serialize;

pub use export::Exported;
pub use import::Imported;
pub use stranded::{StrandedTree, UnpublishedEvent, UnpublishedTask, stranded};

use crate::store::{Error, Project, Store};
use format::{Manifest, decode, encode, share};

/// A shared project identity from the knowledge registry, `p-` and 32
/// lowercase hex digits, the same on every machine.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(transparent)]
pub struct SharedProject(String);

impl SharedProject {
    pub fn try_new(value: impl Into<String>) -> Result<Self, Error> {
        let value = value.into();
        Project::try_new(value.as_str())
            .map_err(|_| Error::Invalid("shared project: expected p- and 32 hex digits"))?;
        Ok(Self(value))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A local project bound to a shared identity on this machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Binding {
    pub local: Project,
    pub shared: SharedProject,
}

/// What replication reads and writes: this machine's substrate and its
/// project bindings.
#[derive(Debug, Clone, Copy)]
pub struct Context<'a> {
    pub substrate: &'a Substrate,
    pub bindings: &'a [Binding],
}

impl Context<'_> {
    fn machine(&self) -> &MachineId {
        self.substrate.machine()
    }

    fn shared(&self, local: &str) -> Option<&SharedProject> {
        self.bindings
            .iter()
            .find(|binding| binding.local.as_str() == local)
            .map(|binding| &binding.shared)
    }
}

/// This machine's switch and identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Sharing {
    pub machine: String,
    pub enabled: bool,
    /// When the switch last changed, as a Unix timestamp.
    pub changed_at: i64,
}

/// Trees and deferred work on this machine.
#[derive(Debug, Serialize)]
pub struct ShareStatus {
    pub sharing: Option<Sharing>,
    /// Trees this machine homes and has exported.
    pub home: usize,
    /// Trees imported from other machines, read-only here.
    pub foreign: Vec<ForeignTree>,
    /// Tasks whose trees await export.
    pub dirty: usize,
    /// Trees whose last exported or imported version withheld parts.
    pub withheld: Vec<WithheldTree>,
}

#[derive(Debug, Serialize)]
pub struct ForeignTree {
    pub project: String,
    pub root: String,
    pub home: String,
    pub written_at: Option<i64>,
    pub imported_at: Option<i64>,
    /// False while some of its records wait for other trees to arrive.
    pub complete: bool,
}

/// A part of a tree its home could not replicate. The tree's other records
/// still travel; the withheld part shows as missing on other machines.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Withheld {
    pub kind: WithheldKind,
    /// The brief's path, or an output's slug and digest.
    pub id: String,
    pub reason: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WithheldKind {
    Brief,
    Output,
}

impl WithheldKind {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Brief => "brief",
            Self::Output => "output",
        }
    }
}

/// A tree with withheld parts, on its home or as imported.
#[derive(Debug, Serialize)]
pub struct WithheldTree {
    pub project: String,
    pub root: String,
    pub home: String,
    pub withheld: Vec<Withheld>,
}

/// A completed takeover.
#[derive(Debug, Serialize)]
pub struct Taken {
    pub project: String,
    pub root: String,
    pub previous_home: String,
    /// When the previous home last wrote the tree, as a Unix timestamp.
    pub previous_written_at: i64,
    pub version: String,
}

/// The directory holding an imported tree's brief inside a project workspace.
#[must_use]
pub fn imported_workspace(workspace: &Path, root: &str) -> PathBuf {
    workspace.join("imported").join(root)
}

impl Store {
    /// This machine's switch, if sharing was ever enabled.
    pub fn sharing(&mut self) -> Result<Option<Sharing>, Error> {
        Ok(self
            .connection
            .query_row(
                "SELECT machine, enabled, changed_at FROM sharing",
                [],
                |row| {
                    Ok(Sharing {
                        machine: row.get(0)?,
                        enabled: row.get(1)?,
                        changed_at: row.get(2)?,
                    })
                },
            )
            .optional()?)
    }

    /// Turn sharing on for `machine` and queue every tree of the bound
    /// projects for export. Returns how many trees were queued.
    pub fn enable_sharing(
        &mut self,
        machine: &MachineId,
        bindings: &[Binding],
        now: i64,
    ) -> Result<usize, Error> {
        let tx = self.write()?;
        let current: Option<String> = tx
            .query_row("SELECT machine FROM sharing", [], |row| row.get(0))
            .optional()?;
        if let Some(current) = current
            && current != machine.as_str()
        {
            return Err(share(format!(
                "this catalog shares as machine {current}, but the substrate state is machine {machine}; trees homed here would become foreign. Restore the substrate state, or reset sharing explicitly"
            )));
        }
        tx.execute(
            "INSERT INTO sharing(singleton, machine, enabled, changed_at) VALUES(0, ?1, 1, ?2)
             ON CONFLICT(singleton) DO UPDATE SET enabled=1, changed_at=excluded.changed_at",
            params![machine.as_str(), now],
        )?;
        let mut queued = 0;
        for binding in bindings {
            queued += queue_trees(&tx, &binding.local)?;
        }
        tx.commit()?;
        Ok(queued)
    }

    /// Record that `project` was just bound to a shared identity: forget a
    /// decline and, while sharing is on, queue its top-level trees for
    /// export. Returns how many trees were queued.
    pub fn share_project(&mut self, project: &Project) -> Result<usize, Error> {
        let tx = self.write()?;
        tx.execute(
            "DELETE FROM share_declines WHERE project=?1",
            [project.as_str()],
        )?;
        let enabled: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM sharing WHERE enabled=1)",
            [],
            |row| row.get(0),
        )?;
        let queued = if enabled {
            queue_trees(&tx, project)?
        } else {
            0
        };
        tx.commit()?;
        Ok(queued)
    }

    /// Remember that the user does not want `project` shared from this
    /// machine. Returns false when it was already declined.
    pub fn decline_share(&mut self, project: &Project, now: i64) -> Result<bool, Error> {
        let tx = self.write()?;
        crate::projects::require_project(&tx, project)?;
        let added = tx.execute(
            "INSERT OR IGNORE INTO share_declines(project, at) VALUES(?1, ?2)",
            params![project.as_str(), now],
        )?;
        tx.commit()?;
        Ok(added > 0)
    }

    /// Whether the user declined sharing `project` from this machine.
    pub fn share_declined(&self, project: &Project) -> Result<bool, Error> {
        Ok(self.connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM share_declines WHERE project=?1)",
            [project.as_str()],
            |row| row.get(0),
        )?)
    }

    /// Turn sharing off. Imported trees stay, read-only, as last seen.
    pub fn disable_sharing(&mut self, now: i64) -> Result<bool, Error> {
        let changed = self.connection.execute(
            "UPDATE sharing SET enabled=0, changed_at=?1 WHERE enabled=1",
            [now],
        )?;
        self.connection.execute("DELETE FROM dirty_tasks", [])?;
        Ok(changed > 0)
    }

    /// Whether sharing is on and local changes wait for export.
    pub fn share_pending(&mut self) -> Result<bool, Error> {
        Ok(self.connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM sharing WHERE enabled=1) AND EXISTS(SELECT 1 FROM dirty_tasks)",
            [],
            |row| row.get(0),
        )?)
    }

    pub fn share_status(&mut self) -> Result<ShareStatus, Error> {
        let sharing = self.sharing()?;
        let machine = sharing
            .as_ref()
            .map(|sharing| sharing.machine.clone())
            .unwrap_or_default();
        let home = self.connection.query_row(
            "SELECT COUNT(*) FROM shared_trees WHERE home=?1",
            [&machine],
            |row| row.get::<_, i64>(0),
        )?;
        let foreign = {
            let mut statement = self.connection.prepare(
                "SELECT project, root, home, written_at, imported_at, complete FROM shared_trees
                 WHERE home<>?1 ORDER BY project, root",
            )?;
            statement
                .query_map([&machine], |row| {
                    Ok(ForeignTree {
                        project: row.get(0)?,
                        root: row.get(1)?,
                        home: row.get(2)?,
                        written_at: row.get(3)?,
                        imported_at: row.get(4)?,
                        complete: row.get(5)?,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?
        };
        let dirty = self
            .connection
            .query_row("SELECT COUNT(*) FROM dirty_tasks", [], |row| {
                row.get::<_, i64>(0)
            })?;
        let withheld = {
            let mut statement = self.connection.prepare(
                "SELECT project, root, home, withheld FROM shared_trees
                 WHERE withheld IS NOT NULL ORDER BY project, root",
            )?;
            statement
                .query_map([], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                    ))
                })?
                .map(|row| {
                    let (project, root, home, json) = row?;
                    let withheld = serde_json::from_str(&json).map_err(|error| {
                        share(format!("withheld parts of {root} in the catalog: {error}"))
                    })?;
                    Ok(WithheldTree {
                        project,
                        root,
                        home,
                        withheld,
                    })
                })
                .collect::<Result<Vec<_>, Error>>()?
        };
        let count = |value: i64| usize::try_from(value).expect("SQLite COUNT is nonnegative");
        Ok(ShareStatus {
            sharing,
            home: count(home),
            foreign,
            dirty: count(dirty),
            withheld,
        })
    }

    /// Whether sharing is on for `machine`, refusing a catalog that shares as
    /// another machine.
    fn sharing_for(&mut self, machine: &MachineId) -> Result<bool, Error> {
        match self.sharing()? {
            None => Ok(false),
            Some(sharing) if sharing.machine != machine.as_str() => Err(share(format!(
                "this catalog shares as machine {}, but the substrate state is machine {machine}",
                sharing.machine
            ))),
            Some(sharing) => Ok(sharing.enabled),
        }
    }
}

/// Queue every top-level tree of `project` for export.
fn queue_trees(tx: &rusqlite::Transaction<'_>, project: &Project) -> Result<usize, Error> {
    Ok(tx.execute(
        "INSERT OR IGNORE INTO dirty_tasks SELECT id FROM tasks WHERE project=?1 AND parent IS NULL",
        [project.as_str()],
    )?)
}

/// The catalog form of a tree's withheld parts: NULL when nothing was withheld.
fn withheld_column(withheld: &[Withheld]) -> Result<Option<String>, Error> {
    if withheld.is_empty() {
        return Ok(None);
    }
    serde_json::to_string(withheld)
        .map(Some)
        .map_err(|error| share(format!("encode withheld parts: {error}")))
}

/// The ref of a tree.
fn tree_ref(shared: &SharedProject, root: &str) -> Result<RefName, Error> {
    Ok(format!("tasks/{}/{root}", shared.as_str()).parse()?)
}

/// Unix seconds, as the catalog stores time.
fn unix(time: SystemTime) -> i64 {
    time.duration_since(UNIX_EPOCH)
        .map_or(0, |age| i64::try_from(age.as_secs()).unwrap_or(i64::MAX))
}

/// Run one export, sync pass and import, as the sync agent does.
///
/// With sharing off this is only the substrate pass.
pub fn pass(
    store: &mut Store,
    context: Context<'_>,
    trigger: Trigger,
    now: SystemTime,
) -> Result<(dots_substrate::Pass, Option<(Exported, Imported)>), Error> {
    let enabled = store.sharing_for(context.machine())?;
    let exported = if enabled {
        Some(export::export(store, context, now)?)
    } else {
        None
    };
    let pass = context.substrate.sync(trigger, now)?;
    let Some(exported) = exported else {
        return Ok((pass, None));
    };
    let imported = import::import(store, context, now)?;
    Ok((pass, Some((exported, imported))))
}

/// Export trees with local changes into the outbox, without syncing.
pub fn export(store: &mut Store, context: Context<'_>, now: SystemTime) -> Result<Exported, Error> {
    if !store.sharing_for(context.machine())? {
        return Ok(Exported::default());
    }
    export::export(store, context, now)
}

/// Import every changed tree from the local substrate copy.
pub fn import(store: &mut Store, context: Context<'_>, now: SystemTime) -> Result<Imported, Error> {
    if !store.sharing_for(context.machine())? {
        return Ok(Imported::default());
    }
    import::import(store, context, now)
}

/// Make this machine the home of `root`'s tree: compare-and-swap from the
/// version this machine last imported. When the tree moved on since, fetch
/// and import it, then refuse, describing what changed; rerunning takes the
/// newly seen version.
pub fn take(
    store: &mut Store,
    context: Context<'_>,
    root: &str,
    now: SystemTime,
) -> Result<Taken, Error> {
    let machine = context.machine().clone();
    if !store.sharing_for(&machine)? {
        return Err(share(
            "sharing is off on this machine; enable it with dots task share enable",
        ));
    }
    let row = import::shared_row(store, root)?.ok_or_else(|| {
        share(format!(
            "{root} is not a tree imported from another machine"
        ))
    })?;
    let project = Project::try_new(row.project.as_str())?;
    let project = &project;
    if row.home == machine.as_str() {
        return Err(share(format!("{root} is already homed on this machine")));
    }
    let shared = SharedProject::try_new(row.shared_project.clone())?;
    let (Some(version), Some(manifest_hash)) = (row.version.clone(), row.manifest) else {
        return Err(share(format!(
            "{root} has no imported version to take over"
        )));
    };
    let seen: Manifest = decode(&context.substrate.blob(&manifest_hash)?, "tree manifest")?;
    if seen.moved_to.is_some() {
        return Err(share(format!(
            "{root} became part of another tree; take over that tree instead"
        )));
    }
    import::check_brief(store, project, root, &seen)?;
    let taken = Manifest {
        home: machine.clone(),
        written_at: unix(now),
        ..seen.clone()
    };
    let blob = encode(&taken)?;
    let name = tree_ref(&shared, root)?;
    let update = Update {
        name,
        expected: Some(dots_substrate::Version::new(version)?),
        root: blob.hash(),
        new_blobs: vec![blob.clone()],
    };
    match context.substrate.update(&update, now)? {
        Outcome::Applied(version) => {
            import::taken(store, project, root, &taken, &blob, &version)?;
            Ok(Taken {
                project: project.as_str().to_owned(),
                root: root.to_owned(),
                previous_home: seen.home.to_string(),
                previous_written_at: seen.written_at,
                version: version.to_string(),
            })
        }
        Outcome::Conflict { .. } => {
            let before = import::tree_events(store, root)?;
            context.substrate.sync(Trigger::Explicit, now)?;
            import::import(store, context, now)?;
            let after = import::shared_row(store, root)?;
            let events = import::tree_events(store, root)?.saturating_sub(before);
            let (home, written_at) = after
                .map(|row| (row.home, row.written_at.unwrap_or_default()))
                .unwrap_or_default();
            Err(share(format!(
                "{root} changed since this machine last saw it: it is now homed on {home}, written at {written_at} (Unix time), with {events} new events; review it with dots task goal show {root} or dots task show {root}, then rerun dots task share take {root} to take over this version"
            )))
        }
        Outcome::Unknown => Err(share(format!(
            "the remote did not confirm the takeover of {root}; rerun dots task share take {root}"
        ))),
    }
}
