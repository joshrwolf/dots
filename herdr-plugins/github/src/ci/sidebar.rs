//! One server-scoped scheduler for GitHub sidebar subscriptions. Network work is
//! bounded and off the service control loop; all cache changes/publication have
//! one owner. This service is read-only with respect to GitHub and agents.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc,
};
use std::thread::{self, JoinHandle};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::Result;
use herdrkit::events::{Events, Subscription};
use herdrkit::service::{Handler, Service};
use herdrkit::{Client, Invocation, MetadataReporter, Tokens, WorkspaceId};
use rusqlite::{Connection, OptionalExtension as _, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::status;
use crate::github::{self, CheckBucket, PullRequestState, SidebarSnapshot};

const SOURCE: &str = "herdr-github";
const NETWORK_SPACING: u64 = 10;
const INVENTORY_INTERVAL: u64 = 15;
const NEGATIVE_INTERVAL: u64 = 300;
const SCHEMA: &str = "PRAGMA journal_mode=WAL; CREATE TABLE IF NOT EXISTS snapshots(url TEXT PRIMARY KEY, body TEXT NOT NULL); CREATE TABLE IF NOT EXISTS bindings(server TEXT NOT NULL, workspace TEXT NOT NULL, checkout TEXT NOT NULL, identity TEXT NOT NULL, url TEXT, PRIMARY KEY(server,workspace));";

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
    values: BTreeMap<String, Option<String>>,
    fetched: u64,
    interval: u64,
}

