//! Explicit project identities and canonical Git common-directory bindings.
use std::path::{Path, PathBuf};

use rusqlite::{OptionalExtension as _, Transaction, params};
use serde::Serialize;

use crate::store::{Error, Project, Store};

#[cfg(test)]
mod tests;

#[derive(Debug, Serialize)]
pub struct RegisteredProject {
    pub id: Project,
    pub name: String,
    pub common_directories: Vec<PathBuf>,
}

impl Store {
    /// Register an unbound project. Display names do not determine its identity.
    pub fn register_project(&mut self, name: &str) -> Result<Project, Error> {
        let tx = self.write()?;
        let project = insert_project(&tx, name)?;
        tx.commit()?;
        Ok(project)
    }

    /// Atomically register and bind, or return the existing binding on retry.
    pub fn initialize_project(
        &mut self,
        name: &str,
        common: &Path,
    ) -> Result<(Project, bool), Error> {
        let name = validate_project_name(name)?;
        let common = canonical_common_directory(common)?;
        let tx = self.write()?;
        let (project, created) = if let Some(id) = binding(&tx, &common)? {
            (Project::try_new(id)?, false)
        } else {
            let project = insert_project(&tx, name)?;
            bind(&tx, &project, &common)?;
            (project, true)
        };
        tx.commit()?;
        Ok((project, created))
    }

