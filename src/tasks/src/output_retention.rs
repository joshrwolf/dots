//! Explicit project-digest erasure and read-only integrity inspection.
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use dots_memory::inventory::{self, Kind};
use dots_memory::mutation_lock::MutationLock;
use dots_memory::{directories, paths, text};
use rusqlite::params;
use serde::Serialize;

use crate::outputs::{self, MAX_OUTPUT_BYTES, OutputContent};
use crate::{Actor, Error, Owner, Project, Store};

#[derive(Debug, Serialize)]
pub struct IntegrityIssue {
    pub path: PathBuf,
    pub state: String,
    pub diagnostic: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct Integrity {
    pub project: Project,
    pub revisions_checked: usize,
    pub issues: Vec<IntegrityIssue>,
    /// SQLite sidecars are reported, not opened independently or deleted.
    pub catalog_sidecars: Vec<PathBuf>,
}

#[derive(Debug, Serialize)]
pub struct Purged {
    pub digest: String,
    pub tombstoned: bool,
    pub removal_complete: bool,
    pub removed: Vec<PathBuf>,
    pub issues: Vec<IntegrityIssue>,
    pub limits: &'static str,
}

impl Store {
    /// Explicitly authorized callers only. Commit the project tombstone before
    /// touching bytes. A failed/interrupted removal can be retried without resurrection.
    pub fn purge_output_digest(
        &mut self,
        project: &Project,
        digest: &str,
        actor: Actor<'_>,
    ) -> Result<Purged, Error> {
        outputs::validate_digest(digest)?;
        self.registered_project(project)?;
        let root = self.document_root()?;
        let _lock = MutationLock::acquire(&root.join("mutation.lock"))?;
        let tx = self.write()?;
        tx.execute("INSERT INTO purged_digests(project,digest,at,actor) VALUES(?1,?2,?3,?4) ON CONFLICT DO NOTHING",params![project.as_str(),digest,actor.now,actor.name.map(Owner::as_str)])?;
        tx.commit()?;
        #[cfg(test)]
        crate::test_pause(&root, "tombstoned");
        let mut receipt = Purged {
            digest: digest.into(),
            tombstoned: true,
            removal_complete: true,
            removed: Vec::new(),
            issues: Vec::new(),
            limits: "Only exact matching bytes under this project's managed outputs, including staging and orphan payloads. Scratch, sources, other projects, backups and session copies are outside this erasure guarantee.",
        };
        let relative = PathBuf::from(format!("workspaces/{}/outputs", project.as_str()));
        let output_root = root.join(&relative);
        let entries = paths::resolve(&root, &relative).and_then(|path| inventory::scan(&path));
        let entries = match entries {
            Ok(entries) => entries,
            Err(error) => {
                receipt.removal_complete = false;
                receipt.issues.push(IntegrityIssue {
                    path: output_root,
                    state: "scan_failed".into(),
                    diagnostic: Some(error.to_string()),
                });
                return Ok(receipt);
            }
        };
        for entry in entries {
            if entry.kind != Kind::File {
                receipt.removal_complete = false;
                receipt.issues.push(IntegrityIssue {
                    path: entry.path,
                    state: "not_regular".into(),
                    diagnostic: None,
                });
                continue;
            }
            let content = match text::read_bytes(&entry.path, MAX_OUTPUT_BYTES) {
                Ok(Some(content)) => content,
                Ok(None) => continue,
                Err(dots_memory::Error::TooLarge { .. }) => {
                    receipt.issues.push(IntegrityIssue {
                        path: entry.path,
                        state: "outside_payload_limit".into(),
                        diagnostic: Some(
                            "Cannot match a supported retained payload; left untouched".into(),
                        ),
                    });
                    continue;
                }
                Err(error) => {
                    receipt.removal_complete = false;
                    receipt.issues.push(IntegrityIssue {
                        path: entry.path,
                        state: "unreadable".into(),
                        diagnostic: Some(error.to_string()),
                    });
                    continue;
                }
            };
            if text::bytes_revision(&content) != digest {
                if is_staging(&entry.path) {
                    receipt.removal_complete = false;
                    receipt.issues.push(IntegrityIssue {path:entry.path,state:"unverified_staging".into(),diagnostic:Some("Different or partial bytes remain outside exact-digest erasure; explicit inspection/cleanup required".into())});
                }
                continue;
            }
            let removed = fs::remove_file(&entry.path)
                .map_err(|source| dots_memory::Error::Io {
                    operation: "remove purged output",
                    path: entry.path.clone(),
                    source,
                })
                .and_then(|()| {
                    directories::sync(entry.path.parent().expect("inventory file has parent"))
                });
            match removed {
                Ok(()) => {
                    receipt.removed.push(entry.path);
                    #[cfg(test)]
                    crate::test_pause(&root, "removed");
                }
                Err(error) => {
                    receipt.removal_complete = false;
                    receipt.issues.push(IntegrityIssue {
                        path: entry.path,
                        state: "removal_failed".into(),
                        diagnostic: Some(error.to_string()),
                    });
                }
            }
        }
        Ok(receipt)
    }

