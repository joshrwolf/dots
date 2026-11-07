//! Home trees and sent answers into deferred substrate updates.
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::time::SystemTime;

use dots_documents::{text, verified_text};
use dots_substrate::{Blob, Error as SubstrateError, Hash, MAX_BLOB_BYTES, Update, Version};
use rusqlite::{OptionalExtension as _, Transaction, params};
use serde::Serialize;

use super::format::{
    AnswerRecord, Answers, ChoiceRecord, ClosureRecord, DecisionRecord, EventRecord,
    EvidenceRecord, FORMAT, GoalRecord, Manifest, OutputRecord, ReferenceRecord, RequestRecord,
    RevisionRecord, RevisionState, SEGMENT_TARGET, Segment, SentAnswer, TaskRecord, decode, encode,
    share,
};
use super::{Context, SharedProject, Withheld, WithheldKind, answers_ref, tree_ref, unix};
use crate::outputs::MAX_OUTPUT_BYTES;
use crate::store::{Error, Store};

/// What one export deferred.
#[derive(Debug, Default, Serialize)]
pub struct Exported {
    /// Roots whose trees changed and now wait in the outbox.
    pub trees: Vec<String>,
    /// Whether this machine's answers changed and wait in the outbox.
    pub answers: bool,
    /// Trees that could not be exported, with the reason; their changes stay
    /// queued and the next export retries them. Other trees still export.
    pub errors: Vec<(String, String)>,
}

/// One tree's local state, encoded.
struct Built {
    manifest: Manifest,
    /// Freshly encoded blobs by hash; others come from the cache.
    blobs: BTreeMap<Hash, Blob>,
    /// Local events placed in this manifest's segments.
    published: Vec<i64>,
}

/// What exporting one tree did.
enum TreeOutcome {
    Deferred,
    Unchanged,
    /// Not exported from here: unbound, or homed elsewhere.
    Skipped,
}

pub(super) fn export(
    store: &mut Store,
    context: Context<'_>,
    now: SystemTime,
) -> Result<Exported, Error> {
    let machine = context.machine().as_str().to_owned();
    let document_root = store.document_root()?;
    let mut exported = Exported::default();
    for (root, project) in candidates(store, &machine)? {
        match export_tree(
            store,
            context,
            &machine,
            &document_root,
            &root,
            &project,
            now,
        ) {
            Ok(TreeOutcome::Deferred) => exported.trees.push(root),
            Ok(TreeOutcome::Unchanged | TreeOutcome::Skipped) => {}
            Err(error) => exported
                .errors
                .push((root, super::import::describe(&error))),
        }
    }
    exported.answers = export_answers(store, context, now)?;
    Ok(exported)
}

