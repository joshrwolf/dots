//! Foreign trees and other machines' answers into the local catalog.
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use dots_documents::mutation_lock::MutationLock;
use dots_documents::staged_file::StagedFile;
use dots_documents::{directories, immutable, text};
use dots_substrate::{Blob, Error as SubstrateError, Hash, Version};
use rusqlite::{OptionalExtension as _, Transaction, params};
use serde::Serialize;

use super::export::TREE;
use super::format::{
    Answers, DecisionRecord, Manifest, OutputRecord, RequestRecord, RevisionState, Segment,
    TaskRecord, decode, share,
};
use super::{Context, imported_workspace, unix};
use crate::store::{Error, Project, Store};

/// What one import changed.
#[derive(Debug, Default, Serialize)]
pub struct Imported {
    /// Foreign trees whose new versions were imported.
    pub trees: Vec<String>,
    /// Home trees whose proposed versions the remote confirmed.
    pub confirmed: Vec<String>,
    /// Home trees another machine took over; changes this machine had not
    /// yet flushed for them stay only in its local history.
    pub lost: Vec<String>,
    /// Answers from other machines recorded on requests homed here.
    pub answers: Vec<String>,
    /// Trees that could not be imported, with the reason.
    pub errors: Vec<(String, String)>,
}

/// The local record of a replicated tree.
#[derive(Debug, Clone)]
pub(super) struct SharedRow {
    pub project: String,
    pub shared_project: String,
    pub home: String,
    pub version: Option<String>,
    pub manifest: Option<Hash>,
    pub proposed: Option<Hash>,
    pub written_at: Option<i64>,
    pub complete: bool,
}

pub(super) fn shared_row(store: &mut Store, root: &str) -> Result<Option<SharedRow>, Error> {
    let row = store
        .connection
        .query_row(
            "SELECT project, shared_project, home, version, manifest, proposed, written_at, complete
             FROM shared_trees WHERE root=?1",
            [root],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, Option<String>>(5)?,
                    row.get::<_, Option<i64>>(6)?,
                    row.get::<_, bool>(7)?,
                ))
            },
        )
        .optional()?;
    let Some((project, shared_project, home, version, manifest, proposed, written_at, complete)) =
        row
    else {
        return Ok(None);
    };
    let hash = |value: Option<String>| {
        value
            .map(|value| value.parse::<Hash>())
            .transpose()
            .map_err(Error::from)
    };
    Ok(Some(SharedRow {
        project,
        shared_project,
        home,
        version,
        manifest: hash(manifest)?,
        proposed: hash(proposed)?,
        written_at,
        complete,
    }))
}

/// How many events the tree of `root` holds here.
pub(super) fn tree_events(store: &mut Store, root: &str) -> Result<usize, Error> {
    let count: i64 = store.connection.query_row(
        &format!("SELECT COUNT(*) FROM events WHERE task IN ({TREE})"),
        [root],
        |row| row.get(0),
    )?;
    Ok(usize::try_from(count).expect("SQLite COUNT is nonnegative"))
}

