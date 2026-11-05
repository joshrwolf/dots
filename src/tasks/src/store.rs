//! Task records, status transitions, and their transactional store.
use std::path::Path;
use std::time::{Duration, Instant};

use rusqlite::{Connection, OptionalExtension as _, Transaction, params};
use serde::Serialize;

use crate::goals;
use crate::references::{self, ExternalReference};

/// Path relative to the caller's platform data directory.
pub const STORE_PATH: &str = "dots/workspace.sqlite3";

const ID_ATTEMPTS: usize = 16;

/// The only catalog schema this build reads or writes.
const SCHEMA: i64 = 9;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Documents(#[from] dots_documents::Error),
    #[error("open or update workspace catalog")]
    Storage(#[from] rusqlite::Error),
    #[error(
        "unsupported workspace catalog schema {0}; initialization will not replace an existing catalog"
    )]
    Schema(i64),
    #[error("project {0} is not registered; initialize and bind it explicitly")]
    UnregisteredProject(String),
    #[error("common directory {common} is already bound to {project}")]
    BindingConflict { common: String, project: String },
    #[error(
        "common directory {common} is not bound to {project}; use an exact recorded path from {bindings:?}"
    )]
    BindingMissing {
        common: String,
        project: String,
        bindings: Vec<std::path::PathBuf>,
    },
    #[error("cannot inspect workspace catalog {path}")]
    CatalogPath {
        path: std::path::PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("cannot resolve project common directory {path}")]
    ProjectPath {
        path: std::path::PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("invalid {0}")]
    Invalid(&'static str),
    #[error("{0}")]
    Output(String),
    #[error("task {0} does not exist in this project")]
    NotFound(String),
    #[error(
        "task {id} is claimed by {owner}; pass --take if that session has stopped working on it"
    )]
    Claimed { id: String, owner: String },
    #[error("task {id} is already {status}")]
    Unchanged { id: String, status: &'static str },
    #[error("task {id} is {status}; reopen it before recording another result")]
    Closed { id: String, status: &'static str },
    #[error("{id} has an unreadable {field}")]
    Corrupt { id: String, field: &'static str },
    #[error("could not allocate a fresh task id")]
    IdSpaceExhausted,
    #[error("task {id} is not ready ({reason})")]
    NotReady { id: String, reason: String },
    #[error("dependency would make {id} wait on itself through {through}")]
    Cycle { id: String, through: String },
    #[error("task {id} is claimed by {owner}; pass --force to change it anyway")]
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
    #[error(
        "task {id} belongs to goal {goal}, coordinated by {coordinator}; only its coordinator changes goal work"
    )]
    NotCoordinator {
        id: String,
        goal: String,
        coordinator: String,
    },
    #[error(
        "task {id} belongs to goal {goal}, which has no coordinator; run dots task goal coordinate {goal} first, with --resume if it is held or closed"
    )]
    Uncoordinated { id: String, goal: String },
    #[error("task {0} is a goal root; coordinate it with dots task goal coordinate {0}")]
    GoalRoot(String),
    #[error("goal {goal} is {status}; pass --resume to reopen and coordinate it")]
    GoalNotOpen { goal: String, status: &'static str },
    #[error(
        "goal {goal} has claims held by others: {claims}; pass --take if those sessions have stopped"
    )]
    ClaimsHeld { goal: String, claims: String },
    #[error("{id} is named by outstanding requests {requests}; incorporate or withdraw them first")]
    Outstanding { id: String, requests: String },
    #[error("request {0} does not exist in this project")]
    RequestNotFound(String),
    #[error("request {id} {reason}")]
    RequestConflict { id: String, reason: String },
    /// An open-only transition met a request that moved on; `replacement`
    /// names the request that superseded it.
    #[error("request {id} {}", crate::attention::not_open_reason(*state, *closure, replacement.as_deref()))]
    RequestNotOpen {
        id: String,
        state: RequestState,
        closure: Option<crate::attention::ClosureKind>,
        replacement: Option<String>,
    },
}

/// Stable local project identity, independent of checkout location and display name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct Project(pub(super) String);

impl Project {
    /// Validate a project identity before opening or querying its work.
    pub fn try_new(key: impl Into<String>) -> Result<Self, Error> {
        let key = key.into();
        if key.strip_prefix("p-").is_none_or(|hex| {
            hex.len() != 32
                || !hex
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        }) {
            return Err(Error::Invalid(
                "project ID: expected p- followed by 32 lowercase hexadecimal characters",
            ));
        }
        Ok(Self(key))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Validated session or agent name; trimmed once so claims and later edits agree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct Owner<'name>(&'name str);

impl<'name> Owner<'name> {
    /// Validate and trim the claim identity shared by later edits.
    pub fn try_new(name: &'name str) -> Result<Self, Error> {
        let name = name.trim();
        if name.is_empty() || name.len() > 128 || name.chars().any(char::is_control) {
            return Err(Error::Invalid("owner"));
        }
        Ok(Self(name))
    }

