//! Bounded output discovery and non-authoritative task associations.
use rusqlite::{Transaction, params};
use serde::Serialize;

use crate::outputs;
use crate::store::{self, Actor, Error, Project, Store};

#[derive(Debug, Serialize)]
pub struct OutputList {
    pub slugs: Vec<String>,
    pub next_offset: Option<usize>,
}

#[derive(Debug, Clone, Copy)]
pub enum Association {
    Attach,
    Detach,
}

impl Store {
    /// Discovery does not imply acceptance; no implicit newest revision is selected.
    pub fn list_outputs(
        &mut self,
        project: &Project,
        task: Option<&str>,
        offset: usize,
    ) -> Result<OutputList, Error> {
        self.registered_project(project)?;
        if let Some(task) = task {
            self.get(project, task)?;
        }
        let mut statement=self.connection.prepare("SELECT slug FROM outputs WHERE project=?1 AND (?2 IS NULL OR slug IN (SELECT slug FROM task_outputs WHERE task=?2 AND project=?1)) ORDER BY slug LIMIT 51 OFFSET ?3")?;
        let mut slugs = statement
            .query_map(
                params![
                    project.as_str(),
                    task,
                    i64::try_from(offset).map_err(|_| Error::Invalid("offset"))?
                ],
                |row| row.get(0),
            )?
            .collect::<Result<Vec<String>, _>>()?;
        let next_offset = (slugs.len() > 50).then(|| offset + 50);
        slugs.truncate(50);
        Ok(OutputList { slugs, next_offset })
    }

    /// Detach removes discovery only; historical event evidence remains unchanged.
    pub fn associate_output(
        &mut self,
        project: &Project,
        id: &str,
        slug: &str,
        association: Association,
        actor: Actor<'_>,
    ) -> Result<bool, Error> {
        outputs::validate_slug(slug)?;
        let tx = self.write()?;
        let changed = associate(&tx, project, id, slug, association, actor)?;
        tx.commit()?;
        Ok(changed)
    }
}

pub(super) fn attached(
    connection: &rusqlite::Connection,
    project: &Project,
    id: &str,
    slug: &str,
) -> Result<bool, Error> {
    Ok(connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM task_outputs WHERE task=?1 AND project=?2 AND slug=?3)",
        params![id, project.as_str(), slug],
        |row| row.get(0),
    )?)
}

pub(super) fn associate(
    tx: &Transaction<'_>,
    project: &Project,
    id: &str,
    slug: &str,
    association: Association,
    actor: Actor<'_>,
) -> Result<bool, Error> {
    let task = store::load(tx, project, id)?;
    let exists: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM outputs WHERE project=?1 AND slug=?2)",
        params![project.as_str(), slug],
        |row| row.get(0),
    )?;
    if !exists {
        return Err(Error::Output(
            "output does not exist in this project".into(),
        ));
    }
    if matches!(association, Association::Attach) == attached(tx, project, id, slug)? {
        return Ok(false);
    }
    if let Some(owner) = task.claimed_by_other(actor.name) {
        return Err(Error::OwnedByAnother {
            id: id.into(),
            owner: owner.into(),
        });
    }
    let (changed,kind)=match association {
            Association::Attach=>(tx.execute("INSERT INTO task_outputs(task,project,slug) VALUES(?1,?2,?3) ON CONFLICT DO NOTHING",params![id,project.as_str(),slug])?,"output-attached"),
            Association::Detach=>(tx.execute("DELETE FROM task_outputs WHERE task=?1 AND project=?2 AND slug=?3",params![id,project.as_str(),slug])?,"output-detached"),
        };
    if changed != 0 {
        store::record(tx, id, actor, kind, Some(slug))?;
    }
    Ok(changed != 0)
}
