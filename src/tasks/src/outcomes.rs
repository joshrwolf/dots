//! Transactional task results with immutable evidence. Each result event links
//! to the one it replaced.
use dots_memory::mutation_lock::MutationLock;
use dots_memory::verified_text;
use rusqlite::{OptionalExtension as _, Transaction, params};
use serde::Serialize;

use crate::outputs::{self, MAX_OUTPUT_BYTES};
use crate::store::{self, Actor, Error, Owner, Project, Status, Store, Task};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct Evidence {
    pub slug: String,
    pub digest: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Operation {
    UpdateResult,
    Review,
    Done,
}

impl Operation {
    fn as_str(self) -> &'static str {
        match self {
            Self::UpdateResult => "update-result",
            Self::Review => "review",
            Self::Done => "done",
        }
    }
    fn status(self) -> Option<Status> {
        match self {
            Self::UpdateResult => None,
            Self::Review => Some(Status::Review),
            Self::Done => Some(Status::Done),
        }
    }
}

#[derive(Debug)]
pub struct Outcome<'input> {
    pub operation: Operation,
    pub result: &'input str,
    pub evidence: &'input [Evidence],
    pub force: bool,
}

#[derive(Debug, Serialize)]
pub struct ResultEvent {
    pub seq: i64,
    pub predecessor: Option<i64>,
    pub operation: String,
    pub result: String,
    pub evidence: Vec<Evidence>,
    /// Current catalog tombstones; other evidence is not thereby verified available.
    pub purged_evidence: Vec<Evidence>,
}

#[derive(Debug, Serialize)]
pub struct OutcomeReceipt {
    pub event: ResultEvent,
    pub task: Task,
}

impl Store {
    pub fn result_event(
        &mut self,
        project: &Project,
        id: &str,
        seq: i64,
    ) -> Result<ResultEvent, Error> {
        let tx = self.connection.transaction()?;
        store::load(&tx, project, id)?;
        load_event(&tx, id, seq)
    }

    /// Replace the current result, optionally changing status. Selected evidence
    /// must be readable and intact at write time.
    pub fn write_result(
        &mut self,
        project: &Project,
        id: &str,
        outcome: &Outcome<'_>,
        actor: Actor<'_>,
    ) -> Result<OutcomeReceipt, Error> {
        store::validate_result(outcome.result)?;
        let mut evidence = outcome.evidence.to_vec();
        evidence.sort();
        evidence.dedup();
        if evidence.len() > 32 {
            return Err(Error::Invalid(
                "evidence: select at most 32 exact revisions",
            ));
        }
        for item in &evidence {
            outputs::validate_slug(&item.slug)?;
            outputs::validate_digest(&item.digest)?;
        }
        let _lock = if evidence.is_empty() {
            None
        } else {
            Some(MutationLock::acquire(
                &self.document_root()?.join("mutation.lock"),
            )?)
        };
        self.verify_evidence(project, &evidence)?;
        let tx = self.write()?;
        let task = store::load(&tx, project, id)?;
        // A closed task's result is its accepted outcome; a late or stale write
        // must not replace it or reverse the status, even when forced.
        if task.status.is_closed() {
            return Err(Error::Closed {
                id: id.into(),
                status: task.status.as_str(),
            });
        }
        if !outcome.force
            && let Some(owner) = task.claimed_by_other(actor.name)
        {
            return Err(Error::OwnedByAnother {
                id: id.into(),
                owner: owner.into(),
            });
        }
        if outcome.operation.status().is_some()
            && !outcome.force
            && (!task.blocked_by.is_empty() || task.unclosed_children > 0)
        {
            return Err(Error::Incomplete {
                id: id.into(),
                reason: format!(
                    "{} blockers and {} unclosed children",
                    task.blocked_by.len(),
                    task.unclosed_children
                ),
            });
        }
        if let Some(status) = outcome.operation.status() {
            tx.execute(
                "UPDATE tasks SET status=?2,closed_at=?3,owner=NULL WHERE id=?1",
                params![id, status.as_str(), status.is_closed().then_some(actor.now)],
            )?;
        }
        tx.execute(
            "UPDATE tasks SET result=?2,updated_at=?3 WHERE id=?1",
            params![id, outcome.result, actor.now],
        )?;
        tx.execute(
            "INSERT INTO events(task,at,actor,kind,result,predecessor,operation) VALUES(?1,?2,?3,?4,?5,?6,?4)",
            params![
                id,
                actor.now,
                actor.name.map(Owner::as_str),
                outcome.operation.as_str(),
                outcome.result,
                task.result_event
            ],
        )?;
        let seq = tx.last_insert_rowid();
        for item in &evidence {
            tx.execute(
                "INSERT INTO event_evidence(event,project,slug,digest) VALUES(?1,?2,?3,?4)",
                params![seq, project.as_str(), item.slug, item.digest],
            )?;
            tx.execute("INSERT INTO task_outputs(task,project,slug) VALUES(?1,?2,?3) ON CONFLICT DO NOTHING",params![id,project.as_str(),item.slug])?;
        }
        let event = load_event(&tx, id, seq)?;
        let task = store::load(&tx, project, id)?;
        tx.commit()?;
        Ok(OutcomeReceipt { event, task })
    }