    /// Cooperative consistency through the workspace lock. No repair, adoption or deletion.
    pub fn output_integrity(&mut self, project: &Project) -> Result<Integrity, Error> {
        self.registered_project(project)?;
        let root = self.document_root()?;
        let _lock = MutationLock::acquire(&root.join("mutation.lock"))?;
        let mut report = Integrity {
            project: project.clone(),
            revisions_checked: 0,
            issues: Vec::new(),
            catalog_sidecars: Vec::new(),
        };
        let revisions = {
            let mut statement=self.connection.prepare("SELECT slug,digest,path FROM output_revisions WHERE project=?1 ORDER BY slug,digest")?;
            statement
                .query_map([project.as_str()], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        PathBuf::from(row.get::<_, String>(2)?),
                    ))
                })?
                .collect::<Result<Vec<_>, _>>()?
        };
        let mut registered = BTreeSet::new();
        for (slug, digest, relative) in revisions {
            report.revisions_checked += 1;
            let path = root.join(relative);
            registered.insert(path.clone());
            let read = match self.read_output(project, &slug, &digest, 1, 200) {
                Ok(read) => read,
                Err(error) => {
                    report.issues.push(IntegrityIssue {
                        path,
                        state: "catalog_revision_error".into(),
                        diagnostic: Some(error.to_string()),
                    });
                    continue;
                }
            };
            let state = match read.content {
                OutputContent::Available { .. } | OutputContent::Unrenderable { .. } => None,
                OutputContent::Missing => Some("missing"),
                OutputContent::Corrupt => Some("corrupt"),
                OutputContent::Unreadable { diagnostic } => {
                    report.issues.push(IntegrityIssue {
                        path: read.path,
                        state: "unreadable".into(),
                        diagnostic: Some(diagnostic),
                    });
                    continue;
                }
                OutputContent::Purged => match fs::symlink_metadata(&read.path) {
                    Ok(_) => Some("purged_bytes_pending_removal"),
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
                    Err(error) => {
                        report.issues.push(IntegrityIssue {
                            path: read.path,
                            state: "unreadable".into(),
                            diagnostic: Some(error.to_string()),
                        });
                        continue;
                    }
                },
            };
            if let Some(state) = state {
                report.issues.push(IntegrityIssue {
                    path: read.path,
                    state: state.into(),
                    diagnostic: None,
                });
            }
        }
        let purged = {
            let mut statement = self
                .connection
                .prepare("SELECT digest FROM purged_digests WHERE project=?1")?;
            statement
                .query_map([project.as_str()], |row| row.get::<_, String>(0))?
                .collect::<Result<BTreeSet<_>, _>>()?
        };
        inspect_unregistered(&root, project, &registered, &purged, &mut report.issues);
        catalog_integrity(&self.connection, &root, &mut report)?;
        catalog_remnants(&root, &mut report)?;
        Ok(report)
    }
}

