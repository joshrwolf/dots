//! `dots task`: project resolution, storage location, and presentation.
use std::fmt::Write as _;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

use anyhow::{Context as _, Result, bail};
use dots_tasks::{Actor, Filter, NewTask, Owner, Project, Status, Store, Task, Update};
use time::OffsetDateTime;

use crate::command::{DepCommand, TaskCommand, TaskMutation, TaskQuery};

pub(crate) struct Request<'a> {
    pub command: TaskCommand,
    pub project: Option<String>,
    pub owner: Option<&'a str>,
    pub directory: &'a Path,
    pub json: bool,
}

pub(crate) fn run(request: Request<'_>) -> Result<String> {
    let project = match request.project {
        Some(key) => Project::new(key)?,
        None => Project::new(main_checkout(request.directory)?)?,
    };
    let path = herdrkit::user_state_directory()?.join(dots_tasks::STORE_PATH);
    let directory = path.parent().context("task store path has no parent")?;
    std::fs::create_dir_all(directory).context("create task store directory")?;
    let mut store = Store::open(&path)?;
    let now = OffsetDateTime::now_utc().unix_timestamp();
    let actor = Actor {
        name: request.owner.map(Owner::new).transpose()?,
        now,
    };
    let outcome = execute(&mut store, &project, request.command, actor)?;
    if request.json {
        let mut value = serde_json::to_value(&outcome)?;
        if let serde_json::Value::Object(fields) = &mut value {
            fields.insert("project".into(), serde_json::to_value(&project)?);
            fields.insert("now".into(), now.into());
        }
        Ok(crate::json::render(value))
    } else {
        Ok(text(&outcome, now))
    }
}

#[derive(Debug, serde::Serialize)]
#[serde(untagged)]
enum Outcome {
    Task {
        task: Box<Shown>,
        #[serde(skip_serializing_if = "Vec::is_empty")]
        events: Vec<dots_tasks::Event>,
        #[serde(skip_serializing_if = "Vec::is_empty")]
        warnings: Vec<String>,
    },
    Tasks {
        tasks: Vec<Shown>,
    },
    Events {
        events: Vec<dots_tasks::Event>,
    },
}

/// A task plus readiness evaluated with this invocation's clock.
#[derive(Debug, serde::Serialize)]
struct Shown {
    #[serde(flatten)]
    task: Task,
    ready: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    not_ready_because: Option<String>,
}

impl Shown {
    fn new(task: Task, now: i64) -> Self {
        let not_ready_because = task.not_ready_because(now);
        Self {
            ready: not_ready_because.is_none(),
            task,
            not_ready_because,
        }
    }
}

