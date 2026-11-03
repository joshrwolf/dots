//! `dots task`: project resolution, storage location, and presentation.
use std::fmt::Write as _;
use std::io;
use std::path::Path;
use std::time::Duration;

use crate::goal_brief::{self, Overview, ShownGoal};
use anyhow::{Context as _, Result};
use dots_tasks::{
    Actor, EventKind, ExternalReference, Filter, Follower, NewTask, Owner, Project, Status, Store,
    Task, Update,
};
use time::OffsetDateTime;
use unicode_segmentation::UnicodeSegmentation as _;

use crate::command::{
    DepCommand, GoalCommand, TaskCommand, TaskMutation, TaskQuery, TaskReferenceCommand,
};

pub(crate) struct Request<'request> {
    pub command: TaskCommand,
    pub project: Option<String>,
    pub owner: Option<&'request str>,
    pub directory: &'request Path,
    pub json: bool,
    /// Where `events --follow` streams; other commands return their output.
    pub out: &'request mut dyn io::Write,
}

pub(crate) fn run(request: Request<'_>) -> Result<String> {
    if let TaskCommand::Query(TaskQuery::EventKinds) = request.command {
        return Ok(event_kinds(request.json));
    }
    let mut store = crate::project::open()?;
    let now = OffsetDateTime::now_utc().unix_timestamp();
    let session_owner = crate::agent::session();
    let actor = Actor {
        name: request
            .owner
            .or(session_owner.as_deref())
            .map(Owner::try_new)
            .transpose()?,
        now,
    };
    // Owners are unique per session, so releasing them needs no project.
    if let TaskCommand::Mutation(TaskMutation::Release { id: None, .. }) = request.command {
        let released = store.release_all(owner(actor)?, now)?;
        return Ok(if request.json {
            crate::json::render(serde_json::json!({ "released": released }))
        } else {
            format!("Released {}.\n", plural(released.len(), "claim"))
        });
    }
    let project =
        crate::project::resolve(&mut store, request.directory, request.project.as_deref())?;
    if let TaskCommand::Query(TaskQuery::Events {
        since,
        follow: true,
        ..
    }) = request.command
    {
        drop(store);
        crate::stream::exit_on_signals()?;
        let mut follower = Follower::open(&crate::project::catalog_path()?, project, since)?;
        follow(
            &mut follower,
            request.json,
            request.out,
            &mut crate::stream::sleep_while_read,
        )?;
        return Ok(String::new());
    }
    let outcome = execute(&mut store, &project, request.command, actor)?;
    let discovery_command = if let Outcome::Task { task, .. } = &outcome {
        Some(format!(
            "dots output list --task {} --project {}",
            task.task.id,
            project.as_str()
        ))
    } else {
        None
    };
    if request.json {
        let mut value = serde_json::to_value(&outcome)?;
        if let serde_json::Value::Object(fields) = &mut value {
            fields.insert("project".into(), serde_json::to_value(&project)?);
            fields.insert("now".into(), now.into());
            if let Some(command) = discovery_command {
                fields.insert("discovery_command".into(), command.into());
            }
        }
        Ok(crate::json::render(value))
    } else {
        let mut out = text(&outcome, now);
        if let Some(command) = discovery_command {
            let _ = writeln!(out, "Reports: {command}");
        }
        Ok(out)
    }
}

/// `event-kinds`: the vocabulary of event kinds, for callers that validate
/// the kinds they wait for.
fn event_kinds(json: bool) -> String {
    let kinds = EventKind::ALL.map(EventKind::as_str);
    if json {
        crate::json::render(serde_json::json!({ "kinds": kinds }))
    } else {
        kinds.iter().flat_map(|kind| [kind, "\n"]).collect()
    }
}

