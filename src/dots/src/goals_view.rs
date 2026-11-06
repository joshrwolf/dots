//! Read-only goal browsing through the shared picker.
use std::borrow::Cow;
use std::collections::HashMap;
use std::fmt::Write as _;
use std::io::{IsTerminal as _, stdin, stdout};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context as _, Result, ensure};
use dots_pick::spec::{PreviewAnchor, PreviewPosition};
use dots_pick::{Action, Actions, Cell, Choice, Column, Entry, Group, Palette, Picker};
use dots_tasks::attention::{GoalRef, Scope};
use dots_tasks::{Project, Status, Store, Task};

use crate::command::GoalPreview;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Section {
    Inspect,
    Active,
    Closed,
}

#[derive(Debug, Clone)]
struct GoalKey {
    project: Project,
    goal: String,
    title: String,
}

#[derive(Debug, Clone)]
enum Target {
    Goal(GoalKey),
    Task(GoalKey, String),
    Document(GoalKey, GoalPreview),
    Preview,
}

#[derive(Debug, Clone)]
struct Row {
    id: String,
    section: Section,
    glyph: String,
    context: String,
    progress: String,
    title: String,
    preview: Vec<String>,
    target: Target,
}

impl Entry for Row {
    type Group = Section;
    fn group(&self) -> Section {
        self.section
    }
    fn cells(&self, palette: &Palette) -> Vec<Cell> {
        vec![
            Cell::tag(
                &self.glyph,
                match self.glyph.as_str() {
                    "!" => palette.red,
                    "✓" => palette.green,
                    "◇" => palette.accent,
                    _ => palette.muted,
                },
            ),
            Cell::new(&self.context, palette.muted),
            Cell::tag(&self.progress, palette.muted),
            Cell::new(&self.title, palette.strong),
        ]
    }
    fn identity(&self) -> Option<Cow<'_, str>> {
        Some(Cow::Borrowed(&self.id))
    }
    fn preview(&self) -> Option<&[String]> {
        Some(&self.preview)
    }
}

fn groups() -> Vec<Group<Section>> {
    let columns = || {
        vec![
            Column::new(3),
            Column::content(22),
            Column::content(18),
            Column::fill(),
        ]
    };
    vec![
        Group::new(Section::Inspect, "Inspect", columns()),
        Group::new(Section::Active, "Unfinished", columns()),
        Group::new(Section::Closed, "Completed", columns()).scoped('='),
    ]
}

fn picker(rows: Vec<Row>, prompt: String) -> Result<Picker<Row>> {
    Ok(Picker::new(rows)
        .groups(groups())
        .prompt(prompt)
        .palette(Palette::load()?)
        .preview_position(PreviewPosition::Below)
        .preview_anchor(PreviewAnchor::Start)
        .preview_every(Duration::from_secs(2)))
}

fn command(bin: &str, key: &GoalKey, page: GoalPreview) -> Vec<String> {
    vec![
        bin.into(),
        "task".into(),
        "goal".into(),
        "preview".into(),
        key.goal.clone(),
        "--project".into(),
        key.project.as_str().into(),
        "--page".into(),
        page.name().into(),
    ]
}

fn task_state(task: &Task) -> &'static str {
    if task.status == Status::Open {
        if task.owner.is_some() {
            return "claimed";
        }
        if !task.holds.is_empty() {
            return "held";
        }
        if !task.blocked_by.is_empty() {
            return "waiting";
        }
    }
    task.status.as_str()
}

fn glyph(status: Status) -> &'static str {
    match status {
        Status::Open => "○",
        Status::Review => "◇",
        Status::Held => "!",
        Status::Done => "✓",
        Status::Dropped => "−",
    }
}

