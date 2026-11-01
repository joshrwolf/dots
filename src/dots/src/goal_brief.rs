//! Goal presentation resolves current Markdown through the shared document reader.
use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result, ensure};
use dots_memory::{Document, Store};
use dots_tasks::{Goal, GoalOverview, GoalProgress, Project, Task};
use serde::Serialize;

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
    Ok(Overview {
        goal: show(project, overview.goal)?,
        tasks: overview.tasks,
        hidden_closed: overview.hidden_closed,
    })
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