#[expect(
    clippy::too_many_lines,
    reason = "one pass over listed trees confirms, diffs or applies each in turn"
)]
pub(super) fn import(
    store: &mut Store,
    context: Context<'_>,
    now: SystemTime,
) -> Result<Imported, Error> {
    let mut imported = Imported::default();
    let names = match context.substrate.names("tasks/") {
        Ok(names) => names,
        Err(SubstrateError::Unsynced) => return Ok(imported),
        Err(error) => return Err(error.into()),
    };
    let machine = context.machine().as_str().to_owned();
    let document_root = store.document_root()?;
    let _lock = MutationLock::acquire(&document_root.join("mutation.lock"))?;
    for name in names {
        let mut parts = name.as_str().splitn(3, '/').skip(1);
        let (Some(shared), Some(root)) = (parts.next(), parts.next()) else {
            continue;
        };
        let Some(binding) = context
            .bindings
            .iter()
            .find(|binding| binding.shared.as_str() == shared)
        else {
            continue;
        };
        let value = match context.substrate.read(&name) {
            Ok(Some(value)) => value,
            Ok(None) | Err(SubstrateError::NotCached(_)) => continue,
            Err(error) => return Err(error.into()),
        };
        let row = shared_row(store, root)?;
        if let Some(row) = &row {
            if row.version.as_deref() == Some(value.version.as_str()) && row.complete {
                continue;
            }
            if row.home == machine && row.proposed == Some(value.root) {
                confirm(store, root, &value.version, value.root, true)?;
                imported.confirmed.push(root.to_owned());
                continue;
            }
        }
        let manifest: Manifest = decode(&context.substrate.blob(&value.root)?, "tree manifest")?;
        if manifest.project != shared || manifest.root != root {
            imported.errors.push((
                root.to_owned(),
                format!(
                    "{name} holds the manifest of {}/{}",
                    manifest.project, manifest.root
                ),
            ));
            continue;
        }
        if manifest.home.as_str() == machine {
            let settled = row.as_ref().and_then(|row| row.proposed) == Some(value.root);
            confirm(store, root, &value.version, value.root, settled)?;
            continue;
        }
        if row
            .as_ref()
            .is_some_and(|row| row.home == machine && row.proposed.is_some())
        {
            imported.lost.push(root.to_owned());
        }
        // Diff against the version imported before, when there was one.
        let previous = match row.as_ref() {
            Some(row) if row.home != machine && row.complete => match row.manifest {
                Some(hash) => context
                    .substrate
                    .blob(&hash)
                    .ok()
                    .map(|blob| decode::<Manifest>(&blob, "tree manifest"))
                    .transpose()?,
                None => None,
            },
            _ => None,
        };
        let tx = store.write()?;
        match apply(
            &tx,
            context,
            &document_root,
            &binding.local,
            &manifest,
            previous.as_ref(),
            now,
        ) {
            Ok(complete) => {
                tx.execute(
                    "INSERT INTO shared_trees(root, project, shared_project, home, version, manifest, proposed,
                       brief, written_at, complete, imported_at)
                     VALUES(?1, ?2, ?3, ?4, ?5, ?6, NULL, ?7, ?8, ?9, ?10)
                     ON CONFLICT(root) DO UPDATE SET project=excluded.project, shared_project=excluded.shared_project,
                       home=excluded.home, version=excluded.version, manifest=excluded.manifest, proposed=NULL,
                       brief=excluded.brief, written_at=excluded.written_at, complete=excluded.complete,
                       imported_at=excluded.imported_at",
                    params![
                        root,
                        binding.local.as_str(),
                        shared,
                        manifest.home.as_str(),
                        value.version.as_str(),
                        value.root.to_string(),
                        manifest
                            .goal
                            .as_ref()
                            .and_then(|goal| goal.content)
                            .map(|hash| hash.to_string()),
                        manifest.written_at,
                        complete,
                        unix(now),
                    ],
                )?;
                tx.commit()?;
                imported.trees.push(root.to_owned());
            }
            Err(error) => {
                drop(tx);
                imported.errors.push((root.to_owned(), describe(&error)));
            }
        }
    }
    import_answers(store, context, &mut imported)?;
    Ok(imported)
}

pub(super) fn confirm(
    store: &mut Store,
    root: &str,
    version: &Version,
    manifest: Hash,
    settled: bool,
) -> Result<(), Error> {
    store.connection.execute(
        "UPDATE shared_trees SET version=?2, manifest=?3, proposed=CASE WHEN ?4 THEN NULL ELSE proposed END
         WHERE root=?1",
        params![root, version.as_str(), manifest.to_string(), settled],
    )?;
    Ok(())
}

