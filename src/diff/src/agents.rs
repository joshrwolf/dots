//! Who works where: interactive Pi sessions from their inbox registrations
//! and Pi workers from the workers' registry, read at runtime and matched
//! to worktrees by directory. Both are other programs' files, read only.

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context as _, Result};
use notify::Watcher as _;
use serde::Deserialize;

/// A registered interactive Pi session.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Agent {
    pub(crate) pid: u32,
    pub(crate) session_id: String,
    #[serde(default)]
    pub(crate) session_file: Option<String>,
    #[serde(default)]
    pub(crate) session_name: Option<String>,
    #[serde(default)]
    pub(crate) rex_block: Option<String>,
    pub(crate) cwd: PathBuf,
    pub(crate) socket: PathBuf,
}

impl Agent {
    /// How the status line names it: its session name, else its directory.
    pub(crate) fn label(&self) -> String {
        if let Some(name) = self.session_name.as_deref().filter(|n| !n.is_empty()) {
            return name.to_owned();
        }
        self.cwd.file_name().map_or_else(
            || self.cwd.display().to_string(),
            |n| n.to_string_lossy().into_owned(),
        )
    }

    /// Its Rex pane, shortened.
    pub(crate) fn pane(&self) -> String {
        self.rex_block.as_deref().map_or_else(
            || format!("pid {}", self.pid),
            |b| {
                let id = b.strip_prefix("block:").unwrap_or(b);
                format!(
                    "pane {}",
                    id.get(id.len().saturating_sub(6)..).unwrap_or(id)
                )
            },
        )
    }
}

/// What a worker is doing, as the registry records it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Status {
    Running,
    /// Waiting for its parent to answer a question.
    Asking,
    /// Its parent reloaded; it resumes when continued.
    Paused,
    Done,
    Failed,
    Stopped,
    /// Recorded as live, but its process is gone.
    Lost,
    /// A state this viewer does not know, as recorded.
    Other(String),
}

impl Status {
    /// The status of a row in `state`, owned by a process that is `alive`.
    fn of(state: &str, alive: bool) -> Self {
        let status = match state {
            "running" => Self::Running,
            "asking" => Self::Asking,
            "paused" => Self::Paused,
            "completed" => Self::Done,
            "failed" | "interrupted" | "unknown" => Self::Failed,
            "stopped" => Self::Stopped,
            "lost" => Self::Lost,
            other => Self::Other(other.to_owned()),
        };
        if status.live() && !alive {
            Self::Lost
        } else {
            status
        }
    }

    /// Still working, or waiting to.
    pub(crate) fn live(&self) -> bool {
        matches!(self, Self::Running | Self::Asking | Self::Paused)
    }

    pub(crate) fn label(&self) -> &str {
        match self {
            Self::Running => "running",
            Self::Asking => "asking",
            Self::Paused => "paused",
            Self::Done => "done",
            Self::Failed => "failed",
            Self::Stopped => "stopped",
            Self::Lost => "lost",
            Self::Other(s) => s,
        }
    }
}

/// A Pi worker, from the workers' registry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Worker {
    pub(crate) id: String,
    pub(crate) label: String,
    pub(crate) status: Status,
    pub(crate) cwd: PathBuf,
    /// As `provider/model`.
    pub(crate) model: Option<String>,
    /// The session file of the runtime that owns it.
    pub(crate) owner_session: Option<String>,
    pub(crate) session_file: Option<String>,
    /// Milliseconds since the epoch.
    pub(crate) updated_at: i64,
}

impl Worker {
    /// Its label and model, as the switcher names it: `fix · opus-5-5`.
    pub(crate) fn name(&self) -> String {
        match self.model_name() {
            Some(model) => format!("{} · {model}", self.label),
            None => self.label.clone(),
        }
    }

    /// The model without its provider or family prefix: `opus-5-5`.
    pub(crate) fn model_name(&self) -> Option<&str> {
        let model = self.model.as_deref()?;
        let model = model.rsplit('/').next().unwrap_or(model);
        Some(model.strip_prefix("claude-").unwrap_or(model))
    }
}

/// Where interactive Pi sessions register: the user's state directory.
pub(crate) fn inbox_dir() -> PathBuf {
    crate::state_home().join("dots").join("pi-inbox")
}

/// The registered sessions whose process is alive, and registrations that
/// could not be read.
#[derive(Debug, Default)]
pub(crate) struct Registered {
    pub(crate) agents: Vec<Agent>,
    pub(crate) problems: Vec<String>,
}

