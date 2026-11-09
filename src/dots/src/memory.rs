//! Shared knowledge snapshots and machine-local project bindings.
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::io::{Read as _, Write as _};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use anyhow::{Context as _, Result, ensure};
use dots_documents::{mutation_lock::MutationLock, staged_file::StagedFile, text};
use dots_memory::{
    Action, Change, DocumentRead, Freshness, Head, ProjectId, Resolution, Scope, Snapshot, Store,
    StoreId, View,
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::json;

use crate::command::{MemoryCommand, MemoryProjectCommand};
use crate::substrate::unix;

const MAX_JSON_BYTES: usize = 66 * 1024 * 1024;
const MAX_CONTROL_BYTES: usize = 1024 * 1024;
/// A local copy older than this is labeled stale: the agent passes every 20
/// seconds, so it has missed several.
const STALE_AFTER: Duration = Duration::from_secs(120);
const BINDINGS: &str = "bindings.json";
const LOCK: &str = "mutation.lock";

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Bindings {
    store: StoreId,
    projects: BTreeMap<String, ProjectId>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Reconciliation {
    local: Snapshot,
    incoming: Snapshot,
    resolutions: Vec<Resolution>,
}

pub(crate) fn open() -> Result<Store> {
    let local = crate::project::data_root()?.join("memory");
    Ok(Store::open(crate::substrate::open()?, &local)?)
}

/// When the local copy was last confirmed current and whether sync is failing.
fn freshness(head: &Head, now: SystemTime) -> serde_json::Value {
    let Freshness { as_of, failing } = &head.freshness;
    let age = now.duration_since(*as_of).unwrap_or_default();
    json!({
        "as_of": unix(*as_of),
        "age_seconds": age.as_secs(),
        "stale": age > STALE_AFTER || failing.is_some(),
        "stale_after_seconds": STALE_AFTER.as_secs(),
        "failing": failing.as_ref().map(|failing| json!({
            "since": unix(failing.since),
            "failures": failing.failures,
            "last_error": failing.reason,
        })),
    })
}

pub(crate) struct Request<'a> {
    pub(crate) command: MemoryCommand,
    pub(crate) directory: &'a Path,
    /// `--project`: a registered local project instead of the checkout's.
    pub(crate) explicit: Option<&'a str>,
    /// `--fresh`: run one sync pass before reading.
    pub(crate) fresh: bool,
    pub(crate) json: bool,
}

#[expect(
    clippy::too_many_lines,
    reason = "Keep command dispatch beside its parsed arguments"
)]
pub(crate) fn run(request: Request<'_>) -> Result<String> {
    let Request {
        command,
        directory,
        explicit,
        fresh,
        json: as_json,
    } = request;
    let uses_local_project = matches!(
        &command,
        MemoryCommand::Context
            | MemoryCommand::Search { scope: None, .. }
            | MemoryCommand::Project {
                command: MemoryProjectCommand::Bind { .. } | MemoryProjectCommand::Unbind { .. }
            }
    );
    ensure!(
        explicit.is_none() || uses_local_project,
        "--project selects a local catalog binding only for context, default search and project bind/unbind; document scopes use exact shared IDs"
    );
    let mut store = open()?;
    let now = SystemTime::now();
    if fresh {
        store.sync(now)?;
    }
    match command {
        MemoryCommand::Init => {
            let snapshot = store.initialize(now)?;
            encode(&json!({"snapshot": snapshot}))
        }
        MemoryCommand::Context => {
            let head = store.head()?;
            let view = store.view(&head, None)?;
            let ProjectScope {
                scope: project,
                diagnostic,
                sharing,
            } = match project_scope(&store, &head, &view, directory, explicit) {
                Ok(scope) => scope,
                Err(error) if explicit.is_none() => ProjectScope {
                    scope: None,
                    diagnostic: Some(format!("Project knowledge unavailable: {error:#}")),
                    sharing: None,
                },
                Err(error) => return Err(error),
            };
            let global = index(&view, &Scope::Global);
            let project = project.map(|scope| index(&view, &scope));
            encode(&json!({
                "store": head.store,
                "snapshot": head.snapshot,
                "freshness": freshness(&head, now),
                "pending": store.pending()?.len(),
                // Touched whenever this machine's copy or local knowledge state changes.
                "changes": store.substrate().changes(),
                "global": global,
                "project": project,
                "diagnostic": diagnostic,
                "sharing": sharing,
            }))
        }
        MemoryCommand::Status => {
            let head = store.head()?;
            let pending = store.pending_plans(&head)?;
            encode(&json!({
                "store": head.store,
                "machine": store.substrate().machine(),
                "snapshot": head.snapshot,
                "freshness": freshness(&head, now),
                "pending": pending,
            }))
        }
        MemoryCommand::Read {
            path,
            scope,
            snapshot,
            start,
            lines,
        } => {
            let head = store.head()?;
            let read = store.view(&head, snapshot.as_ref())?.read(
                &scope,
                &path,
                start,
                usize::from(lines),
            )?;
            if as_json {
                let mut value = serde_json::to_value(&read)?;
                value["freshness"] = freshness(&head, now);
                encode(&value)
            } else {
                Ok(render_document(&read, &head, now))
            }
        }
        MemoryCommand::Write {
            path,
            scope,
            expected_snapshot,
            content_file,
        } => {
            let bytes = read_input(directory, &content_file, dots_memory::MAX_FILE_BYTES)?;
            let content = String::from_utf8(bytes).context("decode knowledge input as UTF-8")?;
            let change = Change {
                scope,
                path,
                action: Action::Write { content },
            };
            encode(&store.publish(&expected_snapshot, &[change], now)?)
        }
        MemoryCommand::Remove {
            path,
            scope,
            expected_snapshot,
        } => {
            let change = Change {
                scope,
                path,
                action: Action::Remove {},
            };
            encode(&store.publish(&expected_snapshot, &[change], now)?)
        }
        MemoryCommand::Publish {
            expected_snapshot,
            input_file,
        } => {
            let input = read_input(directory, &input_file, MAX_JSON_BYTES)?;
            let changes: Vec<Change> =
                serde_json::from_slice(&input).context("decode knowledge changes")?;
            encode(&store.publish(&expected_snapshot, &changes, now)?)
        }
        MemoryCommand::Search {
            query,
            scope,
            snapshot,
            offset,
            limit,
        } => {
            let head = store.head()?;
            let view = store.view(&head, snapshot.as_ref())?;
            let mut diagnostic = None;
            let scopes = if let Some(scope) = scope {
                vec![scope]
            } else {
                let (project, message) =
                    match project_scope(&store, &head, &view, directory, explicit) {
                        Ok(scope) => (scope.scope, scope.diagnostic),
                        Err(error) if explicit.is_none() => (
                            None,
                            Some(format!("Project knowledge unavailable: {error:#}")),
                        ),
                        Err(error) => return Err(error),
                    };
                diagnostic = message;
                let mut scopes = vec![Scope::Global];
                scopes.extend(project);
                scopes
            };
            let results = scopes
                .iter()
                .map(|scope| view.search(scope, &query, offset as usize, usize::from(limit)))
                .collect::<dots_memory::Result<Vec<_>>>()?;
            encode(&json!({
                "snapshot": view.snapshot(),
                "freshness": freshness(&head, now),
                "results": results,
                "diagnostic": diagnostic,
            }))
        }
        MemoryCommand::Project { command } => {
            project_command(&mut store, directory, explicit, command, now)
        }
        MemoryCommand::Reconcile { input_file } => {
            let input = read_input(directory, &input_file, MAX_JSON_BYTES)?;
            let input: Reconciliation =
                serde_json::from_slice(&input).context("decode reconciliation")?;
            let snapshot =
                store.reconcile(&input.local, &input.incoming, &input.resolutions, now)?;
            encode(
                &json!({"state": "published", "snapshot": snapshot, "pending": store.pending()?}),
            )
        }
        MemoryCommand::Inspect {
            candidate,
            path,
            scope,
            start,
            lines,
        } => encode(&store.inspect(&candidate, &scope, &path, start, usize::from(lines))?),
        MemoryCommand::Convert { source } => {
            let source = input_path(directory, &source);
            let conversion = dots_memory::convert(&source, &mut store, now)?;
            let installed = {
                let _lock = MutationLock::acquire(&store.local().join(LOCK))?;
                let path = store.local().join(BINDINGS);
                let absent = text::read_bytes(&path, MAX_CONTROL_BYTES)?.is_none();
                if absent && !conversion.bindings.is_empty() {
                    replace_json(
                        &path,
                        &Bindings {
                            store: conversion.store.clone(),
                            projects: conversion.bindings.clone(),
                        },
                    )?;
                    store.substrate().notify()?;
                }
                absent
            };
            let mut value = serde_json::to_value(&conversion)?;
            value["bindings_installed"] = json!(installed && !conversion.bindings.is_empty());
            encode(&value)
        }
    }
}