/// Defer one tree's current state when it changed. Errors leave the tree's
/// changes queued.
#[expect(
    clippy::too_many_lines,
    reason = "one tree reads, defers and records in turn"
)]
fn export_tree(
    store: &mut Store,
    context: Context<'_>,
    machine: &str,
    document_root: &Path,
    root: &str,
    project: &str,
    now: SystemTime,
) -> Result<TreeOutcome, Error> {
    let Some(shared) = context.shared(project).cloned() else {
        forget(store, root)?;
        return Ok(TreeOutcome::Skipped);
    };
    let name = tree_ref(&shared, root)?;
    let mut row = super::import::shared_row(store, root)?;
    if row.as_ref().is_some_and(|row| row.home != machine) {
        forget(store, root)?;
        return Ok(TreeOutcome::Skipped);
    }
    // A flushed proposal the last import did not confirm yet.
    if let Some(current) = row.as_mut()
        && let Some(proposed) = current.proposed
        && let Ok(Some(value)) = context.substrate.read(&name)
        && value.root == proposed
    {
        super::import::confirm(store, root, &value.version, proposed, true)?;
        current.version = Some(value.version.to_string());
        current.manifest = Some(proposed);
        current.proposed = None;
    }
    let confirmed = match row.as_ref().and_then(|row| row.manifest) {
        Some(hash) => Some(decode::<Manifest>(
            &context.substrate.blob(&hash)?,
            "tree manifest",
        )?),
        None => None,
    };
    let latest = match row.as_ref().and_then(|row| row.proposed) {
        Some(hash) => Some(decode::<Manifest>(
            &context.substrate.blob(&hash)?,
            "tree manifest",
        )?),
        None => confirmed.clone(),
    };
    let tx = store.write()?;
    let built = build(
        &tx,
        context,
        document_root,
        project,
        &shared,
        root,
        latest.as_ref(),
        now,
    )?;
    if latest
        .as_ref()
        .is_some_and(|latest| latest.same_state(&built.manifest))
    {
        tx.execute(
            &format!("DELETE FROM dirty_tasks WHERE task IN ({TREE})"),
            [root],
        )?;
        tx.commit()?;
        return Ok(TreeOutcome::Unchanged);
    }
    let manifest_blob = encode(&built.manifest)?;
    let known = confirmed
        .as_ref()
        .map(Manifest::references)
        .unwrap_or_default();
    let mut new_blobs = vec![manifest_blob.clone()];
    for hash in built.manifest.references() {
        if known.contains(&hash) {
            continue;
        }
        let blob = match built.blobs.get(&hash) {
            Some(blob) => blob.clone(),
            None => context.substrate.blob(&hash)?,
        };
        new_blobs.push(blob);
    }
    let expected = row
        .as_ref()
        .and_then(|row| row.version.clone())
        .map(Version::new)
        .transpose()?;
    context.substrate.defer(
        Update {
            name,
            expected,
            root: manifest_blob.hash(),
            new_blobs,
        },
        now,
    )?;
    tx.execute(
        "INSERT INTO shared_trees(root, project, shared_project, home, proposed, brief, written_at, withheld)
         VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
         ON CONFLICT(root) DO UPDATE SET project=excluded.project, shared_project=excluded.shared_project,
           home=excluded.home, proposed=excluded.proposed, brief=excluded.brief, written_at=excluded.written_at,
           withheld=excluded.withheld",
        params![
            root,
            project,
            shared.as_str(),
            machine,
            manifest_blob.hash().to_string(),
            built
                .manifest
                .goal
                .as_ref()
                .and_then(|goal| goal.content)
                .map(|hash| hash.to_string()),
            built.manifest.written_at,
            super::withheld_column(&built.manifest.withheld)?,
        ],
    )?;
    for seq in &built.published {
        tx.execute("UPDATE events SET published=1 WHERE seq=?1", [seq])?;
    }
    tx.execute(
        &format!("DELETE FROM dirty_tasks WHERE task IN ({TREE})"),
        [root],
    )?;
    tx.commit()?;
    Ok(TreeOutcome::Deferred)
}

/// Every task of the tree rooted at `?1`.
pub(super) const TREE: &str = "WITH RECURSIVE tree(id) AS (SELECT ?1
    UNION SELECT tasks.id FROM tasks JOIN tree ON tasks.parent=tree.id) SELECT id FROM tree";

/// Roots of trees with local changes, plus every home goal tree, whose brief
/// may change without a catalog event.
fn candidates(store: &mut Store, machine: &str) -> Result<BTreeSet<(String, String)>, Error> {
    let tx = store.connection.transaction()?;
    let mut roots = BTreeSet::new();
    {
        let mut statement = tx.prepare(
            "WITH RECURSIVE up(start, id, parent) AS (
                SELECT dirty_tasks.task, tasks.id, tasks.parent FROM dirty_tasks JOIN tasks ON tasks.id=dirty_tasks.task
                UNION SELECT up.start, tasks.id, tasks.parent FROM tasks JOIN up ON tasks.id=up.parent)
             SELECT up.id, tasks.project FROM up JOIN tasks ON tasks.id=up.id WHERE up.parent IS NULL",
        )?;
        let rows = statement.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
        for row in rows {
            roots.insert(row?);
        }
        let mut statement = tx.prepare(
            "SELECT shared_trees.root, shared_trees.project FROM shared_trees
             JOIN goals ON goals.root=shared_trees.root WHERE shared_trees.home=?1",
        )?;
        let rows = statement.query_map([machine], |row| Ok((row.get(0)?, row.get(1)?)))?;
        for row in rows {
            roots.insert(row?);
        }
    }
    // Dirty tasks that no longer exist cannot be exported.
    tx.execute(
        "DELETE FROM dirty_tasks WHERE task NOT IN (SELECT id FROM tasks)",
        [],
    )?;
    tx.commit()?;
    Ok(roots)
}

/// Drop queued changes of a tree this machine does not export.
fn forget(store: &mut Store, root: &str) -> Result<(), Error> {
    store.connection.execute(
        &format!("DELETE FROM dirty_tasks WHERE task IN ({TREE})"),
        [root],
    )?;
    Ok(())
}

