//! The review file and thread contract shared by every consumer.
use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Version of the review file and of the JSON this crate emits.
pub const SCHEMA: u32 = 4;

/// The author name reserved for the human reviewing in the viewer.
pub const USER: &str = "user";

/// Author of the status changes a refresh records.
pub const REFRESH_AUTHOR: &str = "dots review refresh";

/// Who a comment's author is, as far as review rules care.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthorKind {
    /// The human, [`USER`]: the only author who drafts, submits, and settles
    /// the threads it started.
    User,
    /// Refresh itself, [`REFRESH_AUTHOR`]; no caller may write as it.
    Refresh,
    /// A model, an agent, or an imported reviewer.
    Agent,
}

impl AuthorKind {
    pub fn of(author: &str) -> Self {
        match author {
            USER => Self::User,
            REFRESH_AUTHOR => Self::Refresh,
            _ => Self::Agent,
        }
    }
}

/// What a review covers, as the user or agent asked for it. Re-resolving it
/// later yields the revision a refresh compares against. Every target runs
/// through the working tree, tracked and untracked files that are not
/// ignored, over the HEAD commit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum TargetSpec {
    /// The worktree's own review: its branch against its base, chosen again
    /// at every refresh by the rule [`BaseReason`] describes, or against the
    /// merge base with `base` when the user or agent named one.
    Worktree { base: Option<String> },
    /// Uncommitted changes against HEAD.
    Uncommitted,
    /// Everything since the merge base of `reference` and HEAD.
    Base { reference: String },
}

/// Why a revision's base is what it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BaseReason {
    /// An uncommitted-changes review compares with HEAD.
    Head,
    /// The review was started against this reference.
    Given,
    /// The worktree review's base was set explicitly.
    Override,
    /// The branch's upstream, which is another branch than its own name on
    /// a remote, such as the branch a stacked branch builds on.
    Upstream,
    /// The default-branch candidate nearest HEAD: each remote's default
    /// branch (its `HEAD`) and the local `main`, else `master`.
    Default,
    /// HEAD is on the default branch, so the review covers uncommitted
    /// changes against HEAD.
    DefaultBranch,
    /// No default branch was found, so the review covers uncommitted
    /// changes against HEAD.
    NoDefault,
}

/// How a revision's base was chosen: the merge base of HEAD and `reference`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Basis {
    /// A ref such as `upstream/main`, a revision as given, or `HEAD`.
    pub reference: String,
    pub reason: BaseReason,
    /// Every default-branch candidate considered, nearest first; empty
    /// unless the default rule ran.
    pub candidates: Vec<Candidate>,
}

/// One default-branch candidate and how far HEAD is from it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Candidate {
    /// Such as `upstream/main`, `origin/main` or `main`.
    pub reference: String,
    /// Commits reachable from HEAD but not from the candidate.
    pub commits: u64,
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
    /// How `base` was chosen.
    pub basis: Basis,
    /// Tree object holding the reviewed content: the working tree.
    pub head: String,
    /// The HEAD commit the working tree was over.
    pub head_commit: String,
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

/// The file, and optionally the lines, a thread is about, valid at the
/// thread's revision. Without lines it is about the whole file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "AnchorFields", into = "AnchorFields")]
pub struct Anchor {
    /// Repository-relative path.
    pub path: String,
    pub lines: Option<Lines>,
}

/// A range of lines on one side of a diff.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Lines {
    pub side: Side,
    pub start: u32,
    pub end: u32,
}

impl Anchor {
    /// Lines `start..=end` of `side` of `path`.
    pub fn lines(path: impl Into<String>, side: Side, start: u32, end: u32) -> Self {
        Self {
            path: path.into(),
            lines: Some(Lines { side, start, end }),
        }
    }

    /// The whole of `path`.
    pub fn file(path: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            lines: None,
        }
    }
}

/// The serialized anchor: `{path, side, start, end}`, or `{path}` alone for
/// a whole file.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct AnchorFields {
    path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    side: Option<Side>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    start: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    end: Option<u32>,
}

impl TryFrom<AnchorFields> for Anchor {
    type Error = String;

    fn try_from(fields: AnchorFields) -> Result<Self, String> {
        let lines = match (fields.side, fields.start, fields.end) {
            (Some(side), Some(start), Some(end)) => Some(Lines { side, start, end }),
            (None, None, None) => None,
            _ => {
                return Err(format!(
                    "anchor for {} names side, start and end together, or none of them for a whole-file comment",
                    fields.path
                ));
            }
        };
        Ok(Self {
            path: fields.path,
            lines,
        })
    }
}