fn execute(
    store: &mut Store,
    project: &Project,
    command: TaskCommand,
    actor: Actor<'_>,
) -> Result<Outcome> {
    let now = actor.now;
    let shown = |tasks: Vec<Task>| {
        tasks
            .into_iter()
            .map(|task| Shown::new(task, now))
            .collect()
    };
    match command {
        TaskCommand::Query(TaskQuery::List {
            parent,
            claimed_by,
            roots,
            status,
            all,
        }) => {
            let owner = claimed_by.as_deref().map(Owner::new).transpose()?;
            let status = status
                .map(|value| {
                    Status::parse(&value).context("status must be open, held, done or dropped")
                })
                .transpose()?;
            let filter = Filter {
                owner,
                parent: parent.as_deref(),
                roots,
                status,
                include_closed: all,
            };
            Ok(Outcome::Tasks {
                tasks: shown(store.list(project, &filter)?),
            })
        }
        TaskCommand::Query(TaskQuery::Ready { parent }) => Ok(Outcome::Tasks {
            tasks: shown(store.ready(project, parent.as_deref(), now)?),
        }),
        TaskCommand::Query(TaskQuery::Show { id }) => {
            let task = Box::new(Shown::new(store.get(project, &id)?, now));
            let events = store.events(project, &id, 10)?;
            Ok(Outcome::Task {
                task,
                events,
                warnings: Vec::new(),
            })
        }
        TaskCommand::Query(TaskQuery::Events { since, limit }) => Ok(Outcome::Events {
            events: store.events_since(project, since, usize::from(limit))?,
        }),
        TaskCommand::Mutation(mutation) => {
            let adds_dependency = matches!(
                mutation,
                TaskMutation::Dep {
                    command: DepCommand::Add { .. }
                }
            );
            let holds = matches!(mutation, TaskMutation::Hold { .. });
            let task =
                mutate(store, project, mutation, actor)?.context("no ready task to claim")?;
            let mut warnings = Vec::new();
            if adds_dependency
                && let (Some(owner), Some(until)) = (&task.owner, task.lease_until)
                && until > now
            {
                warnings.push(format!(
                    "{} is leased to {owner} and is now blocked; tell that session",
                    task.id
                ));
            }
            if holds && task.children > 0 {
                let below = store.unclosed_descendants(project, &task.id)?;
                if below > 0 {
                    warnings.push(format!(
                        "{below} unclosed descendants stay claimable; hold them individually to pause the effort"
                    ));
                }
            }
            Ok(Outcome::Task {
                task: Box::new(Shown::new(task, now)),
                events: Vec::new(),
                warnings,
            })
        }
    }
}

fn mutate(
    store: &mut Store,
    project: &Project,
    command: TaskMutation,
    actor: Actor<'_>,
) -> Result<Option<Task>> {
    let now = actor.now;
    let task = match command {
        TaskMutation::Add {
            title,
            body,
            parent,
            priority,
            blocked_by,
        } => {
            let task = NewTask {
                title,
                body,
                parent,
                priority,
                blocked_by,
            };
            store.add(project, &task, actor)?
        }
        TaskMutation::Claim {
            id,
            next: _,
            parent,
            lease,
        } => {
            let owner = owner(actor)?;
            let lease = Duration::from_secs(u64::from(lease) * 60);
            return Ok(match id {
                Some(id) => Some(store.claim(project, &id, owner, lease, now)?),
                None => store.claim_next(project, parent.as_deref(), owner, lease, now)?,
            });
        }
        TaskMutation::Release { id } => store.release(project, &id, owner(actor)?, now)?,
        TaskMutation::Update {
            id,
            title,
            body,
            pr,
            branch,
            priority,
            parent,
            note,
            force,
        } => {
            let update = Update {
                title,
                body,
                pr,
                branch,
                priority,
                parent: parent.map(|value| (!value.is_empty()).then_some(value)),
                note,
            };
            store.update(project, &id, &update, actor, force)?
        }
        TaskMutation::Done { id, note, force } => {
            store.set_status(project, &id, Status::Done, note.as_deref(), actor, force)?
        }
        TaskMutation::Hold { id, reason, force } => {
            store.set_status(project, &id, Status::Held, Some(&reason), actor, force)?
        }
        TaskMutation::Drop { id, reason, force } => {
            store.set_status(project, &id, Status::Dropped, Some(&reason), actor, force)?
        }
        TaskMutation::Reopen { id, note, force } => {
            store.set_status(project, &id, Status::Open, note.as_deref(), actor, force)?
        }
        TaskMutation::Dep {
            command: DepCommand::Add { id, blocked_by },
        } => store.add_dependency(project, &id, &blocked_by, actor)?,
        TaskMutation::Dep {
            command: DepCommand::Remove { id, blocked_by },
        } => store.remove_dependency(project, &id, &blocked_by, actor)?,
    };
    Ok(Some(task))
}