#[expect(
    clippy::too_many_arguments,
    clippy::too_many_lines,
    reason = "one tree build reads the catalog, cache and documents in order"
)]
fn build(
    tx: &Transaction<'_>,
    context: Context<'_>,
    document_root: &Path,
    project: &str,
    shared: &SharedProject,
    root: &str,
    latest: Option<&Manifest>,
    now: SystemTime,
) -> Result<Built, Error> {
    let mut blobs = BTreeMap::new();
    let mut withheld = Vec::new();
    let mut add = |blob: Blob| {
        let hash = blob.hash();
        blobs.insert(hash, blob);
        hash
    };
    let members: Vec<String> = {
        let mut statement = tx.prepare(TREE)?;
        statement
            .query_map([root], |row| row.get(0))?
            .collect::<Result<_, _>>()?
    };
    // Tasks new to this tree bring their whole history.
    for id in &members {
        if latest.is_some_and(|latest| !latest.tasks.contains_key(id)) {
            tx.execute("UPDATE events SET published=NULL WHERE task=?1", [id])?;
        }
    }
    let mut tasks = BTreeMap::new();
    let mut slugs = BTreeSet::new();
    for id in &members {
        let record = task_record(tx, id)?;
        slugs.extend(record.outputs.iter().cloned());
        tasks.insert(id.clone(), add(encode(&record)?));
    }
    let goal = match tx
        .query_row("SELECT brief FROM goals WHERE root=?1", [root], |row| {
            row.get::<_, String>(0)
        })
        .optional()?
    {
        Some(brief) => {
            let path = document_root.join("workspaces").join(project).join(&brief);
            let brief_blob = match text::read_bytes(&path, dots_substrate::MAX_BLOB_BYTES) {
                Ok(Some(bytes)) => Some(add(Blob::new(bytes)?)),
                Ok(None) => {
                    withhold(
                        &mut withheld,
                        WithheldKind::Brief,
                        &brief,
                        "missing on its home",
                    );
                    None
                }
                Err(dots_documents::Error::TooLarge { .. }) => {
                    withhold(&mut withheld, WithheldKind::Brief, &brief, TOO_LARGE);
                    None
                }
                Err(error) => {
                    withhold(
                        &mut withheld,
                        WithheldKind::Brief,
                        &brief,
                        &format!("unreadable on its home: {error}"),
                    );
                    None
                }
            };
            Some(GoalRecord {
                brief,
                content: brief_blob,
            })
        }
        None => None,
    };
    let mut requests = BTreeMap::new();
    let mut decisions = BTreeMap::new();
    for record in request_records(tx, &members)? {
        requests.insert(record.id.clone(), add(encode(&record)?));
    }
    for record in decision_records(tx, &members)? {
        decisions.insert(record.id.clone(), add(encode(&record)?));
    }
    // Events not yet in a segment, oldest first.
    let mut fresh = Vec::new();
    let mut published = Vec::new();
    for id in &members {
        let mut statement = tx.prepare(
            "SELECT seq FROM events WHERE task=?1 AND published IS NULL AND kind NOT IN ('claimed','released')",
        )?;
        for seq in statement.query_map([id], |row| row.get::<_, i64>(0))? {
            published.push(seq?);
        }
    }
    published.sort_unstable();
    for seq in &published {
        let record = event_record(tx, *seq)?;
        slugs.extend(record.evidence.iter().map(|evidence| evidence.slug.clone()));
        fresh.push(record);
    }
    // Evidence named by earlier segments stays covered by the outputs map.
    if let Some(latest) = latest {
        slugs.extend(latest.outputs.keys().cloned());
    }
    let mut outputs = BTreeMap::new();
    let mut contents = BTreeSet::new();
    let known_contents = latest.map(|latest| &latest.contents);
    for slug in &slugs {
        let Some(record) = output_record(
            tx,
            context,
            document_root,
            project,
            slug,
            known_contents,
            &mut add,
            &mut withheld,
        )?
        else {
            continue;
        };
        contents.extend(
            record
                .revisions
                .iter()
                .filter(|revision| revision.state == RevisionState::Available)
                .map(|revision| revision.digest),
        );
        outputs.insert(slug.clone(), add(encode(&record)?));
    }
    let mut segments = latest
        .map(|latest| latest.segments.clone())
        .unwrap_or_default();
    if !fresh.is_empty() {
        let mut current = match segments.last() {
            Some(hash) => {
                let blob = context.substrate.blob(hash)?;
                if blob.content().len() < SEGMENT_TARGET {
                    segments.pop();
                    decode::<Segment>(&blob, "event segment")?.events
                } else {
                    Vec::new()
                }
            }
            None => Vec::new(),
        };
        let mut size = serde_json::to_vec(&current).map_or(0, |bytes| bytes.len());
        for event in fresh {
            let event_size = serde_json::to_vec(&event).map_or(0, |bytes| bytes.len());
            if !current.is_empty() && size + event_size > SEGMENT_TARGET {
                segments.push(add(encode(&Segment {
                    format: FORMAT,
                    events: std::mem::take(&mut current),
                })?));
                size = 0;
            }
            size += event_size + 1;
            current.push(event);
        }
        segments.push(add(encode(&Segment {
            format: FORMAT,
            events: current,
        })?));
    }
    let manifest = Manifest {
        format: FORMAT,
        project: shared.as_str().to_owned(),
        root: root.to_owned(),
        home: context.machine().clone(),
        written_at: unix(now),
        moved_to: None,
        goal,
        tasks,
        requests,
        decisions,
        outputs,
        segments,
        contents,
        withheld,
    };
    Ok(Built {
        manifest,
        blobs,
        published,
    })
}