fn index(view: &View<'_>, scope: &Scope) -> serde_json::Value {
    match view.read(scope, Path::new("MEMORY.md"), 1, 200) {
        Ok(read) => {
            json!({"state":"available", "snapshot":read.snapshot, "scope":read.scope, "document":read.document})
        }
        Err(error) => {
            json!({"state":"unavailable", "snapshot":view.snapshot(), "scope":scope, "error":format!("{error:#}")})
        }
    }
}

/// Whether a checkout's project shares across machines, so agents can offer
/// to set it up.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Sharing {
    Bound,
    /// The user chose not to share it from this machine.
    Declined,
    Unshared,
}

struct ProjectScope {
    scope: Option<Scope>,
    diagnostic: Option<String>,
    /// None outside Git.
    sharing: Option<Sharing>,
}

fn project_scope(
    store: &Store,
    head: &Head,
    view: &View<'_>,
    directory: &Path,
    explicit: Option<&str>,
) -> Result<ProjectScope> {
    let unavailable = |diagnostic: String, sharing| ProjectScope {
        scope: None,
        diagnostic: Some(diagnostic),
        sharing,
    };
    if !crate::project::catalog_path()?.try_exists()? {
        ensure!(
            explicit.is_none(),
            "local catalog unavailable; initialize a project explicitly"
        );
        let sharing = crate::project::common_directory(directory)?.map(|_| Sharing::Unshared);
        return Ok(unavailable(
            "Local project catalog is absent; global knowledge remains available.".into(),
            sharing,
        ));
    }
    let mut catalog = crate::project::open()?;
    let local = match crate::project::resolve_optional(&mut catalog, directory, explicit)? {
        crate::project::Resolution::Bound(local) => local,
        unbound @ crate::project::Resolution::Unbound => {
            return Ok(unavailable(
                unbound.message().into(),
                Some(Sharing::Unshared),
            ));
        }
        outside @ crate::project::Resolution::OutsideGit => {
            return Ok(unavailable(outside.message().into(), None));
        }
    };
    let bindings = bindings(store, &head.store)?;
    let Some(id) = bindings.projects.get(local.as_str()) else {
        let sharing = if catalog.share_declined(&local)? {
            Sharing::Declined
        } else {
            Sharing::Unshared
        };
        return Ok(unavailable(
            format!(
                "Local project {} has no shared knowledge binding; dots project share sets one up.",
                local.as_str()
            ),
            Some(sharing),
        ));
    };
    ensure!(
        view.manifest().projects.contains_key(id),
        "shared project binding {id} is unavailable at snapshot {}; no scope fallback",
        view.snapshot()
    );
    Ok(ProjectScope {
        scope: Some(Scope::Project(id.clone())),
        diagnostic: None,
        sharing: Some(Sharing::Bound),
    })
}