/// Apply a foreign manifest to the catalog. Returns whether every record
/// found what it refers to; an incomplete tree is imported again later.
#[expect(
    clippy::too_many_lines,
    reason = "a tree applies its record kinds in dependency order"
)]
fn apply(
    tx: &Transaction<'_>,
    context: Context<'_>,
    document_root: &Path,
    project: &Project,
    manifest: &Manifest,
    previous: Option<&Manifest>,
    now: SystemTime,
) -> Result<bool, Error> {
    let mut complete = true;
    if manifest.moved_to.is_some() {
        return Ok(true);
    }
    let changed = |current: &BTreeMap<String, Hash>, earlier: Option<&BTreeMap<String, Hash>>| {
        current
            .iter()
            .filter(|(id, hash)| earlier.is_none_or(|earlier| earlier.get(*id) != Some(*hash)))
            .map(|(_, hash)| *hash)
            .collect::<Vec<_>>()
    };
    let blob = |hash: &Hash| context.substrate.blob(hash).map_err(Error::from);

    let tasks: Vec<TaskRecord> = changed(&manifest.tasks, previous.map(|p| &p.tasks))
        .iter()
        .map(|hash| decode(&blob(hash)?, "task record"))
        .collect::<Result<_, _>>()?;
    // Parents first.
    let mut pending: Vec<&TaskRecord> = tasks.iter().collect();
    while !pending.is_empty() {
        let before = pending.len();
        let mut rest = Vec::new();
        for record in pending {
            let parent_ready = match &record.parent {
                None => true,
                Some(parent) => tx
                    .query_row("SELECT 1 FROM tasks WHERE id=?1", [parent], |_| Ok(()))
                    .optional()?
                    .is_some(),
            };
            if parent_ready {
                upsert_task(tx, project, record)?;
            } else {
                rest.push(record);
            }
        }
        if rest.len() == before {
            return Err(share(format!(
                "tree {} names parents it does not contain",
                manifest.root
            )));
        }
        pending = rest;
    }
    for record in &tasks {
        tx.execute("DELETE FROM deps WHERE task=?1", [&record.id])?;
        for blocker in &record.blocked_by {
            if tx.execute(
                "INSERT INTO deps(task, blocked_by) SELECT ?1, id FROM tasks WHERE id=?2",
                params![record.id, blocker],
            )? == 0
            {
                complete = false;
            }
        }
        tx.execute("DELETE FROM task_refs WHERE task=?1", [&record.id])?;
        for reference in &record.references {
            tx.execute(
                "INSERT INTO task_refs(task, url, label) VALUES(?1, ?2, ?3)",
                params![record.id, reference.url, reference.label],
            )?;
        }
    }

    match &manifest.goal {
        Some(goal) => {
            tx.execute(
                "INSERT INTO goals(root, brief) VALUES(?1, ?2) ON CONFLICT(root) DO UPDATE SET brief=excluded.brief",
                params![manifest.root, goal.brief],
            )?;
            if let Some(hash) = goal.content {
                let workspace = document_root.join("workspaces").join(project.as_str());
                let target = imported_workspace(&workspace, &manifest.root).join(&goal.brief);
                write_replacing(document_root, &target, blob(&hash)?.content())?;
            }
        }
        None => {
            tx.execute("DELETE FROM goals WHERE root=?1", [&manifest.root])?;
        }
    }

    for hash in changed(&manifest.outputs, previous.map(|p| &p.outputs)) {
        let record: OutputRecord = decode(&blob(&hash)?, "output record")?;
        import_output(tx, context, document_root, project, &record, now)?;
    }
    for record in &tasks {
        tx.execute("DELETE FROM task_outputs WHERE task=?1", [&record.id])?;
        for slug in &record.outputs {
            if tx.execute(
                "INSERT INTO task_outputs(task, project, slug)
                 SELECT ?1, project, slug FROM outputs WHERE project=?2 AND slug=?3",
                params![record.id, project.as_str(), slug],
            )? == 0
            {
                complete = false;
            }
        }
    }

    let mut requests: Vec<RequestRecord> =
        changed(&manifest.requests, previous.map(|p| &p.requests))
            .iter()
            .map(|hash| decode(&blob(hash)?, "request record"))
            .collect::<Result<_, _>>()?;
    requests.sort_by(|a, b| (a.created_at, &a.id).cmp(&(b.created_at, &b.id)));
    for record in &requests {
        upsert_request(tx, project, record)?;
        tx.execute("DELETE FROM request_choices WHERE request=?1", [&record.id])?;
        for (position, choice) in record.choices.iter().enumerate() {
            tx.execute(
                "INSERT INTO request_choices(request, position, id, label, detail) VALUES(?1, ?2, ?3, ?4, ?5)",
                params![record.id, i64::try_from(position).expect("at most 8 choices"), choice.id, choice.label, choice.detail],
            )?;
        }
        tx.execute("DELETE FROM request_tasks WHERE request=?1", [&record.id])?;
        for task in &record.tasks {
            if tx.execute(
                "INSERT INTO request_tasks(request, task) SELECT ?1, id FROM tasks WHERE id=?2",
                params![record.id, task],
            )? == 0
            {
                complete = false;
            }
        }
    }
    for hash in changed(&manifest.decisions, previous.map(|p| &p.decisions)) {
        let record: DecisionRecord = decode(&blob(&hash)?, "decision record")?;
        tx.execute(
            "INSERT INTO decisions(id, project, goal, kind, decision, human_text, human_source, human_provenance,
               request, at, actor)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11) ON CONFLICT(id) DO NOTHING",
            params![
                record.id,
                project.as_str(),
                record.goal,
                record.kind,
                record.decision,
                record.human_text,
                record.human_source,
                record.human_provenance,
                record.request,
                record.at,
                record.actor
            ],
        )?;
    }

    let seen: BTreeSet<Hash> = previous
        .map(|previous| previous.segments.iter().copied().collect())
        .unwrap_or_default();
    for hash in manifest.segments.iter().filter(|hash| !seen.contains(hash)) {
        let segment: Segment = decode(&blob(hash)?, "event segment")?;
        for event in segment.events {
            let exists = tx
                .query_row(
                    "SELECT 1 FROM events WHERE uid=?1",
                    [&event.uid],
                    |_| Ok(()),
                )
                .optional()?
                .is_some();
            if exists {
                continue;
            }
            let predecessor: Option<i64> = match &event.predecessor {
                Some(uid) => tx
                    .query_row("SELECT seq FROM events WHERE uid=?1", [uid], |row| {
                        row.get(0)
                    })
                    .optional()?,
                None => None,
            };
            let inserted = tx.execute(
                "INSERT INTO events(task, at, actor, kind, detail, result, predecessor, operation, uid, published)
                 SELECT id, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 1 FROM tasks WHERE id=?1",
                params![
                    event.task,
                    event.at,
                    event.actor,
                    event.kind,
                    event.detail,
                    event.result,
                    predecessor,
                    event.operation,
                    event.uid
                ],
            )?;
            if inserted == 0 {
                complete = false;
                continue;
            }
            let seq = tx.last_insert_rowid();
            for evidence in &event.evidence {
                tx.execute(
                    "INSERT OR IGNORE INTO event_evidence(event, project, slug, digest)
                     SELECT ?1, project, slug, digest FROM output_revisions
                     WHERE project=?2 AND slug=?3 AND digest=?4",
                    params![seq, project.as_str(), evidence.slug, evidence.digest],
                )?;
            }
        }
    }

    // Answers sent from here: settled ones leave, pending ones stay visible.
    let sent: Vec<(String, String)> = {
        let mut statement = tx.prepare(&format!(
            "SELECT sent_answers.request, sent_answers.answer FROM sent_answers
             JOIN requests ON requests.id=sent_answers.request WHERE requests.goal IN ({TREE})"
        ))?;
        statement
            .query_map([&manifest.root], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<Result<_, _>>()?
    };
    for (request, answer) in sent {
        let state: String = tx.query_row(
            "SELECT state FROM requests WHERE id=?1",
            [&request],
            |row| row.get(0),
        )?;
        if state == "open" {
            let answer: super::format::AnswerRecord = serde_json::from_str(&answer)
                .map_err(|error| share(format!("stored answer for {request}: {error}")))?;
            overlay_answer(tx, &request, &answer)?;
        } else {
            tx.execute("DELETE FROM sent_answers WHERE request=?1", [&request])?;
            tx.execute("UPDATE sharing SET answers_dirty=1", [])?;
        }
    }
    tx.execute(
        &format!("DELETE FROM dirty_tasks WHERE task IN ({TREE})"),
        [&manifest.root],
    )?;
    Ok(complete)
}

