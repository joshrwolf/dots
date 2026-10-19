//! Optional goal context on an existing task root. Documents stay outside the
//! store; neither attaching nor inspecting a goal grants execution authority.
use super::{Actor, Error, NewTask, Project, Status, Store, Task, insert_task, load, record};
use rusqlite::{OptionalExtension as _, Transaction, params};
use serde::Serialize;
use std::path::Path;

#[cfg(test)]
mod tests;

#[derive(Debug, Serialize)]
pub struct Goal {
    /// The root task id is also the goal identity; title and status stay task-owned.
    pub root: Task,
    /// Absolute local document path. Callers resolve and read it, not the store.
    pub brief: String,
}

#[derive(Debug, Serialize)]
pub struct GoalOverview {
    pub goal: Goal,
    /// Descendants in priority order; closed tasks are omitted unless requested.
    pub tasks: Vec<Task>,
    /// Closed descendants omitted from `tasks`; complete progress still includes them.
    pub hidden_closed: usize,
    pub progress: GoalProgress,
}

/// Recorded leaf-task state, not observed PR or worker state. The status counts
/// partition `total`; ready, claimed and `with_pr` are overlapping open-task facets.
#[derive(Debug, Default, Serialize)]
pub struct GoalProgress {
    pub total: usize,
    pub open: usize,
    pub held: usize,
    pub done: usize,
    pub dropped: usize,
    pub ready: usize,
    pub claimed: usize,
    pub with_pr: usize,
}

impl GoalProgress {
    fn from_tasks(tasks: &[Task], now: i64) -> Self {
        let mut progress = Self::default();
        for task in tasks.iter().filter(|task| task.children == 0) {
            progress.total += 1;
            match task.status {
                Status::Done => progress.done += 1,
                Status::Dropped => progress.dropped += 1,
                Status::Held => progress.held += 1,
                Status::Open => {
                    progress.open += 1;
                    progress.ready += usize::from(task.ready(now));
                    progress.claimed += usize::from(
                        task.owner.is_some() && task.lease_until.is_some_and(|until| until > now),
                    );
                    progress.with_pr += usize::from(task.pr.is_some());
                }
            }
        }
        progress
    }
}

impl Store {
    /// Create a root and attach its external brief in one transaction. No claims or execution.
    pub fn create_goal(
        &mut self,
        project: &Project,
        title: &str,
        brief: &str,
        actor: Actor<'_>,
    ) -> Result<Goal, Error> {
        validate_brief(brief)?;
        let tx = self.write()?;
        let root = insert_task(
            &tx,
            project,
            &NewTask {
                title: title.into(),
                ..NewTask::default()
            },
            actor,
        )?;
        tx.execute(
            "INSERT INTO goals(root,brief) VALUES(?1,?2)",
            params![root.id, brief],
        )?;
        record(&tx, &root.id, actor, "goal_attached", Some(brief))?;
        let goal = load_goal(&tx, project, &root.id)?;
        tx.commit()?;
        Ok(goal)
    }

    /// Attach or replace a brief reference. Repeating the same association is a no-op.
    /// This does not read the document or change tasks, claims, or execution state.
    pub fn attach_goal(
        &mut self,
        project: &Project,
        id: &str,
        brief: &str,
        actor: Actor<'_>,
    ) -> Result<Goal, Error> {
        validate_brief(brief)?;
        let tx = self.write()?;
        let root = load(&tx, project, id)?;
        if root.parent.is_some() {
            return Err(Error::Invalid("goal must be a root task"));
        }
        protect(&root, actor)?;
        let previous: Option<String> = tx
            .query_row("SELECT brief FROM goals WHERE root=?1", [id], |row| {
                row.get(0)
            })
            .optional()?;
        if previous.as_deref() != Some(brief) {
            tx.execute(
                "INSERT INTO goals(root,brief) VALUES(?1,?2) ON CONFLICT(root) DO UPDATE SET brief=excluded.brief",
                params![id, brief],
            )?;
            record(&tx, id, actor, "goal_attached", Some(brief))?;
        }
        let goal = load_goal(&tx, project, id)?;
        tx.commit()?;
        Ok(goal)
    }

