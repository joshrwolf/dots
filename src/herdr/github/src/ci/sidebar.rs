//! One server-scoped scheduler for registered PR status. Network work is bounded
//! and off the service control loop; all cache changes/publication have one
//! owner. Each workspace's summary is a `$prs` metadata token for Herdr's Space
//! rows. This service is read-only with respect to GitHub and agents.

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
use herdrkit::service::{Handler, Service};
use herdrkit::{Client, Invocation, MetadataReporter, Tokens, WorkspaceId};
use rusqlite::{Connection, OptionalExtension as _, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::status;
use crate::github::{self, CheckBucket, PullRequestState, ReviewStatus, SidebarSnapshot};

const SOURCE: &str = "herdr-github";
/// Workspace metadata token rendered by `$prs` in `[ui.sidebar.spaces]` rows.
pub const TOKEN: &str = "prs";
const NETWORK_SPACING: u64 = 10;
const INVENTORY_INTERVAL: u64 = 15;
/// Shown pull requests per workspace; the rest are counted.
const TOKEN_ENTRIES: usize = 4;

/// Tokens fade out if the service stops renewing them.
const TOKEN_TTL: Duration = Duration::from_secs(120);
const TOKEN_RENEWAL: u64 = 30;
const SCHEMA: &str = "PRAGMA journal_mode=WAL; DROP TABLE IF EXISTS resource_snapshots; CREATE TABLE IF NOT EXISTS pull_snapshots(url TEXT PRIMARY KEY, body TEXT NOT NULL);";
const DATABASE: &str = "github.sqlite3";

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

/// Compact review and CI state of one pull request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Summary {
    /// Review glyph then CI glyph while open; one state glyph once completed.
    glyphs: String,
    /// The same state in words, for the picker.
    text: String,
}