fn upsert_task(tx: &Transaction<'_>, project: &Project, record: &TaskRecord) -> Result<(), Error> {
    tx.execute(
        "INSERT INTO tasks(id, project, parent, title, body, result, status, branch, owner, created_at, updated_at, closed_at)
         VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, NULL, ?9, ?10, ?11)
         ON CONFLICT(id) DO UPDATE SET project=excluded.project, parent=excluded.parent, title=excluded.title,
           body=excluded.body, result=excluded.result, status=excluded.status, branch=excluded.branch, owner=NULL,
           created_at=excluded.created_at, updated_at=excluded.updated_at, closed_at=excluded.closed_at",
        params![
            record.id,
            project.as_str(),
            record.parent,
            record.title,
            record.body,
            record.result,
            record.status,
            record.branch,
            record.created_at,
            record.updated_at,
            record.closed_at
        ],
    )?;
    Ok(())
}

fn upsert_request(
    tx: &Transaction<'_>,
    project: &Project,
    record: &RequestRecord,
) -> Result<(), Error> {
    let answer = record.answer.as_ref();
    let closure = record.closure.as_ref();
    tx.execute(
        "INSERT INTO requests(id, project, goal, key, kind, title, question, context, recommendation, state,
           created_at, created_by, supersedes, answer_choice, answer_text, answer_source, answer_provenance,
           answer_interpretation, answered_at, answered_by, closure, closed_at, closed_by, close_reason)
         VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20,
           ?21, ?22, ?23, ?24)
         ON CONFLICT(id) DO UPDATE SET state=excluded.state, answer_choice=excluded.answer_choice,
           answer_text=excluded.answer_text, answer_source=excluded.answer_source,
           answer_provenance=excluded.answer_provenance, answer_interpretation=excluded.answer_interpretation,
           answered_at=excluded.answered_at, answered_by=excluded.answered_by, closure=excluded.closure,
           closed_at=excluded.closed_at, closed_by=excluded.closed_by, close_reason=excluded.close_reason",
        params![
            record.id,
            project.as_str(),
            record.goal,
            record.key,
            record.kind,
            record.title,
            record.question,
            record.context,
            record.recommendation,
            record.state,
            record.created_at,
            record.created_by,
            record.supersedes,
            answer.and_then(|answer| answer.choice.clone()),
            answer.and_then(|answer| answer.text.clone()),
            answer.map(|answer| answer.source.clone()),
            answer.map(|answer| answer.provenance.clone()),
            answer.and_then(|answer| answer.interpretation.clone()),
            answer.map(|answer| answer.at),
            answer.and_then(|answer| answer.by.clone()),
            closure.map(|closure| closure.kind.clone()),
            closure.map(|closure| closure.at),
            closure.and_then(|closure| closure.by.clone()),
            closure.and_then(|closure| closure.reason.clone()),
        ],
    )?;
    Ok(())
}

