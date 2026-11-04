//! Goal presentation resolves current Markdown through the shared document reader.
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result, ensure};
use dots_memory::{Document, Store};
use dots_tasks::{Goal, GoalOverview, GoalProgress, Project, Status, Task};
use serde::Serialize;

use crate::goal_graph::{self, Graph, Node};

#[derive(Debug, Serialize)]
pub(crate) struct Brief {
    relative: String,
    path: PathBuf,
    document: Option<Document>,
    diagnostic: Option<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct ShownGoal {
    pub root: Task,
    pub brief: Brief,
    pub progress: GoalProgress,
    pub last_activity: i64,
}

#[derive(Debug, Serialize)]
pub(crate) struct Overview {
    pub goal: ShownGoal,
    pub tasks: Vec<Task>,
    pub hidden_closed: usize,
    /// Dependency graph of the leaf tasks in `tasks`.
    pub graph: Graph,
    /// The same graph without closed leaves, when `tasks` includes any.
    pub open_graph: Option<Graph>,
}

fn documents(project: &Project) -> Result<Store> {
    Ok(Store::try_new(
        crate::project::workspace(project)?,
        crate::project::data_root()?.join("mutation.lock"),
    )?)
}

/// Association requires an existing readable brief; no files are created or copied.
pub(crate) fn existing(project: &Project, relative: &Path) -> Result<String> {
    dots_memory::paths::validate_brief(relative)?;
    let document = documents(project)?.read(relative, 1, 200)?;
    ensure!(
        document.exists,
        "brief is missing: {}",
        document.path.display()
    );
    Ok(relative
        .to_str()
        .context("brief path must be Unicode")?
        .to_owned())
}

pub(crate) fn reference(project: &Project, goal: Goal) -> Result<ShownGoal> {
    let root = crate::project::workspace(project)?;
    let relative = Path::new(&goal.brief);
    dots_memory::paths::validate_brief(relative)?;
    let path = root.join(relative);
    Ok(ShownGoal {
        root: goal.root,
        brief: Brief {
            relative: goal.brief,
            path,
            document: None,
            diagnostic: None,
        },
        progress: goal.progress,
        last_activity: goal.last_activity,
    })
}

pub(crate) fn show(project: &Project, goal: Goal) -> Result<ShownGoal> {
    let mut shown = reference(project, goal)?;
    match documents(project)?.read(Path::new(&shown.brief.relative), 1, 200) {
        Ok(document) => {
            shown.brief.diagnostic = (!document.exists).then(|| "Brief is missing".into());
            shown.brief.document = Some(document);
        }
        Err(error) => shown.brief.diagnostic = Some(error.to_string()),
    }
    Ok(shown)
}

pub(crate) fn overview(project: &Project, overview: GoalOverview) -> Result<Overview> {
    let closed = overview
        .tasks
        .iter()
        .any(|task| task.children == 0 && task.status.is_closed());
    Ok(Overview {
        goal: show(project, overview.goal)?,
        graph: graph(&overview.tasks, false),
        open_graph: closed.then(|| graph(&overview.tasks, true)),
        tasks: overview.tasks,
        hidden_closed: overview.hidden_closed,
    })
}

/// Project prerequisite edges onto leaf tasks: an edge to a group waits on its
/// leaves, and a group's own prerequisites hold for every leaf below it.
/// Closed prerequisites outside the drawing are satisfied and drop out; any
/// other unknown prerequisite stays, so the drawing reports it.
fn graph(tasks: &[Task], open_only: bool) -> Graph {
    let by_id: HashMap<&str, &Task> = tasks.iter().map(|task| (task.id.as_str(), task)).collect();
    let shown = |task: &Task| !open_only || !task.status.is_closed();
    let leaves: Vec<&Task> = tasks
        .iter()
        .filter(|task| task.children == 0 && shown(task))
        .collect();
    let ancestors = |task: &Task| {
        std::iter::successors(
            task.parent.as_deref().and_then(|id| by_id.get(id)),
            |task| task.parent.as_deref().and_then(|id| by_id.get(id)),
        )
        .copied()
    };
    let nodes: Vec<Node<'_>> = leaves
        .iter()
        .map(|&leaf| {
            let mut prerequisites = Vec::new();
            for prerequisite in std::iter::once(leaf)
                .chain(ancestors(leaf))
                .flat_map(|task| &task.prerequisites)
            {
                let id = prerequisite.id.as_str();
                match by_id.get(id) {
                    Some(group) if group.children > 0 => prerequisites.extend(
                        leaves
                            .iter()
                            .filter(|other| ancestors(other).any(|task| task.id == group.id))
                            .map(|other| other.id.as_str()),
                    ),
                    Some(task) if shown(task) => prerequisites.push(id),
                    _ if prerequisite.status.is_closed() => {}
                    _ => prerequisites.push(id),
                }
            }
            prerequisites.sort_unstable();
            prerequisites.dedup();
            Node {
                id: &leaf.id,
                glyph: glyph(leaf),
                prerequisites,
            }
        })
        .collect();
    goal_graph::render(&nodes)
}

fn glyph(task: &Task) -> &'static str {
    match task.status {
        Status::Done => "✓",
        Status::Dropped => "−",
        Status::Held => "!",
        Status::Review => "◇",
        Status::Open if task.owner.is_some() => "●",
        Status::Open if !task.blocked_by.is_empty() => "◌",
        Status::Open => "○",
    }
}

impl Brief {
    pub(crate) fn text(&self) -> String {
        use std::fmt::Write as _;
        let mut text = self.path.display().to_string();
        if let Some(document) = &self.document {
            let _ = write!(text, " (revision {})", document.revision);
            if let Some(next) = document.next_line {
                let _ = write!(
                    text,
                    "\nBrief truncated; read the file from line {next} for the rest."
                );
            }
        }
        if let Some(diagnostic) = &self.diagnostic {
            let _ = write!(text, "\nBrief unavailable: {diagnostic}");
        }
        if let Some(document) = &self.document
            && document.exists
        {
            let _ = write!(text, "\nBrief content:\n{}", document.content);
        }
        text
    }
}