fn roots(store: &mut Store, bin: &str) -> Result<Vec<Row>> {
    let mut rows = Vec::new();
    for project in store.registered_projects()? {
        for goal in store.goals(&project.id, true, None)? {
            let key = GoalKey {
                project: project.id.clone(),
                goal: goal.root.id,
                title: goal.root.title,
            };
            rows.push(Row {
                id: format!("{}:{}", key.project.as_str(), key.goal),
                section: if goal.root.status.is_closed() {
                    Section::Closed
                } else {
                    Section::Active
                },
                glyph: glyph(goal.root.status).into(),
                context: project.name.clone(),
                progress: format!("{}/{} done", goal.progress.done, goal.progress.total),
                title: key.title.clone(),
                preview: command(bin, &key, GoalPreview::Graph),
                target: Target::Goal(key),
            });
        }
    }
    Ok(rows)
}

fn root_picker(bin: String) -> Result<Picker<Row>> {
    let rows = roots(&mut crate::project::open()?, &bin)?;
    Ok(picker(rows, "Goals > ".into())?.refresh_every(
        Duration::from_secs(2),
        Arc::new(move || {
            let bin = bin.clone();
            Box::pin(async move {
                roots(
                    &mut crate::project::open().map_err(|error| error.to_string())?,
                    &bin,
                )
                .map_err(|error| error.to_string())
            })
        }),
    ))
}

fn documents(bin: &str, key: &GoalKey) -> Vec<Row> {
    [
        GoalPreview::Graph,
        GoalPreview::Brief,
        GoalPreview::Decisions,
        GoalPreview::History,
        GoalPreview::Record,
    ]
    .into_iter()
    .map(|page| Row {
        id: format!("{}:{}:{}", key.project.as_str(), key.goal, page.name()),
        section: Section::Inspect,
        glyph: "·".into(),
        context: "Inspect".into(),
        progress: String::new(),
        title: page.label().into(),
        preview: command(bin, key, page),
        target: Target::Document(key.clone(), page),
    })
    .collect()
}

fn task_rows(store: &mut Store, bin: &str, key: &GoalKey, parent: &str) -> Result<Vec<Row>> {
    let overview = store.goal_overview(&key.project, &key.goal, true)?;
    let mut rows = documents(bin, key);
    ensure!(
        parent == key.goal || overview.tasks.iter().any(|task| task.id == parent),
        "Task group is no longer under this goal"
    );
    for task in overview
        .tasks
        .into_iter()
        .filter(|task| task.parent.as_deref() == Some(parent))
    {
        let state = task_state(&task);
        rows.push(Row {
            id: format!("{}:{}", key.project.as_str(), task.id),
            section: if task.status.is_closed() {
                Section::Closed
            } else {
                Section::Active
            },
            glyph: glyph(task.status).into(),
            context: state.into(),
            progress: task.owner.clone().unwrap_or_default(),
            title: task.title,
            preview: {
                let mut argv = command(bin, key, GoalPreview::Task);
                argv.extend(["--task".into(), task.id.clone()]);
                argv
            },
            target: Target::Task(key.clone(), task.id),
        });
    }
    Ok(rows)
}

fn task_picker(bin: String, key: GoalKey, parent: String, title: String) -> Result<Picker<Row>> {
    let rows = task_rows(&mut crate::project::open()?, &bin, &key, &parent)?;
    Ok(picker(rows, title + " > ")?.refresh_every(
        Duration::from_secs(2),
        Arc::new(move || {
            let bin = bin.clone();
            let key = key.clone();
            let parent = parent.clone();
            Box::pin(async move {
                task_rows(
                    &mut crate::project::open().map_err(|error| error.to_string())?,
                    &bin,
                    &key,
                    &parent,
                )
                .map_err(|error| error.to_string())
            })
        }),
    ))
}

fn detail_picker(mut row: Row, title: String) -> Result<Picker<Row>> {
    row.target = Target::Preview;
    let selected = row.id.clone();
    Ok(picker(vec![row], title)?.selected(selected))
}