    #[must_use]
    pub fn as_str(self) -> &'name str {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Open,
    Review,
    Held,
    Done,
    Dropped,
}

impl Status {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Review => "review",
            Self::Held => "held",
            Self::Done => "done",
            Self::Dropped => "dropped",
        }
    }

    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "open" => Some(Self::Open),
            "review" => Some(Self::Review),
            "held" => Some(Self::Held),
            "done" => Some(Self::Done),
            "dropped" => Some(Self::Dropped),
            _ => None,
        }
    }

    pub fn is_closed(self) -> bool {
        matches!(self, Self::Done | Self::Dropped)
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Task {
    pub id: String,
    pub parent: Option<String>,
    pub title: String,
    pub body: Option<String>,
    /// Current recorded outcome; earlier results stay in history.
    pub result: Option<String>,
    /// Event that recorded the current result, for `result-event`.
    pub result_event: Option<i64>,
    pub evidence: Vec<crate::outcomes::Evidence>,
    pub purged_evidence: Vec<crate::outcomes::Evidence>,
    pub status: Status,
    pub references: Vec<ExternalReference>,
    pub branch: Option<String>,
    /// Session that claimed the task, until it releases, closes, or hands off the task.
    pub owner: Option<String>,
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
    /// Root of the goal this task belongs to (itself for a goal root).
    pub goal: Option<String>,
    /// Eligibility holds from outside the task's own prerequisites.
    pub holds: Vec<Hold>,
}

/// Why work cannot proceed although its own prerequisites may be met.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Hold {
    /// An ancestor is held; its whole subtree waits.
    HeldAncestor { task: String },
    /// An open or answered, not yet incorporated, request names this task or an ancestor.
    Request {
        request: String,
        task: String,
        state: RequestState,
    },
}

impl Hold {
    fn describe(&self) -> String {
        match self {
            Self::HeldAncestor { task } => format!("ancestor {task} is held"),
            Self::Request {
                request,
                task,
                state: RequestState::Answered,
            } => format!(
                "request {request} on {task} is answered but not yet incorporated by the coordinator"
            ),
            Self::Request { request, task, .. } => {
                format!("request {request} on {task} awaits an answer")
            }
        }
    }
}

/// Lifecycle of an attention request; transitions only move forward.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RequestState {
    Open,
    Answered,
    Closed,
}

impl RequestState {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Answered => "answered",
            Self::Closed => "closed",
        }
    }

    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "open" => Some(Self::Open),
            "answered" => Some(Self::Answered),
            "closed" => Some(Self::Closed),
            _ => None,
        }
    }
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
    /// Unclosed prerequisites with their status, and what clears them.
    fn blockers(&self) -> String {
        let blockers: Vec<String> = self
            .blocked_by
            .iter()
            .map(|id| {
                self.prerequisites
                    .iter()
                    .find(|prerequisite| &prerequisite.id == id)
                    .map_or_else(
                        || id.clone(),
                        |prerequisite| format!("{id} ({})", prerequisite.status.as_str()),
                    )
            })
            .collect();
        format!(
            "blocked by {}; a prerequisite stops blocking once it is done or dropped",
            blockers.join(", ")
        )
    }

    /// Why this task cannot be worked on regardless of who asks, if anything.
    #[must_use]
    pub fn not_workable(&self) -> Option<String> {
        if !matches!(self.status, Status::Open | Status::Review) {
            return Some(format!("status is {}", self.status.as_str()));
        }
        if let Some(reason) = self.held_because() {
            return Some(reason);
        }
        if !self.blocked_by.is_empty() {
            return Some(self.blockers());
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

    /// The holds on this task, described, if any.
    #[must_use]
    pub fn held_because(&self) -> Option<String> {
        (!self.holds.is_empty()).then(|| {
            self.holds
                .iter()
                .map(Hold::describe)
                .collect::<Vec<_>>()
                .join("; ")
        })
    }

    /// Whether this task is the root of an attached goal.
    #[must_use]
    pub fn is_goal_root(&self) -> bool {
        self.goal.as_deref() == Some(self.id.as_str())
    }

    /// Claim holder other than `owner`, if any.
    pub(super) fn claimed_by_other(&self, owner: Option<Owner<'_>>) -> Option<&str> {
        self.owner
            .as_deref()
            .filter(|current| owner.is_none_or(|owner| owner.as_str() != *current))
    }

    /// Why this task is not selectable by `claim --next`. Review needs explicit selection.
    #[must_use]
    pub fn not_ready_because(&self) -> Option<String> {
        if self.is_goal_root() {
            return Some("goal root; dots task goal coordinate takes coordination".into());
        }
        if let Some(reason) = self.not_workable() {
            return Some(reason);
        }
        if self.status == Status::Review {
            return Some("status is review; claim by id to review or integrate".into());
        }
        self.owner
            .as_deref()
            .map(|owner| format!("claimed by {owner}"))
    }

    /// Claimable by `claim --next`: open, workable, and unclaimed.
    #[must_use]
    pub fn ready(&self) -> bool {
        self.not_ready_because().is_none()
    }
}

/// What a history entry records. The store writes only these kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventKind {
    Created,
    Updated,
    Claimed,
    Released,
    Open,
    Held,
    Dropped,
    Review,
    Done,
    UpdateResult,
    BlockedBy,
    UnblockedBy,
    ReferenceSet,
    ReferenceRemoved,
    OutputAttached,
    OutputDetached,
    GoalAttached,
    GoalDetached,
    RequestRaised,
    RequestAnswered,
    RequestWithdrawn,
    RequestSuperseded,
    RequestIncorporated,
    DecisionRecorded,
}

