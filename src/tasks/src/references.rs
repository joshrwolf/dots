//! Links from tasks to issues, pull requests, or tickets; never provider state.
use crate::store::{self, Actor, Error, Project, Store, Task};
use rusqlite::{Transaction, params};
use serde::Serialize;

/// A link to an issue, pull request, or ticket the work relates to.
#[derive(Debug, Clone, Serialize)]
pub struct ExternalReference {
    pub url: String,
    pub label: Option<String>,
}

impl ExternalReference {
    fn validate(&self) -> Result<(), Error> {
        let url = self.url.as_str();
        let rest = url
            .strip_prefix("https://")
            .or_else(|| url.strip_prefix("http://"));
        let valid_location =
            rest.and_then(|rest| rest.split_once('/'))
                .is_some_and(|(host, path)| {
                    !host.is_empty() && !path.is_empty() && !host.contains(['@', '?', '#', '\\'])
                });
        if !valid_location
            || url.len() > 4096
            || url
                .chars()
                .any(|c| c.is_whitespace() || c.is_control() || c == '\\')
        {
            return Err(Error::Invalid(
                "reference URL: expected a full HTTP(S) URL without credentials",
            ));
        }
        if self.label.as_ref().is_some_and(|label| {
            label.trim().is_empty() || label.len() > 256 || label.chars().any(char::is_control)
        }) {
            return Err(Error::Invalid(
                "reference label: nonempty text, at most 256 bytes",
            ));
        }
        Ok(())
    }
}

impl Store {
    /// Upsert by exact URL within this task. Links never change task state.
    pub fn set_reference(
        &mut self,
        project: &Project,
        id: &str,
        reference: &ExternalReference,
        actor: Actor<'_>,
        force: bool,
    ) -> Result<Task, Error> {
        reference.validate()?;
        let tx = self.write()?;
        let task = store::load(&tx, project, id)?;
        check_owner(&task, actor, force)?;
        tx.execute(
            "INSERT INTO task_refs(task,url,label) VALUES(?1,?2,?3)
             ON CONFLICT(task,url) DO UPDATE SET label=excluded.label",
            params![id, reference.url, reference.label],
        )?;
        store::record(&tx, id, actor, "reference_set", Some(&reference.url))?;
        tx.execute(
            "UPDATE tasks SET updated_at=?2 WHERE id=?1",
            params![id, actor.now],
        )?;
        let updated = store::load(&tx, project, id)?;
        tx.commit()?;
        Ok(updated)
    }

    pub fn remove_reference(
        &mut self,
        project: &Project,
        id: &str,
        url: &str,
        actor: Actor<'_>,
        force: bool,
    ) -> Result<Task, Error> {
        let tx = self.write()?;
        let task = store::load(&tx, project, id)?;
        check_owner(&task, actor, force)?;
        if tx.execute(
            "DELETE FROM task_refs WHERE task=?1 AND url=?2",
            params![id, url],
        )? == 0
        {
            return Err(Error::Invalid(
                "reference URL: not associated with this task",
            ));
        }
        store::record(&tx, id, actor, "reference_removed", Some(url))?;
        tx.execute(
            "UPDATE tasks SET updated_at=?2 WHERE id=?1",
            params![id, actor.now],
        )?;
        let updated = store::load(&tx, project, id)?;
        tx.commit()?;
        Ok(updated)
    }
}

fn check_owner(task: &Task, actor: Actor<'_>, force: bool) -> Result<(), Error> {
    if !force && let Some(owner) = task.claimed_by_other(actor.name) {
        return Err(Error::OwnedByAnother {
            id: task.id.clone(),
            owner: owner.into(),
        });
    }
    Ok(())
}

pub(super) fn load_references(
    tx: &Transaction<'_>,
    id: &str,
) -> Result<Vec<ExternalReference>, Error> {
    let mut statement = tx.prepare("SELECT url,label FROM task_refs WHERE task=?1 ORDER BY url")?;
    let rows = statement.query_map(params![id], |row| {
        Ok(ExternalReference {
            url: row.get(0)?,
            label: row.get(1)?,
        })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}
