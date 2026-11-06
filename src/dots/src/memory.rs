//! Snapshot resources and machine-local knowledge bindings over the native Git owner.
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::io::{Read as _, Write as _};
use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result, ensure};
use dots_documents::{mutation_lock::MutationLock, staged_file::StagedFile, text};
use dots_memory::{
    BundleReceipt, Candidate, Change, Coordinator, DocumentRead, Endpoint, Identity, Peer,
    PeerRequest, PeerResponse, ProjectId, Resolution, Resource, Role, Scope, Snapshot, Store,
    StoreId, Tailscale, View,
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::json;

use crate::command::{MemoryCommand, MemoryPeerCommand, MemoryProjectCommand, MemoryRole};

const MAX_JSON_BYTES: usize = 66 * 1024 * 1024;
const MAX_CONTROL_BYTES: usize = 1024 * 1024;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Bindings {
    store: StoreId,
    projects: BTreeMap<String, ProjectId>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PeerBinding {
    local: Identity,
    peer: Peer,
    tailscale: PathBuf,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Reconciliation {
    recipient: Identity,
    local: Snapshot,
    incoming: Candidate,
    resolutions: Vec<Resolution>,
}

#[expect(
    clippy::too_many_lines,
    reason = "Keep command dispatch beside its parsed arguments"
)]
pub(crate) fn run(
    command: MemoryCommand,
    directory: &Path,
    explicit: Option<&str>,
    as_json: bool,
) -> Result<String> {
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
    if matches!(&command, MemoryCommand::Serve) {
        let mut input = Vec::new();
        std::io::stdin()
            .take((MAX_CONTROL_BYTES + 1) as u64)
            .read_to_end(&mut input)?;
        ensure!(
            input.len() <= MAX_CONTROL_BYTES,
            "peer request exceeds input budget"
        );
        let request: PeerRequest = serde_json::from_slice(&input).context("decode peer request")?;
        let response = match dots_memory::serve_peer(request) {
            Ok(response) => response,
            Err(dots_memory::Error::Conflict { expected, actual }) => {
                PeerResponse::Conflict { expected, actual }
            }
            Err(error) => return Err(error.into()),
        };
        return encode(&response);
    }
    let root = crate::project::data_root()?.join("knowledge");
    match command {
        MemoryCommand::Init { actor } => {
            let store = Store::initialize(&root, &actor)?;
            return encode(&json!({"identity": store.identity(), "snapshot": store.snapshot()?}));
        }
        MemoryCommand::Restore {
            actor,
            role,
            bundle,
            receipt_file,
        } => {
            let receipt: BundleReceipt =
                required_json(&input_path(directory, &receipt_file), MAX_CONTROL_BYTES)?;
            let role = match role {
                MemoryRole::Authority => Role::Authority,
                MemoryRole::Replica => Role::Replica,
            };
            let store = Store::restore(
                &root,
                &actor,
                role,
                &input_path(directory, &bundle),
                &receipt,
            )?;
            return encode(&json!({"identity": store.identity(), "snapshot": store.snapshot()?}));
        }
        _ => {}
    }
    let store = Store::open(&root).with_context(|| {
        format!(
            "knowledge unavailable at {}; initialize or restore explicitly; no filesystem fallback",
            root.display()
        )
    })?;
    match command {
        MemoryCommand::Context => {
            let view = store.latest()?;
            let (project, diagnostic) = match project_scope(&store, &view, directory, explicit) {
                Ok(scope) => scope,
                Err(error) if explicit.is_none() => (
                    None,
                    Some(format!("Project knowledge unavailable: {error:#}")),
                ),
                Err(error) => return Err(error),
            };
            let global = index(&view, &Scope::Global);
            let project = project.map(|scope| index(&view, &scope));
            encode(
                &json!({"identity": store.identity(), "snapshot": view.snapshot(), "global": global, "project": project, "diagnostic": diagnostic}),
            )
        }
        MemoryCommand::Status => {
            let snapshot = store.snapshot()?;
            let binding = peer_binding(&store)?;
            let mut observed = None;
            let mut delivered = None;
            let mut pending = None;
            if let Some(binding) = &binding {
                observed = binding.peer.last_observed(&store)?;
                delivered = binding.peer.last_delivery(&store)?;
                if let Some(incoming) = &observed
                    && !store.is_published(incoming)?
                {
                    let candidate = Candidate {
                        source: binding.peer.identity.clone(),
                        snapshot: incoming.clone(),
                    };
                    pending = Some(Coordinator::new(&store)?.plan(&snapshot, &candidate)?);
                }
            }
            encode(
                &json!({"identity": store.identity(), "snapshot": snapshot, "peer": binding.as_ref().map(|binding| &binding.peer), "last_retained_peer_snapshot": observed, "last_acknowledged_delivery": delivered, "pending_integration": pending}),
            )
        }
        MemoryCommand::Read {
            path,
            scope,
            snapshot,
            start,
            lines,
        } => {
            let view = selected_view(&store, snapshot)?;
            let read = view.read(&scope, &path, start, usize::from(lines))?;
            if as_json {
                encode(&read)
            } else {
                Ok(render_document(&read))
            }
        }
        MemoryCommand::Publish {
            expected_snapshot,
            input_file,
        } => {
            let changes: Vec<Change> =
                required_json(&input_path(directory, &input_file), MAX_JSON_BYTES)?;
            encode(&store.publish(&expected_snapshot, &changes)?)
        }
        MemoryCommand::Search {
            query,
            scope,
            snapshot,
            offset,
            limit,
        } => {
            let view = selected_view(&store, snapshot)?;
            let mut diagnostic = None;
            let scopes = if let Some(scope) = scope {
                vec![scope]
            } else {
                let (project, message) = match project_scope(&store, &view, directory, explicit) {
                    Ok(scope) => scope,
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
            encode(
                &json!({"snapshot": view.snapshot(), "results": results, "diagnostic": diagnostic}),
            )
        }
        MemoryCommand::Project { command } => project_command(&store, directory, explicit, command),
        MemoryCommand::Peer { command } => peer_command(&store, directory, command),
        MemoryCommand::Sync => {
            let binding =
                peer_binding(&store)?.context("no enrolled replica; use memory peer bind")?;
            let tailscale = Tailscale::new(binding.tailscale, crate::xdg::home()?)?;
            let candidate = binding.peer.fetch(&store, &tailscale)?;
            let local = store.snapshot()?;
            let coordinator = Coordinator::new(&store)?;
            let plan = coordinator.plan(&local, &candidate)?;
            if !plan.conflicts().is_empty() {
                return encode(
                    &json!({"state": "reconciliation_required", "snapshot": local, "plan": plan}),
                );
            }
            let snapshot = coordinator.accept(&plan, &[])?;
            let receipt = binding.peer.deliver(&store, &tailscale, &candidate.snapshot, &snapshot)
                .with_context(|| format!("knowledge is published locally at {snapshot}; peer delivery failed and needs a fresh sync"))?;
            encode(
                &json!({"state": if receipt.observed == snapshot { "delivered" } else { "delivered_peer_advanced" }, "snapshot": snapshot, "receipt": receipt}),
            )
        }
        MemoryCommand::Plan {
            expected_snapshot,
            candidate_file,
        } => {
            let candidate: Candidate =
                required_json(&input_path(directory, &candidate_file), MAX_CONTROL_BYTES)?;
            encode(&Coordinator::new(&store)?.plan(&expected_snapshot, &candidate)?)
        }
        MemoryCommand::Reconcile { input_file } => {
            let input: Reconciliation =
                required_json(&input_path(directory, &input_file), MAX_JSON_BYTES)?;
            ensure!(
                input.recipient == store.identity(),
                "reconciliation targets another local node"
            );
            let coordinator = Coordinator::new(&store)?;
            let plan = coordinator.plan(&input.local, &input.incoming)?;
            let snapshot = coordinator.accept(&plan, &input.resolutions)?;
            encode(&json!({"state": "published_locally", "snapshot": snapshot}))
        }
        MemoryCommand::Inspect {
            candidate_file,
            path,
            scope,
            start,
            lines,
        } => {
            let candidate: Candidate =
                required_json(&input_path(directory, &candidate_file), MAX_CONTROL_BYTES)?;
            encode(&candidate.inspect(
                &store,
                &Resource { scope, path },
                start,
                usize::from(lines),
            )?)
        }
        MemoryCommand::Export { output, snapshot } => {
            let snapshot = snapshot.map_or_else(|| store.snapshot(), Ok)?;
            encode(&store.export_bundle(&snapshot, &input_path(directory, &output))?)
        }
        MemoryCommand::Import {
            bundle,
            receipt_file,
        } => {
            let receipt: BundleReceipt =
                required_json(&input_path(directory, &receipt_file), MAX_CONTROL_BYTES)?;
            encode(&store.import_bundle(&input_path(directory, &bundle), &receipt)?)
        }
        MemoryCommand::Init { .. } | MemoryCommand::Restore { .. } | MemoryCommand::Serve => {
            unreachable!("early-return commands")
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

fn selected_view(store: &Store, snapshot: Option<Snapshot>) -> Result<View<'_>> {
    let snapshot = snapshot.map_or_else(|| store.snapshot(), Ok)?;
    Ok(store.view(&snapshot)?)
}

fn project_scope(
    store: &Store,
    view: &View<'_>,
    directory: &Path,
    explicit: Option<&str>,
) -> Result<(Option<Scope>, Option<String>)> {
    if !crate::project::catalog_path()?.try_exists()? {
        ensure!(
            explicit.is_none(),
            "local catalog unavailable; initialize a project explicitly"
        );
        return Ok((
            None,
            Some("Local project catalog is absent; global knowledge remains available.".into()),
        ));
    }
    let mut catalog = crate::project::open()?;
    let local = match crate::project::resolve_optional(&mut catalog, directory, explicit)? {
        crate::project::Resolution::Bound(local) => local,
        crate::project::Resolution::Unavailable(message) => {
            return Ok((None, Some(message.into())));
        }
    };
    let bindings = bindings(store)?;
    let Some(id) = bindings.projects.get(local.as_str()) else {
        return Ok((
            None,
            Some(format!(
                "Local project {} has no shared knowledge binding; use memory project bind SHARED_ID.",
                local.as_str()
            )),
        ));
    };
    ensure!(
        view.manifest().projects.contains_key(id),
        "shared project binding {id} is unavailable at snapshot {}; no scope fallback",
        view.snapshot()
    );
    Ok((Some(Scope::Project(id.clone())), None))
}

fn project_command(
    store: &Store,
    directory: &Path,
    explicit: Option<&str>,
    command: MemoryProjectCommand,
) -> Result<String> {
    match command {
        MemoryProjectCommand::Create {
            name,
            expected_snapshot,
        } => encode(&store.register_project(&expected_snapshot, &name)?),
        MemoryProjectCommand::List => {
            let view = store.latest()?;
            encode(&json!({"snapshot": view.snapshot(), "projects": view.manifest().projects}))
        }
        MemoryProjectCommand::Bind { id } => {
            let mut catalog = crate::project::open()?;
            let local = crate::project::resolve(&mut catalog, directory, explicit)?;
            let _lock = MutationLock::acquire(&store.root().join("mutation.lock"))?;
            ensure!(
                store.latest()?.manifest().projects.contains_key(&id),
                "unknown shared knowledge project {id}"
            );
            let mut bindings = bindings(store)?;
            if let Some(current) = bindings.projects.get(local.as_str()) {
                ensure!(
                    current == &id,
                    "local project is already bound to {current}; unbind that exact identity first"
                );
            }
            bindings
                .projects
                .insert(local.as_str().to_owned(), id.clone());
            replace_json(&store.root().join("bindings.json"), &bindings)?;
            encode(&json!({"local_project": local, "shared_project": id}))
        }
        MemoryProjectCommand::Unbind { expected } => {
            let mut catalog = crate::project::open()?;
            let local = crate::project::resolve(&mut catalog, directory, explicit)?;
            let _lock = MutationLock::acquire(&store.root().join("mutation.lock"))?;
            let mut bindings = bindings(store)?;
            ensure!(
                bindings.projects.get(local.as_str()) == Some(&expected),
                "local knowledge binding changed or is absent; inspect context before unbinding"
            );
            bindings.projects.remove(local.as_str());
            replace_json(&store.root().join("bindings.json"), &bindings)?;
            encode(&json!({"local_project": local, "unbound_shared_project": expected}))
        }
    }
}

fn bindings(store: &Store) -> Result<Bindings> {
    let binding =
        optional_json::<Bindings>(&store.root().join("bindings.json"), MAX_CONTROL_BYTES)?
            .unwrap_or_else(|| Bindings {
                store: store.id().clone(),
                projects: BTreeMap::new(),
            });
    ensure!(
        &binding.store == store.id(),
        "project bindings belong to another knowledge store"
    );
    Ok(binding)
}

fn peer_binding(store: &Store) -> Result<Option<PeerBinding>> {
    let binding = optional_json::<PeerBinding>(&store.root().join("peer.json"), MAX_CONTROL_BYTES)?;
    if let Some(binding) = &binding {
        ensure!(
            binding.local == store.identity(),
            "transport binding belongs to another local node; rebind explicitly"
        );
    }
    Ok(binding)
}

fn peer_command(store: &Store, directory: &Path, command: MemoryPeerCommand) -> Result<String> {
    let path = store.root().join("peer.json");
    match command {
        MemoryPeerCommand::Bind {
            host,
            root,
            executable,
            tailscale,
        } => {
            let executable_path = input_path(directory, &tailscale);
            let transport = Tailscale::new(executable_path.clone(), crate::xdg::home()?)?;
            let peer = Peer::discover(
                store,
                &transport,
                Endpoint {
                    host,
                    root,
                    executable,
                },
            )?;
            let binding = PeerBinding {
                local: store.identity(),
                peer,
                tailscale: executable_path,
            };
            let _lock = MutationLock::acquire(&store.root().join("mutation.lock"))?;
            if let Some(current) = peer_binding(store)? {
                ensure!(
                    serde_json::to_value(&current)? == serde_json::to_value(&binding)?,
                    "another replica/transport is already bound; unbind its exact node first"
                );
            }
            replace_json(&path, &binding)?;
            encode(&binding.peer)
        }
        MemoryPeerCommand::Unbind { expected_node } => {
            let _lock = MutationLock::acquire(&store.root().join("mutation.lock"))?;
            let current = peer_binding(store)?.context("no replica is bound")?;
            ensure!(
                current.peer.identity.node == expected_node,
                "replica binding changed; inspect status before unbinding"
            );
            fs::remove_file(&path).context("remove replica binding")?;
            dots_documents::directories::sync(store.root())?;
            encode(&json!({"unbound_node": expected_node}))
        }
    }
}

fn optional_json<T: DeserializeOwned>(path: &Path, limit: usize) -> Result<Option<T>> {
    text::read_bytes(path, limit)?
        .map(|bytes| {
            serde_json::from_slice(&bytes).with_context(|| format!("decode {}", path.display()))
        })
        .transpose()
}
fn required_json<T: DeserializeOwned>(path: &Path, limit: usize) -> Result<T> {
    optional_json(path, limit)?.with_context(|| format!("input file is absent: {}", path.display()))
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

fn render_document(read: &DocumentRead) -> String {
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