/// Registered sessions in `dir`. Sessions write their registration
/// atomically, so one that does not parse is reported, not skipped quietly.
pub(crate) fn agents(dir: &Path) -> Registered {
    let mut out = Registered::default();
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return out,
        Err(e) => {
            out.problems.push(format!("reading {}: {e}", dir.display()));
            return out;
        }
    };
    for path in entries.flatten().map(|e| e.path()) {
        if path.extension().is_none_or(|x| x != "json") {
            continue;
        }
        let parsed = std::fs::read(&path)
            .map_err(anyhow::Error::from)
            .and_then(|b| Ok(serde_json::from_slice::<Agent>(&b)?));
        match parsed {
            Ok(mut a) if alive(a.pid) => {
                a.cwd = a.cwd.canonicalize().unwrap_or(a.cwd);
                out.agents.push(a);
            }
            Ok(_) => {}
            // Removed by its session between listing and reading.
            Err(e)
                if e.downcast_ref::<std::io::Error>()
                    .is_some_and(|e| e.kind() == std::io::ErrorKind::NotFound) => {}
            Err(e) => out.problems.push(format!("{}: {e:#}", path.display())),
        }
    }
    out.agents.sort_by(|a, b| a.session_id.cmp(&b.session_id));
    out
}

fn alive(pid: u32) -> bool {
    let Ok(pid) = i32::try_from(pid) else {
        return false;
    };
    rustix::process::Pid::from_raw(pid)
        .is_some_and(|p| rustix::process::test_kill_process(p).is_ok())
}

/// The Pi agent directory: `PI_CODING_AGENT_DIR`, else `~/.pi/agent`.
pub(crate) fn agent_dir() -> PathBuf {
    std::env::var_os("PI_CODING_AGENT_DIR").map_or_else(
        || PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".pi/agent"),
        PathBuf::from,
    )
}

/// Where the workers' registry lives in the Pi agent directory.
pub(crate) fn registry_dir(agent_dir: &Path) -> PathBuf {
    agent_dir.join("dots")
}

/// Workers from Pi's worker registry, read only. A missing registry has none.
pub(crate) fn workers() -> Result<Vec<Worker>> {
    workers_in(&agent_dir(), &alive)
}

/// Workers from the registry of the Pi agent directory `dir`; `alive` tells
/// whether a row's owning process still runs.
pub(crate) fn workers_in(dir: &Path, alive: &dyn Fn(u32) -> bool) -> Result<Vec<Worker>> {
    let db = registry_dir(dir).join("workers.db");
    if !db.exists() {
        return Ok(Vec::new());
    }
    read_workers(&db, alive).with_context(|| format!("reading {}", db.display()))
}