/// Show an answer sent from here on the local copy of its request.
pub(crate) fn overlay_answer(
    tx: &Transaction<'_>,
    request: &str,
    answer: &super::format::AnswerRecord,
) -> Result<(), Error> {
    tx.execute(
        "UPDATE requests SET state='answered', answer_choice=?2, answer_text=?3, answer_source=?4,
           answer_provenance=?5, answer_interpretation=?6, answered_at=?7, answered_by=?8
         WHERE id=?1 AND state='open'",
        params![
            request,
            answer.choice,
            answer.text,
            answer.source,
            answer.provenance,
            answer.interpretation,
            answer.at,
            answer.by
        ],
    )?;
    Ok(())
}

fn import_output(
    tx: &Transaction<'_>,
    context: Context<'_>,
    document_root: &Path,
    project: &Project,
    record: &OutputRecord,
    now: SystemTime,
) -> Result<(), Error> {
    tx.execute(
        "INSERT INTO outputs(project, slug, filename, media_type) VALUES(?1, ?2, ?3, ?4) ON CONFLICT DO NOTHING",
        params![project.as_str(), record.slug, record.filename, record.media_type],
    )?;
    let filename: String = tx.query_row(
        "SELECT filename FROM outputs WHERE project=?1 AND slug=?2",
        params![project.as_str(), record.slug],
        |row| row.get(0),
    )?;
    for revision in &record.revisions {
        let digest = revision.digest.to_string();
        let relative = PathBuf::from(format!(
            "workspaces/{}/outputs/{}/sha256-{}/{}",
            project.as_str(),
            record.slug,
            revision.digest.hex(),
            filename
        ));
        match revision.state {
            RevisionState::Available => {
                let text =
                    String::from_utf8(context.substrate.blob(&revision.digest)?.into_content())
                        .map_err(|_| {
                            share(format!("output {} {digest} is not UTF-8", record.slug))
                        })?;
                immutable::publish(
                    document_root,
                    &relative,
                    &text,
                    immutable::Presence::MayCreate,
                )?;
            }
            RevisionState::Purged => {
                tx.execute(
                    "INSERT OR IGNORE INTO purged_digests(project, digest, at, actor) VALUES(?1, ?2, ?3, NULL)",
                    params![project.as_str(), digest, unix(now)],
                )?;
            }
            RevisionState::Missing => {}
        }
        tx.execute(
            "INSERT INTO output_revisions(project, slug, digest, bytes, path, created_at, actor, description, provenance)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9) ON CONFLICT DO NOTHING",
            params![
                project.as_str(),
                record.slug,
                digest,
                revision.bytes,
                relative.to_str(),
                revision.created_at,
                revision.actor,
                revision.description,
                revision.provenance
            ],
        )?;
    }
    Ok(())
}