impl Summary {
    fn from_snapshot(state: &SidebarSnapshot) -> Self {
        let preview = status::Preview::from_snapshot(state);
        let pull = &state.pull_request;
        match pull.state {
            PullRequestState::Merged => {
                return Self {
                    glyphs: "◆".into(),
                    text: preview.state.into(),
                };
            }
            PullRequestState::Closed => {
                return Self {
                    glyphs: "×".into(),
                    text: preview.state.into(),
                };
            }
            PullRequestState::Open => {}
        }
        let review = match pull.review {
            ReviewStatus::Approved => '✓',
            ReviewStatus::ChangesRequested => '!',
            _ if pull.draft => '○',
            ReviewStatus::Needed | ReviewStatus::Requested => '◷',
            ReviewStatus::NotReviewed => '○',
            ReviewStatus::Unknown => '?',
        };
        let checks = |bucket| state.checks.iter().any(|check| check.bucket == bucket);
        let ci = if checks(CheckBucket::Fail) {
            '✗'
        } else if checks(CheckBucket::Pending) {
            '◷'
        } else if checks(CheckBucket::Cancel) || checks(CheckBucket::Unknown) {
            '?'
        } else if preview.checks == Some("✓ CI") {
            '✓'
        } else {
            '—'
        };
        let mut parts = vec![preview.review.unwrap_or(preview.state).to_owned()];
        if let Some(failed) = &preview.failed {
            parts.push(format!("{failed} failed"));
        }
        if let Some(pending) = &preview.pending {
            parts.push(format!("{pending} checks"));
        }
        if let Some(cancelled) = &preview.cancelled {
            parts.push(format!("{cancelled} cancelled"));
        }
        if let Some(unknown) = &preview.unknown {
            parts.push(format!("{unknown} unknown"));
        }
        if let Some(checks) = preview.checks {
            parts.push(checks.to_owned());
        }
        Self {
            glyphs: format!("{review}{ci}"),
            text: parts.join(" · "),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Cached {
    summary: Summary,
    fetched: u64,
    interval: u64,
}

impl Cached {
    fn from_snapshot(state: &SidebarSnapshot, now: u64) -> Self {
        Self {
            summary: Summary::from_snapshot(state),
            fetched: now,
            interval: cadence(state),
        }
    }

    fn stale(&self, now: u64) -> bool {
        now.saturating_sub(self.fetched) > self.interval * 2
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

/// Last observed status of each URL, as the PR picker describes it.
pub(crate) fn describe(state_dir: &Path, urls: &[&str]) -> BTreeMap<String, String> {
    let now = unix_now();
    let Ok(database) = Connection::open_with_flags(
        state_dir.join(DATABASE),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    ) else {
        return BTreeMap::new();
    };
    urls.iter()
        .filter_map(|url| {
            let body: String = database
                .query_row(
                    "SELECT body FROM pull_snapshots WHERE url=?1",
                    [url],
                    |row| row.get(0),
                )
                .ok()?;
            let cache: Cached = serde_json::from_str(&body).ok()?;
            let mut text = cache.summary.text.clone();
            if cache.stale(now) {
                let _ = write!(text, " · ◷ {}s old", now.saturating_sub(cache.fetched));
            }
            Some(((*url).to_owned(), text))
        })
        .collect()
}

#[derive(Debug)]
struct Workspace {
    checkout: PathBuf,
    associations: Associations,
    published: Option<(Option<String>, u64)>,
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

type InventoryRow = (WorkspaceId, PathBuf, Associations);
type PublicationResult = (WorkspaceId, Option<String>, Result<(), String>);

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
        let database = Connection::open(directory.join(DATABASE))?;
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
                "SELECT body FROM pull_snapshots WHERE url=?1",
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
        let present: BTreeSet<_> = rows.iter().map(|(id, _, _)| id.clone()).collect();
        self.workspaces.retain(|id, _| present.contains(id));
        self.manual.retain(|id| present.contains(id));
        for (id, checkout, associations) in rows {
            for association in &associations.pulls {
                self.load_pull(association.url.as_str(), now)?;
            }
            // Revisions are monotonic per workspace ID, including across reuse.
            if let Some(current) = self
                .workspaces
                .get_mut(&id)
                .filter(|current| current.associations.revision == associations.revision)
            {
                current.checkout = checkout;
            } else {
                let published = self
                    .workspaces
                    .remove(&id)
                    .and_then(|previous| previous.published);
                self.workspaces.insert(
                    id,
                    Workspace {
                        checkout,
                        associations,
                        published,
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
                for (id, value, result) in results {
                    let Some(workspace) = self.workspaces.get_mut(&id) else {
                        continue;
                    };
                    match result {
                        // Cleared tokens need no renewal.
                        Ok(()) => {
                            let renew_at = if value.is_some() {
                                now + TOKEN_RENEWAL
                            } else {
                                u64::MAX
                            };
                            workspace.published = Some((value, renew_at));
                        }
                        Err(error) => {
                            self.last_error = Some(error);
                            self.dirty.store(true, Ordering::Relaxed);
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
                        self.database.execute("INSERT INTO pull_snapshots VALUES(?1,?2) ON CONFLICT(url) DO UPDATE SET body=excluded.body", params![url, serde_json::to_string(&cache)?])?;
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
                let result =
                    inventory(&client, &registry_path).map_err(|error| format!("{error:#}"));
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
            self.publish(now);
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

    fn publish(&mut self, now: u64) {
        if self.publishing {
            return;
        }
        let mut batch = Vec::new();
        for (id, workspace) in &self.workspaces {
            let urls: Vec<_> = workspace
                .associations
                .pulls
                .iter()
                .map(|association| association.url.as_str())
                .collect();
            let value = token(&urls, &self.pulls, now);
            if workspace
                .published
                .as_ref()
                .is_some_and(|(published, renew_at)| *published == value && now < *renew_at)
            {
                continue;
            }
            batch.push((id.clone(), workspace.associations.revision, value));
        }
        if batch.is_empty() {
            return;
        }
        self.publishing = true;
        let client = self.client.clone();
        let server_id = self.server_id.clone();
        let sender = self.sender.clone();
        let registry_path = self.registry_path.clone();
        self.workers.push(thread::spawn(move || {
            // Milliseconds keep the sequence monotonic across service restarts.
            let sequence = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |duration| {
                    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
                });
            let results = batch
                .into_iter()
                .map(|(id, revision, value)| {
                    let result = (|| -> Result<()> {
                        anyhow::ensure!(
                            client.server_id()? == server_id,
                            "Herdr server replaced before publication"
                        );
                        let scope = Scope::at(client.endpoint(), id.as_str())?;
                        let tokens = match &value {
                            Some(value) => Tokens::new().set(TOKEN, value.as_str())?,
                            None => Tokens::new().clear(TOKEN)?,
                        };
                        let reporter = MetadataReporter::new(&client, SOURCE, TOKEN_TTL)?;
                        Registry::open(&registry_path)?
                            .while_current(&scope, revision, || {
                                reporter.report_workspace_sequenced(&id, &tokens, sequence)
                            })?
                            .ok_or_else(|| {
                                anyhow::anyhow!("PR associations changed before publication")
                            })??;
                        Ok(())
                    })()
                    .map_err(|error| format!("{error:#}"));
                    (id, value, result)
                })
                .collect();
            let _ = sender.send(Completion::Published(results));
        }));
    }
}

/// Discards associations of closed workspaces, then lists the open ones.
fn inventory(client: &Client, registry_path: &Path) -> Result<Vec<InventoryRow>> {
    let endpoint = client
        .endpoint()
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("non-Unicode endpoint"))?;
    let mut registry = Registry::open(registry_path)?;
    let mut snapshot = None;
    registry.retain(endpoint, || {
        let current = client.snapshot()?;
        let live = current
            .workspaces
            .iter()
            .map(|workspace| workspace.workspace_id.as_str().to_owned())
            .collect();
        snapshot = Some(current);
        Ok::<_, anyhow::Error>(live)
    })?;
    let snapshot = snapshot.ok_or_else(|| anyhow::anyhow!("inventory snapshot missing"))?;
    snapshot
        .workspaces
        .iter()
        .map(|workspace| {
            let scope = Scope::at(client.endpoint(), workspace.workspace_id.as_str())?;
            // gh can inspect an explicit URL without a git checkout.
            let directory = snapshot
                .effective_workspace_dir(&workspace.workspace_id)
                .unwrap_or(Path::new("/"));
            Ok((
                workspace.workspace_id.clone(),
                directory.to_path_buf(),
                registry.list(&scope)?,
            ))
        })
        .collect()
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

/// One workspace's token value, or `None` to clear it when no PR is registered.
///
/// Each PR renders as `#number` plus its glyphs; `…` marks a missing or delayed
/// observation, which keeps its last known state rather than claiming none.
fn token(urls: &[&str], pulls: &BTreeMap<String, PullRequest>, now: u64) -> Option<String> {
    if urls.is_empty() {
        return None;
    }
    let mut entries: Vec<_> = urls
        .iter()
        .take(TOKEN_ENTRIES)
        .map(|url| {
            let number = url.rsplit('/').next().unwrap_or("?");
            let pull = pulls.get(*url);
            match pull.and_then(|pull| pull.cached.as_ref()) {
                None => format!("#{number} …"),
                Some(cache) => {
                    let delayed = pull.is_some_and(|pull| pull.error.is_some()) || cache.stale(now);
                    let marker = if delayed { "…" } else { "" };
                    format!("#{number} {}{marker}", cache.summary.glyphs)
                }
            }
        })
        .collect();
    if urls.len() > TOKEN_ENTRIES {
        entries.push(format!("+{}", urls.len() - TOKEN_ENTRIES));
    }
    Some(entries.join(" · "))
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
    fn url(number: u64) -> String {
        format!("https://github.com/example/project/pull/{number}")
    }
    fn associations(numbers: &[u64], revision: i64) -> Associations {
        Associations {
            revision,
            pulls: numbers
                .iter()
                .map(|number| Association {
                    url: PullRequestUrl::parse(&url(*number)).unwrap(),
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
            associations(numbers, revision),
        )
    }
    fn snapshot(state: PullRequestState, buckets: Vec<CheckBucket>) -> SidebarSnapshot {
        SidebarSnapshot {
            pull_request: github::PullRequest {
                number: 42,
                draft: false,
                state,
                review: ReviewStatus::Approved,
                head_oid: "abc".into(),
            },
            checks: buckets
                .into_iter()
                .map(|bucket| github::Check {
                    bucket,
                    name: String::new(),
                    link: String::new(),
                    workflow: String::new(),
                })
                .collect(),
        }
    }
    fn cached(glyphs: &str, fetched: u64) -> Cached {
        Cached {
            summary: Summary {
                glyphs: glyphs.into(),
                text: String::new(),
            },
            fetched,
            interval: 45,
        }
    }
    fn workspace_token(scheduler: &Scheduler, id: &str, now: u64) -> Option<String> {
        let urls: Vec<_> = scheduler
            .workspaces
            .get(&WorkspaceId::new(id))
            .unwrap()
            .associations
            .pulls
            .iter()
            .map(|pull| pull.url.as_str())
            .collect();
        token(&urls, &scheduler.pulls, now)
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
        let url = url(2);
        scheduler.pulls.get_mut(&url).unwrap().fetching = true;
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
                    url: url.clone(),
                    result: Ok(cached("✓✓", 100)),
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
        // A reused ID arrives with a newer revision and no inherited associations.
        scheduler
            .reconcile_inventory(Ok(vec![inventory("w1", &[], 4)]), 130)
            .unwrap();
        assert!(scheduler.pulls.is_empty());
        assert_eq!(workspace_token(&scheduler, "w1", 130), None);
    }

    #[test]
    fn outage_keeps_last_observation_and_blocks_rate_limited_launches() {
        let mut scheduler = scheduler();
        let url = url(1);
        scheduler
            .reconcile_inventory(Ok(vec![inventory("w1", &[1], 1)]), 100)
            .unwrap();
        assert_eq!(workspace_token(&scheduler, "w1", 100).unwrap(), "#1 …");
        scheduler
            .complete(
                Completion::Fetched {
                    url: url.clone(),
                    result: Ok(cached("✓✓", 100)),
                },
                100,
            )
            .unwrap();
        assert_eq!(workspace_token(&scheduler, "w1", 100).unwrap(), "#1 ✓✓");
        scheduler
            .complete(
                Completion::Fetched {
                    url: url.clone(),
                    result: Err("HTTP 429 rate limit".into()),
                },
                150,
            )
            .unwrap();
        assert_eq!(workspace_token(&scheduler, "w1", 150).unwrap(), "#1 ✓✓…");
        scheduler.schedule_network(151);
        assert!(scheduler.workers.is_empty());
        scheduler.pulls.clear();
        scheduler.load_pull(&url, 160).unwrap();
        assert_eq!(
            scheduler
                .pulls
                .get(&url)
                .unwrap()
                .cached
                .as_ref()
                .unwrap()
                .fetched,
            100
        );
        assert_eq!(workspace_token(&scheduler, "w1", 160).unwrap(), "#1 ✓✓");
        assert_eq!(workspace_token(&scheduler, "w1", 300).unwrap(), "#1 ✓✓…");
    }

    #[test]
    fn tokens_are_bounded_and_cleared_without_registrations() {
        let mut scheduler = scheduler();
        scheduler
            .reconcile_inventory(
                Ok(vec![
                    inventory("w1", &[1, 2, 3, 4, 5, 6], 1),
                    inventory("w2", &[], 0),
                ]),
                100,
            )
            .unwrap();
        assert_eq!(
            workspace_token(&scheduler, "w1", 100).unwrap(),
            "#1 … · #2 … · #3 … · #4 … · +2"
        );
        assert_eq!(workspace_token(&scheduler, "w2", 100), None);
    }

    #[test]
    fn terminal_prs_are_still_polled_and_reopening_restores_open_state() {
        let mut state = snapshot(PullRequestState::Merged, Vec::new());
        let merged = Cached::from_snapshot(&state, 100);
        assert_eq!(merged.summary.glyphs, "◆");
        assert_eq!(merged.summary.text, "◆ Merged");
        assert_eq!(merged.interval, 1800);
        state.pull_request.state = PullRequestState::Closed;
        assert_eq!(Cached::from_snapshot(&state, 100).summary.glyphs, "×");
        state.pull_request.state = PullRequestState::Open;
        let reopened = Cached::from_snapshot(&state, 200);
        assert_eq!(reopened.summary.glyphs, "✓—");
        assert_eq!(reopened.summary.text, "✓ Approved · — No checks");
        state.checks = snapshot(PullRequestState::Open, vec![CheckBucket::Pending; 2]).checks;
        let pending = Cached::from_snapshot(&state, 300);
        assert_eq!(pending.summary.glyphs, "✓◷");
        assert_eq!(pending.summary.text, "✓ Approved · ◷ 2 checks");
        assert_eq!(pending.interval, 45);
    }

    #[test]
    fn review_and_check_glyphs_distinguish_every_state() {
        use CheckBucket::{Cancel, Fail, Pass, Pending, Skipping, Unknown};
        for (buckets, glyphs, text) in [
            (
                vec![Cancel, Cancel, Pending, Pending],
                "✓◷",
                "✓ Approved · ◷ 2 checks · 2 cancelled",
            ),
            (vec![Cancel], "✓?", "✓ Approved · 1 cancelled"),
            (vec![Pass, Unknown], "✓?", "✓ Approved · ? 1 unknown"),
            (
                vec![Fail, Pending, Cancel, Unknown],
                "✓✗",
                "✓ Approved · ✗ 1 failed · ◷ 1 checks · 1 cancelled · ? 1 unknown",
            ),
            (vec![Pass, Skipping], "✓✓", "✓ Approved · ✓ CI"),
            (vec![Skipping], "✓—", "✓ Approved · — Skipped"),
        ] {
            let summary = Summary::from_snapshot(&snapshot(PullRequestState::Open, buckets));
            assert_eq!(
                (summary.glyphs.as_str(), summary.text.as_str()),
                (glyphs, text)
            );
        }
        let mut state = snapshot(PullRequestState::Open, vec![Pass]);
        for (review, draft, glyphs) in [
            (ReviewStatus::ChangesRequested, true, "!✓"),
            (ReviewStatus::Needed, true, "○✓"),
            (ReviewStatus::Requested, false, "◷✓"),
            (ReviewStatus::NotReviewed, false, "○✓"),
            (ReviewStatus::Unknown, false, "?✓"),
        ] {
            state.pull_request.review = review;
            state.pull_request.draft = draft;
            assert_eq!(Summary::from_snapshot(&state).glyphs, glyphs);
        }
    }
}