    /// Remove only the association; keep the document, tasks and history intact.
    pub fn detach_goal(
        &mut self,
        project: &Project,
        id: &str,
        actor: Actor<'_>,
    ) -> Result<(), Error> {
        let tx = self.write()?;
        let root = load(&tx, project, id)?;
        protect(&root, actor)?;
        if tx.execute("DELETE FROM goals WHERE root=?1", [id])? > 0 {
            record(&tx, id, actor, "goal_detached", None)?;
        }
        tx.commit()?;
        Ok(())
    }

    /// List attached roots; ordinary root tasks are not implicitly goals.
    pub fn goals(&mut self, project: &Project, include_closed: bool) -> Result<Vec<Goal>, Error> {
        let tx = self.connection.transaction()?;
        let ids = {
            let mut statement = tx.prepare(
                "SELECT tasks.id FROM tasks JOIN goals ON goals.root=tasks.id
                 WHERE project=?1 AND (?2 OR status IN ('open','held'))
                 ORDER BY priority DESC, created_at ASC, tasks.rowid ASC",
            )?;
            statement
                .query_map(params![project.0, include_closed], |row| {
                    row.get::<_, String>(0)
                })?
                .collect::<Result<Vec<_>, _>>()?
        };
        ids.iter().map(|id| load_goal(&tx, project, id)).collect()
    }

    /// One consistent local snapshot. No network access or document reads.
    pub fn goal_overview(
        &mut self,
        project: &Project,
        id: &str,
        now: i64,
        include_closed: bool,
    ) -> Result<GoalOverview, Error> {
        let tx = self.connection.transaction()?;
        let goal = load_goal(&tx, project, id)?;
        let ids = {
            let mut statement = tx.prepare(
                "WITH RECURSIVE below(id) AS (
                    SELECT id FROM tasks WHERE parent=?1
                    UNION SELECT tasks.id FROM tasks JOIN below ON tasks.parent=below.id)
                 SELECT id FROM tasks WHERE project=?2 AND id IN below
                 ORDER BY priority DESC, created_at ASC, rowid ASC",
            )?;
            statement
                .query_map(params![id, project.0], |row| row.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?
        };
        let mut tasks = ids
            .iter()
            .map(|id| load(&tx, project, id))
            .collect::<Result<Vec<_>, _>>()?;
        let progress = GoalProgress::from_tasks(&tasks, now);
        let total = tasks.len();
        if !include_closed {
            tasks.retain(|task| !task.status.is_closed());
        }
        let hidden_closed = total - tasks.len();
        Ok(GoalOverview {
            goal,
            tasks,
            hidden_closed,
            progress,
        })
    }
}

fn validate_brief(brief: &str) -> Result<(), Error> {
    if !Path::new(brief).is_absolute() || brief.len() > 4096 || brief.chars().any(char::is_control)
    {
        return Err(Error::Invalid(
            "brief: expected an absolute local document path",
        ));
    }
    Ok(())
}

fn protect(root: &Task, actor: Actor<'_>) -> Result<(), Error> {
    if let Some((owner, _)) = root.leased_to_other(actor.name, actor.now) {
        return Err(Error::OwnedByAnother {
            id: root.id.clone(),
            owner: owner.into(),
        });
    }
    Ok(())
}

pub(super) fn attached(tx: &Transaction<'_>, id: &str) -> Result<bool, Error> {
    Ok(tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM goals WHERE root=?1)",
        [id],
        |row| row.get(0),
    )?)
}

fn load_goal(tx: &Transaction<'_>, project: &Project, id: &str) -> Result<Goal, Error> {
    let root = load(tx, project, id)?;
    let brief = tx
        .query_row("SELECT brief FROM goals WHERE root=?1", [id], |row| {
            row.get(0)
        })
        .optional()?
        .ok_or(Error::Invalid("task has no goal association"))?;
    Ok(Goal { root, brief })
}