/// Replace a machine-written copy atomically. The caller holds the mutation lock.
fn write_replacing(document_root: &Path, target: &Path, content: &[u8]) -> Result<(), Error> {
    use std::io::Write as _;
    if text::read_bytes(target, content.len())
        .ok()
        .flatten()
        .as_deref()
        == Some(content)
    {
        return Ok(());
    }
    let parent = target
        .parent()
        .ok_or_else(|| share(format!("{} has no parent", target.display())))?;
    directories::create_within(parent, document_root)?;
    let staged = StagedFile::create(parent)?;
    staged
        .file()
        .write_all(content)
        .map_err(|error| share(format!("write {}: {error}", staged.path().display())))?;
    staged.publish_replace(target)?;
    Ok(())
}

/// Record answers that other machines sent for open requests homed here.
fn import_answers(
    store: &mut Store,
    context: Context<'_>,
    imported: &mut Imported,
) -> Result<(), Error> {
    let names = match context.substrate.names("answers/") {
        Ok(names) => names,
        Err(SubstrateError::Unsynced) => return Ok(()),
        Err(error) => return Err(error.into()),
    };
    let own = super::answers_ref(context.machine())?;
    for name in names.into_iter().filter(|name| *name != own) {
        let value = match context.substrate.read(&name) {
            Ok(Some(value)) => value,
            Ok(None) | Err(SubstrateError::NotCached(_)) => continue,
            Err(error) => return Err(error.into()),
        };
        let answers: Answers = decode(&context.substrate.blob(&value.root)?, "answer list")?;
        for sent in answers.sent {
            let tx = store.write()?;
            let open = tx
                .query_row(
                    "SELECT 1 FROM requests WHERE id=?1 AND goal=?2 AND state='open'",
                    params![sent.request, sent.goal],
                    |_| Ok(()),
                )
                .optional()?
                .is_some();
            if !open || crate::store::foreign(&tx, &sent.goal)?.is_some() {
                continue;
            }
            if let Some(choice) = &sent.answer.choice {
                let known = tx
                    .query_row(
                        "SELECT 1 FROM request_choices WHERE request=?1 AND id=?2",
                        params![sent.request, choice],
                        |_| Ok(()),
                    )
                    .optional()?
                    .is_some();
                if !known {
                    continue;
                }
            }
            overlay_answer(&tx, &sent.request, &sent.answer)?;
            tx.execute(
                "INSERT INTO events(task, at, actor, kind, detail, uid)
                 VALUES(?1, ?2, ?3, 'request-answered', ?4, lower(hex(randomblob(16))))",
                params![sent.goal, sent.answer.at, sent.answer.by, sent.request],
            )?;
            tx.commit()?;
            imported.answers.push(sent.request);
        }
    }
    Ok(())
}