impl From<Anchor> for AnchorFields {
    fn from(anchor: Anchor) -> Self {
        Self {
            path: anchor.path,
            side: anchor.lines.map(|lines| lines.side),
            start: anchor.lines.map(|lines| lines.start),
            end: anchor.lines.map(|lines| lines.end),
        }
    }
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

impl Thread {
    /// Whether the user opened it, so only the user settles it.
    pub fn started_by_user(&self) -> bool {
        self.comments
            .first()
            .is_some_and(|comment| AuthorKind::of(&comment.author) == AuthorKind::User)
    }

    /// Whether the thread waits on the user: it is not settled (it is open,
    /// accepted or outdated, not dismissed, fixed or posted) and its latest
    /// comment other than refresh's own is not the user's. An agent's new
    /// finding waits on the user, and so does a user's thread an agent has
    /// answered; a user's comment nobody has answered yet does not.
    pub fn waits_on_user(&self) -> bool {
        matches!(
            self.status,
            Status::Open | Status::Accepted | Status::Outdated
        ) && self
            .comments
            .iter()
            .rev()
            .find(|comment| AuthorKind::of(&comment.author) != AuthorKind::Refresh)
            .is_some_and(|comment| AuthorKind::of(&comment.author) == AuthorKind::Agent)
    }
}

/// One of the user's pending comments: a new thread, or a reply to a thread
/// when `thread` is set. Drafts are private to the user until submitted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Draft {
    pub id: String,
    /// The submitted thread it replies to; absent for a new thread.
    pub thread: Option<String>,
    /// Index into the review's revisions where `anchor` is valid.
    pub revision: usize,
    /// Where a new thread points; absent for a reply or a review-level thread.
    pub anchor: Option<Anchor>,
    pub body: String,
    pub at: i64,
}

/// One submitted batch of the user's drafts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Submission {
    pub at: i64,
    /// The summary the user wrote for the batch.
    pub body: Option<String>,
    /// The comments it published, in draft order.
    pub comments: Vec<Published>,
}

/// A comment a submission published.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Published {
    pub thread: String,
    /// Index into the thread's comments; 0 when it opened the thread.
    pub comment: usize,
}

/// The content of one path's change: modes and object ids on each side, Git's
/// all-zero id and mode on a side where the path is absent. Two changes of a
/// path are the same change exactly when these are equal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Identity {
    pub old_mode: String,
    pub old: String,
    pub new_mode: String,
    pub new: String,
}

/// The revision the user last looked at in the viewer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Looked {
    /// Index into the review's revisions.
    pub revision: usize,
    pub at: i64,
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
    /// The session that started the review, such as `pi-<id>`, and is told
    /// when the user submits comments; absent when none was known.
    pub owner: Option<String>,
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
    /// The user's pending comments, oldest first. Omitted from output unless
    /// asked for, and from the file when there are none.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub drafts: Vec<Draft>,
    pub next_draft: u32,
    /// Oldest first.
    pub submissions: Vec<Submission>,
    /// Files the user marked reviewed, with the change each mark covers; a
    /// mark holds only while the file's current change is that change.
    pub reviewed: BTreeMap<String, Identity>,
    pub looked: Option<Looked>,
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

/// How a diff changes a path. Renames are off, so a path names the same file
/// on both sides.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FileStatus {
    Added,
    Modified,
    Deleted,
    /// Its kind changed, such as a file becoming a symlink; both sides exist.
    TypeChanged,
}

/// One changed path of a diff.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ChangedFile {
    pub path: String,
    pub status: FileStatus,
    pub added: u32,
    pub removed: u32,
    pub binary: bool,
}

/// One changed path of a review's current revision, with the user's view of it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReviewedFile {
    #[serde(flatten)]
    pub change: ChangedFile,
    /// The user marked this exact change reviewed.
    pub reviewed: bool,
    /// Its change differs from the revision the user last looked at; false
    /// when the user has not looked yet.
    pub changed_since_looked: bool,
}

/// A way to view a review's current revision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Preset {
    /// Base to head: the whole change.
    Everything,
    /// The HEAD commit to the working tree.
    Uncommitted,
    /// Base to the HEAD commit.
    Committed,
}

impl Preset {
    pub const ALL: [Self; 3] = [Self::Everything, Self::Uncommitted, Self::Committed];

    /// The serialized name.
    pub fn name(self) -> &'static str {
        match self {
            Self::Everything => "everything",
            Self::Uncommitted => "uncommitted",
            Self::Committed => "committed",
        }
    }

    /// The preset a serialized name denotes.
    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|preset| preset.name() == name)
    }
}