    /// List registered identities without inferring bindings from filesystem contents.
    pub fn registered_projects(&mut self) -> Result<Vec<RegisteredProject>, Error> {
        let ids = {
            let mut statement = self
                .connection
                .prepare("SELECT id FROM projects ORDER BY name,id")?;
            statement
                .query_map([], |row| row.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?
        };
        ids.into_iter()
            .map(|id| self.registered_project(&Project::try_new(id)?))
            .collect()
    }

    /// Require an explicitly registered project rather than inferring one from a name.
    pub fn registered_project(&mut self, project: &Project) -> Result<RegisteredProject, Error> {
        let tx = self.connection.transaction()?;
        let name = require_project(&tx, project)?;
        let common_directories = bindings(&tx, project)?;
        Ok(RegisteredProject {
            id: project.clone(),
            name,
            common_directories,
        })
    }

    /// Resolve only the supplied Git common directory; absence never registers a project.
    pub fn project_for_common_directory(&self, common: &Path) -> Result<Option<Project>, Error> {
        let common = canonical_common_directory(common)?;
        self.connection
            .query_row(
                "SELECT project FROM project_bindings WHERE common_directory=?1",
                [common],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .map(Project::try_new)
            .transpose()
    }

    /// Bind a checkout to an existing identity. An identical binding is a no-op;
    /// binding another project's directory fails rather than stealing it.
    pub fn bind_project(&mut self, project: &Project, common: &Path) -> Result<(), Error> {
        let common = canonical_common_directory(common)?;
        let tx = self.write()?;
        require_project(&tx, project)?;
        bind(&tx, project, &common)?;
        tx.commit()?;
        Ok(())
    }

    /// Remove one exact binding without deleting the project, tasks or documents.
    /// The recorded path may no longer exist. Another project's binding is never removed.
    pub fn unbind_project(&mut self, project: &Project, common: &Path) -> Result<bool, Error> {
        let common = absolute_path(common)?;
        let tx = self.write()?;
        require_project(&tx, project)?;
        let owner = binding(&tx, &common)?;
        let removed = owner.is_some();
        if let Some(owner) = owner {
            if owner != project.as_str() {
                return Err(Error::BindingConflict {
                    common,
                    project: owner,
                });
            }
            tx.execute(
                "DELETE FROM project_bindings WHERE common_directory=?1 AND project=?2",
                params![common, project.as_str()],
            )?;
        }
        tx.commit()?;
        Ok(removed)
    }

    /// Replace one exact binding after a checkout move. The old directory need not
    /// exist; it must match the recorded path. Other bindings remain untouched.
    pub fn rebind_project(
        &mut self,
        project: &Project,
        previous: &Path,
        common: &Path,
    ) -> Result<(), Error> {
        let previous = absolute_path(previous)?;
        let common = canonical_common_directory(common)?;
        let tx = self.write()?;
        require_project(&tx, project)?;
        let owner = binding(&tx, &previous)?;
        if owner.as_deref() != Some(project.as_str()) {
            return Err(Error::BindingMissing {
                common: previous,
                project: project.as_str().into(),
                bindings: bindings(&tx, project)?,
            });
        }
        bind(&tx, project, &common)?;
        if previous != common {
            tx.execute(
                "DELETE FROM project_bindings WHERE common_directory=?1 AND project=?2",
                params![previous, project.as_str()],
            )?;
        }
        tx.commit()?;
        Ok(())
    }
}

/// Validate and normalize a display name before any catalog initialization.
pub fn validate_project_name(name: &str) -> Result<&str, Error> {
    let name = name.trim();
    if name.is_empty() || name.len() > 128 || name.chars().any(char::is_control) {
        return Err(Error::Invalid(
            "project name: use 1..128 bytes without control characters",
        ));
    }
    Ok(name)
}

fn insert_project(tx: &Transaction<'_>, name: &str) -> Result<Project, Error> {
    let name = validate_project_name(name)?;
    let id = tx.query_row(
        "INSERT INTO projects(id,name) VALUES('p-' || lower(hex(randomblob(16))),?1) RETURNING id",
        [name],
        |row| row.get::<_, String>(0),
    )?;
    Project::try_new(id)
}

fn require_project(tx: &Transaction<'_>, project: &Project) -> Result<String, Error> {
    tx.query_row(
        "SELECT name FROM projects WHERE id=?1",
        [project.as_str()],
        |row| row.get(0),
    )
    .optional()?
    .ok_or_else(|| Error::UnregisteredProject(project.as_str().into()))
}

fn bindings(tx: &Transaction<'_>, project: &Project) -> Result<Vec<PathBuf>, Error> {
    let mut statement = tx.prepare(
        "SELECT common_directory FROM project_bindings WHERE project=?1 ORDER BY common_directory",
    )?;
    Ok(statement
        .query_map([project.as_str()], |row| row.get::<_, String>(0))?
        .map(|row| row.map(PathBuf::from))
        .collect::<Result<Vec<_>, _>>()?)
}

fn binding(tx: &Transaction<'_>, common: &str) -> Result<Option<String>, Error> {
    Ok(tx
        .query_row(
            "SELECT project FROM project_bindings WHERE common_directory=?1",
            [common],
            |row| row.get(0),
        )
        .optional()?)
}

fn bind(tx: &Transaction<'_>, project: &Project, common: &str) -> Result<(), Error> {
    if let Some(owner) = binding(tx, common)? {
        if owner != project.as_str() {
            return Err(Error::BindingConflict {
                common: common.into(),
                project: owner,
            });
        }
        return Ok(());
    }
    tx.execute(
        "INSERT INTO project_bindings(common_directory,project) VALUES(?1,?2)",
        params![common, project.as_str()],
    )?;
    Ok(())
}

fn absolute_path(path: &Path) -> Result<String, Error> {
    let text = path
        .to_str()
        .ok_or(Error::Invalid("non-Unicode common directory"))?;
    if !path.is_absolute() || text.len() > 4096 || text.chars().any(char::is_control) {
        return Err(Error::Invalid(
            "common directory must be an absolute Unicode path without control characters",
        ));
    }
    Ok(text.to_owned())
}

fn canonical_common_directory(path: &Path) -> Result<String, Error> {
    absolute_path(path)?;
    let canonical = path.canonicalize().map_err(|source| Error::ProjectPath {
        path: path.into(),
        source,
    })?;
    if !canonical.is_dir() {
        return Err(Error::Invalid("Git common directory must be a directory"));
    }
    absolute_path(&canonical)
}