fn task_record(tx: &Transaction<'_>, id: &str) -> Result<TaskRecord, Error> {
    let mut record = tx.query_row(
        "SELECT id,parent,title,body,result,status,branch,created_at,updated_at,closed_at FROM tasks WHERE id=?1",
        [id],
        |row| {
            Ok(TaskRecord {
                format: FORMAT,
                id: row.get(0)?,
                parent: row.get(1)?,
                title: row.get(2)?,
                body: row.get(3)?,
                result: row.get(4)?,
                status: row.get(5)?,
                branch: row.get(6)?,
                created_at: row.get(7)?,
                updated_at: row.get(8)?,
                closed_at: row.get(9)?,
                blocked_by: Vec::new(),
                references: Vec::new(),
                outputs: Vec::new(),
            })
        },
    )?;
    record.blocked_by = strings(
        tx,
        "SELECT blocked_by FROM deps WHERE task=?1 ORDER BY blocked_by",
        id,
    )?;
    record.outputs = strings(
        tx,
        "SELECT slug FROM task_outputs WHERE task=?1 ORDER BY slug",
        id,
    )?;
    let mut statement = tx.prepare("SELECT url,label FROM task_refs WHERE task=?1 ORDER BY url")?;
    record.references = statement
        .query_map([id], |row| {
            Ok(ReferenceRecord {
                url: row.get(0)?,
                label: row.get(1)?,
            })
        })?
        .collect::<Result<_, _>>()?;
    Ok(record)
}

fn strings(tx: &Transaction<'_>, sql: &str, id: &str) -> Result<Vec<String>, Error> {
    let mut statement = tx.prepare(sql)?;
    Ok(statement
        .query_map([id], |row| row.get(0))?
        .collect::<Result<_, _>>()?)
}

fn request_records(tx: &Transaction<'_>, members: &[String]) -> Result<Vec<RequestRecord>, Error> {
    let mut records = Vec::new();
    let mut statement = tx.prepare(
        "SELECT id,goal,key,kind,title,question,context,recommendation,state,created_at,created_by,supersedes,
           answer_choice,answer_text,answer_source,answer_provenance,answer_interpretation,answered_at,answered_by,
           closure,closed_at,closed_by,close_reason
         FROM requests WHERE goal=?1 ORDER BY created_at, id",
    )?;
    for goal in members {
        let rows = statement.query_map([goal], |row| {
            let answer = match row.get::<_, Option<String>>(14)? {
                Some(source) => Some(AnswerRecord {
                    choice: row.get(12)?,
                    text: row.get(13)?,
                    source,
                    provenance: row.get::<_, Option<String>>(15)?.unwrap_or_default(),
                    interpretation: row.get(16)?,
                    at: row.get::<_, Option<i64>>(17)?.unwrap_or_default(),
                    by: row.get(18)?,
                }),
                None => None,
            };
            let closure = match row.get::<_, Option<String>>(19)? {
                Some(kind) => Some(ClosureRecord {
                    kind,
                    at: row.get::<_, Option<i64>>(20)?.unwrap_or_default(),
                    by: row.get(21)?,
                    reason: row.get(22)?,
                }),
                None => None,
            };
            Ok(RequestRecord {
                format: FORMAT,
                id: row.get(0)?,
                goal: row.get(1)?,
                key: row.get(2)?,
                kind: row.get(3)?,
                title: row.get(4)?,
                question: row.get(5)?,
                context: row.get(6)?,
                recommendation: row.get(7)?,
                state: row.get(8)?,
                created_at: row.get(9)?,
                created_by: row.get(10)?,
                supersedes: row.get(11)?,
                choices: Vec::new(),
                tasks: Vec::new(),
                answer,
                closure,
            })
        })?;
        for row in rows {
            records.push(row?);
        }
    }
    for record in &mut records {
        let mut statement = tx.prepare(
            "SELECT id,label,detail FROM request_choices WHERE request=?1 ORDER BY position",
        )?;
        record.choices = statement
            .query_map([&record.id], |row| {
                Ok(ChoiceRecord {
                    id: row.get(0)?,
                    label: row.get(1)?,
                    detail: row.get(2)?,
                })
            })?
            .collect::<Result<_, _>>()?;
        record.tasks = strings(
            tx,
            "SELECT task FROM request_tasks WHERE request=?1 ORDER BY task",
            &record.id,
        )?;
    }
    Ok(records)
}