fn open(row: Row, bin: String) -> Result<Action<Row>> {
    let page = match row.target.clone() {
        Target::Preview => return Ok(Action::Stay),
        Target::Goal(key) => task_picker(bin, key.clone(), key.goal.clone(), key.title.clone())?,
        Target::Task(key, id) => {
            let task = crate::project::open()?
                .goal_overview(&key.project, &key.goal, true)?
                .tasks
                .into_iter()
                .find(|task| task.id == id)
                .context("Task is no longer under this goal")?;
            if task.children > 0 {
                task_picker(bin, key, task.id, task.title)?
            } else {
                detail_picker(row, "Task > ".into())?
            }
        }
        Target::Document(key, page) => {
            detail_picker(row, format!("{} · {} > ", key.title, page.label()))?
        }
    };
    Ok(Action::Push(page))
}

pub(crate) fn run(select: Option<GoalRef>, dismiss: Option<String>, json: bool) -> Result<String> {
    ensure!(
        !json && stdin().is_terminal() && stdout().is_terminal(),
        "goal view needs an interactive terminal"
    );
    let bin = std::env::current_exe()?
        .into_os_string()
        .into_string()
        .map_err(|_| anyhow::anyhow!("dots executable path must be Unicode"))?;
    let mut view = root_picker(bin.clone())?;
    if let Some(selected) = select {
        view = view.selected(format!("{}:{}", selected.project.as_str(), selected.goal));
    }
    let actions = Actions::<Row, ()> {
        shortcuts: Vec::new(),
        act: Box::new(move |choice| {
            let Choice::Accept(row) = choice else {
                unreachable!("no shortcuts declared")
            };
            let row = row.clone();
            let bin = bin.clone();
            Box::pin(async move { open(row, bin).map_err(|error| format!("{error:#}")) })
        }),
    };
    view.run_actions(&actions)?;
    if let Some(action) = dismiss {
        crate::attention_view::run_dismiss(&action)?;
    }
    Ok(String::new())
}

fn graph_preview(overview: &dots_tasks::GoalOverview, project_name: &str) -> String {
    let mut out = format!(
        "{}\n{} · {} · {}/{} done · Graph\n\n",
        overview.goal.root.title,
        project_name,
        overview.goal.root.status.as_str(),
        overview.goal.progress.done,
        overview.goal.progress.total
    );
    if overview.goal.root.owner.is_some() {
        out.push_str("Coordinator recorded (not a liveness check).\n\n");
    }
    let tasks: HashMap<_, _> = overview
        .tasks
        .iter()
        .map(|task| (task.id.as_str(), task))
        .collect();
    let graph = crate::goal_brief::graph(&overview.tasks, false);
    for row in graph.rows {
        let task = tasks
            .get(row.task.as_str())
            .expect("graph references supplied tasks");
        for (index, line) in row.lines.iter().enumerate() {
            if index == 0 {
                let _ = writeln!(out, "{line} {:<11} {}", task_state(task), task.title);
            } else {
                let _ = writeln!(out, "{line}");
            }
        }
    }
    for note in graph.notes {
        let _ = writeln!(out, "{note}");
    }
    if overview.tasks.is_empty() {
        out.push_str("No tasks.\n");
    }
    out
}