/// `events --follow`: print each event after the follower's cursor as it
/// commits, one per line, until the reader goes away or storage fails.
fn follow(
    follower: &mut Follower,
    json: bool,
    out: &mut dyn io::Write,
    sleep: &mut dyn FnMut(Duration) -> bool,
) -> Result<()> {
    loop {
        let events = follower.poll()?;
        for event in &events {
            let line = if json {
                format!("{}\n", serde_json::to_string(event)?)
            } else {
                event_line(event)
            };
            if !crate::stream::write(out, &line)? {
                return Ok(());
            }
        }
        if events.is_empty() && !sleep(dots_tasks::FOLLOW_INTERVAL) {
            return Ok(());
        }
    }
}

fn plural(count: usize, noun: &str) -> String {
    if count == 1 {
        format!("1 {noun}")
    } else {
        format!("{count} {noun}s")
    }
}

#[derive(Debug, serde::Serialize)]
#[serde(untagged)]
enum Outcome {
    ResultEvent {
        event: dots_tasks::outcomes::ResultEvent,
    },
    Result {
        #[serde(flatten)]
        receipt: Box<dots_tasks::outcomes::OutcomeReceipt>,
    },
    Goal {
        goal: Box<ShownGoal>,
        follow_hint: &'static str,
    },
    Goals {
        goals: Vec<ShownGoal>,
    },
    Overview {
        overview: Box<Overview>,
    },
    Detached {
        detached: String,
    },
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

/// A task plus its derived readiness.
#[derive(Debug, serde::Serialize)]
struct Shown {
    #[serde(flatten)]
    task: Task,
    ready: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    not_ready_because: Option<String>,
}

impl Shown {
    fn new(task: Task) -> Self {
        let not_ready_because = task.not_ready_because();
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
    let shown = |tasks: Vec<Task>| tasks.into_iter().map(Shown::new).collect();
    match command {
        TaskCommand::Goal { command } => execute_goal(store, project, command, actor),
        TaskCommand::Query(TaskQuery::List {
            parent,
            claimed_by,
            roots,
            flat,
            status,
            all,
        }) => {
            let owner = claimed_by.as_deref().map(Owner::try_new).transpose()?;
            let status = status
                .map(|value| {
                    Status::parse(&value)
                        .context("status must be open, review, held, done or dropped")
                })
                .transpose()?;
            let filter = Filter {
                owner,
                parent: parent.as_deref(),
                roots: roots || (parent.is_none() && !flat && owner.is_none()),
                status,
                include_closed: all,
            };
            Ok(Outcome::Tasks {
                tasks: shown(store.list(project, &filter)?),
            })
        }
        TaskCommand::Query(TaskQuery::Ready { parent }) => Ok(Outcome::Tasks {
            tasks: shown(store.ready(project, parent.as_deref())?),
        }),
        TaskCommand::Query(TaskQuery::ResultEvent { id, event }) => Ok(Outcome::ResultEvent {
            event: store.result_event(project, &id, event)?,
        }),
        TaskCommand::Query(TaskQuery::Show { id }) => {
            let task = Box::new(Shown::new(store.get(project, &id)?));
            let events = store.events(project, &id, 10)?;
            Ok(Outcome::Task {
                task,
                events,
                warnings: Vec::new(),
            })
        }
        TaskCommand::Query(TaskQuery::Events {
            since,
            limit,
            follow: _,
        }) => Ok(Outcome::Events {
            events: store.events_since(project, since, usize::from(limit))?,
        }),
        TaskCommand::Query(TaskQuery::EventKinds) => {
            anyhow::bail!("event-kinds is handled before opening the catalog")
        }
        TaskCommand::Mutation(mutation) => {
            let adds_dependency = matches!(
                mutation,
                TaskMutation::Dep {
                    command: DepCommand::Add { .. }
                }
            );
            let holds = matches!(mutation, TaskMutation::Hold { .. });
            let task = match mutate(store, project, mutation, actor)? {
                MutationResult::Task(task) => task.context("no ready task to claim")?,
                MutationResult::Result(receipt) => {
                    return Ok(Outcome::Result {
                        receipt: Box::new(receipt),
                    });
                }
            };
            let mut warnings = Vec::new();
            if adds_dependency && let Some(owner) = &task.owner {
                warnings.push(format!(
                    "{} is claimed by {owner} and is now blocked; tell that session",
                    task.id
                ));
            }
            if holds && task.children > 0 {
                let below = store.unclosed_descendants(project, &task.id)?;
                if below > 0 {
                    warnings.push(format!(
                        "{below} unclosed descendants stay claimable; hold them individually to pause the goal"
                    ));
                }
            }
            Ok(Outcome::Task {
                task: Box::new(Shown::new(task)),
                events: Vec::new(),
                warnings,
            })
        }
    }
}

enum MutationResult {
    Task(Option<Task>),
    Result(dots_tasks::outcomes::OutcomeReceipt),
}

#[expect(
    clippy::too_many_lines,
    reason = "flat command dispatch keeps mutation variants and their arguments together"
)]
fn mutate(
    store: &mut Store,
    project: &Project,
    command: TaskMutation,
    actor: Actor<'_>,
) -> Result<MutationResult> {
    let now = actor.now;
    let task = match command {
        TaskMutation::Add {
            title,
            body,
            parent,
            blocked_by,
        } => {
            let task = NewTask {
                title,
                body,
                parent,
                blocked_by,
            };
            store.add(project, &task, actor)?
        }
        TaskMutation::Claim {
            id,
            next: _,
            parent,
            take,
        } => {
            let owner = owner(actor)?;
            return Ok(MutationResult::Task(match id {
                Some(id) => Some(store.claim(project, &id, owner, take, now)?),
                None => store.claim_next(project, parent.as_deref(), owner, now)?,
            }));
        }
        TaskMutation::Release { id, all: _ } => {
            let id = id.context("release --all is handled before project resolution")?;
            store.release(project, &id, owner(actor)?, now)?
        }
        TaskMutation::Update {
            id,
            title,
            body,
            result,
            evidence,
            branch,
            parent,
            force,
        } => {
            if let Some(result) = result {
                return write_result(
                    store,
                    project,
                    actor,
                    ResultInput {
                        id: &id,
                        result: &result,
                        evidence: &evidence,
                        operation: dots_tasks::outcomes::Operation::UpdateResult,
                        force,
                    },
                );
            }
            let update = Update {
                title,
                body,
                branch,
                parent: parent.map(|value| (!value.is_empty()).then_some(value)),
            };
            store.update(project, &id, &update, actor, force)?
        }
        TaskMutation::Review {
            id,
            result,
            evidence,
            force,
        } => {
            return write_result(
                store,
                project,
                actor,
                ResultInput {
                    id: &id,
                    result: &result,
                    evidence: &evidence,
                    operation: dots_tasks::outcomes::Operation::Review,
                    force,
                },
            );
        }
        TaskMutation::Done {
            id,
            result,
            evidence,
            force,
        } => {
            return write_result(
                store,
                project,
                actor,
                ResultInput {
                    id: &id,
                    result: &result,
                    evidence: &evidence,
                    operation: dots_tasks::outcomes::Operation::Done,
                    force,
                },
            );
        }
        TaskMutation::Ref { command } => match command {
            TaskReferenceCommand::Set {
                id,
                url,
                label,
                force,
            } => {
                let reference = ExternalReference { url, label };
                store.set_reference(project, &id, &reference, actor, force)?
            }
            TaskReferenceCommand::Remove { id, url, force } => {
                store.remove_reference(project, &id, &url, actor, force)?
            }
        },
        TaskMutation::Hold { id, reason, force } => {
            store.set_status(project, &id, Status::Held, Some(&reason), actor, force)?
        }
        TaskMutation::Drop { id, reason, force } => {
            store.set_status(project, &id, Status::Dropped, Some(&reason), actor, force)?
        }
        TaskMutation::Reopen { id, reason, force } => {
            store.set_status(project, &id, Status::Open, reason.as_deref(), actor, force)?
        }
        TaskMutation::Dep {
            command: DepCommand::Add { id, blocked_by },
        } => store.add_dependency(project, &id, &blocked_by, actor)?,
        TaskMutation::Dep {
            command: DepCommand::Remove { id, blocked_by },
        } => store.remove_dependency(project, &id, &blocked_by, actor)?,
    };
    Ok(MutationResult::Task(Some(task)))
}

