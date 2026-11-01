//! The review file and thread contract shared by every consumer.
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Version of the review file and of the JSON this crate emits.
pub const SCHEMA: u32 = 2;

/// Author of the status changes a refresh records.
pub const REFRESH_AUTHOR: &str = "dots review refresh";

/// What a review covers, as the user or agent asked for it. Re-resolving it
/// later yields the revision a refresh compares against.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum TargetSpec {
    /// Uncommitted changes, tracked and untracked, against HEAD.
    Worktree,
    /// Everything since the merge base of `reference` and HEAD, through the
    /// working tree.
    Base { reference: String },
    /// Committed changes from `from` to `to`.
    Range { from: String, to: String },
}

/// The worktree a review belongs to, as it was when the review started.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Worktree {
    /// Top level of the working tree.
    pub path: PathBuf,
    /// Checked-out branch, absent when HEAD was detached.
    pub branch: Option<String>,
}

/// The exact content one revision of a review covers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Revision {
    /// Commit the change is compared against.
    pub base: String,
    /// Tree object holding the reviewed content.
    pub head: String,
    /// Commit at the head, when the head is a commit or a working tree over one.
    pub head_commit: Option<String>,
    /// Whether `head` includes uncommitted working-tree content.
    pub includes_worktree: bool,
    /// Pin commit whose tree is `head`, so viewers can diff `base..snapshot`.
    pub snapshot: String,
    pub at: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    /// Breaks correctness, security or data in ordinary use; must not ship.
    Critical,
    /// A real defect or rule violation that should be fixed before merging.
    Important,
    /// Worth fixing, not blocking.
    Minor,
    /// Cosmetic.
    Nit,
}

/// Which side of the reviewed diff an anchor's line numbers count on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Side {
    /// The head: added and context lines.
    New,
    /// The base: removed and context lines.
    Old,
}

/// Lines a thread is about, valid at the thread's revision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Anchor {
    /// Repository-relative path.
    pub path: String,
    pub side: Side,
    pub start: u32,
    pub end: u32,
}

/// The instruction-file lines a thread relies on, at the head revision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuleCitation {
    pub path: String,
    pub start: u32,
    pub end: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Open,
    Accepted,
    Dismissed,
    Fixed,
    Posted,
    /// Its lines changed after it was raised; only a refresh sets this.
    Outdated,
}

impl Status {
    pub const ALL: [Self; 6] = [
        Self::Open,
        Self::Accepted,
        Self::Dismissed,
        Self::Fixed,
        Self::Posted,
        Self::Outdated,
    ];

    /// Whether a refresh carries the thread to the new revision.
    pub fn live(self) -> bool {
        matches!(self, Self::Open | Self::Accepted)
    }

    /// The serialized name.
    pub fn name(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Accepted => "accepted",
            Self::Dismissed => "dismissed",
            Self::Fixed => "fixed",
            Self::Posted => "posted",
            Self::Outdated => "outdated",
        }
    }

    /// The status a serialized name denotes.
    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|status| status.name() == name)
    }
}

impl Severity {
    /// The serialized name.
    pub fn name(self) -> &'static str {
        match self {
            Self::Critical => "critical",
            Self::Important => "important",
            Self::Minor => "minor",
            Self::Nit => "nit",
        }
    }
}

/// One thread as a reviewer or the user opens it. A plain comment needs only
/// `body` and `author`; an agent finding adds the finding fields.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NewThread {
    /// Absent for review-level threads.
    #[serde(default)]
    pub anchor: Option<Anchor>,
    /// The opening comment; Markdown, at most 16 KiB.
    pub body: String,
    /// Who opened it: a model, an agent, `user`, or an imported reviewer.
    pub author: String,
    /// One line, imperative, at most 200 characters.
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub severity: Option<Severity>,
    /// The defect predates this change.
    #[serde(default)]
    pub pre_existing: bool,
    /// 0 to 1: how sure the author is that the finding is real.
    #[serde(default)]
    pub confidence: Option<f64>,
    /// The reviewer angle that produced it, like `correctness`.
    #[serde(default)]
    pub angle: Option<String>,
    #[serde(default)]
    pub rule: Option<RuleCitation>,
}

/// One entry of a thread's conversation: a reply, a status change, or both.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Comment {
    pub author: String,
    pub at: i64,
    pub body: Option<String>,
    /// The status this entry set, when it changed the thread's status.
    pub status: Option<Status>,
}

/// A recorded thread: where it points, its status, and its conversation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Thread {
    pub id: String,
    /// Index into the review's revisions where `anchor` is valid.
    pub revision: usize,
    pub anchor: Option<Anchor>,
    pub status: Status,
    pub title: Option<String>,
    pub severity: Option<Severity>,
    pub pre_existing: bool,
    pub confidence: Option<f64>,
    pub angle: Option<String>,
    pub rule: Option<RuleCitation>,
    /// Oldest first; the first holds the opening body.
    pub comments: Vec<Comment>,
    pub created_at: i64,
    pub updated_at: i64,
}

/// A review file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Review {
    pub schema: u32,
    pub id: String,
    pub target: TargetSpec,
    /// Opaque name of the profile that produced it, if any.
    pub profile: Option<String>,
    pub worktree: Worktree,
    pub created_at: i64,
    pub updated_at: i64,
    /// Position of the review's last change among the changes to its
    /// worktree's reviews; the highest is the most recently updated review,
    /// whatever the clock says.
    pub sequence: u64,
    /// Oldest first; the last is current.
    pub revisions: Vec<Revision>,
    pub threads: Vec<Thread>,
    pub next_thread: u32,
}

impl Review {
    /// Record a change at time `at`, taking `sequence`, the position the
    /// review's store hands its next change.
    pub(crate) fn touch(&mut self, at: i64, sequence: u64) {
        self.updated_at = at;
        self.sequence = sequence;
    }

    /// The revision the review currently covers.
    pub fn current(&self) -> &Revision {
        self.revisions
            .last()
            .expect("a review always has at least one revision")
    }
}

/// One changed path of a revision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ChangedFile {
    pub path: String,
    pub added: u32,
    pub removed: u32,
    pub binary: bool,
}