#[expect(
    clippy::too_many_lines,
    reason = "One read boundary validates goal membership and formats its inspection pages."
)]
pub(crate) fn preview(
    store: &mut Store,
    project: &Project,
    goal: &str,
    page: GoalPreview,
    task: Option<&str>,
) -> Result<String> {
    ensure!(
        task.is_none() || page == GoalPreview::Task,
        "--task is only valid with --page task"
    );
    let overview = store.goal_overview(project, goal, true)?;
    let text: Result<String> = match page {
        GoalPreview::Task => {
            let id = task.context("Task page requires --task")?;
            ensure!(
                overview.tasks.iter().any(|task| task.id == id),
                "Task is no longer under the selected goal"
            );
            crate::tasks::inspect_task(store, project, id)
        }
        GoalPreview::Graph => {
            let project_name = store
                .registered_projects()?
                .into_iter()
                .find(|registered| registered.id == *project)
                .context("Project is no longer registered")?
                .name;
            Ok(graph_preview(&overview, &project_name))
        }
        GoalPreview::Brief => {
            let shown = crate::goal_brief::show(project, overview.goal)?;
            if let Some(error) = shown.brief.diagnostic {
                anyhow::bail!("{error}");
            }
            let document = shown.brief.document.context("Brief was not read")?;
            let mut text = document.content;
            if let Some(line) = document.next_line {
                let _ = writeln!(
                    text,
                    "\nBrief continues at line {line}: {}",
                    document.path.display()
                );
            }
            Ok(text)
        }
        GoalPreview::Decisions => {
            let list = store.decisions(
                &Scope::Goals(vec![GoalRef {
                    project: project.clone(),
                    goal: goal.into(),
                }]),
                None,
            )?;
            ensure!(
                list.errors.is_empty(),
                "Decisions are unavailable: {:?}",
                list.errors
            );
            let text = list
                .goals
                .iter()
                .flat_map(|goal| &goal.decisions)
                .map(crate::attention::decision_text)
                .collect::<Vec<_>>()
                .join("\n\n");
            Ok(if text.is_empty() {
                "No decisions recorded.".into()
            } else {
                text
            })
        }
        GoalPreview::History => {
            let events = store.history(project, Some(goal), 0, 101)?;
            let mut text = events
                .events
                .iter()
                .take(100)
                .map(|event| {
                    format!(
                        "#{} · {} · {}\n{}",
                        event.seq,
                        event.kind,
                        event.actor.as_deref().unwrap_or("unattributed"),
                        event
                            .result
                            .as_deref()
                            .or(event.detail.as_deref())
                            .unwrap_or("")
                    )
                })
                .collect::<Vec<_>>()
                .join("\n\n");
            if events.events.len() > 100 {
                let _ = writeln!(
                    text,
                    "\nMore history: dots task events --project {} --under {} --since {}",
                    project.as_str(),
                    goal,
                    events.events[99].seq
                );
            }
            Ok(text)
        }
        GoalPreview::Record => crate::tasks::inspect_task(store, project, goal),
    };
    let text = text?;
    if page == GoalPreview::Graph {
        return Ok(text);
    }
    let columns = std::env::var("COLUMNS")
        .ok()
        .map(|value| value.parse::<usize>())
        .transpose()?
        .unwrap_or(96)
        .clamp(1, 96);
    Ok(crate::terminal_text::wrap(&text, columns).join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use dots_tasks::{Actor, NewTask, Owner};

    #[test]
    fn an_open_detail_does_not_add_another_page_on_enter() {
        let key = GoalKey {
            project: Project::try_new("p-00000000000000000000000000000000").unwrap(),
            goal: "goal".into(),
            title: "Fixture".into(),
        };
        for section in [Section::Inspect, Section::Closed] {
            let mut row = documents("dots", &key).remove(0);
            row.section = section;
            let model = detail_picker(row, "Detail > ".into())
                .unwrap()
                .build()
                .unwrap();
            let row = model.selected_entry().expect("exact detail stays visible");
            assert!(matches!(row.target, Target::Preview));
            assert!(matches!(
                open(row.clone(), "unused".into()).unwrap(),
                Action::Stay
            ));
        }
    }

    struct Home(std::path::PathBuf);
    impl Home {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "dots-goal-picker-{}-{}",
                std::process::id(),
                time::OffsetDateTime::now_utc().unix_timestamp_nanos()
            ));
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Home {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    fn actor() -> Actor<'static> {
        Actor {
            name: Some(Owner::try_new("fixture-coordinator").unwrap()),
            now: 1,
        }
    }
    fn add(
        store: &mut Store,
        project: &Project,
        title: &str,
        parent: &str,
        blocked_by: Vec<String>,
    ) -> Task {
        store
            .add(
                project,
                &NewTask {
                    title: title.into(),
                    parent: Some(parent.into()),
                    blocked_by,
                    ..NewTask::default()
                },
                actor(),
            )
            .unwrap()
    }

    #[test]
    fn graph_preview_keeps_branches_joins_and_task_state_and_rows_inspect_groups() {
        let home = Home::new();
        let mut store = Store::create(&home.0.join("catalog.sqlite3")).unwrap();
        let project = store.register_project("fixture").unwrap();
        let goal = store
            .create_goal(&project, "Unify clients", "briefs/goal.md", actor())
            .unwrap();
        store
            .coordinate(
                &project,
                &goal.root.id,
                actor().name.unwrap(),
                false,
                false,
                1,
            )
            .unwrap();
        let key = GoalKey {
            project: project.clone(),
            goal: goal.root.id,
            title: goal.root.title,
        };
        let base = add(&mut store, &project, "Define contract", &key.goal, vec![]);
        let left = add(
            &mut store,
            &project,
            "Adapt command client",
            &key.goal,
            vec![base.id.clone()],
        );
        let right = add(
            &mut store,
            &project,
            "Adapt library client",
            &key.goal,
            vec![base.id],
        );
        add(
            &mut store,
            &project,
            "Verify clients",
            &key.goal,
            vec![left.id, right.id],
        );
        let graph = preview(&mut store, &project, &key.goal, GoalPreview::Graph, None).unwrap();
        for title in [
            "Define contract",
            "Adapt command client",
            "Adapt library client",
            "Verify clients",
        ] {
            assert!(graph.contains(title), "{graph}");
        }
        assert!(
            graph.contains('╮') && graph.contains('╯'),
            "branches and joins: {graph}"
        );
        assert!(graph.contains("waiting"), "{graph}");
        assert!(!graph.contains("running"), "claims are not liveness");
        let group = add(&mut store, &project, "Shared API", &key.goal, vec![]);
        let child = add(&mut store, &project, "Document contract", &group.id, vec![]);
        let top = task_rows(&mut store, "dots", &key, &key.goal).unwrap();
        assert!(top.iter().any(|row| row.title == "Shared API"));
        assert!(!top.iter().any(|row| row.title == child.title));
        let below = task_rows(&mut store, "dots", &key, &group.id).unwrap();
        assert!(below.iter().any(|row| row.title == child.title));
        assert!(below.iter().any(|row| row.title == "Brief"));
        assert!(
            preview(
                &mut store,
                &project,
                &key.goal,
                GoalPreview::Task,
                Some("t-absent")
            )
            .is_err()
        );
        assert!(
            preview(
                &mut store,
                &project,
                &key.goal,
                GoalPreview::Graph,
                Some(&child.id)
            )
            .is_err()
        );
    }

    #[test]
    fn all_goal_rows_include_drafts_and_completed_work_without_claiming_and_preselect_exactly() {
        let home = Home::new();
        let mut store = Store::create(&home.0.join("catalog.sqlite3")).unwrap();
        let project = store.register_project("fixture").unwrap();
        let draft = store
            .create_goal(&project, "Draft goal", "briefs/goal.md", actor())
            .unwrap();
        let done = store
            .create_goal(&project, "Completed goal", "briefs/done.md", actor())
            .unwrap();
        store
            .coordinate(
                &project,
                &done.root.id,
                actor().name.unwrap(),
                false,
                false,
                1,
            )
            .unwrap();
        store
            .write_result(
                &project,
                &done.root.id,
                &dots_tasks::outcomes::Outcome {
                    operation: dots_tasks::outcomes::Operation::Done,
                    result: "Verified",
                    evidence: &[],
                    force: false,
                },
                actor(),
            )
            .unwrap();
        let rows = roots(&mut store, "dots").unwrap();
        assert_eq!(rows.len(), 2);
        assert!(
            rows.iter()
                .any(|row| row.title == "Completed goal" && row.section == Section::Closed)
        );
        let wanted = format!("{}:{}", project.as_str(), done.root.id);
        let model = picker(rows.clone(), "Goals > ".into())
            .unwrap()
            .selected(&wanted)
            .build()
            .unwrap();
        assert_eq!(model.selected_entry().unwrap().title, "Completed goal");
        assert!(
            picker(rows, "Goals > ".into())
                .unwrap()
                .selected("absent")
                .build()
                .is_err()
        );
        assert!(
            store
                .goal_overview(&project, &draft.root.id, true)
                .unwrap()
                .goal
                .root
                .owner
                .is_none()
        );
    }
}
