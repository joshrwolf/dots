//! One server-scoped scheduler for GitHub sidebar subscriptions. Network work is
//! bounded and off the service control loop; all cache changes/publication have
//! one owner. This service is read-only with respect to GitHub and agents.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc,
};
use std::thread::{self, JoinHandle};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::Result;
use herdr_prs::{Associations, Registry, Scope};
use herdrkit::events::{Events, Subscription};
use herdrkit::resources::{Replace, Resource, Tone};
use herdrkit::service::{Handler, Service};
use herdrkit::{Client, Invocation, WorkspaceId};
use rusqlite::{Connection, OptionalExtension as _, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::status;
use crate::github::{self, CheckBucket, PullRequestState, SidebarSnapshot};

const SOURCE: &str = "herdr-github";
const NETWORK_SPACING: u64 = 10;
const INVENTORY_INTERVAL: u64 = 15;

const RESOURCE_TTL_MS: u64 = 120_000;
const RESOURCE_RENEWAL: u64 = 30;
const SCHEMA: &str = "PRAGMA journal_mode=WAL; CREATE TABLE IF NOT EXISTS resource_snapshots(url TEXT PRIMARY KEY, body TEXT NOT NULL);";

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Request {
    Reconcile,
    Refresh { workspace: WorkspaceId },
}

pub fn invoke(invocation: &Invocation, manual: bool) -> Result<()> {
    let service = Service::new(invocation, "github-status")?;
    if service.is_worker() {
        return service
            .run(Scheduler::open(
                invocation.client().clone(),
                invocation.state_dir(),
            )?)
            .map_err(Into::into);
    }
    let request = if manual {
        Request::Refresh {
            workspace: invocation.require_workspace_id()?.clone(),
        }
    } else {
        Request::Reconcile
    };
    service.wake(serde_json::to_value(request)?)?;
    Ok(())
}

pub fn show_status(invocation: &Invocation) -> Result<()> {
    let service = Service::new(invocation, "github-status")?;
    println!("{}", serde_json::to_string_pretty(&service.status()?)?);
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Cached {
    resource: Resource,
    fetched: u64,
    interval: u64,
}

impl Cached {
    fn from_snapshot(state: &SidebarSnapshot, now: u64) -> Self {
        let preview = status::Preview::from_snapshot(state);
        let completed = state.pull_request.state != PullRequestState::Open;
        let mut parts = vec![preview.state.to_owned()];
        if !completed {
            parts.extend(preview.failed.iter().cloned());
            parts.extend(preview.pending.iter().cloned());
            parts.extend(preview.checks.map(str::to_owned));
            parts.extend(preview.review.map(str::to_owned));
        }
        let tone = if completed {
            Tone::Muted
        } else if preview.failed.is_some() {
            Tone::Failure
        } else if preview.pending.is_some() {
            Tone::Warning
        } else {
            Tone::Normal
        };
        Self {
            resource: Resource {
                id: String::new(),
                reference: String::new(),
                label: preview.pull_request,
                status: parts.join(" · "),
                tone,
                completed,
                action: "open-link".into(),
            },
            fetched: now,
            interval: cadence(state),
        }
    }
}

fn cadence(state: &SidebarSnapshot) -> u64 {
    if state.pull_request.state != PullRequestState::Open {
        1800
    } else if state
        .checks
        .iter()
        .any(|check| check.bucket == CheckBucket::Pending)
    {
        45
    } else {
        180
    }
}

fn retry_delay(failures: u32) -> u64 {
    30_u64.saturating_mul(1_u64 << failures.min(6)).min(1800)
}

#[derive(Debug)]
struct Workspace {
    checkout: PathBuf,
    identity: String,
    associations: Associations,
    published: Option<(String, u64)>,
}

#[derive(Debug)]
struct PullRequest {
    cached: Option<Cached>,
    due: u64,
    failures: u32,
    error: Option<String>,
    fetching: bool,
}

#[derive(Debug)]
enum Completion {
    Inventory(Result<Vec<InventoryRow>, String>),
    Fetched {
        url: String,
        result: Result<Cached, String>,
    },
    Published(Vec<PublicationResult>),
}

type InventoryRow = (WorkspaceId, PathBuf, String, Associations);
type PublicationResult = (WorkspaceId, String, String, Result<(), String>);

#[derive(Debug)]
struct Scheduler {
    client: Client,
    server_id: String,
    database: Connection,
    registry_path: PathBuf,
    workspaces: BTreeMap<WorkspaceId, Workspace>,
    pulls: BTreeMap<String, PullRequest>,
    manual: BTreeSet<WorkspaceId>,
    sender: mpsc::Sender<Completion>,
    receiver: mpsc::Receiver<Completion>,
    workers: Vec<JoinHandle<()>>,
    event_worker: Option<JoinHandle<()>>,
    stopping: Arc<AtomicBool>,
    dirty: Arc<AtomicBool>,
    inventory_at: u64,
    inventory_running: bool,
    network_at: u64,
    publish_at: u64,
    last_error: Option<String>,
    network_active: usize,
    publishing: bool,
}

impl Scheduler {
    fn open(client: Client, directory: &Path) -> Result<Self> {
        let server_id = client.server_id()?;
        let database = Connection::open(directory.join("github.sqlite3"))?;
        database.busy_timeout(Duration::from_secs(2))?;
        database.execute_batch(SCHEMA)?;
        let (sender, receiver) = mpsc::channel();
        let stopping = Arc::new(AtomicBool::new(false));
        let dirty = Arc::new(AtomicBool::new(true));
        let event_worker = {
            let client = client.clone();
            let stopping = stopping.clone();
            let dirty = dirty.clone();
            thread::spawn(move || {
                while !stopping.load(Ordering::Relaxed) {
                    match Events::subscribe(&client, &Subscription::workspace_topology()) {
                        Ok(mut events) => {
                            while !stopping.load(Ordering::Relaxed) {
                                match events.changes(Duration::from_secs(1)) {
                                    Ok(Some(_)) => {
                                        dirty.store(true, Ordering::Relaxed);
                                    }
                                    Ok(None) => {}
                                    Err(error) => {
                                        eprintln!("GitHub topology subscription: {error}");
                                        break;
                                    }
                                }
                            }
                        }
                        Err(error) => eprintln!("GitHub topology subscription: {error}"),
                    }
                    for _ in 0..50 {
                        if stopping.load(Ordering::Relaxed) {
                            break;
                        }
                        thread::sleep(Duration::from_millis(100));
                    }
                }
            })
        };
        let registry_path = herdrkit::user_state_directory()?.join(herdr_prs::REGISTRY_PATH);
        if let Some(parent) = registry_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        Registry::open(&registry_path)?;
        Ok(Self {
            registry_path,
            client,
            server_id,
            database,
            workspaces: BTreeMap::new(),
            pulls: BTreeMap::new(),
            manual: BTreeSet::new(),
            sender,
            receiver,
            workers: Vec::new(),
            event_worker: Some(event_worker),
            stopping,
            dirty,
            inventory_at: 0,
            inventory_running: false,
            network_at: 0,
            publish_at: 0,
            last_error: None,
            network_active: 0,
            publishing: false,
        })
    }

    fn load_pull(&mut self, url: &str, now: u64) -> Result<()> {
        if self.pulls.contains_key(url) {
            return Ok(());
        }
        let cached = self
            .database
            .query_row(
                "SELECT body FROM resource_snapshots WHERE url=?1",
                [url],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .map(|value| serde_json::from_str::<Cached>(&value))
            .transpose()?;
        let due = cached.as_ref().map_or(now, |cache| {
            cache
                .fetched
                .saturating_add(cache.interval)
                .min(now + cache.interval)
        });
        self.pulls.insert(
            url.into(),
            PullRequest {
                cached,
                due,
                failures: 0,
                error: None,
                fetching: false,
            },
        );
        Ok(())
    }

    fn reconcile_inventory(
        &mut self,
        result: Result<Vec<InventoryRow>, String>,
        now: u64,
    ) -> Result<()> {
        self.inventory_running = false;
        let rows = match result {
            Ok(rows) => rows,
            Err(error) => {
                self.last_error = Some(error);
                return Ok(());
            }
        };
        let present: BTreeSet<_> = rows.iter().map(|(id, _, _, _)| id.clone()).collect();
        self.workspaces.retain(|id, _| present.contains(id));
        self.manual.retain(|id| present.contains(id));
        for (id, checkout, identity, associations) in rows {
            for association in &associations.pulls {
                self.load_pull(association.url.as_str(), now)?;
            }
            if let Some(current) = self.workspaces.get_mut(&id).filter(|current| {
                current.identity == identity
                    && current.associations.revision == associations.revision
            }) {
                current.checkout = checkout;
            } else {
                self.workspaces.insert(
                    id,
                    Workspace {
                        checkout,
                        identity,
                        associations,
                        published: None,
                    },
                );
            }
        }
        let subscribed: BTreeSet<_> = self
            .workspaces
            .values()
            .flat_map(|w| w.associations.pulls.iter().map(|p| p.url.as_str()))
            .collect();
        self.pulls
            .retain(|url, pull| pull.fetching || subscribed.contains(url.as_str()));
        Ok(())
    }

    fn complete(&mut self, completion: Completion, now: u64) -> Result<()> {
        if matches!(&completion, Completion::Fetched { .. }) {
            self.network_active = self.network_active.saturating_sub(1);
        }
        match completion {
            Completion::Inventory(result) => self.reconcile_inventory(result, now)?,
            Completion::Published(results) => {
                self.publishing = false;
                for (id, identity, signature, result) in results {
                    if let Some(workspace) = self
                        .workspaces
                        .get_mut(&id)
                        .filter(|w| w.identity == identity)
                    {
                        match result {
                            Ok(()) => {
                                workspace.published = Some((signature, now + RESOURCE_RENEWAL));
                            }
                            Err(error) => {
                                self.last_error = Some(error);
                                self.dirty.store(true, Ordering::Relaxed);
                            }
                        }
                    }
                }
            }
            Completion::Fetched { url, result } => {
                let Some(pull) = self.pulls.get_mut(&url) else {
                    return Ok(());
                };
                pull.fetching = false;
                match result {
                    Ok(cache) => {
                        self.database.execute("INSERT INTO resource_snapshots VALUES(?1,?2) ON CONFLICT(url) DO UPDATE SET body=excluded.body", params![url, serde_json::to_string(&cache)?])?;
                        pull.due = now + cache.interval;
                        pull.cached = Some(cache);
                        pull.failures = 0;
                        pull.error = None;
                    }
                    Err(error) => {
                        pull.failures += 1;
                        pull.due = now + retry_delay(pull.failures);
                        if rate_limited(&error) {
                            self.network_at = now + 1800;
                        }
                        pull.error = Some(error.clone());
                        self.last_error = Some(error);
                    }
                }
            }
        }
        self.publish_at = 0;
        Ok(())
    }

    fn advance(&mut self) -> Result<()> {
        let now = unix_now();
        while let Ok(completion) = self.receiver.try_recv() {
            self.complete(completion, now)?;
        }
        let mut active = Vec::new();
        for worker in self.workers.drain(..) {
            if worker.is_finished() {
                if worker.join().is_err() {
                    anyhow::bail!("GitHub worker panicked");
                }
            } else {
                active.push(worker);
            }
        }
        self.workers = active;
        if !self.inventory_running
            && (now >= self.inventory_at
                || (self.dirty.load(Ordering::Relaxed) && now + 10 >= self.inventory_at))
        {
            self.dirty.store(false, Ordering::Relaxed);
            self.inventory_running = true;
            self.inventory_at = now + INVENTORY_INTERVAL;
            let client = self.client.clone();
            let registry_path = self.registry_path.clone();
            let sender = self.sender.clone();
            self.workers.push(thread::spawn(move || {
                let result = (|| -> Result<Vec<InventoryRow>> {
                    let snapshot = client.snapshot()?;
                    let endpoint = client.endpoint().to_str().ok_or_else(|| anyhow::anyhow!("non-Unicode endpoint"))?;
                    let mut registry = Registry::open(&registry_path)?;
                    snapshot.workspaces.iter().map(|workspace| {
                        let identity = workspace.identity.clone().ok_or_else(|| anyhow::anyhow!("Herdr needs persistent workspace identities and workspace.resources support"))?;
                        let scope = Scope::new(endpoint.into(), identity.clone())?;
                        // gh can inspect an explicit URL without a git checkout.
                        let directory = snapshot.effective_workspace_dir(&workspace.workspace_id).unwrap_or(Path::new("/"));
                        Ok((workspace.workspace_id.clone(), directory.to_path_buf(), identity, registry.list(&scope)?))
                    }).collect()
                })().map_err(|error| format!("{error:#}"));
                let _ = sender.send(Completion::Inventory(result));
            }));
        }
        for workspace in std::mem::take(&mut self.manual) {
            if let Some(current) = self.workspaces.get(&workspace) {
                for association in &current.associations.pulls {
                    if let Some(pull) = self
                        .pulls
                        .get_mut(association.url.as_str())
                        .filter(|pull| !pull.fetching)
                    {
                        pull.due = now;
                    }
                }
            } else {
                self.manual.insert(workspace);
            }
        }
        self.schedule_network(now);
        if now >= self.publish_at {
            self.publish_at = now + 15;
            self.publish(now)?;
        }
        Ok(())
    }

    fn schedule_network(&mut self, now: u64) {
        if self.network_active >= 2 || now < self.network_at {
            return;
        }
        let Some((url, pull)) = self
            .pulls
            .iter_mut()
            .filter(|(url, pull)| {
                !pull.fetching
                    && pull.due <= now
                    && self
                        .workspaces
                        .values()
                        .any(|w| w.associations.pulls.iter().any(|p| p.url.as_str() == *url))
            })
            .min_by_key(|(_, pull)| pull.due)
        else {
            return;
        };
        let Some(checkout) = self
            .workspaces
            .values()
            .find(|w| w.associations.pulls.iter().any(|p| p.url.as_str() == url))
            .map(|w| w.checkout.clone())
        else {
            return;
        };
        pull.fetching = true;
        self.network_active += 1;
        let url = url.clone();
        let sender = self.sender.clone();
        self.workers.push(thread::spawn(move || {
            let result = github::sidebar(&checkout, &url)
                .map(|state| Cached::from_snapshot(&state, unix_now()))
                .map_err(|error| format!("{error:#}"));
            let _ = sender.send(Completion::Fetched { url, result });
        }));
        self.network_at = now + NETWORK_SPACING;
    }

    fn publish(&mut self, now: u64) -> Result<()> {
        if self.publishing {
            return Ok(());
        }
        let mut batch = Vec::new();
        for (id, workspace) in &mut self.workspaces {
            let resources: Vec<_> = workspace
                .associations
                .pulls
                .iter()
                .map(|association| {
                    resource(
                        association.url.as_str(),
                        self.pulls.get(association.url.as_str()),
                        now,
                    )
                })
                .collect();
            let encoded = serde_json::to_string(&resources)?;
            if workspace
                .published
                .as_ref()
                .is_some_and(|(signature, renew_at)| signature == &encoded && now < *renew_at)
            {
                continue;
            }
            batch.push((
                id.clone(),
                workspace.identity.clone(),
                workspace.associations.revision,
                encoded,
                resources,
            ));
        }
        if batch.is_empty() {
            return Ok(());
        }
        self.publishing = true;
        let client = self.client.clone();
        let server_id = self.server_id.clone();
        let sender = self.sender.clone();
        let registry_path = self.registry_path.clone();
        self.workers.push(thread::spawn(move || {
            let results = batch
                .into_iter()
                .map(|(id, identity, revision, signature, resources)| {
                    let result = (|| -> Result<()> {
                        anyhow::ensure!(
                            client.server_id()? == server_id,
                            "Herdr server replaced before publication"
                        );
                        let scope = Scope::new(
                            client.endpoint().to_string_lossy().into_owned(),
                            identity.clone(),
                        )?;
                        let mut registry = Registry::open(&registry_path)?;
                        let current = client.workspace_resources(&id)?;
                        anyhow::ensure!(current.identity == identity, "workspace lifetime changed");
                        registry
                            .while_current(&scope, revision, || {
                                client.replace_workspace_resources(&Replace {
                                    workspace_id: &id,
                                    identity: &identity,
                                    plugin_id: SOURCE,
                                    expected_revision: current
                                        .collections
                                        .get(SOURCE)
                                        .map_or(0, |collection| collection.revision),
                                    ttl_ms: RESOURCE_TTL_MS,
                                    resources: &resources,
                                })?;
                                Ok::<_, anyhow::Error>(())
                            })?
                            .ok_or_else(|| {
                                anyhow::anyhow!("PR associations changed before publication")
                            })??;
                        Ok(())
                    })()
                    .map_err(|error| format!("{error:#}"));
                    (id, identity, signature, result)
                })
                .collect();
            let _ = sender.send(Completion::Published(results));
        }));
        Ok(())
    }
}

impl Handler for Scheduler {
    fn request(&mut self, payload: Value) -> Result<Value, String> {
        match serde_json::from_value::<Request>(payload).map_err(|e| e.to_string())? {
            Request::Reconcile => {
                self.dirty.store(true, Ordering::Relaxed);
            }
            Request::Refresh { workspace } => {
                if let Some(current) = self.workspaces.get(&workspace) {
                    for association in &current.associations.pulls {
                        if let Some(pull) = self
                            .pulls
                            .get_mut(association.url.as_str())
                            .filter(|pull| !pull.fetching)
                        {
                            pull.due = 0;
                        }
                    }
                } else {
                    self.manual.insert(workspace);
                }
                self.dirty.store(true, Ordering::Relaxed);
            }
        }
        Ok(json!({ "queued": true }))
    }
    fn tick(&mut self) -> Result<(), String> {
        self.advance().map_err(|e| format!("{e:#}"))
    }
    fn status(&self) -> Value {
        json!({ "workspaces": self.workspaces.len(), "pull_requests": self.pulls.len(), "active_jobs": self.workers.len(), "next_network_at": self.network_at, "last_error": self.last_error })
    }
}

impl Drop for Scheduler {
    fn drop(&mut self) {
        self.stopping.store(true, Ordering::Relaxed);
        if let Some(worker) = self.event_worker.take() {
            let _ = worker.join();
        }
        for worker in self.workers.drain(..) {
            let _ = worker.join();
        }
    }
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs())
}
fn rate_limited(error: &str) -> bool {
    let error = error.to_ascii_lowercase();
    error.contains("rate limit") || error.contains("http 429")
}
fn resource(url: &str, pull: Option<&PullRequest>, now: u64) -> Resource {
    let mut resource = pull.and_then(|pull| pull.cached.as_ref()).map_or_else(
        || Resource {
            id: url.into(),
            reference: url.into(),
            label: format!("#{}", url.rsplit('/').next().unwrap_or("?")),
            status: "Unverified · waiting for GitHub".into(),
            tone: Tone::Muted,
            completed: false,
            action: "open-link".into(),
        },
        |cache| cache.resource.clone(),
    );
    resource.id = url.into();
    resource.reference = url.into();
    if let Some(pull) = pull {
        if let Some(cache) = &pull.cached {
            if pull.error.is_some() || now.saturating_sub(cache.fetched) > cache.interval * 2 {
                let _ = write!(
                    resource.status,
                    " · delayed (checked {}s ago)",
                    now.saturating_sub(cache.fetched)
                );
                resource.tone = Tone::Warning;
            }
        } else if pull.error.is_some() {
            resource.status = "Unavailable · retrying".into();
            resource.tone = Tone::Warning;
        }
    }
    resource
}

#[cfg(test)]
mod tests {
    use super::*;
    use herdr_prs::{Association, PullRequestUrl};

    fn scheduler() -> Scheduler {
        let database = Connection::open_in_memory().unwrap();
        database.execute_batch(SCHEMA).unwrap();
        let (sender, receiver) = mpsc::channel();
        Scheduler {
            client: Client::new("/unused"),
            server_id: "test".into(),
            database,
            registry_path: "/unused".into(),
            workspaces: BTreeMap::new(),
            pulls: BTreeMap::new(),
            manual: BTreeSet::new(),
            sender,
            receiver,
            workers: Vec::new(),
            event_worker: None,
            stopping: Arc::new(AtomicBool::new(false)),
            dirty: Arc::new(AtomicBool::new(false)),
            inventory_at: 0,
            inventory_running: false,
            network_at: 0,
            publish_at: 0,
            last_error: None,
            network_active: 0,
            publishing: false,
        }
    }
    fn associations(numbers: &[u64], revision: i64) -> Associations {
        Associations {
            revision,
            pulls: numbers
                .iter()
                .map(|number| Association {
                    url: PullRequestUrl::parse(&format!(
                        "https://github.com/example/project/pull/{number}"
                    ))
                    .unwrap(),
                    registered_at: 100,
                    source: "agent".into(),
                })
                .collect(),
        }
    }
    fn inventory(id: &str, numbers: &[u64], revision: i64) -> InventoryRow {
        (
            WorkspaceId::new(id),
            "/checkout".into(),
            format!("lifetime-{id}"),
            associations(numbers, revision),
        )
    }
    #[test]
    fn subscriptions_share_fetches_and_late_results_cannot_restore_registration() {
        let mut scheduler = scheduler();
        scheduler
            .reconcile_inventory(
                Ok(vec![inventory("w1", &[1, 2], 2), inventory("w2", &[1], 1)]),
                100,
            )
            .unwrap();
        assert_eq!(scheduler.pulls.len(), 2);
        let url = "https://github.com/example/project/pull/2";
        scheduler.pulls.get_mut(url).unwrap().fetching = true;
        scheduler.network_active = 1;
        scheduler
            .reconcile_inventory(
                Ok(vec![inventory("w1", &[1], 3), inventory("w2", &[1], 1)]),
                110,
            )
            .unwrap();
        scheduler
            .complete(
                Completion::Fetched {
                    url: url.into(),
                    result: Ok(Cached {
                        resource: resource(url, None, 100),
                        fetched: 100,
                        interval: 45,
                    }),
                },
                120,
            )
            .unwrap();
        assert_eq!(scheduler.network_active, 0);
        assert!(scheduler.workspaces.values().all(|workspace| {
            workspace
                .associations
                .pulls
                .iter()
                .all(|pull| pull.url.as_str() != url)
        }));
        // A fresh lifetime at the same short ID cannot inherit associations or publication state.
        let mut row = inventory("w1", &[], 0);
        row.2 = "new-lifetime".into();
        scheduler.reconcile_inventory(Ok(vec![row]), 130).unwrap();
        assert!(scheduler.pulls.is_empty());
        assert!(
            scheduler
                .workspaces
                .get(&WorkspaceId::new("w1"))
                .unwrap()
                .associations
                .pulls
                .is_empty()
        );
    }
    #[test]
    fn outage_keeps_last_observation_and_blocks_rate_limited_launches() {
        let mut scheduler = scheduler();
        let url = "https://github.com/example/project/pull/1";
        scheduler
            .reconcile_inventory(Ok(vec![inventory("w1", &[1], 1)]), 100)
            .unwrap();
        let mut item = resource(url, None, 100);
        item.status = "Open · ✓ CI".into();
        scheduler
            .complete(
                Completion::Fetched {
                    url: url.into(),
                    result: Ok(Cached {
                        resource: item,
                        fetched: 100,
                        interval: 45,
                    }),
                },
                100,
            )
            .unwrap();
        scheduler
            .complete(
                Completion::Fetched {
                    url: url.into(),
                    result: Err("HTTP 429 rate limit".into()),
                },
                150,
            )
            .unwrap();
        let projected = resource(url, scheduler.pulls.get(url), 150);
        assert!(projected.status.contains("✓ CI"));
        assert!(projected.status.contains("50s ago"));
        scheduler.schedule_network(151);
        assert!(scheduler.workers.is_empty());
        assert_eq!(scheduler.workspaces.len(), 1);
        scheduler.pulls.clear();
        scheduler.load_pull(url, 160).unwrap();
        assert_eq!(
            scheduler
                .pulls
                .get(url)
                .unwrap()
                .cached
                .as_ref()
                .unwrap()
                .fetched,
            100
        );
    }
    #[test]
    fn terminal_prs_are_still_polled_and_reopening_restores_open_row() {
        let mut state = SidebarSnapshot {
            pull_request: github::PullRequest {
                number: 1,
                draft: false,
                state: PullRequestState::Merged,
                review: github::ReviewStatus::Approved,
                head_oid: "abc".into(),
            },
            checks: Vec::new(),
        };
        let merged = Cached::from_snapshot(&state, 100);
        assert!(merged.resource.completed);
        assert_eq!(merged.interval, 1800);
        state.pull_request.state = PullRequestState::Open;
        let reopened = Cached::from_snapshot(&state, 200);
        assert!(!reopened.resource.completed);
        assert!(reopened.resource.status.contains("No checks"));
    }
}