fn decision_records(
    tx: &Transaction<'_>,
    members: &[String],
) -> Result<Vec<DecisionRecord>, Error> {
    let mut statement = tx.prepare(
        "SELECT id,goal,kind,decision,human_text,human_source,human_provenance,request,at,actor
         FROM decisions WHERE goal=?1 ORDER BY at, id",
    )?;
    let mut records = Vec::new();
    for goal in members {
        let rows = statement.query_map([goal], |row| {
            Ok(DecisionRecord {
                format: FORMAT,
                id: row.get(0)?,
                goal: row.get(1)?,
                kind: row.get(2)?,
                decision: row.get(3)?,
                human_text: row.get(4)?,
                human_source: row.get(5)?,
                human_provenance: row.get(6)?,
                request: row.get(7)?,
                at: row.get(8)?,
                actor: row.get(9)?,
            })
        })?;
        for row in rows {
            records.push(row?);
        }
    }
    Ok(records)
}

fn event_record(tx: &Transaction<'_>, seq: i64) -> Result<EventRecord, Error> {
    let mut record = tx.query_row(
        "SELECT events.uid,events.task,events.at,events.actor,events.kind,events.detail,events.result,
           (SELECT previous.uid FROM events AS previous WHERE previous.seq=events.predecessor),events.operation
         FROM events WHERE seq=?1",
        [seq],
        |row| {
            Ok(EventRecord {
                uid: row.get(0)?,
                task: row.get(1)?,
                at: row.get(2)?,
                actor: row.get(3)?,
                kind: row.get(4)?,
                detail: row.get(5)?,
                result: row.get(6)?,
                predecessor: row.get(7)?,
                operation: row.get(8)?,
                evidence: Vec::new(),
            })
        },
    )?;
    let mut statement =
        tx.prepare("SELECT slug,digest FROM event_evidence WHERE event=?1 ORDER BY slug, digest")?;
    record.evidence = statement
        .query_map([seq], |row| {
            Ok(EvidenceRecord {
                slug: row.get(0)?,
                digest: row.get(1)?,
            })
        })?
        .collect::<Result<_, _>>()?;
    Ok(record)
}

/// Why a part stays behind when it exceeds the substrate's blob limit.
const TOO_LARGE: &str = "larger than the substrate's 1 MiB blob limit";

fn output_state(state: &verified_text::State) -> String {
    match state {
        verified_text::State::Available(_) => "available".into(),
        verified_text::State::Missing => "missing".into(),
        verified_text::State::Corrupt => "altered".into(),
        verified_text::State::Unreadable(reason) => format!("unreadable ({reason})"),
    }
}

fn withhold(withheld: &mut Vec<Withheld>, kind: WithheldKind, id: &str, reason: &str) {
    withheld.push(Withheld {
        kind,
        id: id.to_owned(),
        reason: reason.to_owned(),
    });
}