fn read_workers(db: &Path, alive: &dyn Fn(u32) -> bool) -> Result<Vec<Worker>> {
    let conn = rusqlite::Connection::open_with_flags(
        db,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    conn.busy_timeout(Duration::from_millis(500))?;
    let mut stmt = conn.prepare(
        "select id, label, state, cwd, model, owner_session, session_file, updated_at, pid from workers",
    )?;
    let rows = stmt.query_map([], |r| {
        let state: String = r.get(2)?;
        let cwd: String = r.get(3)?;
        let pid: i64 = r.get(8)?;
        Ok(Worker {
            id: r.get(0)?,
            label: r.get(1)?,
            status: Status::of(&state, u32::try_from(pid).is_ok_and(alive)),
            cwd: PathBuf::from(&cwd)
                .canonicalize()
                .unwrap_or_else(|_| cwd.into()),
            model: r.get(4)?,
            owner_session: r.get(5)?,
            session_file: r.get(6)?,
            updated_at: r.get(7)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// The worktree a directory belongs to: the deepest one containing it.
pub(crate) fn worktree_of<'a>(dir: &Path, worktrees: &'a [PathBuf]) -> Option<&'a Path> {
    worktrees
        .iter()
        .filter(|w| dir.starts_with(w))
        .max_by_key(|w| w.components().count())
        .map(PathBuf::as_path)
}

/// Every session and worker, as last read.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Snapshot {
    pub(crate) agents: Vec<Agent>,
    pub(crate) workers: Vec<Worker>,
    /// Registrations that could not be read.
    pub(crate) problems: Vec<String>,
    /// Why the workers' registry could not be read, when it could not.
    pub(crate) registry: Option<String>,
}

impl Snapshot {
    /// Reads the inbox registrations and the workers' registry.
    pub(crate) fn read() -> Self {
        let Registered { agents, problems } = agents(&inbox_dir());
        let (workers, registry) = match workers() {
            Ok(w) => (w, None),
            Err(e) => (Vec::new(), Some(format!("{e:#}"))),
        };
        Self {
            agents,
            workers,
            problems,
            registry,
        }
    }

    /// The first problem reading the sessions and workers.
    pub(crate) fn problem(&self) -> Option<&str> {
        self.problems
            .first()
            .or(self.registry.as_ref())
            .map(String::as_str)
    }

    /// Who works in each of `worktrees`, in their order.
    pub(crate) fn owners(&self, worktrees: &[PathBuf]) -> Vec<Owners> {
        let mut out = vec![Owners::default(); worktrees.len()];
        let index = |dir: &Path| {
            let w = worktree_of(dir, worktrees)?;
            worktrees.iter().position(|x| x == w)
        };
        for a in &self.agents {
            if let Some(i) = index(&a.cwd) {
                out[i].sessions.push(a.clone());
            }
        }
        for w in &self.workers {
            if let Some(i) = index(&w.cwd) {
                out[i].workers.push(w.clone());
            }
        }
        for o in &mut out {
            // Live first, then the most recent.
            o.workers
                .sort_by_key(|w| (!w.status.live(), std::cmp::Reverse(w.updated_at)));
        }
        out
    }
}

/// How often sessions and workers are read without a file event: a
/// process that exits writes nothing.
const POLL: Duration = Duration::from_secs(10);
/// Quiet time after a file event before reading, so a burst of writes is
/// read once.
const SETTLE: Duration = Duration::from_millis(150);

/// Reads sessions and workers on a thread of its own whenever their files
/// change, and every [`POLL`], handing each new snapshot to `changed` until
/// it returns false.
pub(crate) fn follow(changed: impl Fn(Snapshot) -> bool + Send + 'static) {
    std::thread::spawn(move || {
        let (tx, rx) = std::sync::mpsc::channel();
        // A directory that is missing or cannot be watched is still read
        // at every poll.
        let mut watcher = notify::recommended_watcher(move |e: notify::Result<notify::Event>| {
            if e.is_ok_and(|e| !matches!(e.kind, notify::EventKind::Access(_))) {
                let _ = tx.send(());
            }
        })
        .ok();
        if let Some(w) = &mut watcher {
            for dir in [inbox_dir(), registry_dir(&agent_dir())] {
                let _ = w.watch(&dir, notify::RecursiveMode::NonRecursive);
            }
        }
        let mut last = None;
        loop {
            let snapshot = Snapshot::read();
            if last.as_ref() != Some(&snapshot) {
                if !changed(snapshot.clone()) {
                    return;
                }
                last = Some(snapshot);
            }
            match rx.recv_timeout(POLL) {
                Ok(()) => {
                    std::thread::sleep(SETTLE);
                    while rx.try_recv().is_ok() {}
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                    std::thread::sleep(POLL);
                }
            }
        }
    });
}

/// The interactive sessions and workers working in one worktree.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Owners {
    pub(crate) sessions: Vec<Agent>,
    /// Live first, then the most recently updated.
    pub(crate) workers: Vec<Worker>,
}

impl Owners {
    /// The worker whose status the worktree shows: one asking, else one
    /// live, else the latest.
    pub(crate) fn lead(&self) -> Option<&Worker> {
        self.workers
            .iter()
            .find(|w| w.status == Status::Asking)
            .or_else(|| self.workers.first())
    }

    /// A worker is still at work here.
    pub(crate) fn busy(&self) -> bool {
        self.workers.iter().any(|w| w.status.live())
    }

    /// Who works here, most relevant first: the lead worker, the
    /// interactive sessions, then how many other workers.
    pub(crate) fn names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.lead().map(Worker::name).into_iter().collect();
        names.extend(self.sessions.iter().map(|a| format!("pi {}", a.label())));
        let others = self.workers.len().saturating_sub(1);
        if others > 0 {
            names.push(format!(
                "+{others} worker{}",
                if others == 1 { "" } else { "s" }
            ));
        }
        names
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn agent(id: &str, cwd: &str) -> Agent {
        Agent {
            pid: 1,
            session_id: id.into(),
            session_file: Some(format!("/s/{id}.jsonl")),
            session_name: Some(id.into()),
            rex_block: None,
            cwd: cwd.into(),
            socket: format!("/i/{id}.sock").into(),
        }
    }

    fn worker(id: &str, cwd: &str, status: Status, at: i64) -> Worker {
        Worker {
            id: id.into(),
            label: id.into(),
            status,
            cwd: cwd.into(),
            model: Some("anthropic/claude-opus-5-5".into()),
            owner_session: Some("/s/p.jsonl".into()),
            session_file: Some(format!("/s/{id}.jsonl")),
            updated_at: at,
        }
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("dots-diff-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("dots")).expect("dir");
        dir
    }

    #[test]
    fn reads_workers_through_the_registry_schema() {
        // The table is the workers extension's; read its own definition.
        let source = include_str!("../../pi/extensions/workers/registry.ts");
        let schema = source
            .split_once("const SCHEMA = `")
            .and_then(|(_, rest)| rest.split_once('`'))
            .expect("registry.ts defines SCHEMA")
            .0;
        let dir = scratch("registry");
        let conn = rusqlite::Connection::open(dir.join("dots/workers.db")).expect("open");
        conn.execute_batch(schema).expect("schema");
        conn.execute_batch(
            "insert into workers (id, root, owner_session, label, state, cwd, model, started_at, last_event_at, updated_at, session_file, pid) values
               ('w-1', 'w-1', '/s/p.jsonl', 'fix', 'running', '/nowhere', 'anthropic/claude-opus-5-5', 1, 2, 3, '/s/w-1.jsonl', 9),
               ('w-2', 'w-2', '/s/p.jsonl', 'gone', 'asking', '/nowhere', null, 1, 2, 4, null, 10),
               ('w-3', 'w-3', '/s/p.jsonl', 'old', 'completed', '/nowhere', null, 1, 2, 5, null, 10);",
        )
        .expect("insert");
        drop(conn);
        let found = workers_in(&dir, &|pid| pid == 9).expect("readable");
        assert_eq!(
            found[0],
            Worker {
                id: "w-1".into(),
                label: "fix".into(),
                status: Status::Running,
                cwd: "/nowhere".into(),
                model: Some("anthropic/claude-opus-5-5".into()),
                owner_session: Some("/s/p.jsonl".into()),
                session_file: Some("/s/w-1.jsonl".into()),
                updated_at: 3,
            }
        );
        let statuses: Vec<&Status> = found.iter().map(|w| &w.status).collect();
        assert_eq!(
            statuses,
            [&Status::Running, &Status::Lost, &Status::Done],
            "a live row whose process is gone is lost"
        );
        assert_eq!(
            workers_in(&dir.join("missing"), &|_| true).expect("none"),
            []
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn statuses_follow_the_registry_states() {
        let of = |s| Status::of(s, true);
        assert_eq!(of("completed"), Status::Done);
        assert_eq!(of("interrupted"), Status::Failed);
        assert_eq!(of("stopped"), Status::Stopped);
        assert_eq!(of("paused"), Status::Paused);
        assert_eq!(of("new-state"), Status::Other("new-state".into()));
        assert_eq!(Status::of("paused", false), Status::Lost);
        assert_eq!(
            Status::of("failed", false),
            Status::Failed,
            "settled rows stay"
        );
    }

    #[test]
    fn owners_are_matched_to_the_deepest_worktree() {
        let worktrees: Vec<PathBuf> = vec!["/r".into(), "/r/.wt/feat".into(), "/w/other".into()];
        let snapshot = Snapshot {
            agents: vec![agent("main", "/r/src"), agent("lead", "/home")],
            workers: vec![
                worker("old", "/r/.wt/feat", Status::Done, 1),
                worker("newer", "/r/.wt/feat/src", Status::Failed, 3),
                worker("live", "/r/.wt/feat", Status::Running, 2),
                worker("q", "/w/other", Status::Done, 9),
                worker("ask", "/w/other", Status::Asking, 1),
            ],
            problems: Vec::new(),
            registry: None,
        };
        let owners = snapshot.owners(&worktrees);
        assert_eq!(owners[0].names(), ["pi main"]);
        assert!(!owners[0].busy());
        let feat: Vec<&str> = owners[1].workers.iter().map(|w| w.id.as_str()).collect();
        assert_eq!(feat, ["live", "newer", "old"], "live first, then newest");
        assert_eq!(owners[1].names(), ["live · opus-5-5", "+2 workers"]);
        assert!(owners[1].busy());
        assert_eq!(owners[2].lead().map(|w| w.id.as_str()), Some("ask"));
        assert_eq!(snapshot.owners(&[]), []);
    }

    #[test]
    fn malformed_registrations_are_reported() {
        let dir = scratch("inbox");
        let me = agent("me", "/r");
        let live = serde_json::json!({
            "pid": std::process::id(), "sessionId": me.session_id, "cwd": "/r", "socket": "/i/me.sock",
        });
        std::fs::write(dir.join("1.json"), live.to_string()).expect("write");
        std::fs::write(dir.join("2.json"), "{\"pid\":").expect("write");
        let found = agents(&dir);
        assert_eq!(
            found
                .agents
                .iter()
                .map(|a| a.session_id.as_str())
                .collect::<Vec<_>>(),
            ["me"]
        );
        assert_eq!(found.problems.len(), 1, "{:?}", found.problems);
        assert!(found.problems[0].contains("2.json"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