impl EventKind {
    /// Every kind, in the order help and listings present them.
    pub const ALL: [Self; 24] = [
        Self::Created,
        Self::Updated,
        Self::Claimed,
        Self::Released,
        Self::Open,
        Self::Held,
        Self::Dropped,
        Self::Review,
        Self::Done,
        Self::UpdateResult,
        Self::BlockedBy,
        Self::UnblockedBy,
        Self::ReferenceSet,
        Self::ReferenceRemoved,
        Self::OutputAttached,
        Self::OutputDetached,
        Self::GoalAttached,
        Self::GoalDetached,
        Self::RequestRaised,
        Self::RequestAnswered,
        Self::RequestWithdrawn,
        Self::RequestSuperseded,
        Self::RequestIncorporated,
        Self::DecisionRecorded,
    ];

    /// The stored and printed name.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Created => "created",
            Self::Updated => "updated",
            Self::Claimed => "claimed",
            Self::Released => "released",
            Self::Open => "open",
            Self::Held => "held",
            Self::Dropped => "dropped",
            Self::Review => "review",
            Self::Done => "done",
            Self::UpdateResult => "update-result",
            Self::BlockedBy => "blocked_by",
            Self::UnblockedBy => "unblocked_by",
            Self::ReferenceSet => "reference_set",
            Self::ReferenceRemoved => "reference_removed",
            Self::OutputAttached => "output-attached",
            Self::OutputDetached => "output-detached",
            Self::GoalAttached => "goal_attached",
            Self::GoalDetached => "goal_detached",
            Self::RequestRaised => "request-raised",
            Self::RequestAnswered => "request-answered",
            Self::RequestWithdrawn => "request-withdrawn",
            Self::RequestSuperseded => "request-superseded",
            Self::RequestIncorporated => "request-incorporated",
            Self::DecisionRecorded => "decision-recorded",
        }
    }
}

impl From<Status> for EventKind {
    /// The entry recording a change to `status`.
    fn from(status: Status) -> Self {
        match status {
            Status::Open => Self::Open,
            Status::Review => Self::Review,
            Status::Held => Self::Held,
            Status::Done => Self::Done,
            Status::Dropped => Self::Dropped,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Event {
    pub seq: i64,
    pub task: String,
    pub at: i64,
    pub actor: Option<String>,
    /// One of [`EventKind::ALL`].
    pub kind: String,
    pub detail: Option<String>,
    pub result: Option<String>,
    pub predecessor: Option<i64>,
    pub operation: Option<String>,
}

/// A page of project history and the project's newest event `seq`.
#[derive(Debug, Clone, Serialize)]
pub struct History {
    pub events: Vec<Event>,
    /// The project's newest event `seq` when the page was read, or 0 before
    /// its first event: a cursor for reading only what follows.
    pub head: i64,
}

/// One page of project history and the catalog position it covers.
#[derive(Debug)]
pub(super) struct EventPage {
    pub events: Vec<Event>,
    /// Highest `seq` of any project this page accounts for.
    pub through: i64,
}

#[derive(Debug, Clone, Default)]
pub struct NewTask {
    pub title: String,
    pub body: Option<String>,
    pub parent: Option<String>,
    pub blocked_by: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct Update {
    pub title: Option<String>,
    pub body: Option<String>,
    pub branch: Option<String>,
    pub parent: Option<Option<String>>,
}

impl Update {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.title.is_none()
            && self.body.is_none()
            && self.branch.is_none()
            && self.parent.is_none()
    }
}

#[derive(Debug, Clone, Default)]
pub struct Filter<'criteria> {
    /// Only tasks claimed by this owner.
    pub owner: Option<Owner<'criteria>>,
    /// Only tasks anywhere below this one.
    pub parent: Option<&'criteria str>,
    /// Only tasks without a parent; ignored when `parent` is set.
    pub roots: bool,
    pub status: Option<Status>,
    pub include_closed: bool,
}

/// Actor and time for one mutation; the caller supplies both explicitly.
#[derive(Debug, Clone, Copy)]
pub struct Actor<'name> {
    pub name: Option<Owner<'name>>,
    pub now: i64,
}

/// One connection per caller; SQLite serializes competing writers.
#[derive(Debug)]
pub struct Store {
    pub(super) connection: Connection,
}

impl Store {
    /// Open an initialized catalog. Never create a database or schema on a read.
    pub fn open(path: &Path) -> Result<Self, Error> {
        let connection =
            Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE)?;
        connection.busy_timeout(Duration::from_secs(5))?;
        let version: i64 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
        if version != SCHEMA {
            return Err(Error::Schema(version));
        }
        Self::initialize(connection)
    }

    /// Explicitly initialize a new catalog; refuse to replace any existing file.
    pub fn create(path: &Path) -> Result<Self, Error> {
        let parent = path
            .parent()
            .ok_or(Error::Invalid("catalog path has no parent"))?;
        let staged = dots_documents::staged_file::StagedFile::create(parent)?;
        let store = Self::initialize(Connection::open_with_flags(
            staged.path(),
            rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE,
        )?)?;
        // Consolidate WAL bytes before publishing the database under its final name.
        store
            .connection
            .pragma_update(None, "journal_mode", "DELETE")?;
        store
            .connection
            .close()
            .map_err(|(_, error)| Error::Storage(error))?;
        staged.publish_new(path)?;
        Self::open(path)
    }