/// Refuse a takeover that would replace a different local brief at the
/// goal's path.
pub(super) fn check_brief(
    store: &mut Store,
    project: &Project,
    root: &str,
    manifest: &Manifest,
) -> Result<(), Error> {
    let Some(goal) = &manifest.goal else {
        return Ok(());
    };
    let workspace = store
        .document_root()?
        .join("workspaces")
        .join(project.as_str());
    let local = workspace.join(&goal.brief);
    let imported = imported_workspace(&workspace, root).join(&goal.brief);
    let read = |path: &Path| {
        text::read_bytes(path, dots_substrate::MAX_BLOB_BYTES)
            .ok()
            .flatten()
    };
    match (read(&local), read(&imported)) {
        (Some(local_bytes), Some(imported_bytes)) if local_bytes != imported_bytes => {
            Err(share(format!(
                "{} already exists here with different content; move it aside before taking over {root}",
                local.display()
            )))
        }
        _ => Ok(()),
    }
}

/// Record a completed takeover and place the brief where the home edits it.
pub(super) fn taken(
    store: &mut Store,
    project: &Project,
    root: &str,
    manifest: &Manifest,
    blob: &Blob,
    version: &Version,
) -> Result<(), Error> {
    let document_root = store.document_root()?;
    let _lock = MutationLock::acquire(&document_root.join("mutation.lock"))?;
    if let Some(goal) = &manifest.goal {
        let workspace = document_root.join("workspaces").join(project.as_str());
        let imported = imported_workspace(&workspace, root).join(&goal.brief);
        if let Some(bytes) = text::read_bytes(&imported, dots_substrate::MAX_BLOB_BYTES)? {
            write_replacing(&document_root, &workspace.join(&goal.brief), &bytes)?;
        }
    }
    store.connection.execute(
        "UPDATE shared_trees SET home=?2, version=?3, manifest=?4, proposed=NULL, written_at=?5 WHERE root=?1",
        params![
            root,
            manifest.home.as_str(),
            version.as_str(),
            blob.hash().to_string(),
            manifest.written_at
        ],
    )?;
    Ok(())
}

/// An error with its sources, for reports that outlive it.
fn describe(error: &Error) -> String {
    let mut text = error.to_string();
    let mut source = std::error::Error::source(error);
    while let Some(cause) = source {
        text.push_str(": ");
        text.push_str(&cause.to_string());
        source = cause.source();
    }
    text
}
