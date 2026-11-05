//! Immutable output publication and exact-revision inspection in the workspace catalog.
use std::path::{Path, PathBuf};

use dots_memory::immutable::{self, Presence};
use dots_memory::mutation_lock::MutationLock;
use dots_memory::{Document, paths, text, verified_text};
use rusqlite::{OptionalExtension as _, params};
use serde::Serialize;

use crate::output_discovery::{self, Association};
use crate::store::{Actor, Error, Owner, Project, Store};

#[cfg(test)]
mod tests;

pub const MAX_OUTPUT_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PublishMode {
    Create,
    Extend,
}

#[derive(Debug)]
pub struct PublishOutput<'input> {
    pub slug: &'input str,
    pub filename: &'input str,
    pub media_type: &'input str,
    pub source: &'input Path,
    pub mode: PublishMode,
    pub description: &'input str,
    /// Attributed producer/session/source-version notes, not independently verified facts.
    pub provenance: &'input str,
    pub task: Option<&'input str>,
}

#[derive(Debug, Clone, Serialize)]
pub struct OutputRevision {
    pub slug: String,
    pub digest: String,
    pub bytes: u32,
    /// Relative to the dots storage root, including the project identity.
    pub path: PathBuf,
    pub created_at: i64,
    pub actor: Option<String>,
    pub description: String,
    pub provenance: String,
    pub purged: bool,
}

#[derive(Debug, Serialize)]
pub struct OutputManifest {
    pub slug: String,
    pub filename: String,
    pub media_type: String,
    pub revisions: Vec<OutputRevision>,
    pub next_offset: Option<usize>,
}

#[derive(Debug, Serialize)]
pub struct OutputPublished {
    pub revision: OutputRevision,
    pub created: bool,
    /// Description, provenance or actor differs; retry wall-clock time is not metadata.
    pub metadata_ignored: bool,
}

#[derive(Debug, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum OutputContent {
    Available {
        document: Document,
    },
    Missing,
    Corrupt,
    Unreadable {
        diagnostic: String,
    },
    /// Bytes verify, but a line exceeds the bounded display budget; native reads remain available.
    Unrenderable {
        diagnostic: String,
    },
    Purged,
}

#[derive(Debug, Serialize)]
pub struct OutputRead {
    pub revision: OutputRevision,
    pub path: PathBuf,
    pub content: OutputContent,
}

pub fn validate_slug(slug: &str) -> Result<(), Error> {
    if slug.is_empty()
        || slug.len() > 100
        || !slug
            .bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
        || slug.starts_with('-')
        || slug.ends_with('-')
    {
        return Err(Error::Invalid(
            "output slug: use 1..100 lowercase ASCII letters, digits and internal hyphens",
        ));
    }
    Ok(())
}

pub fn validate_digest(digest: &str) -> Result<(), Error> {
    if digest.strip_prefix("sha256:").is_none_or(|hex| {
        hex.len() != 64
            || !hex
                .bytes()
                .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
    }) {
        return Err(Error::Invalid(
            "digest: expected sha256: followed by 64 lowercase hexadecimal characters",
        ));
    }
    Ok(())
}

impl Store {
    pub(super) fn document_root(&self) -> Result<PathBuf, Error> {
        let path = self.connection.path().ok_or(Error::Invalid(
            "document operations require an on-disk workspace catalog",
        ))?;
        let root = Path::new(path)
            .parent()
            .ok_or(Error::Invalid("catalog has no storage root"))?;
        if !root.is_absolute() {
            return Err(Error::Invalid(
                "catalog path must be absolute for document operations",
            ));
        }
        Ok(root.to_owned())
    }