    pub(super) fn initialize(mut connection: Connection) -> Result<Self, Error> {
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
                "CREATE TABLE projects(id TEXT PRIMARY KEY, name TEXT NOT NULL);
                 CREATE TABLE project_bindings(common_directory TEXT PRIMARY KEY, project TEXT NOT NULL REFERENCES projects(id));
                 CREATE TABLE tasks(
                    id TEXT PRIMARY KEY, project TEXT NOT NULL REFERENCES projects(id), parent TEXT REFERENCES tasks(id),
                    title TEXT NOT NULL, body TEXT, result TEXT, status TEXT NOT NULL,
                    branch TEXT, owner TEXT,
                    created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL, closed_at INTEGER);
                 CREATE INDEX tasks_project ON tasks(project, status);
                 CREATE TABLE deps(task TEXT NOT NULL REFERENCES tasks(id), blocked_by TEXT NOT NULL REFERENCES tasks(id), PRIMARY KEY(task, blocked_by));
                 CREATE TABLE events(seq INTEGER PRIMARY KEY, task TEXT NOT NULL REFERENCES tasks(id), at INTEGER NOT NULL, actor TEXT, kind TEXT NOT NULL, detail TEXT, result TEXT, predecessor INTEGER, operation TEXT);
                 CREATE TABLE goals(root TEXT PRIMARY KEY REFERENCES tasks(id), brief TEXT NOT NULL);
                 CREATE TABLE task_refs(task TEXT NOT NULL REFERENCES tasks(id), url TEXT NOT NULL, label TEXT, PRIMARY KEY(task,url));
                 CREATE TABLE outputs(project TEXT NOT NULL REFERENCES projects(id), slug TEXT NOT NULL, filename TEXT NOT NULL, media_type TEXT NOT NULL, PRIMARY KEY(project,slug));
                 CREATE TABLE output_revisions(project TEXT NOT NULL, slug TEXT NOT NULL, digest TEXT NOT NULL, bytes INTEGER NOT NULL, path TEXT NOT NULL UNIQUE, created_at INTEGER NOT NULL, actor TEXT, description TEXT NOT NULL, provenance TEXT NOT NULL, PRIMARY KEY(project,slug,digest), FOREIGN KEY(project,slug) REFERENCES outputs(project,slug));
                 CREATE TABLE task_outputs(task TEXT NOT NULL REFERENCES tasks(id), project TEXT NOT NULL, slug TEXT NOT NULL, PRIMARY KEY(task,project,slug), FOREIGN KEY(project,slug) REFERENCES outputs(project,slug));
                 CREATE TABLE event_evidence(event INTEGER NOT NULL REFERENCES events(seq), project TEXT NOT NULL, slug TEXT NOT NULL, digest TEXT NOT NULL, PRIMARY KEY(event,project,slug,digest), FOREIGN KEY(project,slug,digest) REFERENCES output_revisions(project,slug,digest));
                 CREATE UNIQUE INDEX result_predecessor ON events(task,COALESCE(predecessor,0)) WHERE result IS NOT NULL;
                 CREATE TABLE purged_digests(project TEXT NOT NULL REFERENCES projects(id), digest TEXT NOT NULL, at INTEGER NOT NULL, actor TEXT, PRIMARY KEY(project,digest));
                 CREATE TABLE requests(
                    id TEXT PRIMARY KEY, project TEXT NOT NULL REFERENCES projects(id), goal TEXT NOT NULL REFERENCES tasks(id),
                    key TEXT NOT NULL, kind TEXT NOT NULL, title TEXT NOT NULL, question TEXT NOT NULL, context TEXT, recommendation TEXT,
                    state TEXT NOT NULL, created_at INTEGER NOT NULL, created_by TEXT, supersedes TEXT UNIQUE REFERENCES requests(id),
                    answer_choice TEXT, answer_text TEXT, answer_source TEXT, answer_provenance TEXT, answer_interpretation TEXT,
                    answered_at INTEGER, answered_by TEXT,
                    closure TEXT, closed_at INTEGER, closed_by TEXT, close_reason TEXT);
                 CREATE UNIQUE INDEX requests_key ON requests(goal,key) WHERE closure IS NOT 'superseded';
                 CREATE INDEX requests_project ON requests(project,state);
                 CREATE TABLE request_choices(request TEXT NOT NULL REFERENCES requests(id), position INTEGER NOT NULL, id TEXT NOT NULL, label TEXT NOT NULL, detail TEXT, PRIMARY KEY(request,position), UNIQUE(request,id));
                 CREATE TABLE request_tasks(request TEXT NOT NULL REFERENCES requests(id), task TEXT NOT NULL REFERENCES tasks(id), PRIMARY KEY(request,task));
                 CREATE INDEX request_tasks_task ON request_tasks(task);
                 CREATE TABLE decisions(
                    id TEXT PRIMARY KEY, project TEXT NOT NULL REFERENCES projects(id), goal TEXT NOT NULL REFERENCES tasks(id),
                    kind TEXT NOT NULL, decision TEXT NOT NULL, human_text TEXT, human_source TEXT, human_provenance TEXT,
                    request TEXT UNIQUE REFERENCES requests(id), at INTEGER NOT NULL, actor TEXT);
                 CREATE INDEX decisions_goal ON decisions(goal,kind);
                 PRAGMA user_version=9;",
            )?,
            SCHEMA => {}
            other => return Err(Error::Schema(other)),
        }
        tx.commit()?;
        Ok(Self { connection })
    }

    /// Early goal-authority check for operations that do work before their write transaction.
    pub(super) fn check_authority(&mut self, id: &str, actor: Actor<'_>) -> Result<(), Error> {
        let tx = self.connection.transaction()?;
        authorize(&tx, id, actor.name)
    }

    pub(super) fn write(&mut self) -> Result<Transaction<'_>, Error> {
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
                 ORDER BY created_at ASC, rowid ASC",
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

    /// Unclaimed, eligible leaf tasks below `parent`, oldest first. Grouping
    /// tasks and goal roots are excluded because `claim_next` never hands them
    /// out; whether the caller coordinates a task's goal is checked on claim.
    pub fn ready(&mut self, project: &Project, parent: Option<&str>) -> Result<Vec<Task>, Error> {
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
            .filter(|task| task.children == 0 && task.ready())
            .collect())
    }

    /// Atomically claim a task. Claiming one's own task again changes nothing;
    /// another owner's claim requires `take`. A goal task is claimable only by
    /// its goal's coordinator; goal roots are coordinated, never claimed.
    pub fn claim(
        &mut self,
        project: &Project,
        id: &str,
        owner: Owner<'_>,
        take_over: bool,
        now: i64,
    ) -> Result<Task, Error> {
        let tx = self.write()?;
        let task = load(&tx, project, id)?;
        if task.is_goal_root() {
            return Err(Error::GoalRoot(task.id));
        }
        authorize(&tx, id, Some(owner))?;
        take(&tx, &task, owner, take_over, now)?;
        let claimed = load(&tx, project, id)?;
        tx.commit()?;
        Ok(claimed)
    }

    /// Atomically claim the oldest ready task, if any, skipping goal work the
    /// owner does not coordinate.
    pub fn claim_next(
        &mut self,
        project: &Project,
        parent: Option<&str>,
        owner: Owner<'_>,
        now: i64,
    ) -> Result<Option<Task>, Error> {
        let tx = self.write()?;
        let ids = {
            let mut statement = tx.prepare(
                "WITH RECURSIVE below(id) AS (SELECT id FROM tasks WHERE parent=?2 UNION SELECT tasks.id FROM tasks JOIN below ON tasks.parent=below.id)
                 SELECT id FROM tasks WHERE project=?1 AND status='open' AND (?2 IS NULL OR id IN below)
                 ORDER BY created_at ASC, rowid ASC",
            )?;
            let rows =
                statement.query_map(params![project.0, parent], |row| row.get::<_, String>(0))?;
            rows.collect::<Result<Vec<_>, _>>()?
        };
        for id in ids {
            let task = load(&tx, project, &id)?;
            if task.children == 0 && task.ready() && authorize(&tx, &id, Some(owner)).is_ok() {
                take(&tx, &task, owner, false, now)?;
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
        if let Some(current) = task.claimed_by_other(Some(owner)) {
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
        unclaim(&tx, id, owner, now)?;
        let released = load(&tx, project, id)?;
        tx.commit()?;
        Ok(released)
    }

    /// Release every claim `owner` holds in any project, for a session that
    /// ends. Returns the released task IDs.
    pub fn release_all(&mut self, owner: Owner<'_>, now: i64) -> Result<Vec<String>, Error> {
        let tx = self.write()?;
        let ids = {
            let mut statement =
                tx.prepare("SELECT id FROM tasks WHERE owner=?1 ORDER BY created_at, rowid")?;
            statement
                .query_map([owner.as_str()], |row| row.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?
        };
        for id in &ids {
            unclaim(&tx, id, owner, now)?;
        }
        tx.commit()?;
        Ok(ids)
    }

    /// Edit fields. A task claimed by another owner needs `force`.
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
        authorize(&tx, id, actor.name)?;
        if !force && let Some(owner) = before.claimed_by_other(actor.name) {
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
        if let Some(parent) = &update.parent {
            if parent.is_some() && goals::attached(&tx, id)? {
                return Err(Error::Invalid(
                    "goal must remain a root task; detach it before reparenting",
                ));
            }
            if let Some(parent) = parent {
                let target = load(&tx, project, parent)?;
                if parent == id || descends_from(&tx, parent, id)? {
                    return Err(Error::AncestorCycle {
                        id: id.into(),
                        through: parent.clone(),
                    });
                }
                authorize(&tx, parent, actor.name)?;
                if target.goal != before.goal {
                    leaving_goal(&tx, id)?;
                }
            } else if before.goal.is_some() && !before.is_goal_root() {
                leaving_goal(&tx, id)?;
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
        record(
            &tx,
            id,
            actor,
            EventKind::Updated,
            Some(&changes.join(", ")),
        )?;
        tx.execute(
            "UPDATE tasks SET updated_at=?2 WHERE id=?1",
            params![id, actor.now],
        )?;
        let updated = load(&tx, project, id)?;
        tx.commit()?;
        Ok(updated)
    }

    /// Hold, drop, or reopen without changing the result or evidence.
    /// A task claimed by another owner requires `force`.
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
        if matches!(status, Status::Review | Status::Done) {
            return Err(Error::Invalid("review and done require a result"));
        }
        authorize(&tx, id, actor.name)?;
        if status.is_closed() && task.is_goal_root() {
            require_settled_goal(&tx, id)?;
        }
        if task.status == status {
            return Err(Error::Unchanged {
                id: id.into(),
                status: status.as_str(),
            });
        }
        if let Some(owner) = task.claimed_by_other(actor.name)
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
                Some(task.blockers())
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
            "UPDATE tasks SET status=?2, closed_at=?3, owner=NULL, updated_at=?4 WHERE id=?1",
            params![id, status.as_str(), closed_at, actor.now],
        )?;
        record(&tx, id, actor, status.into(), reason)?;
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
        authorize(&tx, id, actor.name)?;
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
        record(&tx, id, actor, EventKind::BlockedBy, Some(blocked_by))?;
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
        authorize(&tx, id, actor.name)?;
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
        record(&tx, id, actor, EventKind::UnblockedBy, Some(blocked_by))?;
        let updated = load(&tx, project, id)?;
        tx.commit()?;
        Ok(updated)
    }

    /// Project history after `since` (an event `seq`), oldest first, at most `limit`,
    /// with the project's newest `seq` as of the same read.
    ///
    /// With `under`, only events on that task and its descendants, judged by
    /// the tree as it is now; the head stays the whole project's, so it is a
    /// cursor from which no later event in any subtree is missed. Oversized
    /// limits saturate to SQLite's signed integer maximum.
    pub fn history(
        &mut self,
        project: &Project,
        under: Option<&str>,
        since: i64,
        limit: usize,
    ) -> Result<History, Error> {
        let tx = self.connection.transaction()?;
        let events = query_events(&tx, project, under, since, limit)?;
        let head = tx.query_row(
            "SELECT COALESCE(MAX(events.seq),0) FROM events JOIN tasks ON tasks.id=events.task WHERE tasks.project=?1",
            params![project.0],
            |row| row.get(0),
        )?;
        Ok(History { events, head })
    }

    /// Like [`Self::history`] without the head, plus how far the page covers
    /// the whole catalog.
    ///
    /// Every write transaction is immediate and events are never deleted, so
    /// `seq` (the rowid) grows in commit order: an event committed after this
    /// read has a `seq` above any this read could see. A page that is not
    /// full therefore covers every project's history up to the newest `seq`,
    /// and a follower can resume there rather than rescanning other projects'
    /// events after its own last one. A task that moves into `under` after
    /// this read contributes only its later events.
    pub(super) fn event_page(
        &mut self,
        project: &Project,
        under: Option<&str>,
        since: i64,
        limit: usize,
    ) -> Result<EventPage, Error> {
        let tx = self.connection.transaction()?;
        let events = query_events(&tx, project, under, since, limit)?;
        let through = match events.last() {
            Some(last) if events.len() >= limit => last.seq,
            _ => tx.query_row(
                "SELECT MAX(?1, COALESCE(MAX(seq), ?1)) FROM events",
                params![since],
                |row| row.get(0),
            )?,
        };
        Ok(EventPage { events, through })
    }

    pub fn events(
        &mut self,
        project: &Project,
        id: &str,
        limit: usize,
    ) -> Result<Vec<Event>, Error> {
        let tx = self.connection.transaction()?;
        load(&tx, project, id)?;
        let mut statement = tx.prepare("SELECT seq,task,at,actor,kind,detail,result,predecessor,operation FROM events WHERE task=?1 ORDER BY seq DESC LIMIT ?2")?;
        let limit = i64::try_from(limit).unwrap_or(i64::MAX);
        let rows = statement.query_map(params![id, limit], event_row)?;
        let mut events = rows.collect::<Result<Vec<_>, _>>()?;
        events.reverse();
        Ok(events)
    }
}

/// A project's events after `since`, oldest first, optionally only on `under`
/// and its current descendants; an `under` outside the project is an error.
fn query_events(
    tx: &Transaction<'_>,
    project: &Project,
    under: Option<&str>,
    since: i64,
    limit: usize,
) -> Result<Vec<Event>, Error> {
    if let Some(id) = under {
        let known: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM tasks WHERE id=?1 AND project=?2)",
            params![id, project.0],
            |row| row.get(0),
        )?;
        if !known {
            return Err(Error::NotFound(id.to_owned()));
        }
    }
    let mut statement = tx.prepare(
        "WITH RECURSIVE subtree(id) AS (SELECT ?4 UNION SELECT tasks.id FROM tasks JOIN subtree ON tasks.parent=subtree.id)
         SELECT events.seq,events.task,events.at,events.actor,events.kind,events.detail,events.result,events.predecessor,events.operation FROM events
         JOIN tasks ON tasks.id=events.task WHERE tasks.project=?1 AND events.seq>?2 AND (?4 IS NULL OR events.task IN subtree)
         ORDER BY events.seq ASC LIMIT ?3",
    )?;
    let limit = i64::try_from(limit).unwrap_or(i64::MAX);
    let rows = statement.query_map(params![project.0, since, limit, under], event_row)?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

pub(super) fn validate_result(result: &str) -> Result<(), Error> {
    if result.trim().is_empty() || result.len() > 24 * 1024 {
        return Err(Error::Invalid(
            "result: provide nonempty Markdown, at most 24 KiB",
        ));
    }
    Ok(())
}

fn take(
    tx: &Transaction<'_>,
    task: &Task,
    owner: Owner<'_>,
    take_over: bool,
    now: i64,
) -> Result<(), Error> {
    if let Some(reason) = task.not_workable() {
        return Err(Error::NotReady {
            id: task.id.clone(),
            reason,
        });
    }
    let detail = match task.owner.as_deref() {
        Some(current) if current == owner.as_str() => return Ok(()),
        Some(current) if !take_over => {
            return Err(Error::Claimed {
                id: task.id.clone(),
                owner: current.into(),
            });
        }
        Some(current) => Some(format!("taken from {current}")),
        None => None,
    };
    tx.execute(
        "UPDATE tasks SET owner=?2, updated_at=?3 WHERE id=?1",
        params![task.id, owner.as_str(), now],
    )?;
    let actor = Actor {
        name: Some(owner),
        now,
    };
    record(tx, &task.id, actor, EventKind::Claimed, detail.as_deref())
}

fn unclaim(tx: &Transaction<'_>, id: &str, owner: Owner<'_>, now: i64) -> Result<(), Error> {
    tx.execute(
        "UPDATE tasks SET owner=NULL, updated_at=?2 WHERE id=?1",
        params![id, now],
    )?;
    let actor = Actor {
        name: Some(owner),
        now,
    };
    record(tx, id, actor, EventKind::Released, None)
}

pub(super) fn insert_task(
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
        authorize(tx, parent, actor.name)?;
    }
    for blocker in &task.blocked_by {
        load(tx, project, blocker)?;
    }
    let id = fresh_id(tx, "t", "tasks")?;
    tx.execute(
        "INSERT INTO tasks(id,project,parent,title,body,status,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,'open',?6,?6)",
        params![id, project.0, task.parent, task.title.trim(), task.body, actor.now],
    )?;
    for blocker in &task.blocked_by {
        validate_dependency_hierarchy(tx, &id, blocker)?;
        tx.execute(
            "INSERT INTO deps VALUES(?1,?2) ON CONFLICT DO NOTHING",
            params![id, blocker],
        )?;
    }
    record(tx, &id, actor, EventKind::Created, None)?;
    load(tx, project, &id)
}

pub(super) fn record(
    tx: &Transaction<'_>,
    id: &str,
    actor: Actor<'_>,
    kind: EventKind,
    detail: Option<&str>,
) -> Result<(), Error> {
    tx.execute(
        "INSERT INTO events(task,at,actor,kind,detail) VALUES(?1,?2,?3,?4,?5)",
        params![
            id,
            actor.now,
            actor.name.map(Owner::as_str),
            kind.as_str(),
            detail
        ],
    )?;
    Ok(())
}

/// A random `prefix-<32 hex>` identifier not yet used in `table`.
pub(super) fn fresh_id(
    tx: &Transaction<'_>,
    prefix: &str,
    table: &'static str,
) -> Result<String, Error> {
    for _ in 0..ID_ATTEMPTS {
        let suffix: String =
            tx.query_row("SELECT lower(hex(randomblob(16)))", [], |row| row.get(0))?;
        let id = format!("{prefix}-{suffix}");
        let taken: bool = tx.query_row(
            &format!("SELECT EXISTS(SELECT 1 FROM {table} WHERE id=?1)"),
            params![id],
            |row| row.get(0),
        )?;
        if !taken {
            return Ok(id);
        }
    }
    Err(Error::IdSpaceExhausted)
}

/// Root of the goal `id` belongs to, and that root's current claim.
pub(super) fn membership(
    tx: &Transaction<'_>,
    id: &str,
) -> Result<Option<(String, Option<String>)>, Error> {
    Ok(tx
        .query_row(
            "WITH RECURSIVE chain(id) AS (SELECT ?1 UNION SELECT tasks.parent FROM tasks JOIN chain ON tasks.id=chain.id WHERE tasks.parent IS NOT NULL)
             SELECT tasks.id, tasks.owner FROM tasks JOIN goals ON goals.root=tasks.id WHERE tasks.id IN chain",
            params![id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?)
}

/// Require `actor` to coordinate the goal `id` belongs to, if any. Tasks
/// outside goals keep ordinary claim-based ownership.
pub(super) fn authorize(
    tx: &Transaction<'_>,
    id: &str,
    actor: Option<Owner<'_>>,
) -> Result<(), Error> {
    let Some((goal, coordinator)) = membership(tx, id)? else {
        return Ok(());
    };
    match coordinator {
        Some(current) if actor.is_some_and(|actor| actor.as_str() == current) => Ok(()),
        Some(coordinator) => Err(Error::NotCoordinator {
            id: id.into(),
            goal,
            coordinator,
        }),
        None => Err(Error::Uncoordinated {
            id: id.into(),
            goal,
        }),
    }
}

/// Open or answered requests of `goal`, oldest first.
pub(super) fn outstanding_requests(tx: &Transaction<'_>, goal: &str) -> Result<Vec<String>, Error> {
    let mut statement = tx.prepare(
        "SELECT id FROM requests WHERE goal=?1 AND state IN ('open','answered') ORDER BY created_at, rowid",
    )?;
    Ok(statement
        .query_map([goal], |row| row.get(0))?
        .collect::<Result<Vec<String>, _>>()?)
}

/// A goal root may close or lose its goal only once no request is outstanding,
/// whichever task the request names; `force` never bypasses this.
pub(super) fn require_settled_goal(tx: &Transaction<'_>, goal: &str) -> Result<(), Error> {
    let requests = outstanding_requests(tx, goal)?;
    if requests.is_empty() {
        Ok(())
    } else {
        Err(Error::Outstanding {
            id: goal.into(),
            requests: requests.join(", "),
        })
    }
}

/// Refuse to move a subtree out of its goal while a request names any of it.
fn leaving_goal(tx: &Transaction<'_>, id: &str) -> Result<(), Error> {
    let mut statement = tx.prepare(
        "WITH RECURSIVE subtree(id) AS (SELECT ?1 UNION SELECT tasks.id FROM tasks JOIN subtree ON tasks.parent=subtree.id)
         SELECT DISTINCT requests.id FROM request_tasks JOIN requests ON requests.id=request_tasks.request
         WHERE request_tasks.task IN subtree AND requests.state IN ('open','answered') ORDER BY requests.id",
    )?;
    let requests = statement
        .query_map([id], |row| row.get(0))?
        .collect::<Result<Vec<String>, _>>()?;
    if requests.is_empty() {
        Ok(())
    } else {
        Err(Error::Outstanding {
            id: id.into(),
            requests: requests.join(", "),
        })
    }
}

/// Held ancestors and unresolved requests on the task or an ancestor.
fn holds(tx: &Transaction<'_>, id: &str) -> Result<Vec<Hold>, Error> {
    let mut holds = Vec::new();
    let mut statement = tx.prepare(
        "WITH RECURSIVE chain(id, depth) AS (SELECT parent, 1 FROM tasks WHERE id=?1 AND parent IS NOT NULL
            UNION SELECT tasks.parent, chain.depth+1 FROM tasks JOIN chain ON tasks.id=chain.id WHERE tasks.parent IS NOT NULL)
         SELECT tasks.id FROM tasks JOIN chain ON chain.id=tasks.id WHERE tasks.status='held' ORDER BY chain.depth",
    )?;
    for task in statement.query_map([id], |row| row.get::<_, String>(0))? {
        holds.push(Hold::HeldAncestor { task: task? });
    }
    let mut statement = tx.prepare(
        "WITH RECURSIVE chain(id) AS (SELECT ?1 UNION SELECT tasks.parent FROM tasks JOIN chain ON tasks.id=chain.id WHERE tasks.parent IS NOT NULL)
         SELECT requests.id, request_tasks.task, requests.state FROM request_tasks JOIN requests ON requests.id=request_tasks.request
         WHERE request_tasks.task IN chain AND requests.state<>'closed' ORDER BY requests.created_at, requests.rowid, request_tasks.task",
    )?;
    let rows = statement.query_map([id], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
        ))
    })?;
    for row in rows {
        let (request, task, state) = row?;
        let state = RequestState::parse(&state).ok_or_else(|| Error::Corrupt {
            id: request.clone(),
            field: "request state",
        })?;
        holds.push(Hold::Request {
            request,
            task,
            state,
        });
    }
    Ok(holds)
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
        result: row.get(6)?,
        predecessor: row.get(7)?,
        operation: row.get(8)?,
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