fn project_command(
    store: &mut Store,
    directory: &Path,
    explicit: Option<&str>,
    command: MemoryProjectCommand,
    now: SystemTime,
) -> Result<String> {
    match command {
        MemoryProjectCommand::Create {
            name,
            expected_snapshot,
        } => encode(&store.register_project(&expected_snapshot, &name, now)?),
        MemoryProjectCommand::List => {
            let head = store.head()?;
            let view = store.view(&head, None)?;
            encode(&json!({
                "snapshot": head.snapshot,
                "freshness": freshness(&head, now),
                "projects": view.manifest().projects,
            }))
        }
        MemoryProjectCommand::Bind { id } => {
            let mut catalog = crate::project::open()?;
            let local = crate::project::resolve(&mut catalog, directory, explicit)?;
            let queued = bind(store, &mut catalog, &local, &id)?;
            encode(&json!({"local_project": local, "shared_project": id, "queued": queued}))
        }
        MemoryProjectCommand::Unbind { expected } => {
            let mut catalog = crate::project::open()?;
            let local = crate::project::resolve(&mut catalog, directory, explicit)?;
            let head = store.head()?;
            let _lock = MutationLock::acquire(&store.local().join(LOCK))?;
            let mut bindings = bindings(store, &head.store)?;
            ensure!(
                bindings.projects.get(local.as_str()) == Some(&expected),
                "local knowledge binding changed or is absent; inspect context before unbinding"
            );
            bindings.projects.remove(local.as_str());
            replace_json(&store.local().join(BINDINGS), &bindings)?;
            store.substrate().notify()?;
            encode(&json!({"local_project": local, "unbound_shared_project": expected}))
        }
    }
}

/// Bind `local` to the shared identity `id` on this machine. Clears a
/// decline and, while sharing is on, queues and exports the project's
/// existing trees; returns how many were queued. Rebinding the same identity
/// is a no-op binding that still queues.
pub(crate) fn bind(
    store: &Store,
    catalog: &mut dots_tasks::Store,
    local: &dots_tasks::Project,
    id: &ProjectId,
) -> Result<usize> {
    let head = store.head()?;
    let view = store.view(&head, None)?;
    let known = &view.manifest().projects;
    ensure!(
        known.contains_key(id),
        "unknown shared knowledge project {id}; known identities: {}",
        identities(known)
    );
    {
        let _lock = MutationLock::acquire(&store.local().join(LOCK))?;
        let mut bindings = bindings(store, &head.store)?;
        if let Some(current) = bindings.projects.get(local.as_str()) {
            ensure!(
                current == id,
                "local project is already bound to {current}; unbind that exact identity first (dots memory project unbind --expected {current})"
            );
        }
        bindings
            .projects
            .insert(local.as_str().to_owned(), id.clone());
        replace_json(&store.local().join(BINDINGS), &bindings)?;
    }
    let queued = catalog.share_project(local)?;
    store.substrate().notify()?;
    crate::share::after_write(catalog);
    Ok(queued)
}