    /// Publish complete bytes before catalog registration. The lock spans both operations.
    pub fn publish_output(
        &mut self,
        project: &Project,
        input: &PublishOutput<'_>,
        actor: Actor<'_>,
    ) -> Result<OutputPublished, Error> {
        validate_publication(input)?;
        self.registered_project(project)?;
        let root = self.document_root()?;
        let _lock = MutationLock::acquire(&root.join("mutation.lock"))?;
        if let Some(task) = input.task {
            let existing = self.get(project, task)?;
            self.check_authority(task, actor)?;
            if !output_discovery::attached(&self.connection, project, task, input.slug)?
                && let Some(owner) = existing.claimed_by_other(actor.name)
            {
                return Err(Error::OwnedByAnother {
                    id: task.into(),
                    owner: owner.into(),
                });
            }
        }
        // Read exactly once; hashing and staging use the same bounded snapshot.
        let content = text::read(input.source, MAX_OUTPUT_BYTES)?
            .ok_or_else(|| Error::Output("output source is missing".into()))?;
        let digest = text::revision(Some(&content));
        if self.digest_purged(project, &digest)? {
            return Err(Error::Output(
                "output digest is purged; republishing is refused".into(),
            ));
        }
        let existing = self.output_identity(project, input.slug)?;
        if let Some((filename, media_type)) = &existing {
            if filename != input.filename || media_type != input.media_type {
                return Err(Error::Output(
                    "output slug identity differs; choose a distinct slug".into(),
                ));
            }
            if input.mode == PublishMode::Create {
                let sole_match: bool = self.connection.query_row(
                    "SELECT COUNT(*)=1 AND MIN(digest)=?3 FROM output_revisions WHERE project=?1 AND slug=?2",
                    params![project.as_str(),input.slug,digest], |row|row.get(0))?;
                if !sole_match {
                    return Err(Error::Output(
                        "output already exists; extend explicitly or choose a distinct slug".into(),
                    ));
                }
            }
        } else if input.mode == PublishMode::Extend {
            return Err(Error::Output(
                "output does not exist; first publication requires --create".into(),
            ));
        }
        let prior = self.find_output_revision(project, input.slug, &digest)?;
        let relative = PathBuf::from(format!(
            "workspaces/{}/outputs/{}/sha256-{}/{}",
            project.as_str(),
            input.slug,
            &digest[7..],
            input.filename
        ));
        immutable::publish(
            &root,
            &relative,
            &content,
            if prior.is_some() {
                Presence::Required
            } else {
                Presence::MayCreate
            },
        )?;
        #[cfg(test)]
        crate::test_pause(&root, "published");
        let tx = self.write()?;
        tx.execute("INSERT INTO outputs(project,slug,filename,media_type) VALUES(?1,?2,?3,?4) ON CONFLICT DO NOTHING", params![project.as_str(),input.slug,input.filename,input.media_type])?;
        let created = tx.execute("INSERT INTO output_revisions(project,slug,digest,bytes,path,created_at,actor,description,provenance) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9) ON CONFLICT DO NOTHING", params![project.as_str(),input.slug,digest,u32::try_from(content.len()).expect("bounded output length"),relative.to_str(),actor.now,actor.name.map(Owner::as_str),input.description,input.provenance])? != 0;
        if let Some(task) = input.task {
            output_discovery::associate(
                &tx,
                project,
                task,
                input.slug,
                Association::Attach,
                actor,
            )?;
        }
        tx.commit()?;
        #[cfg(test)]
        crate::test_pause(&root, "committed");
        let revision = self
            .find_output_revision(project, input.slug, &digest)?
            .expect("committed output exists");
        let metadata_ignored = !created
            && (revision.description != input.description
                || revision.provenance != input.provenance
                || revision.actor.as_deref() != actor.name.map(Owner::as_str));
        Ok(OutputPublished {
            revision,
            created,
            metadata_ignored,
        })
    }