pub(super) fn load(tx: &Transaction<'_>, project: &Project, id: &str) -> Result<Task, Error> {
    let row = tx
        .query_row(
            "SELECT parent,title,body,status,result,owner,created_at,updated_at,closed_at,branch FROM tasks WHERE id=?1 AND project=?2",
            params![id, project.0],
            |row| {
                Ok((
                    row.get::<_, Option<String>>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, Option<String>>(5)?,
                    row.get::<_, i64>(6)?,
                    row.get::<_, i64>(7)?,
                    row.get::<_, Option<i64>>(8)?,
                    row.get::<_, Option<String>>(9)?,
                ))
            },
        )
        .optional()?
        .ok_or_else(|| Error::NotFound(id.to_owned()))?;
    let (parent, title, body, status, result, owner, created_at, updated_at, closed_at, branch) =
        row;
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
            "SELECT seq,task,at,actor,kind,detail,result,predecessor,operation FROM events WHERE task=?1 ORDER BY seq DESC LIMIT 1",
            params![id],
            event_row,
        )
        .optional()?;
    let (children, unclosed_children): (i64, i64) = tx.query_row(
        "SELECT COUNT(*), COALESCE(SUM(status NOT IN ('done','dropped')),0) FROM tasks WHERE parent=?1",
        params![id],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    let result_event = crate::outcomes::current_event(tx, id)?;
    let (evidence, purged_evidence) = match result_event {
        Some(seq) => (
            crate::outcomes::evidence(tx, seq)?,
            crate::outcomes::purged_evidence(tx, seq)?,
        ),
        None => (Vec::new(), Vec::new()),
    };
    let goal = membership(tx, id)?.map(|(root, _)| root);
    Ok(Task {
        holds: holds(tx, id)?,
        goal,
        id: id.to_owned(),
        parent,
        title,
        body,
        result,
        result_event,
        evidence,
        purged_evidence,
        status,
        references: references::load_references(tx, id)?,
        branch,
        owner,
        created_at,
        updated_at,
        closed_at,
        blocked_by,
        prerequisites,
        children: u64::try_from(children).expect("SQLite COUNT must be nonnegative"),
        unclosed_children: u64::try_from(unclosed_children)
            .expect("sum of nonnegative child indicators must be nonnegative"),
        last_event,
    })
}

#[cfg(test)]
mod coordination_tests;

#[cfg(test)]
mod tests;