#[derive(Clone, Copy)]
struct ResultInput<'input> {
    id: &'input str,
    result: &'input str,
    evidence: &'input [String],
    operation: dots_tasks::outcomes::Operation,
    force: bool,
}

fn write_result(
    store: &mut Store,
    project: &Project,
    actor: Actor<'_>,
    input: ResultInput<'_>,
) -> Result<MutationResult> {
    let evidence = input
        .evidence
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| dots_tasks::outcomes::Evidence {
            slug: pair[0].clone(),
            digest: pair[1].clone(),
        })
        .collect::<Vec<_>>();
    let outcome = dots_tasks::outcomes::Outcome {
        operation: input.operation,
        result: input.result,
        evidence: &evidence,
        force: input.force,
    };
    Ok(MutationResult::Result(
        store.write_result(project, input.id, &outcome, actor)?,
    ))
}

fn execute_goal(
    store: &mut Store,
    project: &Project,
    command: GoalCommand,
    actor: Actor<'_>,
) -> Result<Outcome> {
    match command {
        GoalCommand::Create { title, brief } => {
            let brief = goal_brief::existing(project, &brief)?;
            let goal = store.create_goal(project, &title, &brief, actor)?;
            Ok(Outcome::Goal {
                goal: Box::new(goal_brief::show(project, goal)?),
                follow_hint: "Created the goal; follow it in Pi with goal_focus.",
            })
        }
        GoalCommand::Attach { id, brief } => {
            let brief = goal_brief::existing(project, &brief)?;
            let goal = store.attach_goal(project, &id, &brief, actor)?;
            Ok(Outcome::Goal {
                goal: Box::new(goal_brief::show(project, goal)?),
                follow_hint: "Attached the brief; follow the goal in Pi with goal_focus.",
            })
        }
        GoalCommand::List { all, claimed_by } => Ok(Outcome::Goals {
            goals: store
                .goals(
                    project,
                    all,
                    claimed_by.as_deref().map(Owner::try_new).transpose()?,
                )?
                .into_iter()
                .map(|goal| goal_brief::reference(project, goal))
                .collect::<Result<_>>()?,
        }),
        GoalCommand::Show { id, all } => Ok(Outcome::Overview {
            overview: Box::new(goal_brief::overview(
                project,
                store.goal_overview(project, &id, all)?,
            )?),
        }),
        GoalCommand::Detach { id } => {
            store.detach_goal(project, &id, actor)?;
            Ok(Outcome::Detached { detached: id })
        }
    }
}

