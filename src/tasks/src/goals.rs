//! Goal briefs attached to task roots. Documents stay outside the store.
use crate::store::{self, Actor, Error, NewTask, Owner, Project, Status, Store, Task};
use rusqlite::{OptionalExtension as _, Transaction, params};
use serde::Serialize;
use std::path::Path;

#[cfg(test)]
mod tests;

#[derive(Debug, Serialize)]
pub struct Goal {
    /// The root task id is also the goal identity; title and status stay task-owned.
    pub root: Task,
    /// Project-relative Markdown path under briefs/. Callers resolve and read it.
    pub brief: String,
    pub progress: GoalProgress,
    /// Latest event anywhere in the goal, as a Unix timestamp.
    pub last_activity: i64,
}

#[derive(Debug, Serialize)]
pub struct GoalOverview {
    pub goal: Goal,
    /// Descendants, oldest first; closed tasks are omitted unless requested.
    pub tasks: Vec<Task>,
    /// Closed descendants omitted from `tasks`; progress still counts them.
    pub hidden_closed: usize,
}

/// Leaf-task counts. The status counts partition `total`; ready and claimed overlap them.
#[derive(Debug, Default, Serialize)]
pub struct GoalProgress {
    pub total: usize,
    pub open: usize,
    pub review: usize,
    pub held: usize,
    pub done: usize,
    pub dropped: usize,
    pub ready: usize,
    pub claimed: usize,
}

impl GoalProgress {
    fn from_tasks(tasks: &[Task]) -> Self {
        let mut progress = Self::default();
        for task in tasks.iter().filter(|task| task.children == 0) {
            progress.total += 1;
            match task.status {
                Status::Done => progress.done += 1,
                Status::Dropped => progress.dropped += 1,
                Status::Held => progress.held += 1,
                Status::Open | Status::Review => {
                    progress.open += usize::from(task.status == Status::Open);
                    progress.review += usize::from(task.status == Status::Review);
                    progress.ready += usize::from(task.ready());
                    progress.claimed += usize::from(task.owner.is_some());
                }
            }
        }
        progress
    }
}

impl Store {
    /// Create a root and attach its workspace brief in one transaction.
    pub fn create_goal(
        &mut self,
        project: &Project,
        title: &str,
        brief: &str,
        actor: Actor<'_>,
    ) -> Result<Goal, Error> {
        validate_brief(brief)?;
        let tx = self.write()?;
        let root = store::insert_task(
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
        store::record(&tx, &root.id, actor, "goal_attached", Some(brief))?;
        let goal = load_goal(&tx, project, &root.id)?;
        tx.commit()?;
        Ok(goal)
    }

    /// Attach or replace a brief reference. Repeating the same association is a no-op.
    pub fn attach_goal(
        &mut self,
        project: &Project,
        id: &str,
        brief: &str,
        actor: Actor<'_>,
    ) -> Result<Goal, Error> {
        validate_brief(brief)?;
        let tx = self.write()?;
        let root = store::load(&tx, project, id)?;
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
            store::record(&tx, id, actor, "goal_attached", Some(brief))?;
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
        let root = store::load(&tx, project, id)?;
        protect(&root, actor)?;
        if tx.execute("DELETE FROM goals WHERE root=?1", [id])? > 0 {
            store::record(&tx, id, actor, "goal_detached", None)?;
        }
        tx.commit()?;
        Ok(())
    }

    /// List attached roots, most recently active first; ordinary root tasks are not goals.
    pub fn goals(
        &mut self,
        project: &Project,
        include_closed: bool,
        claimed_by: Option<Owner<'_>>,
    ) -> Result<Vec<Goal>, Error> {
        let tx = self.connection.transaction()?;
        let ids = {
            let mut statement = tx.prepare(
                "WITH RECURSIVE claimed(id, parent) AS (
                    SELECT id, parent FROM tasks WHERE project=?1 AND owner=?3
                      AND status IN ('open','review')
                    UNION SELECT tasks.id, tasks.parent FROM tasks JOIN claimed ON tasks.id=claimed.parent
                      WHERE tasks.project=?1)
                 SELECT tasks.id FROM tasks JOIN goals ON goals.root=tasks.id
                 WHERE project=?1 AND (?2 OR status NOT IN ('done','dropped'))
                   AND (?3 IS NULL OR tasks.id IN (SELECT id FROM claimed))
                 ORDER BY created_at ASC, tasks.rowid ASC",
            )?;
            statement
                .query_map(
                    params![project.0, include_closed, claimed_by.map(Owner::as_str)],
                    |row| row.get::<_, String>(0),
                )?
                .collect::<Result<Vec<_>, _>>()?
        };
        let mut goals = ids
            .iter()
            .map(|id| load_goal(&tx, project, id))
            .collect::<Result<Vec<_>, _>>()?;
        goals.sort_by_key(|goal| std::cmp::Reverse(goal.last_activity));
        Ok(goals)
    }

    /// One consistent local snapshot. No network access or document reads.
    pub fn goal_overview(
        &mut self,
        project: &Project,
        id: &str,
        include_closed: bool,
    ) -> Result<GoalOverview, Error> {
        let tx = self.connection.transaction()?;
        let goal = load_goal(&tx, project, id)?;
        let mut tasks = descendants(&tx, project, id)?;
        let total = tasks.len();
        if !include_closed {
            tasks.retain(|task| !task.status.is_closed());
        }
        let hidden_closed = total - tasks.len();
        Ok(GoalOverview {
            goal,
            tasks,
            hidden_closed,
        })
    }
}

fn descendants(tx: &Transaction<'_>, project: &Project, id: &str) -> Result<Vec<Task>, Error> {
    let ids = {
        let mut statement = tx.prepare(
            "WITH RECURSIVE below(id) AS (
                SELECT id FROM tasks WHERE parent=?1
                UNION SELECT tasks.id FROM tasks JOIN below ON tasks.parent=below.id)
             SELECT id FROM tasks WHERE project=?2 AND id IN below
             ORDER BY created_at ASC, rowid ASC",
        )?;
        statement
            .query_map(params![id, project.0], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?
    };
    ids.iter().map(|id| store::load(tx, project, id)).collect()
}

fn validate_brief(brief: &str) -> Result<(), Error> {
    Ok(dots_memory::paths::validate_brief(Path::new(brief))?)
}

fn protect(root: &Task, actor: Actor<'_>) -> Result<(), Error> {
    if let Some(owner) = root.claimed_by_other(actor.name) {
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
    let root = store::load(tx, project, id)?;
    let brief = tx
        .query_row("SELECT brief FROM goals WHERE root=?1", [id], |row| {
            row.get(0)
        })
        .optional()?
        .ok_or(Error::Invalid("task has no goal association"))?;
    let progress = GoalProgress::from_tasks(&descendants(tx, project, id)?);
    let last_activity = tx.query_row(
        "WITH RECURSIVE goal(id) AS (
            SELECT ?1 UNION SELECT tasks.id FROM tasks JOIN goal ON tasks.parent=goal.id)
         SELECT MAX(at) FROM events WHERE task IN goal",
        [id],
        |row| row.get::<_, Option<i64>>(0),
    )?;
    Ok(Goal {
        last_activity: last_activity.unwrap_or(root.created_at),
        root,
        brief,
        progress,
    })
}