impl Cached {
    fn from_snapshot(state: &SidebarSnapshot, now: u64) -> Self {
        let preview = status::Preview::from_snapshot(state);
        let values = [
            (status::PR_TOKEN, Some(preview.pull_request)),
            (status::STATE_TOKEN, Some(preview.state.into())),
            (status::REVIEW_TOKEN, preview.review.map(str::to_owned)),
            (status::CHECKS_TOKEN, preview.checks.map(str::to_owned)),
            (status::FAILED_TOKEN, preview.failed),
            (status::PENDING_TOKEN, preview.pending),
        ]
        .into_iter()
        .map(|(key, value)| (key.into(), value))
        .collect();
        Self {
            values,
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
    url: Option<String>,
    discover_at: u64,
    discovering: bool,
    published: Option<String>,
    error: Option<String>,
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
    Discovered {
        workspace: WorkspaceId,
        identity: String,
        result: Result<Option<String>, String>,
    },
    Fetched {
        url: String,
        result: Result<Cached, String>,
    },
    Published(Vec<PublicationResult>),
}

type InventoryRow = (WorkspaceId, PathBuf, Result<String, String>);
type PublicationResult = (WorkspaceId, String, String, Result<(), String>);

#[derive(Debug)]
struct Scheduler {
    client: Client,
    server_id: String,
    database: Connection,
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
        Ok(Self {
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
            .query_row("SELECT body FROM snapshots WHERE url=?1", [url], |row| {
                row.get::<_, String>(0)
            })
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
        let present: BTreeSet<_> = rows.iter().map(|(id, _, _)| id.clone()).collect();
        self.workspaces.retain(|id, _| present.contains(id));
        self.manual.retain(|id| present.contains(id));
        for (id, checkout, identity) in rows {
            let identity = match identity {
                Ok(value) => value,
                Err(error) => {
                    self.workspaces.remove(&id);
                    MetadataReporter::retained(&self.client, SOURCE)
                        .report_workspace(&id, &clear()?)?;
                    self.last_error = Some(error);
                    continue;
                }
            };
            if self
                .workspaces
                .get(&id)
                .is_some_and(|w| w.checkout == checkout && w.identity == identity)
            {
                continue;
            }
            let persisted = self
                .database
                .query_row(
                    "SELECT url FROM bindings WHERE server=?1 AND workspace=?2 AND checkout=?3 AND identity=?4",
                    params![self.server_id, id.as_str(), checkout.to_string_lossy(), identity],
                    |row| row.get::<_, Option<String>>(0),
                )
                .optional()?
                .flatten();
            if let Some(url) = &persisted {
                self.load_pull(url, now)?;
            }
            self.workspaces.insert(
                id,
                Workspace {
                    checkout,
                    identity,
                    url: persisted,
                    discover_at: now,
                    discovering: false,
                    published: None,
                    error: None,
                },
            );
        }
        Ok(())
    }

    fn complete(&mut self, completion: Completion, now: u64) -> Result<()> {
        if matches!(
            &completion,
            Completion::Fetched { .. } | Completion::Discovered { .. }
        ) {
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
                            Ok(()) => workspace.published = Some(signature),
                            Err(error) => {
                                self.last_error = Some(error);
                                self.dirty.store(true, Ordering::Relaxed);
                            }
                        }
                    }
                }
            }
            Completion::Discovered {
                workspace,
                identity,
                result,
            } => {
                let Some(current) = self
                    .workspaces
                    .get_mut(&workspace)
                    .filter(|w| w.identity == identity)
                else {
                    return Ok(());
                };
                current.discovering = false;
                current.discover_at = now + NEGATIVE_INTERVAL;
                match result {
                    Ok(url) => {
                        if url.is_some() {
                            current.discover_at = now + 1800;
                        }
                        current.error = None;
                        current.url.clone_from(&url);
                        self.database.execute("INSERT INTO bindings VALUES(?1,?2,?3,?4,?5) ON CONFLICT(server,workspace) DO UPDATE SET checkout=excluded.checkout, identity=excluded.identity, url=excluded.url", params![self.server_id, workspace.as_str(), current.checkout.to_string_lossy(), identity, url])?;
                        if let Some(url) = url {
                            self.load_pull(&url, now)?;
                        }
                    }
                    Err(error) => {
                        if rate_limited(&error) {
                            self.network_at = now + 1800;
                        }
                        current.error = Some(error.clone());
                        self.last_error = Some(error);
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
                        self.database.execute("INSERT INTO snapshots VALUES(?1,?2) ON CONFLICT(url) DO UPDATE SET body=excluded.body", params![url, serde_json::to_string(&cache)?])?;
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
            let sender = self.sender.clone();
            self.workers.push(thread::spawn(move || {
                let result = client
                    .snapshot()
                    .map(|snapshot| {
                        snapshot
                            .workspaces
                            .iter()
                            .filter_map(|workspace| {
                                snapshot
                                    .effective_workspace_dir(&workspace.workspace_id)
                                    .map(|directory| {
                                        (
                                            workspace.workspace_id.clone(),
                                            directory.to_path_buf(),
                                            github::checkout_identity(directory)
                                                .map_err(|e| format!("{e:#}")),
                                        )
                                    })
                            })
                            .collect()
                    })
                    .map_err(|e| e.to_string());
                let _ = sender.send(Completion::Inventory(result));
            }));
        }
        for workspace in std::mem::take(&mut self.manual) {
            if let Some(current) = self.workspaces.get_mut(&workspace) {
                if let Some(pull) = current.url.as_ref().and_then(|url| self.pulls.get_mut(url)) {
                    if !pull.fetching {
                        pull.due = now;
                    }
                } else {
                    current.discover_at = now;
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
        // At most two network workers, at least ten seconds between launches.
        // A sidebar job makes at most two gh calls; manual requests share this budget.
        if self.network_active < 2 && now >= self.network_at {
            let discovery_due = self
                .workspaces
                .values()
                .filter(|w| !w.discovering)
                .map(|w| w.discover_at)
                .min()
                .unwrap_or(u64::MAX);
            if let Some((url, pull)) = self
                .pulls
                .iter_mut()
                .filter(|(url, p)| {
                    !p.fetching
                        && p.due <= now
                        && p.due <= discovery_due
                        && self
                            .workspaces
                            .values()
                            .any(|w| w.url.as_ref() == Some(*url))
                })
                .min_by_key(|(_, p)| p.due)
            {
                if let Some(checkout) = self
                    .workspaces
                    .values()
                    .find(|w| w.url.as_ref() == Some(url))
                    .map(|w| w.checkout.clone())
                {
                    pull.fetching = true;
                    self.network_active += 1;
                    let url = url.clone();
                    let sender = self.sender.clone();
                    self.workers.push(thread::spawn(move || {
                        let result = github::sidebar(&checkout, &url)
                            .map(|state| Cached::from_snapshot(&state, unix_now()))
                            .map_err(|e| format!("{e:#}"));
                        let _ = sender.send(Completion::Fetched { url, result });
                    }));
                    self.network_at = now + NETWORK_SPACING;
                }
            } else if let Some((id, current)) = self
                .workspaces
                .iter_mut()
                .filter(|(_, w)| !w.discovering && w.discover_at <= now)
                .min_by_key(|(_, w)| w.discover_at)
            {
                current.discovering = true;
                self.network_active += 1;
                let (workspace, identity, checkout) = (
                    id.clone(),
                    current.identity.clone(),
                    current.checkout.clone(),
                );
                let sender = self.sender.clone();
                self.workers.push(thread::spawn(move || {
                    let result = github::discover(&checkout)
                        .and_then(|url| {
                            anyhow::ensure!(
                                github::checkout_identity(&checkout)? == identity,
                                "checkout changed during PR discovery"
                            );
                            Ok(url)
                        })
                        .map_err(|e| format!("{e:#}"));
                    let _ = sender.send(Completion::Discovered {
                        workspace,
                        identity,
                        result,
                    });
                }));
                self.network_at = now + NETWORK_SPACING;
            }
        }
    }

    fn publish(&mut self, now: u64) -> Result<()> {
        if self.publishing {
            return Ok(());
        }
        let mut batch = Vec::new();
        for (id, workspace) in &mut self.workspaces {
            let mut tokens = clear()?;
            if let Some(pull) = workspace.url.as_ref().and_then(|url| self.pulls.get(url)) {
                if let Some(cache) = &pull.cached {
                    for (key, value) in &cache.values {
                        tokens = match value {
                            Some(value) => tokens.set(key, value)?,
                            None => tokens.clear(key)?,
                        };
                    }
                    let checked =
                        time::OffsetDateTime::from_unix_timestamp(i64::try_from(cache.fetched)?)?;
                    tokens = tokens.set(
                        "gh_updated",
                        format!(
                            "↻ {:02}-{:02} {:02}:{:02}Z",
                            u8::from(checked.month()),
                            checked.day(),
                            checked.hour(),
                            checked.minute()
                        ),
                    )?;
                    if now.saturating_sub(cache.fetched) > cache.interval * 2 {
                        tokens = tokens.set("gh_health", "! Delayed")?;
                    }
                }
                if pull.error.is_some() {
                    tokens = tokens.set("gh_health", "! Retry")?;
                }
            }
            if workspace.error.is_some() {
                tokens = tokens.set("gh_health", "! Lookup")?;
            }
            let encoded = serde_json::to_string(&tokens)?;
            if workspace.published.as_ref() == Some(&encoded) {
                continue;
            }
            batch.push((
                id.clone(),
                workspace.checkout.clone(),
                workspace.identity.clone(),
                encoded,
                tokens,
            ));
        }
        if batch.is_empty() {
            return Ok(());
        }
        self.publishing = true;
        let client = self.client.clone();
        let server_id = self.server_id.clone();
        let sender = self.sender.clone();
        self.workers.push(thread::spawn(move || {
            let reporter = MetadataReporter::retained(&client, SOURCE);
            let results = batch.into_iter().map(|(id, checkout, identity, signature, tokens)| {
                    let result = (|| -> Result<()> {
                        anyhow::ensure!(client.server_id()? == server_id, "Herdr server was replaced before publication");
                        let snapshot = client.snapshot()?;
                        anyhow::ensure!(snapshot.effective_workspace_dir(&id) == Some(checkout.as_path()), "workspace checkout changed before publication");
                    match github::checkout_identity(&checkout) {
                        Ok(actual) if actual == identity => reporter.report_workspace(&id, &tokens)?,
                        other => {
                            reporter.report_workspace(&id, &clear()?)?;
                            anyhow::bail!("checkout changed or became unavailable before publication: {other:?}");
                        }
                    }
                    Ok(())
                })().map_err(|error| format!("{error:#}"));
                (id, identity, signature, result)
            }).collect();
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
                let in_flight = self
                    .workspaces
                    .get(&workspace)
                    .and_then(|current| current.url.as_ref())
                    .and_then(|url| self.pulls.get(url))
                    .is_some_and(|pull| pull.fetching);
                if !in_flight {
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
fn clear() -> Result<Tokens> {
    let mut tokens = Tokens::new();
    for key in [
        status::PR_TOKEN,
        status::STATE_TOKEN,
        status::REVIEW_TOKEN,
        status::CHECKS_TOKEN,
        status::FAILED_TOKEN,
        status::PENDING_TOKEN,
        "gh_health",
        "gh_updated",
    ] {
        tokens = tokens.clear(key)?;
    }
    Ok(tokens)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scheduler() -> Scheduler {
        let database = Connection::open_in_memory().unwrap();
        database.execute_batch(SCHEMA).unwrap();
        let (sender, receiver) = mpsc::channel();
        Scheduler {
            client: Client::new("/unused"),
            server_id: "test-server".into(),
            database,
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

    fn workspace(identity: &str) -> Workspace {
        Workspace {
            checkout: "/checkout".into(),
            identity: identity.into(),
            url: None,
            discover_at: 0,
            discovering: false,
            published: None,
            error: None,
        }
    }

    #[test]
    fn subscriptions_share_one_cache_and_old_branch_results_cannot_rebind() {
        let mut scheduler = scheduler();
        let url = "https://github.com/example/project/pull/7";
        for id in ["w1", "w2"] {
            scheduler
                .workspaces
                .insert(WorkspaceId::new(id), workspace("main"));
            scheduler
                .complete(
                    Completion::Discovered {
                        workspace: WorkspaceId::new(id),
                        identity: "main".into(),
                        result: Ok(Some(url.into())),
                    },
                    100,
                )
                .unwrap();
        }
        assert_eq!(scheduler.pulls.len(), 1);
        scheduler
            .workspaces
            .insert(WorkspaceId::new("w1"), workspace("other"));
        scheduler
            .complete(
                Completion::Discovered {
                    workspace: WorkspaceId::new("w1"),
                    identity: "main".into(),
                    result: Ok(Some(url.into())),
                },
                110,
            )
            .unwrap();
        assert!(
            scheduler
                .workspaces
                .get(&WorkspaceId::new("w1"))
                .unwrap()
                .url
                .is_none()
        );
    }

    #[test]
    fn failed_fetch_retains_cache_and_rate_limit_blocks_all_launches() {
        let mut scheduler = scheduler();
        let url = "https://github.com/example/project/pull/7";
        scheduler.load_pull(url, 100).unwrap();
        let cached = Cached {
            values: BTreeMap::from([("gh_pr".into(), Some("#7".into()))]),
            fetched: 100,
            interval: 45,
        };
        scheduler
            .complete(
                Completion::Fetched {
                    url: url.into(),
                    result: Ok(cached),
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
        let pull = scheduler.pulls.get(url).unwrap();
        assert_eq!(pull.cached.as_ref().unwrap().fetched, 100);
        assert!(pull.due > 150 && scheduler.network_at > 150);
        scheduler
            .workspaces
            .insert(WorkspaceId::new("w1"), workspace("main"));
        scheduler.schedule_network(151);
        assert_eq!(scheduler.network_active, 0);
        assert!(scheduler.workers.is_empty());
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
    fn removed_subscribers_do_not_release_an_inflight_network_slot() {
        let mut scheduler = scheduler();
        scheduler.network_active = 2;
        scheduler
            .workspaces
            .insert(WorkspaceId::new("w1"), workspace("main"));
        scheduler.schedule_network(100);
        assert_eq!(scheduler.network_active, 2);
        assert!(scheduler.workers.is_empty());
        scheduler
            .complete(
                Completion::Discovered {
                    workspace: WorkspaceId::new("gone"),
                    identity: "main".into(),
                    result: Ok(None),
                },
                110,
            )
            .unwrap();
        assert_eq!(scheduler.network_active, 1);
    }

    #[test]
    fn manual_refresh_joins_fetch_even_if_it_completes_before_next_tick() {
        let mut scheduler = scheduler();
        let url = "https://github.com/example/project/pull/7";
        scheduler.load_pull(url, 100).unwrap();
        scheduler.pulls.get_mut(url).unwrap().fetching = true;
        let mut current = workspace("main");
        current.url = Some(url.into());
        scheduler.workspaces.insert(WorkspaceId::new("w1"), current);
        scheduler
            .request(json!({"kind":"refresh", "workspace":"w1"}))
            .unwrap();
        assert!(scheduler.manual.is_empty());
        scheduler
            .complete(
                Completion::Fetched {
                    url: url.into(),
                    result: Ok(Cached {
                        values: BTreeMap::new(),
                        fetched: 110,
                        interval: 45,
                    }),
                },
                110,
            )
            .unwrap();
        assert_eq!(scheduler.pulls.get(url).unwrap().due, 155);
    }

    #[test]
    fn backoff_is_bounded_and_clear_removes_all_projection_fields() {
        assert!(retry_delay(2) > retry_delay(1));
        assert_eq!(retry_delay(100), 1800);
        let fields = serde_json::to_value(clear().unwrap()).unwrap();
        assert_eq!(fields.as_object().unwrap().len(), 8);
        assert!(fields.as_object().unwrap().values().all(Value::is_null));
    }
}