fn owner(actor: Actor<'_>) -> Result<Owner<'_>> {
    actor
        .name
        .context("No session identity available; pass --owner to name the session or agent acting on the task")
}

fn text(outcome: &Outcome, now: i64) -> String {
    match outcome {
        Outcome::ResultEvent { event } => format!(
            "Result event {} ({})\n{}\n{}",
            event.seq,
            event.operation,
            event.result,
            evidence_text(&event.evidence, &event.purged_evidence, false)
        ),
        Outcome::Result { receipt } => format!(
            "{}Recorded result event {}\n{}",
            line(&receipt.task, now),
            receipt.event.seq,
            evidence_text(
                &receipt.event.evidence,
                &receipt.event.purged_evidence,
                true
            )
        ),
        Outcome::Goal { goal, follow_hint } => format!("{}{follow_hint}\n", goal_text(goal, now)),
        Outcome::Goals { goals } if goals.is_empty() => "No matching goals.\n".into(),
        Outcome::Goals { goals } => goals.iter().map(|goal| goal_text(goal, now)).collect(),
        Outcome::Detached { detached } => {
            format!("Detached the brief from {detached}; its tasks and document remain.\n")
        }
        Outcome::Overview { overview } => overview_text(overview, now),
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
            for reference in &task.references {
                let _ = writeln!(
                    out,
                    "Reference: {}{}",
                    reference
                        .label
                        .as_deref()
                        .map(|label| format!("{label} "))
                        .unwrap_or_default(),
                    reference.url
                );
            }
            if let Some(body) = &task.body {
                let _ = writeln!(out, "Description:\n{body}");
            }
            if let Some(result) = &task.result {
                let _ = writeln!(out, "Result:\n{result}");
            }
            out.push_str(&evidence_text(&task.evidence, &task.purged_evidence, false));
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
            if !events.is_empty() {
                out.push_str("History:\n");
            }
            for event in events {
                out.push_str(&event_line(event));
            }
            out
        }
    }
}

