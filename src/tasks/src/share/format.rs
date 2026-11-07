//! The replicated form of a task tree and of a machine's answers.
//!
//! A tree's ref points at a small manifest blob. The manifest names one blob
//! per task, request, decision and output, the brief's content blob, and the
//! event segments in order. Records are immutable content-addressed blobs, so
//! an update uploads only the records that changed. Events live in segments
//! of a few KiB; a change appends to the last segment while it is small and
//! otherwise starts a new one, so an update carries roughly its new events.
//! Live claims never replicate.
use std::collections::{BTreeMap, BTreeSet};

use dots_substrate::{Blob, Hash, MachineId};
use serde::{Deserialize, Serialize, de::DeserializeOwned};

use crate::store::Error;

/// The format every manifest and record of this build carries.
pub(super) const FORMAT: u32 = 1;
/// A segment below this size takes the next events; a larger one is sealed.
pub(super) const SEGMENT_TARGET: usize = 4 * 1024;

/// One tree's current state and history.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Manifest {
    pub format: u32,
    /// The shared project identity.
    pub project: String,
    /// The tree's top-level task.
    pub root: String,
    /// The machine that alone writes this tree.
    pub home: MachineId,
    /// When the home wrote this manifest, as a Unix timestamp.
    pub written_at: i64,
    /// Set when the root became part of another tree; tasks then live there.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub moved_to: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub goal: Option<GoalRecord>,
    /// Task records by task id.
    pub tasks: BTreeMap<String, Hash>,
    /// Request records by request id.
    pub requests: BTreeMap<String, Hash>,
    /// Decision records by decision id.
    pub decisions: BTreeMap<String, Hash>,
    /// Output records by slug.
    pub outputs: BTreeMap<String, Hash>,
    /// Event segments, oldest first.
    pub segments: Vec<Hash>,
    /// Output contents that travel with the tree.
    pub contents: BTreeSet<Hash>,
}

impl Manifest {
    /// The same state, ignoring who wrote it and when.
    pub(super) fn same_state(&self, other: &Self) -> bool {
        Self {
            home: other.home.clone(),
            written_at: other.written_at,
            ..self.clone()
        } == *other
    }

    /// Every blob this manifest references, itself excluded.
    pub(super) fn references(&self) -> BTreeSet<Hash> {
        let mut hashes: BTreeSet<Hash> = self
            .tasks
            .values()
            .chain(self.requests.values())
            .chain(self.decisions.values())
            .chain(self.outputs.values())
            .chain(&self.segments)
            .copied()
            .collect();
        hashes.extend(self.goal.as_ref().and_then(|goal| goal.content));
        hashes.extend(self.contents.iter().copied());
        hashes
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct GoalRecord {
    /// Project-relative brief path under briefs/.
    pub brief: String,
    /// The brief's bytes when the home could read them.
    pub content: Option<Hash>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct TaskRecord {
    pub format: u32,
    pub id: String,
    pub parent: Option<String>,
    pub title: String,
    pub body: Option<String>,
    pub result: Option<String>,
    pub status: String,
    pub branch: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
    pub closed_at: Option<i64>,
    /// Prerequisites, possibly in other trees of the project.
    pub blocked_by: Vec<String>,
    pub references: Vec<ReferenceRecord>,
    /// Output slugs associated with the task.
    pub outputs: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ReferenceRecord {
    pub url: String,
    pub label: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RequestRecord {
    pub format: u32,
    pub id: String,
    pub goal: String,
    pub key: String,
    pub kind: String,
    pub title: String,
    pub question: String,
    pub context: Option<String>,
    pub recommendation: Option<String>,
    pub state: String,
    pub created_at: i64,
    pub created_by: Option<String>,
    pub supersedes: Option<String>,
    pub choices: Vec<ChoiceRecord>,
    pub tasks: Vec<String>,
    pub answer: Option<AnswerRecord>,
    pub closure: Option<ClosureRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ChoiceRecord {
    pub id: String,
    pub label: String,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct AnswerRecord {
    pub choice: Option<String>,
    pub text: Option<String>,
    pub source: String,
    pub provenance: String,
    pub interpretation: Option<String>,
    pub at: i64,
    pub by: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ClosureRecord {
    pub kind: String,
    pub at: i64,
    pub by: Option<String>,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct DecisionRecord {
    pub format: u32,
    pub id: String,
    pub goal: String,
    pub kind: String,
    pub decision: String,
    pub human_text: Option<String>,
    pub human_source: Option<String>,
    pub human_provenance: Option<String>,
    pub request: Option<String>,
    pub at: i64,
    pub actor: Option<String>,
}

/// One output slug with every revision the project holds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct OutputRecord {
    pub format: u32,
    pub slug: String,
    pub filename: String,
    pub media_type: String,
    pub revisions: Vec<RevisionRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RevisionRecord {
    /// `sha256:<hex>` of the content, which is also its blob hash.
    pub digest: Hash,
    pub bytes: u32,
    pub created_at: i64,
    pub actor: Option<String>,
    pub description: String,
    pub provenance: String,
    pub state: RevisionState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum RevisionState {
    /// The content blob travels with the tree.
    Available,
    /// The home purged it; only the record replicates.
    Purged,
    /// The home could not read intact bytes; only the record replicates.
    Missing,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Segment {
    pub format: u32,
    pub events: Vec<EventRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct EventRecord {
    pub uid: String,
    pub task: String,
    pub at: i64,
    pub actor: Option<String>,
    pub kind: String,
    pub detail: Option<String>,
    pub result: Option<String>,
    /// The uid of the result event this one replaced.
    pub predecessor: Option<String>,
    pub operation: Option<String>,
    pub evidence: Vec<EvidenceRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct EvidenceRecord {
    pub slug: String,
    pub digest: String,
}

/// Answers this machine gave to requests of trees other machines home.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Answers {
    pub format: u32,
    pub machine: MachineId,
    pub sent: Vec<SentAnswer>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SentAnswer {
    pub request: String,
    pub goal: String,
    pub answer: AnswerRecord,
}

/// Encode a record as a blob.
pub(super) fn encode(value: &impl Serialize) -> Result<Blob, Error> {
    let bytes = serde_json::to_vec(value).map_err(|error| share(format!("encode: {error}")))?;
    Ok(Blob::new(bytes)?)
}

/// Decode a blob, refusing other formats.
pub(super) fn decode<T: DeserializeOwned>(blob: &Blob, what: &str) -> Result<T, Error> {
    let value: serde_json::Value = serde_json::from_slice(blob.content())
        .map_err(|error| share(format!("decode {what} {}: {error}", blob.hash())))?;
    match value.get("format").and_then(serde_json::Value::as_u64) {
        Some(format) if format == u64::from(FORMAT) => {}
        other => {
            return Err(share(format!(
                "{what} {} has format {other:?}; this build reads format {FORMAT}",
                blob.hash()
            )));
        }
    }
    serde_json::from_value(value)
        .map_err(|error| share(format!("decode {what} {}: {error}", blob.hash())))
}

pub(super) fn share(message: impl Into<String>) -> Error {
    Error::Share(message.into())
}