/// The record of one output slug, adding content blobs the tree lacks.
/// A revision whose bytes cannot travel is recorded as missing and withheld.
#[expect(
    clippy::too_many_arguments,
    clippy::too_many_lines,
    reason = "one record reads the catalog, cache and documents for each revision"
)]
fn output_record(
    tx: &Transaction<'_>,
    context: Context<'_>,
    document_root: &Path,
    project: &str,
    slug: &str,
    known: Option<&BTreeSet<Hash>>,
    add: &mut impl FnMut(Blob) -> Hash,
    withheld: &mut Vec<Withheld>,
) -> Result<Option<OutputRecord>, Error> {
    type Row = (
        String,
        u32,
        String,
        i64,
        Option<String>,
        String,
        String,
        bool,
    );
    let Some((filename, media_type)) = tx
        .query_row(
            "SELECT filename, media_type FROM outputs WHERE project=?1 AND slug=?2",
            params![project, slug],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()?
    else {
        return Ok(None);
    };
    let rows: Vec<Row> = {
        let mut statement = tx.prepare(
            "SELECT digest,bytes,path,created_at,actor,description,provenance,
               EXISTS(SELECT 1 FROM purged_digests WHERE purged_digests.project=output_revisions.project
                 AND purged_digests.digest=output_revisions.digest)
             FROM output_revisions WHERE project=?1 AND slug=?2 ORDER BY created_at, digest",
        )?;
        statement
            .query_map(params![project, slug], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                    row.get(7)?,
                ))
            })?
            .collect::<Result<_, _>>()?
    };
    let mut revisions = Vec::new();
    for (digest, bytes, path, created_at, actor, description, provenance, purged) in rows {
        let hash: Hash = digest
            .parse()
            .map_err(|error: SubstrateError| share(format!("output {slug} digest: {error}")))?;
        let state = if purged {
            RevisionState::Purged
        } else if known.is_some_and(|known| known.contains(&hash))
            || context.substrate.blob(&hash).is_ok()
        {
            RevisionState::Available
        } else if usize::try_from(bytes).map_or(true, |bytes| bytes > MAX_BLOB_BYTES) {
            withhold(
                withheld,
                WithheldKind::Output,
                &format!("{slug} {digest}"),
                TOO_LARGE,
            );
            RevisionState::Missing
        } else {
            match verified_text::inspect(
                document_root,
                Path::new(&path),
                &digest,
                bytes,
                MAX_OUTPUT_BYTES,
            ) {
                verified_text::State::Available(bytes) => {
                    add(Blob::new(bytes.into_bytes())?);
                    RevisionState::Available
                }
                state => {
                    withhold(
                        withheld,
                        WithheldKind::Output,
                        &format!("{slug} {digest}"),
                        &format!("{} on its home", output_state(&state)),
                    );
                    RevisionState::Missing
                }
            }
        };
        revisions.push(RevisionRecord {
            digest: hash,
            bytes,
            created_at,
            actor,
            description,
            provenance,
            state,
        });
    }
    Ok(Some(OutputRecord {
        format: FORMAT,
        slug: slug.to_owned(),
        filename,
        media_type,
        revisions,
    }))
}

/// Defer this machine's answers when they changed.
fn export_answers(store: &mut Store, context: Context<'_>, now: SystemTime) -> Result<bool, Error> {
    let tx = store.write()?;
    let dirty: bool = tx.query_row(
        "SELECT answers_dirty OR EXISTS(SELECT 1 FROM sent_answers WHERE exported IS NULL) FROM sharing",
        [],
        |row| row.get(0),
    )?;
    if !dirty {
        return Ok(false);
    }
    let answers = {
        let mut statement =
            tx.prepare("SELECT request, goal, answer FROM sent_answers ORDER BY at, request")?;
        statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })?
            .map(|row| {
                let (request, goal, answer) = row?;
                let answer: AnswerRecord = serde_json::from_str(&answer)
                    .map_err(|error| share(format!("stored answer for {request}: {error}")))?;
                Ok(SentAnswer {
                    request,
                    goal,
                    answer,
                })
            })
            .collect::<Result<Vec<_>, Error>>()?
    };
    let blob = encode(&Answers {
        format: FORMAT,
        machine: context.machine().clone(),
        sent: answers,
    })?;
    let name = answers_ref(context.machine())?;
    let expected = match context.substrate.read(&name) {
        Ok(value) => value.map(|value| value.version),
        Err(SubstrateError::Unsynced) => None,
        Err(error) => return Err(error.into()),
    };
    context.substrate.defer(
        Update {
            name,
            expected,
            root: blob.hash(),
            new_blobs: vec![blob],
        },
        now,
    )?;
    tx.execute("UPDATE sent_answers SET exported=1", [])?;
    tx.execute("UPDATE sharing SET answers_dirty=0", [])?;
    tx.commit()?;
    Ok(true)
}