fn evidence_text(
    evidence: &[dots_tasks::outcomes::Evidence],
    purged: &[dots_tasks::outcomes::Evidence],
    verified: bool,
) -> String {
    let mut out = String::new();
    for item in evidence {
        let state = if purged.contains(item) {
            " (purged)"
        } else if verified {
            " (verified)"
        } else {
            ""
        };
        let _ = writeln!(out, "Evidence: {} {}{state}", item.slug, item.digest);
    }
    out
}

fn overview_text(overview: &Overview, now: i64) -> String {
    let mut out = goal_text(&overview.goal, now);
    if overview.hidden_closed > 0 {
        let _ = writeln!(
            out,
            "{} closed tasks hidden; add --all to show them.",
            overview.hidden_closed
        );
    }
    for task in &overview.tasks {
        out.push_str(&line(task, now));
        if let Some(branch) = task.branch.as_deref().filter(|value| !value.is_empty()) {
            let _ = writeln!(out, "  Branch: {}", preview(branch));
        }
        if let Some(result) = task.result.as_deref().filter(|value| !value.is_empty()) {
            let _ = writeln!(out, "  Result: {}", preview(result));
        }
    }
    if overview.tasks.iter().any(|task| task.result.is_some()) {
        out.push_str("dots task show ID shows a task's full result and history.\n");
    }
    out
}

/// First line within a character budget, cut only between grapheme clusters.
fn preview(value: &str) -> String {
    const LIMIT: usize = 160;
    let (first, multiline) = value
        .split_once(['\r', '\n'])
        .map_or((value, false), |(first, _)| (first, true));
    let mut output = String::new();
    let mut used = 0;
    let mut shortened = multiline;
    for cluster in first.graphemes(true) {
        let width = cluster.chars().count();
        if used + width > LIMIT {
            shortened = true;
            break;
        }
        output.push_str(cluster);
        used += width;
    }
    if shortened {
        output.push('…');
    }
    output
}

fn goal_text(goal: &ShownGoal, now: i64) -> String {
    let p = &goal.progress;
    let counts = [
        (p.open, "open"),
        (p.review, "in review"),
        (p.held, "held"),
        (p.done, "done"),
        (p.dropped, "dropped"),
        (p.claimed, "claimed"),
    ]
    .iter()
    .filter(|(count, _)| *count > 0)
    .map(|(count, label)| format!("{count} {label}"))
    .collect::<Vec<_>>();
    let counts = if counts.is_empty() {
        "no tasks".to_owned()
    } else {
        counts.join(" · ")
    };
    format!(
        "{}  {counts} · last activity {} ago\nBrief: {}\n",
        line(&goal.root, now),
        age(now - goal.last_activity),
        goal.brief.text()
    )
}