fn is_staging(path: &Path) -> bool {
    path.file_name()
        .is_some_and(|name| name.to_string_lossy().starts_with(".staged-"))
}

fn inspect_unregistered(
    root: &Path,
    project: &Project,
    registered: &BTreeSet<PathBuf>,
    purged: &BTreeSet<String>,
    issues: &mut Vec<IntegrityIssue>,
) {
    let relative = PathBuf::from(format!("workspaces/{}/outputs", project.as_str()));
    let entries = match paths::resolve(root, &relative).and_then(|path| inventory::scan(&path)) {
        Ok(entries) => entries,
        Err(error) => {
            issues.push(IntegrityIssue {
                path: root.join(relative),
                state: "scan_failed".into(),
                diagnostic: Some(error.to_string()),
            });
            return;
        }
    };
    for entry in entries {
        if registered.contains(&entry.path) {
            continue;
        }
        let mut diagnostic = None;
        let state = if entry.kind == Kind::File {
            match text::read_bytes(&entry.path, MAX_OUTPUT_BYTES) {
                Ok(Some(bytes)) if purged.contains(&text::bytes_revision(&bytes)) => {
                    "purged_bytes_pending_removal"
                }
                Ok(_) => {
                    if is_staging(&entry.path) {
                        "staged_output"
                    } else {
                        "unregistered_output"
                    }
                }
                Err(dots_memory::Error::TooLarge { .. }) => "outside_payload_limit",
                Err(error) => {
                    diagnostic = Some(error.to_string());
                    "unreadable"
                }
            }
        } else {
            "not_regular"
        };
        issues.push(IntegrityIssue {
            path: entry.path,
            state: state.into(),
            diagnostic,
        });
    }
}

fn catalog_integrity(
    connection: &rusqlite::Connection,
    root: &Path,
    report: &mut Integrity,
) -> Result<(), Error> {
    let mut statement = connection.prepare("PRAGMA integrity_check")?;
    for row in statement.query_map([], |row| row.get::<_, String>(0))? {
        let diagnostic = row?;
        if diagnostic != "ok" {
            report.issues.push(IntegrityIssue {
                path: root.join("workspace.sqlite3"),
                state: "catalog_integrity_error".into(),
                diagnostic: Some(diagnostic),
            });
        }
    }
    let mut statement = connection.prepare("PRAGMA foreign_key_check")?;
    for row in statement.query_map([], |row| row.get::<_, String>(0))? {
        report.issues.push(IntegrityIssue {
            path: root.join("workspace.sqlite3"),
            state: "catalog_foreign_key_error".into(),
            diagnostic: Some(format!("invalid reference in {}", row?)),
        });
    }
    Ok(())
}

fn catalog_remnants(root: &Path, report: &mut Integrity) -> Result<(), Error> {
    // A staged SQLite file can be a hard-link alias of the live catalog.
    // Enumerate names only: never open, adopt, or remove these as another database.
    for entry in fs::read_dir(root).map_err(|source| dots_memory::Error::Io {
        operation: "inspect catalog directory",
        path: root.to_owned(),
        source,
    })? {
        let entry = entry.map_err(|source| dots_memory::Error::Io {
            operation: "inspect catalog entry",
            path: root.to_owned(),
            source,
        })?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with(".staged-") {
            report.issues.push(IntegrityIssue {
                path: entry.path(),
                state: "catalog_staging_remnant".into(),
                diagnostic: Some(
                    "May alias the live catalog; do not open independently or blindly delete"
                        .into(),
                ),
            });
        } else if name.ends_with("-wal") || name.ends_with("-shm") || name.ends_with("-journal") {
            report.catalog_sidecars.push(entry.path());
        }
    }
    Ok(())
}