/// The shared identity `local` is bound to on this machine, if any.
pub(crate) fn binding(store: &Store, local: &dots_tasks::Project) -> Result<Option<ProjectId>> {
    let head = store.head()?;
    Ok(bindings(store, &head.store)?
        .projects
        .get(local.as_str())
        .cloned())
}

/// Shared identities as `ID (name)`, for errors that list valid choices.
pub(crate) fn identities(known: &BTreeMap<ProjectId, String>) -> String {
    if known.is_empty() {
        return "none".into();
    }
    known
        .iter()
        .map(|(id, name)| format!("{id} ({name})"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Local projects bound to shared project identities on this machine, as
/// (local, shared) keys.
pub(crate) fn shared_bindings() -> Result<Vec<(String, String)>> {
    let path = crate::project::data_root()?.join("memory").join(BINDINGS);
    Ok(optional_json::<Bindings>(&path, MAX_CONTROL_BYTES)?
        .map(|bindings| {
            bindings
                .projects
                .into_iter()
                .map(|(local, shared)| (local, shared.as_str().to_owned()))
                .collect()
        })
        .unwrap_or_default())
}

fn bindings(store: &Store, id: &StoreId) -> Result<Bindings> {
    let binding = optional_json::<Bindings>(&store.local().join(BINDINGS), MAX_CONTROL_BYTES)?
        .unwrap_or_else(|| Bindings {
            store: id.clone(),
            projects: BTreeMap::new(),
        });
    ensure!(
        &binding.store == id,
        "project bindings belong to another knowledge store"
    );
    Ok(binding)
}

fn bounded_stdin(limit: usize) -> Result<Vec<u8>> {
    let mut input = Vec::new();
    std::io::stdin()
        .take((limit + 1) as u64)
        .read_to_end(&mut input)?;
    ensure!(
        input.len() <= limit,
        "stdin exceeds input budget ({limit} bytes)"
    );
    Ok(input)
}

fn read_input(directory: &Path, path: &Path, limit: usize) -> Result<Vec<u8>> {
    if path.as_os_str() == "-" {
        bounded_stdin(limit)
    } else {
        let path = input_path(directory, path);
        text::read_bytes(&path, limit)?
            .with_context(|| format!("input file is absent: {}", path.display()))
    }
}

fn optional_json<T: DeserializeOwned>(path: &Path, limit: usize) -> Result<Option<T>> {
    text::read_bytes(path, limit)?
        .map(|bytes| {
            serde_json::from_slice(&bytes).with_context(|| format!("decode {}", path.display()))
        })
        .transpose()
}

fn replace_json(path: &Path, value: &impl Serialize) -> Result<()> {
    let parent = path.parent().context("metadata needs a parent")?;
    let bytes = serde_json::to_vec(value)?;
    ensure!(
        bytes.len() <= MAX_CONTROL_BYTES,
        "local knowledge metadata exceeds its budget"
    );
    let stage = StagedFile::create(parent)?;
    stage.file().write_all(&bytes)?;
    stage.publish_replace(path)?;
    Ok(())
}

fn input_path(directory: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_owned()
    } else {
        directory.join(path)
    }
}

fn encode(value: &impl Serialize) -> Result<String> {
    Ok(format!("{}\n", serde_json::to_string_pretty(value)?))
}

fn render_document(read: &DocumentRead, head: &Head, now: SystemTime) -> String {
    let scope = match &read.scope {
        Scope::Global => "global",
        Scope::Project(id) => id.as_str(),
    };
    let mut out = format!(
        "Snapshot {} · scope {} · {}\n",
        read.snapshot,
        scope,
        read.document.path.display()
    );
    let Freshness { as_of, failing } = &head.freshness;
    let age = now.duration_since(*as_of).unwrap_or_default();
    if let Some(failing) = failing {
        let _ = writeln!(
            out,
            "Stale: local copy from {}s ago; sync is failing ({}).",
            age.as_secs(),
            failing.reason
        );
    } else if age > STALE_AFTER {
        let _ = writeln!(
            out,
            "Stale: local copy from {}s ago; is the sync agent running?",
            age.as_secs()
        );
    }
    if !read.document.exists {
        out.push_str("Document is absent.\n");
    }
    out.push_str(&read.document.content);
    if let Some(next) = read.document.next_line {
        let _ = writeln!(
            out,
            "More: repeat this read with --start {next}; retain scope {scope} and snapshot {}",
            read.snapshot
        );
    }
    out
}