fn age(seconds: i64) -> String {
    match seconds {
        ..3600 => "<1h".into(),
        3600..86_400 => format!("{}h", seconds / 3600),
        _ => format!("{}d", seconds / 86_400),
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
    // Results can be long; history points at them instead of repeating them.
    if event.result.is_some() {
        let _ = write!(
            out,
            ": result in dots task result-event {} --event {}",
            event.task, event.seq
        );
    } else if let Some(detail) = &event.detail {
        let _ = write!(out, ": {detail}");
    }
    out.push('\n');
    out
}

fn line(task: &Task, now: i64) -> String {
    let mut out = format!("{}  {:<7} {}", task.id, task.status.as_str(), task.title);
    if let Some(owner) = &task.owner {
        let _ = write!(out, "  [claimed by {owner}]");
    }
    if task.status == Status::Review {
        let _ = write!(out, "  (in review {})", age(now - task.updated_at));
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

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;

    struct Catalog {
        root: PathBuf,
        store: Store,
        project: Project,
    }

    impl Catalog {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let root = std::env::temp_dir().join(format!(
                "dots-task-follow-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir(&root).unwrap();
            let mut store = Store::create(&root.join("workspace.sqlite3")).unwrap();
            let project = store.register_project("fixture").unwrap();
            Self {
                root,
                store,
                project,
            }
        }

        fn add(&mut self, title: &str) {
            let task = NewTask {
                title: title.into(),
                ..NewTask::default()
            };
            let actor = Actor { name: None, now: 1 };
            self.store.add(&self.project, &task, actor).unwrap();
        }

        fn follower(&self) -> Follower {
            Follower::open(
                &self.root.join("workspace.sqlite3"),
                self.project.clone(),
                0,
            )
            .unwrap()
        }
    }

    impl Drop for Catalog {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.root).unwrap();
        }
    }

    /// Accepts `lines` lines, then reports that its reader went away.
    struct Reader {
        text: String,
        lines: usize,
    }

    impl io::Write for Reader {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            if self.text.matches('\n').count() >= self.lines {
                return Err(io::ErrorKind::BrokenPipe.into());
            }
            self.text.push_str(std::str::from_utf8(bytes).unwrap());
            Ok(bytes.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn follow_prints_the_backlog_then_each_commit_as_one_json_line() {
        let mut catalog = Catalog::new();
        catalog.add("first");
        let mut follower = catalog.follower();
        let mut reader = Reader {
            text: String::new(),
            lines: usize::MAX,
        };
        let mut pauses = Vec::new();
        follow(&mut follower, true, &mut reader, &mut |pause| {
            pauses.push(pause);
            match pauses.len() {
                1 => catalog.add("second"),
                2 => {}
                _ => return false,
            }
            true
        })
        .unwrap();
        let events: Vec<serde_json::Value> = reader
            .text
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        let seqs: Vec<_> = events.iter().map(|event| event["seq"].clone()).collect();
        assert_eq!(seqs, [1, 2]);
        assert!(events.iter().all(|event| event["kind"] == "created"));
        assert_eq!(pauses, [dots_tasks::FOLLOW_INTERVAL; 3]);
    }

    #[test]
    fn follow_prints_readable_lines_and_ends_with_its_reader() {
        let mut catalog = Catalog::new();
        catalog.add("first");
        catalog.add("second");
        let mut reader = Reader {
            text: String::new(),
            lines: 1,
        };
        follow(&mut catalog.follower(), false, &mut reader, &mut |_| {
            panic!("a closed reader ends follow before it waits")
        })
        .unwrap();
        assert_eq!(reader.text.lines().count(), 1);
        assert!(reader.text.contains(" #1 t-"), "{}", reader.text);
        assert!(reader.text.ends_with(" created\n"), "{}", reader.text);
    }

    #[test]
    fn event_kinds_lists_the_store_vocabulary() {
        let listed: serde_json::Value = serde_json::from_str(&event_kinds(true)).unwrap();
        let names: Vec<_> = EventKind::ALL.map(EventKind::as_str).to_vec();
        assert_eq!(listed, serde_json::json!({ "kinds": names }));
        assert_eq!(event_kinds(false), format!("{}\n", names.join("\n")));
    }
}