fn owner(actor: Actor<'_>) -> Result<Owner<'_>> {
    actor
        .name
        .context("--owner is required: name the session or agent acting on the task")
}

/// Linked worktrees share the main checkout, so they share one task list.
fn main_checkout(directory: &Path) -> Result<String> {
    let output = Command::new("git")
        .args(["rev-parse", "--path-format=absolute", "--git-common-dir"])
        .current_dir(directory)
        .output()
        .context("run git to resolve the project")?;
    if !output.status.success() {
        bail!(
            "{} is not inside a Git repository; pass --project KEY to choose a task list",
            directory.display()
        );
    }
    let common = String::from_utf8(output.stdout).context("git printed a non-UTF-8 path")?;
    let common = Path::new(common.trim_end());
    let root = common.parent().context("git common dir has no parent")?;
    let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    root.to_str()
        .map(ToOwned::to_owned)
        .context("checkout path is not Unicode")
}

fn text(outcome: &Outcome, now: i64) -> String {
    match outcome {
        Outcome::Events { events } if events.is_empty() => "No events.\n".to_owned(),
        Outcome::Events { events } => events.iter().map(event_line).collect(),
        Outcome::Tasks { tasks } if tasks.is_empty() => "No matching tasks.\n".to_owned(),
        Outcome::Tasks { tasks } => tasks.iter().map(|task| line(&task.task, now)).collect(),
        Outcome::Task {
            task,
            events,
            warnings,
        } => {
            let not_ready = task.not_ready_because.as_deref();
            let task = &task.task;
            let mut out = line(task, now);
            for warning in warnings {
                let _ = writeln!(out, "Warning: {warning}");
            }
            if let Some(reason) = not_ready
                && task.status == Status::Open
            {
                let _ = writeln!(out, "Not ready: {reason}");
            }
            if let Some(parent) = &task.parent {
                let _ = writeln!(out, "Parent: {parent}");
            }
            if task.children > 0 {
                let _ = writeln!(out, "Children: {}", task.children);
            }
            if !task.blocked_by.is_empty() {
                let _ = writeln!(out, "Blocked by: {}", task.blocked_by.join(", "));
            }
            if let Some(pr) = &task.pr {
                let _ = writeln!(out, "PR: {pr}");
            }
            if let Some(branch) = &task.branch {
                let _ = writeln!(out, "Branch: {branch}");
            }
            let closed: Vec<_> = task
                .prerequisites
                .iter()
                .filter(|dep| task.blocked_by.iter().all(|open| *open != dep.id))
                .map(|dep| dep.id.as_str())
                .collect();
            if !closed.is_empty() {
                let _ = writeln!(out, "Satisfied prerequisites: {}", closed.join(", "));
            }
            if let Some(body) = &task.body {
                let _ = writeln!(out, "\n{body}\n");
            }
            for event in events {
                out.push_str(&event_line(event));
            }
            out
        }
    }
}

fn event_line(event: &dots_tasks::Event) -> String {
    let mut out = format!(
        "{} #{} {} {}",
        stamp(event.at),
        event.seq,
        event.task,
        event.kind
    );
    if let Some(actor) = &event.actor {
        let _ = write!(out, " by {actor}");
    }
    if let Some(detail) = &event.detail {
        let _ = write!(out, ": {detail}");
    }
    out.push('\n');
    out
}

fn line(task: &Task, now: i64) -> String {
    let mut out = format!(
        "{}  {:<7} p{:<3} {}",
        task.id,
        task.status.as_str(),
        task.priority,
        task.title
    );
    match (&task.owner, task.lease_until) {
        (Some(owner), Some(until)) if until > now => {
            let _ = write!(out, "  [{owner} until {}]", stamp(until));
        }
        (Some(owner), _) => {
            let _ = write!(out, "  [{owner}, lease expired]");
        }
        _ => {}
    }
    if task.status == Status::Open && !task.blocked_by.is_empty() {
        out.push_str("  (blocked)");
    }
    if task.children > 0 {
        let _ = write!(
            out,
            "  [{}/{} children unclosed]",
            task.unclosed_children, task.children
        );
    }
    out.push('\n');
    out
}

fn stamp(unix: i64) -> String {
    u64::try_from(unix).map_or_else(|_| unix.to_string(), crate::render::observed_time)
}