    fn verify_evidence(&self, project: &Project, evidence: &[Evidence]) -> Result<(), Error> {
        if evidence.is_empty() {
            return Ok(());
        }
        let root = self.document_root()?;
        for item in evidence {
            let revision = self
                .find_output_revision(project, &item.slug, &item.digest)?
                .ok_or_else(|| {
                    Error::Output(format!(
                        "evidence {} {} is not registered in this project",
                        item.slug, item.digest
                    ))
                })?;
            if revision.purged {
                return Err(Error::Output(format!(
                    "evidence {} {} is purged",
                    item.slug, item.digest
                )));
            }
            let failure = match verified_text::inspect(
                &root,
                &revision.path,
                &item.digest,
                revision.bytes,
                MAX_OUTPUT_BYTES,
            ) {
                verified_text::State::Available(_) => None,
                verified_text::State::Missing => Some("missing".into()),
                verified_text::State::Corrupt => Some("corrupt".into()),
                verified_text::State::Unreadable(diagnostic) => {
                    Some(format!("unreadable: {diagnostic}"))
                }
            };
            if let Some(failure) = failure {
                return Err(Error::Output(format!(
                    "evidence {} {} is {failure}",
                    item.slug, item.digest
                )));
            }
        }
        Ok(())
    }
}

pub(super) fn current_event(tx: &Transaction<'_>, id: &str) -> Result<Option<i64>, Error> {
    Ok(tx.query_row(
        "SELECT MAX(seq) FROM events WHERE task=?1 AND result IS NOT NULL",
        [id],
        |row| row.get(0),
    )?)
}

pub(super) fn evidence(tx: &Transaction<'_>, seq: i64) -> Result<Vec<Evidence>, Error> {
    let mut statement =
        tx.prepare("SELECT slug,digest FROM event_evidence WHERE event=?1 ORDER BY slug,digest")?;
    Ok(statement
        .query_map([seq], |row| {
            Ok(Evidence {
                slug: row.get(0)?,
                digest: row.get(1)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?)
}

pub(super) fn purged_evidence(tx: &Transaction<'_>, seq: i64) -> Result<Vec<Evidence>, Error> {
    let mut statement=tx.prepare("SELECT e.slug,e.digest FROM event_evidence e JOIN purged_digests p ON p.project=e.project AND p.digest=e.digest WHERE e.event=?1 ORDER BY e.slug,e.digest")?;
    statement
        .query_map([seq], |row| {
            Ok(Evidence {
                slug: row.get(0)?,
                digest: row.get(1)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()
        .map_err(Error::from)
}

fn load_event(tx: &Transaction<'_>, id: &str, seq: i64) -> Result<ResultEvent, Error> {
    let (predecessor,operation,result)=tx.query_row("SELECT predecessor,operation,result FROM events WHERE seq=?1 AND task=?2 AND result IS NOT NULL",params![seq,id],|row|Ok((row.get(0)?,row.get(1)?,row.get(2)?))).optional()?.ok_or(Error::Invalid("result event does not belong to this task"))?;
    Ok(ResultEvent {
        seq,
        predecessor,
        operation,
        result,
        evidence: evidence(tx, seq)?,
        purged_evidence: purged_evidence(tx, seq)?,
    })
}
