//! Shared, claimable work items for one project, safe across concurrent sessions.
//!
//! The store records intent and coordination: what remains, who holds a slice,
//! and which PR it produced. Git and GitHub remain the truth about code. Callers
//! resolve the project identity and storage path; readiness is derived from
//! status, dependencies and leases rather than stored.
use std::path::Path;
use std::time::{Duration, Instant};

use rusqlite::{Connection, OptionalExtension as _, Transaction, params};
use serde::Serialize;

mod goals;
pub use goals::{Goal, GoalOverview, GoalProgress};

/// Path relative to the caller's platform state directory.
pub const STORE_PATH: &str = "dots/tasks/tasks.sqlite3";

const ID_ATTEMPTS: usize = 16;

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    #[error("open or update task storage")]
    Storage(#[from] rusqlite::Error),
    #[error("unsupported task store schema {0}")]
    Schema(i64),
    #[error("invalid {0}")]
    Invalid(&'static str),
    #[error("task {0} does not exist in this project")]
    NotFound(String),
    #[error("task {id} is leased to {owner} for another {remaining_secs}s")]
    Claimed {
        id: String,
        owner: String,
        lease_until: i64,
        remaining_secs: i64,
    },
    #[error("task {id} is already {status}")]
    Unchanged { id: String, status: &'static str },
    #[error("task {id} has an unreadable {field}")]
    Corrupt { id: String, field: &'static str },
    #[error("could not allocate a fresh task id")]
    IdSpaceExhausted,
    #[error("task {id} is not ready ({reason})")]
    NotReady { id: String, reason: String },
    #[error("dependency would make {id} wait on itself through {through}")]
    Cycle { id: String, through: String },
    #[error("task {id} is leased to {owner}")]
    OwnedByAnother { id: String, owner: String },
    #[error("task {id} cannot be closed yet: {reason}; pass --force to close it anyway")]
    Incomplete { id: String, reason: String },
    #[error("task {id} does not wait on {blocked_by}")]
    NoDependency { id: String, blocked_by: String },
    #[error("task {id} and {other} share a parent chain; a parent already waits for its children")]
    Hierarchy { id: String, other: String },
    #[error("task {id} would become its own ancestor through {through}")]
    AncestorCycle { id: String, through: String },
    #[error("task {id} already waits on {blocked_by}")]
    DuplicateDependency { id: String, blocked_by: String },
}

/// Opaque project key; the application derives it from the main checkout.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct Project(String);

impl Project {
    pub fn new(key: impl Into<String>) -> Result<Self, Error> {
        let key = key.into();
        if key.is_empty() || key.len() > 4096 || key.chars().any(char::is_control) {
            return Err(Error::Invalid("project key"));
        }
        Ok(Self(key))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Validated session or agent name; trimmed once so claims and later edits agree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct Owner<'a>(&'a str);

impl<'a> Owner<'a> {
    pub fn new(name: &'a str) -> Result<Self, Error> {
        let name = name.trim();
        if name.is_empty() || name.len() > 128 || name.chars().any(char::is_control) {
            return Err(Error::Invalid("owner"));
        }
        Ok(Self(name))
    }

    pub fn as_str(self) -> &'a str {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Open,
    Held,
    Done,
    Dropped,
}

impl Status {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Held => "held",
            Self::Done => "done",
            Self::Dropped => "dropped",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "open" => Some(Self::Open),
            "held" => Some(Self::Held),
            "done" => Some(Self::Done),
            "dropped" => Some(Self::Dropped),
            _ => None,
        }
    }

    fn is_closed(self) -> bool {
        matches!(self, Self::Done | Self::Dropped)
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Task {
    pub id: String,
    pub parent: Option<String>,
    pub title: String,
    pub body: Option<String>,
    pub status: Status,
    pub priority: i64,
    pub pr: Option<String>,
    pub branch: Option<String>,
    pub owner: Option<String>,
    pub lease_until: Option<i64>,
    pub created_at: i64,
    pub updated_at: i64,
    pub closed_at: Option<i64>,
    /// Open or held prerequisites. Closed prerequisites no longer block.
    pub blocked_by: Vec<String>,
    /// Every declared prerequisite with its current status, for provenance.
    pub prerequisites: Vec<Prerequisite>,
    pub children: u64,
    /// Children still open or held; a grouping task is ready only once they close.
    pub unclosed_children: u64,
    /// Most recent history entry, so lists carry the last reason or note.
    pub last_event: Option<Event>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Prerequisite {
    pub id: String,
    pub title: String,
    pub status: Status,
    pub parent: Option<String>,
    pub owner: Option<String>,
}

fn plural(count: u64, one: &str, many: &str) -> String {
    if count == 1 {
        format!("1 {one}")
    } else {
        format!("{count} {many}")
    }
}

impl Task {
    /// Why this task cannot be worked on regardless of who asks, if anything.
    pub fn not_workable(&self) -> Option<String> {
        if self.status != Status::Open {
            return Some(format!("status is {}", self.status.as_str()));
        }
        if !self.blocked_by.is_empty() {
            return Some(format!("blocked by {}", self.blocked_by.join(", ")));
        }
        if self.unclosed_children > 0 {
            return Some(plural(
                self.unclosed_children,
                "unclosed child",
                "unclosed children",
            ));
        }
        None
    }

    /// Live lease holder other than `owner`, if any.
    fn leased_to_other(&self, owner: Option<Owner<'_>>, now: i64) -> Option<(&str, i64)> {
        match (self.owner.as_deref(), self.lease_until) {
            (Some(current), Some(until))
                if until > now && owner.is_none_or(|owner| owner.as_str() != current) =>
            {
                Some((current, until))
            }
            _ => None,
        }
    }

    /// Why this task is not fresh work for `claim --next`, if anything. A slice
    /// with a PR recorded is in review, not unstarted; pick it up by explicit id.
    pub fn not_ready_because(&self, now: i64) -> Option<String> {
        if let Some(reason) = self.not_workable() {
            return Some(reason);
        }
        if let Some(pr) = &self.pr {
            return Some(format!("PR open: {pr}"));
        }
        self.leased_to_other(None, now)
            .map(|(owner, _)| format!("leased to {owner}"))
    }

    /// Claimable by `claim --next`: workable, no PR yet, and not under any live lease.
    pub fn ready(&self, now: i64) -> bool {
        self.not_ready_because(now).is_none()
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Event {
    pub seq: i64,
    pub task: String,
    pub at: i64,
    pub actor: Option<String>,
    pub kind: String,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct NewTask {
    pub title: String,
    pub body: Option<String>,
    pub parent: Option<String>,
    pub priority: i64,
    pub blocked_by: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct Update {
    pub title: Option<String>,
    pub body: Option<String>,
    /// Full pull/merge request URL.
    pub pr: Option<String>,
    pub branch: Option<String>,
    pub priority: Option<i64>,
    pub parent: Option<Option<String>>,
    pub note: Option<String>,
}

impl Update {
    /// Changes something other than appending a note.
    pub fn edits_fields(&self) -> bool {
        self.title.is_some()
            || self.body.is_some()
            || self.pr.is_some()
            || self.branch.is_some()
            || self.priority.is_some()
            || self.parent.is_some()
    }

    pub fn is_empty(&self) -> bool {
        !self.edits_fields() && self.note.is_none()
    }
}

#[derive(Debug, Clone, Default)]
pub struct Filter<'a> {
    /// Only tasks whose recorded owner matches, live lease or not.
    pub owner: Option<Owner<'a>>,
    /// Only tasks anywhere below this one.
    pub parent: Option<&'a str>,
    /// Only tasks without a parent; ignored when `parent` is set.
    pub roots: bool,
    pub status: Option<Status>,
    pub include_closed: bool,
}

/// Actor and time for one mutation; the caller supplies both explicitly.
#[derive(Debug, Clone, Copy)]
pub struct Actor<'a> {
    pub name: Option<Owner<'a>>,
    pub now: i64,
}

/// One connection per caller; SQLite serializes competing writers.
#[derive(Debug)]
pub struct Store {
    connection: Connection,
}

impl Store {
    pub fn open(path: &Path) -> Result<Self, Error> {
        Self::initialize(Connection::open(path)?)
    }

    fn initialize(mut connection: Connection) -> Result<Self, Error> {
        // Simultaneous first-open callers can bypass the busy handler while
        // promoting the journal to WAL; retry that promotion briefly.
        connection.busy_timeout(Duration::from_millis(50))?;
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            match connection.pragma_update(None, "journal_mode", "WAL") {
                Ok(()) => break,
                Err(rusqlite::Error::SqliteFailure(error, _))
                    if matches!(
                        error.code,
                        rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked
                    ) && Instant::now() < deadline =>
                {
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(error) => return Err(error.into()),
            }
        }
        connection.busy_timeout(Duration::from_secs(5))?;
        connection.pragma_update(None, "foreign_keys", "ON")?;
        let tx = connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let version: i64 = tx.pragma_query_value(None, "user_version", |row| row.get(0))?;
        match version {
            0 => tx.execute_batch(
                "CREATE TABLE tasks(
                    id TEXT PRIMARY KEY, project TEXT NOT NULL, parent TEXT REFERENCES tasks(id),
                    title TEXT NOT NULL, body TEXT, status TEXT NOT NULL, priority INTEGER NOT NULL DEFAULT 0,
                    pr TEXT, branch TEXT, owner TEXT, lease_until INTEGER,
                    created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL, closed_at INTEGER);
                 CREATE INDEX tasks_project ON tasks(project, status);
                 CREATE TABLE deps(task TEXT NOT NULL REFERENCES tasks(id), blocked_by TEXT NOT NULL REFERENCES tasks(id), PRIMARY KEY(task, blocked_by));
                 CREATE TABLE events(seq INTEGER PRIMARY KEY, task TEXT NOT NULL REFERENCES tasks(id), at INTEGER NOT NULL, actor TEXT, kind TEXT NOT NULL, detail TEXT);
                 PRAGMA user_version=2;",
            )?,
            1 => tx.execute_batch("ALTER TABLE tasks ADD COLUMN branch TEXT; PRAGMA user_version=2;")?,
            2 | 3 => {}
            other => return Err(Error::Schema(other)),
        }
        if version <= 2 {
            tx.execute_batch(
                "CREATE TABLE goals(root TEXT PRIMARY KEY REFERENCES tasks(id), brief TEXT NOT NULL);
                 PRAGMA user_version=3;",
            )?;
        }
        tx.commit()?;
        Ok(Self { connection })
    }

    fn write(&mut self) -> Result<Transaction<'_>, Error> {
        Ok(self
            .connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?)
    }

    pub fn add(
        &mut self,
        project: &Project,
        task: &NewTask,
        actor: Actor<'_>,
    ) -> Result<Task, Error> {
        let tx = self.write()?;
        let created = insert_task(&tx, project, task, actor)?;
        tx.commit()?;
        Ok(created)
    }

    pub fn get(&mut self, project: &Project, id: &str) -> Result<Task, Error> {
        let tx = self.connection.transaction()?;
        load(&tx, project, id)
    }

    pub fn list(&mut self, project: &Project, filter: &Filter<'_>) -> Result<Vec<Task>, Error> {
        let tx = self.connection.transaction()?;
        let ids = {
            let mut statement = tx.prepare(
                "WITH RECURSIVE below(id) AS (SELECT id FROM tasks WHERE parent=?2 UNION SELECT tasks.id FROM tasks JOIN below ON tasks.parent=below.id)
                 SELECT id FROM tasks WHERE project=?1 AND (?2 IS NULL OR id IN below) AND (NOT ?3 OR parent IS NULL)
                 ORDER BY priority DESC, created_at ASC, rowid ASC",
            )?;
            let roots = filter.roots && filter.parent.is_none();
            let rows = statement.query_map(params![project.0, filter.parent, roots], |row| {
                row.get::<_, String>(0)
            })?;
            rows.collect::<Result<Vec<_>, _>>()?
        };
        let mut tasks = Vec::new();
        for id in ids {
            let task = load(&tx, project, &id)?;
            let keep = match filter.status {
                Some(status) => task.status == status,
                None => filter.include_closed || !task.status.is_closed(),
            } && filter
                .owner
                .is_none_or(|owner| task.owner.as_deref() == Some(owner.as_str()));
            if keep {
                tasks.push(task);
            }
        }
        Ok(tasks)
    }

    /// Claimable leaf tasks below `parent`, in priority order. Expired leases count as free;
    /// grouping tasks are excluded because `claim_next` never hands them out.
    pub fn ready(
        &mut self,
        project: &Project,
        parent: Option<&str>,
        now: i64,
    ) -> Result<Vec<Task>, Error> {
        let open = self.list(
            project,
            &Filter {
                owner: None,
                parent,
                roots: false,
                status: Some(Status::Open),
                include_closed: false,
            },
        )?;
        Ok(open
            .into_iter()
            .filter(|task| task.children == 0 && task.ready(now))
            .collect())
    }

    /// Atomically take a task. Renews a lease the same owner already holds.
    pub fn claim(
        &mut self,
        project: &Project,
        id: &str,
        owner: Owner<'_>,
        lease: Duration,
        now: i64,
    ) -> Result<Task, Error> {
        let tx = self.write()?;
        let task = load(&tx, project, id)?;
        take(&tx, &task, owner, lease, now)?;
        let claimed = load(&tx, project, id)?;
        tx.commit()?;
        Ok(claimed)
    }

    /// Atomically take the highest-priority ready task, if any.
    pub fn claim_next(
        &mut self,
        project: &Project,
        parent: Option<&str>,
        owner: Owner<'_>,
        lease: Duration,
        now: i64,
    ) -> Result<Option<Task>, Error> {
        let tx = self.write()?;
        let ids = {
            let mut statement = tx.prepare(
                "WITH RECURSIVE below(id) AS (SELECT id FROM tasks WHERE parent=?2 UNION SELECT tasks.id FROM tasks JOIN below ON tasks.parent=below.id)
                 SELECT id FROM tasks WHERE project=?1 AND status='open' AND (?2 IS NULL OR id IN below)
                 ORDER BY priority DESC, created_at ASC, rowid ASC",
            )?;
            let rows =
                statement.query_map(params![project.0, parent], |row| row.get::<_, String>(0))?;
            rows.collect::<Result<Vec<_>, _>>()?
        };
        for id in ids {
            let task = load(&tx, project, &id)?;
            if task.children == 0 && task.ready(now) {
                take(&tx, &task, owner, lease, now)?;
                let claimed = load(&tx, project, &id)?;
                tx.commit()?;
                return Ok(Some(claimed));
            }
        }
        Ok(None)
    }

    pub fn release(
        &mut self,
        project: &Project,
        id: &str,
        owner: Owner<'_>,
        now: i64,
    ) -> Result<Task, Error> {
        let tx = self.write()?;
        let task = load(&tx, project, id)?;
        if let Some((current, _)) = task.leased_to_other(Some(owner), now) {
            return Err(Error::OwnedByAnother {
                id: id.into(),
                owner: current.into(),
            });
        }
        if task.owner.is_none() {
            return Err(Error::Unchanged {
                id: id.into(),
                status: "unclaimed",
            });
        }
        {
            tx.execute(
                "UPDATE tasks SET owner=NULL, lease_until=NULL, updated_at=?2 WHERE id=?1",
                params![id, now],
            )?;
            record(
                &tx,
                id,
                Actor {
                    name: Some(owner),
                    now,
                },
                "released",
                None,
            )?;
        }
        let released = load(&tx, project, id)?;
        tx.commit()?;
        Ok(released)
    }

    /// Edit fields or append a note. Field edits on a task under another
    /// owner's live lease need `force`; notes are commentary and always allowed.
    pub fn update(
        &mut self,
        project: &Project,
        id: &str,
        update: &Update,
        actor: Actor<'_>,
        force: bool,
    ) -> Result<Task, Error> {
        if update.is_empty() {
            return Err(Error::Invalid("update: no fields given"));
        }
        let tx = self.write()?;
        let before = load(&tx, project, id)?;
        if update.edits_fields()
            && !force
            && let Some((owner, _)) = before.leased_to_other(actor.name, actor.now)
        {
            return Err(Error::OwnedByAnother {
                id: id.into(),
                owner: owner.into(),
            });
        }
        let mut changes = Vec::new();
        if let Some(title) = &update.title {
            if title.trim().is_empty() || title.len() > 512 {
                return Err(Error::Invalid("title"));
            }
            tx.execute(
                "UPDATE tasks SET title=?2 WHERE id=?1",
                params![id, title.trim()],
            )?;
            changes.push("title".to_owned());
        }
        if let Some(body) = &update.body {
            tx.execute("UPDATE tasks SET body=?2 WHERE id=?1", params![id, body])?;
            changes.push("body".to_owned());
        }
        if let Some(branch) = &update.branch {
            tx.execute(
                "UPDATE tasks SET branch=?2 WHERE id=?1",
                params![id, branch],
            )?;
            changes.push(format!("branch {branch}"));
        }
        if let Some(priority) = update.priority {
            tx.execute(
                "UPDATE tasks SET priority=?2 WHERE id=?1",
                params![id, priority],
            )?;
            changes.push(format!("priority {} -> {priority}", before.priority));
        }
        if let Some(parent) = &update.parent {
            if parent.is_some() && goals::attached(&tx, id)? {
                return Err(Error::Invalid(
                    "goal must remain a root task; detach it before reparenting",
                ));
            }
            if let Some(parent) = parent {
                load(&tx, project, parent)?;
                if parent == id || descends_from(&tx, parent, id)? {
                    return Err(Error::AncestorCycle {
                        id: id.into(),
                        through: parent.clone(),
                    });
                }
            }
            tx.execute(
                "UPDATE tasks SET parent=?2 WHERE id=?1",
                params![id, parent],
            )?;
            validate_subtree_dependencies(&tx, id)?;
            changes.push(format!(
                "parent {} -> {}",
                before.parent.as_deref().unwrap_or("none"),
                parent.as_deref().unwrap_or("none")
            ));
        }
        if !changes.is_empty() {
            record(&tx, id, actor, "updated", Some(&changes.join(", ")))?;
        }
        if let Some(pr) = &update.pr {
            let pr = valid_pr_url(pr)?;
            tx.execute("UPDATE tasks SET pr=?2 WHERE id=?1", params![id, pr])?;
            record(&tx, id, actor, "pr", Some(pr))?;
        }
        if let Some(note) = &update.note {
            record(&tx, id, actor, "note", Some(note))?;
        }
        tx.execute(
            "UPDATE tasks SET updated_at=?2 WHERE id=?1",
            params![id, actor.now],
        )?;
        let updated = load(&tx, project, id)?;
        tx.commit()?;
        Ok(updated)
    }

    /// Close, hold, or reopen. Another owner's live lease requires `force`.
    pub fn set_status(
        &mut self,
        project: &Project,
        id: &str,
        status: Status,
        reason: Option<&str>,
        actor: Actor<'_>,
        force: bool,
    ) -> Result<Task, Error> {
        let tx = self.write()?;
        let task = load(&tx, project, id)?;
        if task.status == status {
            return Err(Error::Unchanged {
                id: id.into(),
                status: status.as_str(),
            });
        }
        if let Some((owner, _)) = task.leased_to_other(actor.name, actor.now)
            && !force
        {
            return Err(Error::OwnedByAnother {
                id: id.into(),
                owner: owner.into(),
            });
        }
        if status.is_closed() && !force {
            let reason = if task.blocked_by.is_empty() {
                (task.unclosed_children > 0).then(|| {
                    plural(
                        task.unclosed_children,
                        "unclosed child",
                        "unclosed children",
                    )
                })
            } else {
                Some(format!("blocked by {}", task.blocked_by.join(", ")))
            };
            if let Some(reason) = reason {
                return Err(Error::Incomplete {
                    id: id.into(),
                    reason,
                });
            }
        }
        let closed_at = status.is_closed().then_some(actor.now);
        tx.execute(
            "UPDATE tasks SET status=?2, closed_at=?3, owner=NULL, lease_until=NULL, updated_at=?4 WHERE id=?1",
            params![id, status.as_str(), closed_at, actor.now],
        )?;
        record(&tx, id, actor, status.as_str(), reason)?;
        let updated = load(&tx, project, id)?;
        tx.commit()?;
        Ok(updated)
    }

    pub fn add_dependency(
        &mut self,
        project: &Project,
        id: &str,
        blocked_by: &str,
        actor: Actor<'_>,
    ) -> Result<Task, Error> {
        if id == blocked_by {
            return Err(Error::Cycle {
                id: id.into(),
                through: blocked_by.into(),
            });
        }
        let tx = self.write()?;
        load(&tx, project, id)?;
        load(&tx, project, blocked_by)?;
        if reaches(&tx, blocked_by, id)? {
            return Err(Error::Cycle {
                id: id.into(),
                through: blocked_by.into(),
            });
        }
        validate_dependency_hierarchy(&tx, id, blocked_by)?;
        if tx.execute(
            "INSERT INTO deps VALUES(?1,?2) ON CONFLICT DO NOTHING",
            params![id, blocked_by],
        )? == 0
        {
            return Err(Error::DuplicateDependency {
                id: id.into(),
                blocked_by: blocked_by.into(),
            });
        }
        record(&tx, id, actor, "blocked_by", Some(blocked_by))?;
        let updated = load(&tx, project, id)?;
        tx.commit()?;
        Ok(updated)
    }

    pub fn remove_dependency(
        &mut self,
        project: &Project,
        id: &str,
        blocked_by: &str,
        actor: Actor<'_>,
    ) -> Result<Task, Error> {
        let tx = self.write()?;
        load(&tx, project, id)?;
        if tx.execute(
            "DELETE FROM deps WHERE task=?1 AND blocked_by=?2",
            params![id, blocked_by],
        )? == 0
        {
            return Err(Error::NoDependency {
                id: id.into(),
                blocked_by: blocked_by.into(),
            });
        }
        record(&tx, id, actor, "unblocked_by", Some(blocked_by))?;
        let updated = load(&tx, project, id)?;
        tx.commit()?;
        Ok(updated)
    }

    /// Project-wide history after `since` (an event `seq`), oldest first, at most `limit`.
    pub fn events_since(
        &mut self,
        project: &Project,
        since: i64,
        limit: usize,
    ) -> Result<Vec<Event>, Error> {
        let tx = self.connection.transaction()?;
        let mut statement = tx.prepare(
            "SELECT events.seq,events.task,events.at,events.actor,events.kind,events.detail FROM events
             JOIN tasks ON tasks.id=events.task WHERE tasks.project=?1 AND events.seq>?2 ORDER BY events.seq ASC LIMIT ?3",
        )?;
        let limit = i64::try_from(limit).unwrap_or(i64::MAX);
        let rows = statement.query_map(params![project.0, since, limit], event_row)?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// Unclosed descendants, for warning before holding a grouping task.
    pub fn unclosed_descendants(&mut self, project: &Project, id: &str) -> Result<u64, Error> {
        let tx = self.connection.transaction()?;
        load(&tx, project, id)?;
        let count: i64 = tx.query_row(
            "WITH RECURSIVE below(id) AS (SELECT id FROM tasks WHERE parent=?1 UNION SELECT tasks.id FROM tasks JOIN below ON tasks.parent=below.id)
             SELECT COUNT(*) FROM tasks WHERE id IN below AND status IN ('open','held')",
            params![id],
            |row| row.get(0),
        )?;
        Ok(u64::try_from(count).unwrap_or_default())
    }

    pub fn events(
        &mut self,
        project: &Project,
        id: &str,
        limit: usize,
    ) -> Result<Vec<Event>, Error> {
        let tx = self.connection.transaction()?;
        load(&tx, project, id)?;
        let mut statement = tx.prepare("SELECT seq,task,at,actor,kind,detail FROM events WHERE task=?1 ORDER BY seq DESC LIMIT ?2")?;
        let limit = i64::try_from(limit).unwrap_or(i64::MAX);
        let rows = statement.query_map(params![id, limit], event_row)?;
        let mut events = rows.collect::<Result<Vec<_>, _>>()?;
        events.reverse();
        Ok(events)
    }
}

/// A full pull or merge request URL; anything else is a note, not a PR.
fn valid_pr_url(value: &str) -> Result<&str, Error> {
    const EXPECTED: Error = Error::Invalid("pr: expected a full pull request URL");
    let value = value.trim();
    let rest = value
        .strip_prefix("https://")
        .or_else(|| value.strip_prefix("http://"))
        .ok_or(EXPECTED)?;
    let (host, path) = rest.split_once('/').unwrap_or((rest, ""));
    let numbered = |marker: &str| {
        path.split_once(marker).is_some_and(|(_, tail)| {
            let number = tail.split('/').next().unwrap_or_default();
            !number.is_empty() && number.bytes().all(|byte| byte.is_ascii_digit())
        })
    };
    if host.is_empty()
        || value.chars().any(|c| c.is_whitespace() || c.is_control())
        || !(numbered("/pull/") || numbered("/merge_requests/"))
    {
        return Err(EXPECTED);
    }
    Ok(value)
}

fn take(
    tx: &Transaction<'_>,
    task: &Task,
    owner: Owner<'_>,
    lease: Duration,
    now: i64,
) -> Result<(), Error> {
    if let Some(reason) = task.not_workable() {
        return Err(Error::NotReady {
            id: task.id.clone(),
            reason,
        });
    }
    if let Some((current, until)) = task.leased_to_other(Some(owner), now) {
        return Err(Error::Claimed {
            id: task.id.clone(),
            owner: current.into(),
            lease_until: until,
            remaining_secs: until - now,
        });
    }
    let seconds = i64::try_from(lease.as_secs()).unwrap_or(i64::MAX);
    let until = now.saturating_add(seconds.max(1));
    tx.execute(
        "UPDATE tasks SET owner=?2, lease_until=?3, updated_at=?4 WHERE id=?1",
        params![task.id, owner.as_str(), until, now],
    )?;
    let actor = Actor {
        name: Some(owner),
        now,
    };
    let (kind, detail) = match task.owner.as_deref() {
        Some(current) if current == owner.as_str() => ("renewed", None),
        Some(expired) => ("reclaimed", Some(format!("lease of {expired} had expired"))),
        None => ("claimed", None),
    };
    record(tx, &task.id, actor, kind, detail.as_deref())
}

fn insert_task(
    tx: &Transaction<'_>,
    project: &Project,
    task: &NewTask,
    actor: Actor<'_>,
) -> Result<Task, Error> {
    if task.title.trim().is_empty() || task.title.len() > 512 {
        return Err(Error::Invalid("title"));
    }
    if let Some(parent) = &task.parent {
        load(tx, project, parent)?;
    }
    for blocker in &task.blocked_by {
        load(tx, project, blocker)?;
    }
    let id = fresh_id(tx)?;
    tx.execute(
        "INSERT INTO tasks(id,project,parent,title,body,status,priority,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,'open',?6,?7,?7)",
        params![id, project.0, task.parent, task.title.trim(), task.body, task.priority, actor.now],
    )?;
    for blocker in &task.blocked_by {
        validate_dependency_hierarchy(tx, &id, blocker)?;
        tx.execute(
            "INSERT INTO deps VALUES(?1,?2) ON CONFLICT DO NOTHING",
            params![id, blocker],
        )?;
    }
    record(tx, &id, actor, "created", None)?;
    load(tx, project, &id)
}

fn record(
    tx: &Transaction<'_>,
    id: &str,
    actor: Actor<'_>,
    kind: &str,
    detail: Option<&str>,
) -> Result<(), Error> {
    tx.execute(
        "INSERT INTO events(task,at,actor,kind,detail) VALUES(?1,?2,?3,?4,?5)",
        params![id, actor.now, actor.name.map(Owner::as_str), kind, detail],
    )?;
    Ok(())
}

fn fresh_id(tx: &Transaction<'_>) -> Result<String, Error> {
    for _ in 0..ID_ATTEMPTS {
        let suffix: String =
            tx.query_row("SELECT lower(hex(randomblob(3)))", [], |row| row.get(0))?;
        let id = format!("t-{suffix}");
        let taken: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM tasks WHERE id=?1)",
            params![id],
            |row| row.get(0),
        )?;
        if !taken {
            return Ok(id);
        }
    }
    Err(Error::IdSpaceExhausted)
}

/// Whether `from` already waits, transitively, on `target`.
fn reaches(tx: &Transaction<'_>, from: &str, target: &str) -> Result<bool, Error> {
    let found: bool = tx.query_row(
        "WITH RECURSIVE chain(id) AS (SELECT blocked_by FROM deps WHERE task=?1 UNION SELECT deps.blocked_by FROM deps JOIN chain ON deps.task=chain.id)
         SELECT EXISTS(SELECT 1 FROM chain WHERE id=?2)",
        params![from, target],
        |row| row.get(0),
    )?;
    Ok(found)
}

fn validate_dependency_hierarchy(
    tx: &Transaction<'_>,
    id: &str,
    blocked_by: &str,
) -> Result<(), Error> {
    if descends_from(tx, id, blocked_by)? || descends_from(tx, blocked_by, id)? {
        return Err(Error::Hierarchy {
            id: id.into(),
            other: blocked_by.into(),
        });
    }
    Ok(())
}

/// Check both incoming and outgoing edges after a move, within its transaction.
/// Descendants move too; checking just the moved root would miss their edges.
fn validate_subtree_dependencies(tx: &Transaction<'_>, id: &str) -> Result<(), Error> {
    let mut statement = tx.prepare(
        "WITH RECURSIVE subtree(id) AS (SELECT ?1 UNION SELECT tasks.id FROM tasks JOIN subtree ON tasks.parent=subtree.id)
         SELECT task,blocked_by FROM deps WHERE task IN (SELECT id FROM subtree) OR blocked_by IN (SELECT id FROM subtree)",
    )?;
    let dependencies = statement.query_map(params![id], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    for dependency in dependencies {
        let (task, blocker) = dependency?;
        validate_dependency_hierarchy(tx, &task, &blocker)?;
    }
    Ok(())
}

/// Whether `candidate` sits below `target` in the parent chain.
fn descends_from(tx: &Transaction<'_>, candidate: &str, target: &str) -> Result<bool, Error> {
    let found: bool = tx.query_row(
        "WITH RECURSIVE chain(id) AS (SELECT parent FROM tasks WHERE id=?1 UNION SELECT tasks.parent FROM tasks JOIN chain ON tasks.id=chain.id)
         SELECT EXISTS(SELECT 1 FROM chain WHERE id=?2)",
        params![candidate, target],
        |row| row.get(0),
    )?;
    Ok(found)
}

fn event_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Event> {
    Ok(Event {
        seq: row.get(0)?,
        task: row.get(1)?,
        at: row.get(2)?,
        actor: row.get(3)?,
        kind: row.get(4)?,
        detail: row.get(5)?,
    })
}

fn prerequisites(tx: &Transaction<'_>, id: &str) -> Result<Vec<Prerequisite>, Error> {
    let mut statement = tx.prepare(
        "SELECT deps.blocked_by, tasks.status, tasks.title, tasks.parent, tasks.owner FROM deps JOIN tasks ON tasks.id=deps.blocked_by
         WHERE deps.task=?1 ORDER BY deps.blocked_by",
    )?;
    let rows = statement.query_map(params![id], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, Option<String>>(3)?,
            row.get::<_, Option<String>>(4)?,
        ))
    })?;
    rows.map(|row| {
        let (id, status, title, parent, owner) = row?;
        let status = Status::parse(&status).ok_or(Error::Corrupt {
            id: id.clone(),
            field: "status",
        })?;
        Ok(Prerequisite {
            id,
            title,
            status,
            parent,
            owner,
        })
    })
    .collect()
}

fn load(tx: &Transaction<'_>, project: &Project, id: &str) -> Result<Task, Error> {
    let row = tx
        .query_row(
            "SELECT parent,title,body,status,priority,pr,owner,lease_until,created_at,updated_at,closed_at,branch FROM tasks WHERE id=?1 AND project=?2",
            params![id, project.0],
            |row| {
                Ok((
                    row.get::<_, Option<String>>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, Option<String>>(5)?,
                    row.get::<_, Option<String>>(6)?,
                    row.get::<_, Option<i64>>(7)?,
                    row.get::<_, i64>(8)?,
                    row.get::<_, i64>(9)?,
                    row.get::<_, Option<i64>>(10)?,
                    row.get::<_, Option<String>>(11)?,
                ))
            },
        )
        .optional()?
        .ok_or_else(|| Error::NotFound(id.to_owned()))?;
    let (
        parent,
        title,
        body,
        status,
        priority,
        pr,
        owner,
        lease_until,
        created_at,
        updated_at,
        closed_at,
        branch,
    ) = row;
    let status = Status::parse(&status).ok_or(Error::Corrupt {
        id: id.to_owned(),
        field: "status",
    })?;
    let prerequisites = prerequisites(tx, id)?;
    let blocked_by = prerequisites
        .iter()
        .filter(|dep| !dep.status.is_closed())
        .map(|dep| dep.id.clone())
        .collect();
    let last_event = tx
        .query_row(
            "SELECT seq,task,at,actor,kind,detail FROM events WHERE task=?1 ORDER BY seq DESC LIMIT 1",
            params![id],
            event_row,
        )
        .optional()?;
    let (children, unclosed_children): (i64, i64) = tx.query_row(
        "SELECT COUNT(*), COALESCE(SUM(status IN ('open','held')),0) FROM tasks WHERE parent=?1",
        params![id],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    Ok(Task {
        id: id.to_owned(),
        parent,
        title,
        body,
        status,
        priority,
        pr,
        branch,
        owner,
        lease_until,
        created_at,
        updated_at,
        closed_at,
        blocked_by,
        prerequisites,
        children: u64::try_from(children).unwrap_or_default(),
        unclosed_children: u64::try_from(unclosed_children).unwrap_or_default(),
        last_event,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> Store {
        Store::initialize(Connection::open_in_memory().unwrap()).unwrap()
    }

    fn project() -> Project {
        Project::new("/checkout").unwrap()
    }

    fn owner(name: &str) -> Owner<'_> {
        Owner::new(name).unwrap()
    }

    fn actor(now: i64) -> Actor<'static> {
        Actor {
            name: Some(owner("parent")),
            now,
        }
    }

    fn add(store: &mut Store, title: &str) -> Task {
        store
            .add(
                &project(),
                &NewTask {
                    title: title.into(),
                    ..NewTask::default()
                },
                actor(1),
            )
            .unwrap()
    }

    fn counts(store: &Store) -> (i64, i64, i64) {
        store.connection.query_row(
            "SELECT (SELECT count(*) FROM tasks), (SELECT count(*) FROM deps), (SELECT count(*) FROM events)",
            [], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        ).unwrap()
    }

    #[test]
    fn insertion_rejects_parent_and_indirect_ancestor_dependencies_atomically() {
        let mut store = store();
        let root = add(&mut store, "root");
        let parent = store
            .add(
                &project(),
                &NewTask {
                    title: "parent".into(),
                    parent: Some(root.id.clone()),
                    ..NewTask::default()
                },
                actor(2),
            )
            .unwrap();
        let unrelated = add(&mut store, "unrelated");
        for blocker in [&parent.id, &root.id] {
            let before = counts(&store);
            assert!(matches!(
                store.add(
                    &project(),
                    &NewTask {
                        title: "child".into(),
                        parent: Some(parent.id.clone()),
                        blocked_by: vec![unrelated.id.clone(), blocker.clone()],
                        ..NewTask::default()
                    },
                    actor(3)
                ),
                Err(Error::Hierarchy { .. })
            ));
            assert_eq!(
                counts(&store),
                before,
                "task, first dependency, and events roll back"
            );
        }
        store
            .add(
                &project(),
                &NewTask {
                    title: "valid child".into(),
                    parent: Some(parent.id),
                    blocked_by: vec![unrelated.id],
                    ..NewTask::default()
                },
                actor(4),
            )
            .unwrap();
    }

    #[test]
    fn reparenting_checks_edges_in_both_directions_for_the_whole_subtree() {
        for nested in [false, true] {
            for incoming in [false, true] {
                let mut store = store();
                let moving = add(&mut store, "moving");
                let target = add(&mut store, "target");
                let descendant = store
                    .add(
                        &project(),
                        &NewTask {
                            title: "descendant".into(),
                            parent: Some(moving.id.clone()),
                            ..NewTask::default()
                        },
                        actor(2),
                    )
                    .unwrap();
                let endpoint = if nested { &descendant.id } else { &moving.id };
                let (dependent, blocker) = if incoming {
                    (&target.id, endpoint)
                } else {
                    (endpoint, &target.id)
                };
                store
                    .add_dependency(&project(), dependent, blocker, actor(3))
                    .unwrap();
                let before = counts(&store);
                assert!(matches!(
                    store.update(
                        &project(),
                        &moving.id,
                        &Update {
                            parent: Some(Some(target.id.clone())),
                            title: Some("must roll back".into()),
                            ..Update::default()
                        },
                        actor(4),
                        false
                    ),
                    Err(Error::Hierarchy { .. })
                ));
                assert_eq!(counts(&store), before);
                let restored = store.get(&project(), &moving.id).unwrap();
                assert_eq!(restored.parent, None);
                assert_eq!(restored.title, "moving");
                assert_eq!(
                    store
                        .get(&project(), &descendant.id)
                        .unwrap()
                        .parent
                        .as_deref(),
                    Some(moving.id.as_str())
                );
                let unrelated = add(&mut store, "unrelated");
                store
                    .update(
                        &project(),
                        &moving.id,
                        &Update {
                            parent: Some(Some(unrelated.id)),
                            ..Update::default()
                        },
                        actor(5),
                        false,
                    )
                    .unwrap();
            }
        }
    }

    #[test]
    fn readiness_follows_dependencies_status_and_leases() {
        let mut store = store();
        let first = add(&mut store, "first");
        let second = add(&mut store, "second");
        store
            .add_dependency(&project(), &second.id, &first.id, actor(2))
            .unwrap();
        assert_eq!(store.ready(&project(), None, 3).unwrap().len(), 1);
        assert!(matches!(
            store.claim(
                &project(),
                &second.id,
                owner("a"),
                Duration::from_secs(60),
                3
            ),
            Err(Error::NotReady { .. })
        ));
        assert!(matches!(
            store.add_dependency(&project(), &first.id, &second.id, actor(3)),
            Err(Error::Cycle { .. })
        ));
        let claimed = store
            .claim(
                &project(),
                &first.id,
                owner("a"),
                Duration::from_secs(60),
                4,
            )
            .unwrap();
        assert_eq!(claimed.lease_until, Some(64));
        assert!(matches!(
            store.claim(
                &project(),
                &first.id,
                owner("b"),
                Duration::from_secs(60),
                5
            ),
            Err(Error::Claimed { .. })
        ));
        store
            .claim(
                &project(),
                &first.id,
                owner("a"),
                Duration::from_secs(60),
                6,
            )
            .unwrap();
        let reclaimed = store
            .claim(
                &project(),
                &first.id,
                owner("b"),
                Duration::from_secs(60),
                70,
            )
            .unwrap();
        assert_eq!(reclaimed.owner.as_deref(), Some("b"));
        assert!(matches!(
            store.set_status(&project(), &first.id, Status::Done, None, actor(71), false),
            Err(Error::OwnedByAnother { .. })
        ));
        let done = store
            .set_status(
                &project(),
                &first.id,
                Status::Done,
                Some("merged"),
                Actor {
                    name: Some(owner("b")),
                    now: 72,
                },
                false,
            )
            .unwrap();
        assert_eq!(done.closed_at, Some(72));
        assert!(done.owner.is_none());
        let second = store.get(&project(), &second.id).unwrap();
        assert!(second.blocked_by.is_empty() && second.ready(73));
        let held = store
            .set_status(
                &project(),
                &second.id,
                Status::Held,
                Some("waiting"),
                actor(74),
                false,
            )
            .unwrap();
        assert!(!held.ready(75));
        assert!(store.ready(&project(), None, 75).unwrap().is_empty());
        history_is_recorded(&mut store, &first.id);
    }

    fn history_is_recorded(store: &mut Store, first: &str) {
        let kinds: Vec<_> = store
            .events(&project(), first, 10)
            .unwrap()
            .into_iter()
            .map(|event| event.kind)
            .collect();
        assert_eq!(
            kinds,
            ["created", "claimed", "renewed", "reclaimed", "done"]
        );
        assert!(matches!(
            store.get(&Project::new("/other").unwrap(), first),
            Err(Error::NotFound(_))
        ));
    }

    #[test]
    fn hierarchy_and_updates_are_scoped() {
        let mut store = store();
        let epic = add(&mut store, "migration");
        let slice = store
            .add(
                &project(),
                &NewTask {
                    title: "slice".into(),
                    parent: Some(epic.id.clone()),
                    priority: 2,
                    ..NewTask::default()
                },
                actor(1),
            )
            .unwrap();
        assert_eq!(store.get(&project(), &epic.id).unwrap().children, 1);
        assert_eq!(
            store
                .list(
                    &project(),
                    &Filter {
                        parent: Some(&epic.id),
                        ..Filter::default()
                    }
                )
                .unwrap()
                .len(),
            1
        );
        let updated = store
            .update(
                &project(),
                &slice.id,
                &Update {
                    pr: Some("https://github.com/example/project/pull/1".into()),
                    note: Some("opened".into()),
                    ..Update::default()
                },
                actor(2),
                false,
            )
            .unwrap();
        assert!(updated.pr.is_some());
        assert!(matches!(
            store.update(
                &project(),
                &slice.id,
                &Update {
                    parent: Some(Some(slice.id.clone())),
                    ..Update::default()
                },
                actor(3),
                false,
            ),
            Err(Error::AncestorCycle { .. })
        ));
        store
            .set_status(
                &project(),
                &slice.id,
                Status::Dropped,
                Some("obsolete"),
                actor(4),
                false,
            )
            .unwrap();
        assert!(
            store
                .list(&project(), &Filter::default())
                .unwrap()
                .iter()
                .all(|task| task.id == epic.id)
        );
        assert_eq!(
            store
                .list(
                    &project(),
                    &Filter {
                        include_closed: true,
                        ..Filter::default()
                    }
                )
                .unwrap()
                .len(),
            2
        );
        let reopened = store
            .set_status(&project(), &slice.id, Status::Open, None, actor(5), false)
            .unwrap();
        assert!(reopened.closed_at.is_none());
    }

    #[test]
    fn competing_sessions_never_share_a_claim() {
        let directory = std::env::temp_dir().join(format!("dots-tasks-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("tasks.sqlite3");
        let _ = std::fs::remove_file(&path);
        let mut store = Store::open(&path).unwrap();
        let contested = add(&mut store, "contested");
        for number in 0..4 {
            add(&mut store, &format!("slice {number}"));
        }
        drop(store);
        let workers: Vec<_> = (0..8)
            .map(|number| {
                let path = path.clone();
                let contested = contested.id.clone();
                std::thread::spawn(move || {
                    let mut store = Store::open(&path).unwrap();
                    let name = format!("session-{number}");
                    let won = store
                        .claim(
                            &project(),
                            &contested,
                            owner(&name),
                            Duration::from_secs(600),
                            10,
                        )
                        .is_ok();
                    let next = store
                        .claim_next(&project(), None, owner(&name), Duration::from_secs(600), 10)
                        .unwrap();
                    (won, next.map(|task| task.id))
                })
            })
            .collect();
        let results: Vec<_> = workers
            .into_iter()
            .map(|worker| worker.join().unwrap())
            .collect();
        assert_eq!(results.iter().filter(|(won, _)| *won).count(), 1);
        let mut taken: Vec<_> = results
            .iter()
            .filter_map(|(_, next)| next.clone())
            .collect();
        let distinct = taken.len();
        taken.sort();
        taken.dedup();
        assert_eq!(distinct, 4);
        assert_eq!(taken.len(), 4);
        let mut store = Store::open(&path).unwrap();
        assert!(store.ready(&project(), None, 11).unwrap().is_empty());
        assert_eq!(store.ready(&project(), None, 1000).unwrap().len(), 5);
        drop(store);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn release_reopen_and_explicit_claims_respect_leases_and_children() {
        let mut store = store();
        let epic = add(&mut store, "epic");
        let slice = store
            .add(
                &project(),
                &NewTask {
                    title: "slice".into(),
                    parent: Some(epic.id.clone()),
                    ..NewTask::default()
                },
                actor(1),
            )
            .unwrap();
        let lease = Duration::from_secs(60);
        assert!(matches!(
            store.claim(&project(), &epic.id, owner("a"), lease, 2),
            Err(Error::NotReady { .. })
        ));
        store
            .claim(&project(), &slice.id, owner("a"), lease, 2)
            .unwrap();
        assert!(matches!(
            store.release(&project(), &slice.id, owner("b"), 3),
            Err(Error::OwnedByAnother { .. })
        ));
        assert!(matches!(
            store.set_status(&project(), &slice.id, Status::Open, None, actor(3), false),
            Err(Error::Unchanged { .. })
        ));
        let held = store
            .set_status(
                &project(),
                &slice.id,
                Status::Held,
                Some("wait"),
                actor(3),
                true,
            )
            .unwrap();
        assert!(held.owner.is_none());
        store
            .set_status(&project(), &slice.id, Status::Open, None, actor(4), false)
            .unwrap();
        assert!(matches!(
            store.release(&project(), &slice.id, owner("a"), 4),
            Err(Error::Unchanged { .. })
        ));
        store
            .claim(&project(), &slice.id, owner("a"), lease, 4)
            .unwrap();
        let released = store.release(&project(), &slice.id, owner("a"), 5).unwrap();
        assert!(released.owner.is_none());
        store
            .claim(&project(), &slice.id, owner("a"), lease, 6)
            .unwrap();
        let after_expiry = store
            .release(&project(), &slice.id, owner("b"), 100)
            .unwrap();
        assert!(after_expiry.owner.is_none());
        assert!(matches!(
            store.update(
                &project(),
                &epic.id,
                &Update {
                    parent: Some(Some(slice.id.clone())),
                    ..Update::default()
                },
                actor(7),
                false,
            ),
            Err(Error::AncestorCycle { .. })
        ));
        let roots = store
            .list(
                &project(),
                &Filter {
                    roots: true,
                    ..Filter::default()
                },
            )
            .unwrap();
        assert_eq!(roots.len(), 1);
        assert!(Owner::new(" ").is_err());
        assert_eq!(Owner::new(" a ").unwrap().as_str(), "a");
    }

    #[test]
    fn a_slice_in_review_is_not_fresh_work_and_its_fields_are_lease_protected() {
        let mut store = store();
        let slice = add(&mut store, "slice");
        let lease = Duration::from_secs(60);
        store
            .claim(&project(), &slice.id, owner("a"), lease, 1)
            .unwrap();
        let pr = Update {
            pr: Some("https://github.com/example/project/pull/7".into()),
            ..Update::default()
        };
        assert!(matches!(
            store.update(
                &project(),
                &slice.id,
                &pr,
                Actor {
                    name: Some(owner("b")),
                    now: 2
                },
                false
            ),
            Err(Error::OwnedByAnother { .. })
        ));
        assert!(matches!(
            store.update(
                &project(),
                &slice.id,
                &pr,
                Actor { name: None, now: 2 },
                false
            ),
            Err(Error::OwnedByAnother { .. })
        ));
        let noted = store
            .update(
                &project(),
                &slice.id,
                &Update {
                    note: Some("seen".into()),
                    ..Update::default()
                },
                Actor {
                    name: Some(owner("b")),
                    now: 2,
                },
                false,
            )
            .unwrap();
        assert!(noted.pr.is_none(), "notes never need the lease");
        let recorded = store
            .update(
                &project(),
                &slice.id,
                &pr,
                Actor {
                    name: Some(owner("a")),
                    now: 3,
                },
                false,
            )
            .unwrap();
        assert_eq!(recorded.last_event.as_ref().unwrap().kind, "pr");
        // Lease expired, PR still open: not fresh work, but explicitly claimable.
        assert!(!recorded.ready(100));
        assert_eq!(
            recorded.not_ready_because(100).as_deref(),
            Some("PR open: https://github.com/example/project/pull/7")
        );
        assert!(
            store
                .claim_next(&project(), None, owner("b"), lease, 100)
                .unwrap()
                .is_none()
        );
        assert!(store.ready(&project(), None, 100).unwrap().is_empty());
        store
            .claim(&project(), &slice.id, owner("b"), lease, 100)
            .unwrap();
    }
}