    fn output_identity(
        &self,
        project: &Project,
        slug: &str,
    ) -> Result<Option<(String, String)>, Error> {
        Ok(self
            .connection
            .query_row(
                "SELECT filename,media_type FROM outputs WHERE project=?1 AND slug=?2",
                params![project.as_str(), slug],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?)
    }

    pub(super) fn digest_purged(&self, project: &Project, digest: &str) -> Result<bool, Error> {
        Ok(self.connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM purged_digests WHERE project=?1 AND digest=?2)",
            params![project.as_str(), digest],
            |row| row.get(0),
        )?)
    }

    pub(super) fn find_output_revision(
        &self,
        project: &Project,
        slug: &str,
        digest: &str,
    ) -> Result<Option<OutputRevision>, Error> {
        Ok(self.connection.query_row(
            "SELECT r.slug,r.digest,r.bytes,r.path,r.created_at,r.actor,r.description,r.provenance,EXISTS(SELECT 1 FROM purged_digests p WHERE p.project=r.project AND p.digest=r.digest) FROM output_revisions r WHERE r.project=?1 AND r.slug=?2 AND r.digest=?3",
            params![project.as_str(),slug,digest],revision_row).optional()?)
    }

    /// Up to 20 revisions, ordered by publication time without an authoritative head.
    pub fn output_manifest(
        &mut self,
        project: &Project,
        slug: &str,
        offset: usize,
    ) -> Result<OutputManifest, Error> {
        validate_slug(slug)?;
        self.registered_project(project)?;
        let (filename, media_type) = self.output_identity(project, slug)?.ok_or_else(|| {
            Error::Output(format!("output {slug} does not exist in this project"))
        })?;
        let mut statement=self.connection.prepare("SELECT r.slug,r.digest,r.bytes,r.path,r.created_at,r.actor,r.description,r.provenance,EXISTS(SELECT 1 FROM purged_digests p WHERE p.project=r.project AND p.digest=r.digest) FROM output_revisions r WHERE r.project=?1 AND r.slug=?2 ORDER BY r.created_at,r.digest LIMIT 21 OFFSET ?3")?;
        let mut revisions = statement
            .query_map(
                params![
                    project.as_str(),
                    slug,
                    i64::try_from(offset).map_err(|_| Error::Invalid("offset"))?
                ],
                revision_row,
            )?
            .collect::<Result<Vec<_>, _>>()?;
        let next_offset = (revisions.len() > 20).then(|| offset + 20);
        revisions.truncate(20);
        Ok(OutputManifest {
            slug: slug.into(),
            filename,
            media_type,
            revisions,
            next_offset,
        })
    }

    /// Verify exact bytes and recheck purge state at the read's linearization point.
    pub fn read_output(
        &mut self,
        project: &Project,
        slug: &str,
        digest: &str,
        start: usize,
        lines: usize,
    ) -> Result<OutputRead, Error> {
        validate_slug(slug)?;
        validate_digest(digest)?;
        text::validate_page(start, lines)?;
        self.registered_project(project)?;
        let mut revision = self
            .find_output_revision(project, slug, digest)?
            .ok_or_else(|| {
                Error::Output("output revision does not exist in this project".into())
            })?;
        let root = self.document_root()?;
        let path = root.join(&revision.path);
        let content = match verified_text::inspect(
            &root,
            &revision.path,
            digest,
            revision.bytes,
            MAX_OUTPUT_BYTES,
        ) {
            verified_text::State::Available(content) => {
                match text::page(&path, Some(&content), start, lines) {
                    Ok(document) => OutputContent::Available { document },
                    Err(error) => OutputContent::Unrenderable {
                        diagnostic: error.to_string(),
                    },
                }
            }
            verified_text::State::Missing => OutputContent::Missing,
            verified_text::State::Corrupt => OutputContent::Corrupt,
            verified_text::State::Unreadable(diagnostic) => {
                OutputContent::Unreadable { diagnostic }
            }
        };
        revision.purged = self.digest_purged(project, digest)?;
        let content = if revision.purged {
            OutputContent::Purged
        } else {
            content
        };
        Ok(OutputRead {
            revision,
            path,
            content,
        })
    }
}

fn validate_publication(input: &PublishOutput<'_>) -> Result<(), Error> {
    validate_slug(input.slug)?;
    paths::validate_relative(Path::new(input.filename))?;
    if input.filename.contains('/') || input.filename.len() > 128 {
        return Err(Error::Invalid(
            "output filename: expected one component of at most 128 bytes",
        ));
    }
    if !matches!(
        input.media_type,
        "text/markdown" | "text/plain" | "application/json"
    ) {
        return Err(Error::Invalid(
            "media type: choose text/markdown, text/plain, application/json",
        ));
    }
    if input.description.len() > 4096 || input.provenance.len() > 4096 {
        return Err(Error::Invalid(
            "output description and provenance must each be at most 4096 bytes",
        ));
    }
    Ok(())
}

fn revision_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<OutputRevision> {
    Ok(OutputRevision {
        slug: row.get(0)?,
        digest: row.get(1)?,
        bytes: row.get(2)?,
        path: PathBuf::from(row.get::<_, String>(3)?),
        created_at: row.get(4)?,
        actor: row.get(5)?,
        description: row.get(6)?,
        provenance: row.get(7)?,
        purged: row.get(8)?,
    })
}
