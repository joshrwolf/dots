//! Reviews of a repository's changes: exact revisions, threads anchored to
//! the reviewed diff, and carry-forward when the change moves.
//!
//! A review names its content by object id: a base commit and a head tree,
//! with uncommitted work captured as a tree, so every thread's lines refer
//! to bytes that can be read back with `git show HEAD_TREE:PATH`. Review
//! files live in their worktree's own Git directory and go away with it. A
//! shared ref, `refs/dots/reviews/ID`, keeps reviewed content safe from
//! garbage collection in every worktree and makes ids unique across the
//! repository, so any worktree can find and change any review; removing the
//! review releases it, and pins whose review file is gone from every worktree
//! are released by the next start or remove.
//!
//! The user, [`USER`], reviews alongside agents. The user's comments can be
//! drafts, private until the user submits them all at once; a thread the
//! user started is settled only by the user. Reviewed marks and the revision
//! the user last looked at are the viewer's own state; presets are the
//! viewer's ways of looking at the current revision.
//! Callers pass the time explicitly.

mod diff;
mod git;
mod model;
mod store;
mod view;

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};

use serde::Serialize;

pub use model::{
    Anchor, AuthorKind, ChangedFile, Comment, Draft, FileStatus, Identity, Lines, Looked,
    NewThread, Preset, Published, REFRESH_AUTHOR, Review, ReviewedFile, Revision, RuleCitation,
    SCHEMA, Severity, Side, Status, Submission, TargetSpec, Thread, USER, Worktree,
};
pub use view::View;

use diff::{Change, Patch};
use git::Git;
use store::Store;
use view::Frame;

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Invalid(String),
    #[error("no review {0} in this repository; dots review list --all shows every review")]
    NotFound(String),
    #[error(
        "no reviews in this worktree; start one with dots review start or name a review id from dots review list --all"
    )]
    NoReviews,
    #[error("review {id} has no thread {thread}")]
    NoThread { id: String, thread: String },
    #[error("review {id} has no draft {draft}; dots review show --drafts lists them")]
    NoDraft { id: String, draft: String },
    #[error(
        "{thread} was started by the user, so only the user changes its status; reply with your outcome instead"
    )]
    UserDecides { thread: String },
    #[error(
        "review {id} is at revision {current}, not revision {expected} the threads were anchored against; nothing was recorded. Read the current revision with dots review show and anchor against it"
    )]
    Moved {
        id: String,
        expected: usize,
        current: usize,
    },
    #[error("nothing to review: {0}")]
    NothingToReview(String),
    #[error("{} thread(s) rejected; nothing was recorded:\n{}", .0.len(), Rejections(.0))]
    Rejected(Vec<Rejection>),
    #[error("review file {path} is unreadable: {reason}")]
    Corrupt { path: PathBuf, reason: String },
    #[error("{operation}: {message}")]
    Git {
        operation: &'static str,
        message: String,
    },
    #[error("{operation}")]
    Process {
        operation: &'static str,
        #[source]
        source: dots_process::Error,
    },
    #[error("{operation} {}", path.display())]
    Io {
        operation: &'static str,
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

/// Why one submitted thread was refused.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Rejection {
    /// Position in the submitted batch, from zero.
    pub index: usize,
    pub field: &'static str,
    pub message: String,
}

struct Rejections<'a>(&'a [Rejection]);

impl fmt::Display for Rejections<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (position, rejection) in self.0.iter().enumerate() {
            if position > 0 {
                writeln!(f)?;
            }
            write!(
                f,
                "  #{} {}: {}",
                rejection.index, rejection.field, rejection.message
            )?;
        }
        Ok(())
    }
}

/// Most threads one batch may hold.
pub const BATCH_LIMIT: usize = 200;
/// Most threads one review may hold.
pub const REVIEW_LIMIT: usize = 2000;
/// Most comments one thread may hold.
pub const COMMENT_LIMIT: usize = 500;

/// Most drafts one review may hold, and so one submission.
pub const DRAFT_LIMIT: usize = BATCH_LIMIT;

/// A review with its file and the files its current revision changes.
#[derive(Debug, Serialize)]
pub struct Overview {
    /// Its drafts only when asked for.
    pub review: Review,
    /// The review file.
    pub file: PathBuf,
    pub files: Vec<ReviewedFile>,
    /// Reads the reviewed patch.
    pub diff_command: String,
    /// The preset asked for.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub view: Option<View>,
}

/// What [`Reviews::show`] includes beyond the review itself.
#[derive(Debug, Clone, Copy, Default)]
pub struct ShowOptions {
    /// The user's drafts.
    pub drafts: bool,
    /// A preset view of the current revision.
    pub preset: Option<Preset>,
}

/// One of the user's comments before it becomes a draft: a new thread, or a
/// reply when `thread` is set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewDraft {
    pub thread: Option<String>,
    /// For a new thread, in the line numbers of the preset it was written in.
    pub anchor: Option<Anchor>,
    pub body: String,
}

/// What a submission published.
#[derive(Debug, Serialize)]
pub struct Submitted {
    pub submission: Submission,
    /// The threads it opened or replied to, as they now stand.
    pub threads: Vec<Thread>,
}

/// A review's identity, place, target and counts, without threads.
#[derive(Debug, Serialize)]
pub struct Summary {
    pub id: String,
    pub target: TargetSpec,
    pub profile: Option<String>,
    /// The session that started the review, if known.
    pub owner: Option<String>,
    pub worktree: Worktree,
    /// The review file.
    pub file: PathBuf,
    pub created_at: i64,
    pub updated_at: i64,
    pub revision: Revision,
    /// Thread counts by status.
    pub threads: BTreeMap<Status, usize>,
    /// How many drafts the user holds, when asked for.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub drafts: Option<usize>,
}

/// What a refresh did.
#[derive(Debug, Serialize)]
pub struct Refreshed {
    pub review: Review,
    /// Whether the target resolved to new content.
    pub changed: bool,
    /// Live threads now anchored to the current revision.
    pub carried: Vec<String>,
    /// Live threads whose lines changed; now `outdated`.
    pub outdated: Vec<String>,
    /// What changed since the previous revision, for re-review.
    pub delta: Option<Delta>,
    pub files: Vec<ChangedFile>,
}

/// The change between two revisions of one review.
#[derive(Debug, Serialize)]
pub struct Delta {
    pub from: Revision,
    pub to: Revision,
    pub files: Vec<ChangedFile>,
    pub diff_command: String,
}

impl TargetSpec {
    /// `FROM..TO`, both non-empty; `...` is refused because a range review
    /// compares exactly `FROM` with `TO`.
    pub fn range(value: &str) -> Result<Self> {
        match value.split_once("..") {
            Some((from, to)) if !from.is_empty() && !to.is_empty() && !to.starts_with('.') => {
                Ok(Self::Range {
                    from: from.to_owned(),
                    to: to.to_owned(),
                })
            }
            _ => Err(Error::Invalid(format!(
                "range {value:?} must be FROM..TO with both revisions named"
            ))),
        }
    }
}

impl fmt::Display for TargetSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Worktree => f.write_str("uncommitted changes against HEAD"),
            Self::Base { reference } => {
                write!(f, "changes since {reference}, with uncommitted work")
            }
            Self::Range { from, to } => write!(f, "{from}..{to}"),
        }
    }
}

/// The content a target resolves to, before it is pinned.
struct Resolved {
    base: String,
    head: String,
    head_commit: Option<String>,
    includes_worktree: bool,
}

impl Resolved {
    fn revision(&self, snapshot: String, at: i64) -> Revision {
        Revision {
            base: self.base.clone(),
            head: self.head.clone(),
            head_commit: self.head_commit.clone(),
            includes_worktree: self.includes_worktree,
            snapshot,
            at,
        }
    }
}

/// The reviews of a repository, opened from one of its worktrees. New reviews
/// and the default review belong to that worktree; a review named by id is
/// found in whichever worktree holds it, and changed under that worktree's
/// lock.
#[derive(Debug)]
pub struct Reviews {
    git: Git,
    store: Store,
}

impl Reviews {
    /// Open the reviews of the repository containing `directory`, from the
    /// worktree containing it.
    pub fn open(directory: &Path) -> Result<Self> {
        let git = Git::discover(directory)?;
        let store = Store::new(git.own_dir.join("dots/reviews"));
        Ok(Self { git, store })
    }

    /// Where this worktree keeps its review files.
    pub fn directory(&self) -> &Path {
        self.store.directory()
    }

    /// Start a review of `target` in this worktree on behalf of `owner`, the
    /// session to tell about replies. Refuses an empty change.
    pub fn start(
        &self,
        target: TargetSpec,
        profile: Option<String>,
        owner: Option<String>,
        now: i64,
    ) -> Result<Overview> {
        if let Some(profile) = &profile {
            name("profile", profile).map_err(Error::Invalid)?;
        }
        if let Some(owner) = &owner {
            text("owner", owner, 200, false).map_err(Error::Invalid)?;
        }
        self.store.prepare()?;
        let resolved = resolve(&self.git, self.store.directory(), &target)?;
        let change = Change::read(&self.git, &resolved.base, &resolved.head, 3)?;
        if change.raw.is_empty() {
            return Err(Error::NothingToReview(format!(
                "{target} resolves to no changes ({}..{})",
                short(&resolved.base),
                short(&resolved.head)
            )));
        }
        let worktree = Worktree {
            path: self.git.toplevel.clone(),
            branch: self.git.branch()?,
        };
        let review = self.store.create(
            Review {
                schema: SCHEMA,
                id: String::new(),
                target,
                profile,
                owner,
                worktree,
                created_at: now,
                updated_at: now,
                sequence: 0,
                revisions: Vec::new(),
                threads: Vec::new(),
                next_thread: 1,
                drafts: Vec::new(),
                next_draft: 1,
                submissions: Vec::new(),
                reviewed: BTreeMap::new(),
                looked: None,
            },
            |review| {
                let Some(snapshot) = self.git.pin_new(
                    &review.id,
                    &resolved.base,
                    &resolved.head,
                    resolved.head_commit.as_deref(),
                )?
                else {
                    return Ok(false);
                };
                review.revisions = vec![resolved.revision(snapshot, now)];
                Ok(true)
            },
        )?;
        self.release_orphans()?;
        let file = self.store.path(&review.id);
        self.overview(review, file, &change, ShowOptions::default())
    }

    /// The id of this worktree's most recently updated review.
    pub fn latest(&self) -> Result<String> {
        self.store
            .all()?
            .into_iter()
            .max_by_key(|review| review.sequence)
            .map(|review| review.id)
            .ok_or(Error::NoReviews)
    }

    /// This worktree's reviews or, with `all`, those of every worktree of the
    /// repository; oldest first.
    /// With `drafts`, each summary counts the user's drafts.
    pub fn list(&self, all: bool, drafts: bool) -> Result<Vec<Summary>> {
        let directories = if all {
            store::directories(&self.git.shared_dir)?
        } else {
            vec![self.store.directory().to_owned()]
        };
        let mut summaries = Vec::new();
        for directory in directories {
            let store = Store::new(directory);
            for review in store.all()? {
                let mut threads = BTreeMap::new();
                for thread in &review.threads {
                    *threads.entry(thread.status).or_default() += 1;
                }
                summaries.push(Summary {
                    file: store.path(&review.id),
                    revision: review.current().clone(),
                    id: review.id,
                    target: review.target,
                    profile: review.profile,
                    owner: review.owner,
                    worktree: review.worktree,
                    created_at: review.created_at,
                    updated_at: review.updated_at,
                    threads,
                    drafts: drafts.then_some(review.drafts.len()),
                });
            }
        }
        summaries
            .sort_by(|left, right| (left.created_at, &left.id).cmp(&(right.created_at, &right.id)));
        Ok(summaries)
    }

    /// A review with its changed files and, as `options` ask, the user's
    /// drafts and a preset view.
    pub fn show(&self, id: &str, options: ShowOptions) -> Result<Overview> {
        let store = self.locate(id)?;
        let review = store.load(id)?;
        let current = review.current();
        let change = Change::read(&self.git, &current.base, &current.head, 3)?;
        self.overview(review, store.path(id), &change, options)
    }

    /// Record `draft`, one of the user's comments, at the current revision.
    /// Its anchor counts lines of `preset` and must map to the review's
    /// lines; a reply names a submitted thread. `expected` works as for
    /// [`Reviews::add`].
    pub fn draft(
        &self,
        id: &str,
        draft: NewDraft,
        preset: Preset,
        expected: Option<usize>,
        now: i64,
    ) -> Result<Draft> {
        text("body", &draft.body, BODY_LIMIT, true).map_err(Error::Invalid)?;
        if draft.thread.is_some() && draft.anchor.is_some() {
            return Err(Error::Invalid(
                "a reply takes its thread's place; give a thread or an anchor, not both".into(),
            ));
        }
        self.locate(id)?.update(id, |review, _| {
            if review.drafts.len() >= DRAFT_LIMIT {
                return Err(Error::Invalid(format!(
                    "review {id} already holds {DRAFT_LIMIT} drafts; submit or discard some"
                )));
            }
            let at = review.revisions.len() - 1;
            if let Some(expected) = expected.filter(|expected| *expected != at) {
                return Err(Error::Moved {
                    id: id.to_owned(),
                    expected,
                    current: at,
                });
            }
            if let Some(thread) = &draft.thread
                && !review.threads.iter().any(|entry| &entry.id == thread)
            {
                return Err(Error::NoThread {
                    id: id.to_owned(),
                    thread: thread.clone(),
                });
            }
            let anchor = match &draft.anchor {
                None => None,
                Some(anchor) => {
                    check_anchor(anchor).map_err(Error::Invalid)?;
                    let revision = review.current();
                    let change = Change::read(&self.git, &revision.base, &revision.head, 3)?;
                    let available = view::available(&self.git, revision)?;
                    let frame = Frame::new(&self.git, revision, &change, preset, &available)?;
                    let mapped = frame.unplace(anchor)?.map_err(Error::Invalid)?;
                    anchor_in(&mapped, &change).map_err(Error::Invalid)?;
                    Some(mapped)
                }
            };
            let entry = Draft {
                id: format!("d{}", review.next_draft),
                thread: draft.thread,
                revision: at,
                anchor,
                body: draft.body,
                at: now,
            };
            review.next_draft += 1;
            review.drafts.push(entry.clone());
            Ok(entry)
        })
    }

    /// Drop draft `draft`, or every draft when it is `None`; returns what was
    /// dropped.
    pub fn discard(&self, id: &str, draft: Option<&str>) -> Result<Vec<Draft>> {
        self.locate(id)?.update(id, |review, _| match draft {
            None => Ok(std::mem::take(&mut review.drafts)),
            Some(draft) => {
                let position = review
                    .drafts
                    .iter()
                    .position(|entry| entry.id == draft)
                    .ok_or_else(|| Error::NoDraft {
                        id: id.to_owned(),
                        draft: draft.to_owned(),
                    })?;
                Ok(vec![review.drafts.remove(position)])
            }
        })
    }

    /// Publish every draft at once, in order, with an optional summary
    /// `body`, and record the submission; all or none, listing every draft
    /// that cannot be published. A new thread whose lines changed since it
    /// was drafted cannot.
    pub fn submit(&self, id: &str, body: Option<String>, now: i64) -> Result<Submitted> {
        if let Some(body) = &body {
            text("body", body, BODY_LIMIT, true).map_err(Error::Invalid)?;
        }
        self.locate(id)?.update(id, |review, sequence| {
            if review.drafts.is_empty() && body.is_none() {
                return Err(Error::Invalid(
                    "nothing to submit: no drafts and no summary".into(),
                ));
            }
            let opened = review
                .drafts
                .iter()
                .filter(|draft| draft.thread.is_none())
                .count();
            if review.threads.len() + opened > REVIEW_LIMIT {
                return Err(Error::Invalid(format!(
                    "review {id} would exceed {REVIEW_LIMIT} threads"
                )));
            }
            let rejections = unsubmittable(review);
            if !rejections.is_empty() {
                return Err(Error::Rejected(rejections));
            }
            let mut published = Vec::with_capacity(review.drafts.len());
            let mut touched: Vec<String> = Vec::new();
            for draft in std::mem::take(&mut review.drafts) {
                let thread = match draft.thread {
                    None => {
                        let thread = format!("t{}", review.next_thread);
                        review.next_thread += 1;
                        let new = NewThread {
                            anchor: draft.anchor,
                            body: draft.body,
                            author: USER.to_owned(),
                            title: None,
                            severity: None,
                            pre_existing: false,
                            confidence: None,
                            angle: None,
                            rule: None,
                        };
                        review
                            .threads
                            .push(open(thread.clone(), draft.revision, new, now));
                        published.push(Published {
                            thread: thread.clone(),
                            comment: 0,
                        });
                        thread
                    }
                    Some(thread) => {
                        let entry = review
                            .threads
                            .iter_mut()
                            .find(|entry| entry.id == thread)
                            .ok_or_else(|| Error::NoThread {
                                id: id.to_owned(),
                                thread: thread.clone(),
                            })?;
                        entry.comments.push(Comment {
                            author: USER.to_owned(),
                            at: now,
                            body: Some(draft.body),
                            status: None,
                        });
                        entry.updated_at = now;
                        published.push(Published {
                            thread: thread.clone(),
                            comment: entry.comments.len() - 1,
                        });
                        thread
                    }
                };
                if !touched.contains(&thread) {
                    touched.push(thread);
                }
            }
            let submission = Submission {
                at: now,
                body,
                comments: published,
            };
            review.submissions.push(submission.clone());
            review.touch(now, sequence);
            let threads = touched
                .iter()
                .filter_map(|thread| review.threads.iter().find(|entry| &entry.id == thread))
                .cloned()
                .collect();
            Ok(Submitted {
                submission,
                threads,
            })
        })
    }

    /// Mark changed file `path` reviewed as it now stands, or clear its mark.
    pub fn mark(&self, id: &str, path: &str, reviewed: bool) -> Result<ReviewedFile> {
        let store = self.locate(id)?;
        store.update(id, |review, _| {
            let revision = review.current();
            let change = Change::read(&self.git, &revision.base, &revision.head, 3)?;
            let Some(entry) = change.raw.get(path) else {
                return Err(Error::Invalid(format!(
                    "{path} is not changed in review {id}'s current revision"
                )));
            };
            if reviewed {
                review
                    .reviewed
                    .insert(path.to_owned(), entry.identity.clone());
            } else {
                review.reviewed.remove(path);
            }
            let looked = self.looked_change(review)?;
            Ok(reviewed_files(review, &change, looked.as_ref())
                .into_iter()
                .find(|file| file.change.path == path)
                .expect("a changed path has a file"))
        })
    }

    /// Record that the user looked at the current revision.
    pub fn look(&self, id: &str, now: i64) -> Result<Looked> {
        self.locate(id)?.update(id, |review, _| {
            let looked = Looked {
                revision: review.revisions.len() - 1,
                at: now,
            };
            review.looked = Some(looked.clone());
            Ok(looked)
        })
    }

    /// Open `threads` at the current revision, all or none. With `expected`,
    /// the revision index the anchors were read against, a review that has
    /// moved since refuses the batch with [`Error::Moved`].
    pub fn add(
        &self,
        id: &str,
        threads: Vec<NewThread>,
        expected: Option<usize>,
        now: i64,
    ) -> Result<Vec<Thread>> {
        if threads.is_empty() {
            return Err(Error::Invalid("no threads submitted".into()));
        }
        if threads.len() > BATCH_LIMIT {
            return Err(Error::Invalid(format!(
                "{} threads submitted; a batch holds at most {BATCH_LIMIT}",
                threads.len()
            )));
        }
        self.locate(id)?.update(id, |review, sequence| {
            if review.threads.len() + threads.len() > REVIEW_LIMIT {
                return Err(Error::Invalid(format!(
                    "review {id} would exceed {REVIEW_LIMIT} threads"
                )));
            }
            let at = review.revisions.len() - 1;
            if let Some(expected) = expected.filter(|expected| *expected != at) {
                return Err(Error::Moved {
                    id: id.to_owned(),
                    expected,
                    current: at,
                });
            }
            let revision = review.current().clone();
            let change = Change::read(&self.git, &revision.base, &revision.head, 3)?;
            let mut rejections = Vec::new();
            for (index, thread) in threads.iter().enumerate() {
                self.check(index, thread, &revision, &change, &mut rejections)?;
            }
            if !rejections.is_empty() {
                return Err(Error::Rejected(rejections));
            }
            let mut added = Vec::with_capacity(threads.len());
            for thread in threads {
                let id = format!("t{}", review.next_thread);
                review.next_thread += 1;
                added.push(open(id, at, thread, now));
            }
            review.threads.extend(added.iter().cloned());
            review.touch(now, sequence);
            Ok(added)
        })
    }

    /// Add a comment to a thread without changing its status.
    pub fn reply(
        &self,
        id: &str,
        thread: &str,
        author: String,
        body: String,
        now: i64,
    ) -> Result<Thread> {
        comment(&author, Some(&body))?;
        self.comment(id, thread, now, |entry, _| {
            entry.comments.push(Comment {
                author,
                at: now,
                body: Some(body),
                status: None,
            });
            Ok(())
        })
    }

    /// Change a thread's status, recorded as a comment by `author` with an
    /// optional body. `outdated` belongs to refresh and is refused, as is
    /// reopening a thread left at an earlier revision. Only the user changes
    /// the status of a thread the user started.
    ///
    /// The user's status changes take effect at once, never as drafts: a
    /// status is the user's decision about a thread, not a comment to batch.
    pub fn set(
        &self,
        id: &str,
        thread: &str,
        status: Status,
        author: String,
        body: Option<String>,
        now: i64,
    ) -> Result<Thread> {
        if status == Status::Outdated {
            return Err(Error::Invalid(
                "outdated is set by refresh when a thread's lines change".into(),
            ));
        }
        comment(&author, body.as_deref())?;
        self.comment(id, thread, now, |entry, revisions| {
            if entry.started_by_user() && AuthorKind::of(&author) != AuthorKind::User {
                return Err(Error::UserDecides {
                    thread: thread.to_owned(),
                });
            }
            if status.live() && entry.revision + 1 != revisions {
                return Err(Error::Invalid(format!(
                    "{thread}'s lines changed after revision {}; open a new thread against the current revision instead of reopening it",
                    entry.revision
                )));
            }
            entry.status = status;
            entry.comments.push(Comment {
                author,
                at: now,
                body,
                status: Some(status),
            });
            Ok(())
        })
    }

    /// Re-resolve the target in the review's own worktree. When it moved,
    /// move every thread whose lines are unchanged and still in the diff to
    /// the new revision; live ones whose lines changed become `outdated`,
    /// other dispositions stay behind.
    pub fn refresh(&self, id: &str, now: i64) -> Result<Refreshed> {
        let store = self.locate(id)?;
        store.update(id, |review, sequence| {
            let git = worktree_git(review, &store)?;
            let previous = review.current().clone();
            let next = resolve(&git, store.directory(), &review.target)?;
            if next.base == previous.base && next.head == previous.head {
                let change = Change::read(&git, &previous.base, &previous.head, 3)?;
                return Ok(Refreshed {
                    review: public(review.clone()),
                    changed: false,
                    carried: Vec::new(),
                    outdated: Vec::new(),
                    delta: None,
                    files: change.files(),
                });
            }
            let heads = Change::read(&git, &previous.head, &next.head, 0)?;
            let bases = if next.base == previous.base {
                Patch::default()
            } else {
                Patch::parse(&git.diff(&previous.base, &next.base, 0)?)?
            };
            let change = Change::read(&git, &next.base, &next.head, 3)?;
            let snapshot = git.pin_next(id, &next.base, &next.head, next.head_commit.as_deref())?;
            let next = next.revision(snapshot, now);
            let current = review.revisions.len() - 1;
            review.revisions.push(next.clone());
            let carry = |anchor: &Option<Anchor>| match anchor {
                None => Some(None),
                Some(anchor) => carry(anchor, &heads.patch, &bases, &change).map(Some),
            };
            // A draft whose lines changed stays behind; submit refuses it.
            for draft in &mut review.drafts {
                if draft.thread.is_none()
                    && draft.revision == current
                    && let Some(anchor) = carry(&draft.anchor)
                {
                    draft.anchor = anchor;
                    draft.revision = current + 1;
                }
            }
            let (mut carried, mut outdated) = (Vec::new(), Vec::new());
            for thread in &mut review.threads {
                if thread.revision != current {
                    continue;
                }
                let moved = carry(&thread.anchor);
                // Dispositions follow their lines when they can; only live
                // threads become outdated or count as carried.
                let live = thread.status.live();
                if let Some(anchor) = moved {
                    thread.anchor = anchor;
                    thread.revision = current + 1;
                    if live {
                        thread.updated_at = now;
                        carried.push(thread.id.clone());
                    }
                } else if live {
                    thread.updated_at = now;
                    thread.status = Status::Outdated;
                    thread.comments.push(Comment {
                        author: REFRESH_AUTHOR.to_owned(),
                        at: now,
                        body: None,
                        status: Some(Status::Outdated),
                    });
                    outdated.push(thread.id.clone());
                }
            }
            review.touch(now, sequence);
            Ok(Refreshed {
                review: public(review.clone()),
                changed: true,
                carried,
                outdated,
                delta: Some(Delta {
                    diff_command: Git::diff_command(&previous.head, &next.head),
                    files: heads.files(),
                    from: previous,
                    to: next,
                }),
                files: change.files(),
            })
        })
    }

    /// Delete a review and release its pinned content.
    pub fn remove(&self, id: &str) -> Result<()> {
        self.locate(id)?.remove(id, || self.git.unpin(id, None))?;
        self.release_orphans()
    }

    /// The store of the worktree that holds review `id`.
    fn locate(&self, id: &str) -> Result<Store> {
        store::check_id(id)?;
        store::directories(&self.git.shared_dir)?
            .into_iter()
            .map(Store::new)
            .find(|store| store.path(id).is_file())
            .ok_or_else(|| Error::NotFound(id.to_owned()))
    }

    /// Append to a thread's conversation through `change`, under the lock of
    /// the review's store. `change` receives the thread and the number of
    /// revisions.
    fn comment(
        &self,
        id: &str,
        thread: &str,
        now: i64,
        change: impl FnOnce(&mut Thread, usize) -> Result<()>,
    ) -> Result<Thread> {
        self.locate(id)?.update(id, |review, sequence| {
            let revisions = review.revisions.len();
            let entry = review
                .threads
                .iter_mut()
                .find(|entry| entry.id == thread)
                .ok_or_else(|| Error::NoThread {
                    id: id.to_owned(),
                    thread: thread.to_owned(),
                })?;
            if entry.comments.len() >= COMMENT_LIMIT {
                return Err(Error::Invalid(format!(
                    "{thread} already holds {COMMENT_LIMIT} comments"
                )));
            }
            change(entry, revisions)?;
            entry.updated_at = now;
            let updated = entry.clone();
            review.touch(now, sequence);
            Ok(updated)
        })
    }

    /// Release pins whose review file is gone from every worktree, such as
    /// after `git worktree remove`. Pins are listed before review files, and
    /// a review file always exists before its pin, so a review being started
    /// concurrently is never mistaken for an orphan.
    fn release_orphans(&self) -> Result<()> {
        let pins = self.git.pins()?;
        let live = store::live_ids(&self.git.shared_dir)?;
        for (id, commit) in pins.iter().filter(|(id, _)| !live.contains(id)) {
            self.git.unpin(id, Some(commit))?;
        }
        Ok(())
    }

    /// `review`, stored in `file`, whose current revision's change is
    /// `change`, with what `options` ask for.
    fn overview(
        &self,
        mut review: Review,
        file: PathBuf,
        change: &Change,
        options: ShowOptions,
    ) -> Result<Overview> {
        let looked = self.looked_change(&review)?;
        let files = reviewed_files(&review, change, looked.as_ref());
        let revision = review.current();
        let view = match options.preset {
            None => None,
            Some(preset) => {
                let available = view::available(&self.git, revision)?;
                let frame = Frame::new(&self.git, revision, change, preset, &available)?;
                let current = review.revisions.len() - 1;
                let threads = review
                    .threads
                    .iter()
                    .filter(|thread| thread.revision == current)
                    .filter_map(|thread| Some((thread.id.as_str(), thread.anchor.as_ref()?)));
                let drafts = review
                    .drafts
                    .iter()
                    .filter(|draft| options.drafts && draft.revision == current)
                    .filter_map(|draft| Some((draft.id.as_str(), draft.anchor.as_ref()?)));
                Some(frame.view(available, threads.chain(drafts))?)
            }
        };
        let diff_command = Git::diff_command(&revision.base, &revision.head);
        if !options.drafts {
            review = public(review);
        }
        Ok(Overview {
            review,
            file,
            files,
            diff_command,
            view,
        })
    }

    /// The change of the revision the user last looked at, unless that is the
    /// current one or the user has not looked.
    fn looked_change(&self, review: &Review) -> Result<Option<Change>> {
        let Some(looked) = &review.looked else {
            return Ok(None);
        };
        if looked.revision + 1 == review.revisions.len() {
            return Ok(None);
        }
        let revision = &review.revisions[looked.revision];
        Change::read(&self.git, &revision.base, &revision.head, 0).map(Some)
    }

    fn check(
        &self,
        index: usize,
        thread: &NewThread,
        revision: &Revision,
        change: &Change,
        rejections: &mut Vec<Rejection>,
    ) -> Result<()> {
        let mut reject = |field, message: String| {
            rejections.push(Rejection {
                index,
                field,
                message,
            });
        };
        if let Some(title) = &thread.title
            && let Err(message) = text("title", title, 200, false)
        {
            reject("title", message);
        }
        if let Err(message) = text("body", &thread.body, BODY_LIMIT, true) {
            reject("body", message);
        }
        if let Err(message) = author(&thread.author) {
            reject("author", message);
        }
        if let Some(angle) = &thread.angle
            && let Err(message) = name("angle", angle)
        {
            reject("angle", message);
        }
        if let Some(confidence) = thread.confidence
            && !(0.0..=1.0).contains(&confidence)
        {
            reject("confidence", format!("{confidence} is not between 0 and 1"));
        }
        if let Some(anchor) = &thread.anchor
            && let Err(message) = check_anchor(anchor).and_then(|()| anchor_in(anchor, change))
        {
            reject("anchor", message);
        }
        if let Some(rule) = &thread.rule {
            if let Err(message) = relative("rule path", &rule.path).and(lines(rule.start, rule.end))
            {
                reject("rule", message);
            } else {
                match self.git.file(&revision.head, &rule.path)? {
                    None => reject(
                        "rule",
                        format!("{} does not exist in the reviewed revision", rule.path),
                    ),
                    Some(bytes) => {
                        let count = line_count(&bytes);
                        if rule.end > count {
                            reject(
                                "rule",
                                format!(
                                    "{} has {count} lines; cited {}-{}",
                                    rule.path, rule.start, rule.end
                                ),
                            );
                        }
                    }
                }
            }
        }
        Ok(())
    }
}

/// Largest comment body.
const BODY_LIMIT: usize = 16 * 1024;

/// Git for the worktree that owns `review`, whose files are in `store`. The
/// worktree is found where the review recorded it and must still be the
/// checkout whose Git directory holds the store.
fn worktree_git(review: &Review, store: &Store) -> Result<Git> {
    let gone = || {
        Error::Invalid(format!(
            "review {} belongs to the worktree at {}, which is gone or moved; remove the review and start a new one",
            review.id,
            review.worktree.path.display()
        ))
    };
    let git = Git::discover(&review.worktree.path).map_err(|_| gone())?;
    let own = std::fs::canonicalize(git.own_dir.join("dots/reviews")).map_err(|_| gone())?;
    let holder = std::fs::canonicalize(store.directory()).map_err(|_| gone())?;
    if own != holder {
        return Err(gone());
    }
    Ok(git)
}

/// What `target` names now in the worktree of `git`, capturing uncommitted
/// work with a temporary index under `scratch`.
fn resolve(git: &Git, scratch: &Path, target: &TargetSpec) -> Result<Resolved> {
    let head_commit = || {
        git.head_commit()?.ok_or_else(|| {
            Error::Invalid("HEAD has no commit yet; commit once before reviewing".into())
        })
    };
    Ok(match target {
        TargetSpec::Worktree => {
            let head = head_commit()?;
            Resolved {
                base: head.clone(),
                head: git.capture_worktree(scratch)?,
                head_commit: Some(head),
                includes_worktree: true,
            }
        }
        TargetSpec::Base { reference } => {
            let reference = git.object(reference, "commit")?;
            let head = head_commit()?;
            Resolved {
                base: git.merge_base(&reference, &head)?,
                head: git.capture_worktree(scratch)?,
                head_commit: Some(head),
                includes_worktree: true,
            }
        }
        TargetSpec::Range { from, to } => Resolved {
            base: git.object(from, "commit")?,
            head: git.object(to, "tree")?,
            head_commit: Some(git.object(to, "commit")?),
            includes_worktree: false,
        },
    })
}

/// Why each of `review`'s drafts that cannot be published cannot: a new
/// thread whose lines changed since it was drafted, or a reply that would
/// overfill its thread. Positions count drafts.
fn unsubmittable(review: &Review) -> Vec<Rejection> {
    let current = review.revisions.len() - 1;
    let mut rejections = Vec::new();
    let mut replies: BTreeMap<&str, usize> = BTreeMap::new();
    for (index, draft) in review.drafts.iter().enumerate() {
        match &draft.thread {
            Some(thread) => *replies.entry(thread).or_default() += 1,
            None if draft.anchor.is_some() && draft.revision != current => {
                rejections.push(Rejection {
                    index,
                    field: "anchor",
                    message: format!(
                        "{}'s lines changed after revision {}; discard it and comment on the current revision",
                        draft.id, draft.revision
                    ),
                });
            }
            None => {}
        }
    }
    for (thread, count) in replies {
        let held = review
            .threads
            .iter()
            .find(|entry| entry.id == thread)
            .map_or(0, |entry| entry.comments.len());
        if held + count <= COMMENT_LIMIT {
            continue;
        }
        for (index, draft) in review.drafts.iter().enumerate() {
            if draft.thread.as_deref() == Some(thread) {
                rejections.push(Rejection {
                    index,
                    field: "thread",
                    message: format!(
                        "{} would take {thread} past {COMMENT_LIMIT} comments",
                        draft.id
                    ),
                });
            }
        }
    }
    rejections.sort_by_key(|rejection| rejection.index);
    rejections
}

/// `review` as anyone but the user's viewer sees it: without drafts.
fn public(mut review: Review) -> Review {
    review.drafts.clear();
    review
}

/// Where `anchor`, valid before a refresh, lands after it: `heads` and
/// `bases` are the zero-context patches from the old head and base to the
/// new ones, `change` the new revision's change. A whole-file anchor holds
/// while its path is still changed; lines must be unchanged and still within
/// the new diff. `None` when it does not carry.
fn carry(anchor: &Anchor, heads: &Patch, bases: &Patch, change: &Change) -> Option<Anchor> {
    if !change.changes(&anchor.path) {
        return None;
    }
    let Some(lines) = anchor.lines else {
        return Some(anchor.clone());
    };
    let map = match lines.side {
        Side::New => heads,
        Side::Old => bases,
    };
    map.map(&anchor.path, lines.start, lines.end)
        .filter(|&(start, end)| change.patch.shows(&anchor.path, lines.side, start, end))
        .map(|(start, end)| Anchor::lines(&anchor.path, lines.side, start, end))
}

/// Each changed file of `change`, the review's current one, with whether the
/// user's mark covers it and whether it differs from `looked`, the change of
/// the revision the user last looked at when that is an earlier one.
fn reviewed_files(review: &Review, change: &Change, looked: Option<&Change>) -> Vec<ReviewedFile> {
    change
        .files()
        .into_iter()
        .map(|file| {
            let identity = &change.raw[&file.path].identity;
            ReviewedFile {
                reviewed: review.reviewed.get(&file.path) == Some(identity),
                changed_since_looked: looked.is_some_and(|looked| {
                    looked.raw.get(&file.path).map(|entry| &entry.identity) != Some(identity)
                }),
                change: file,
            }
        })
        .collect()
}

/// An anchor's own shape: a relative path and, if it has lines, a range.
fn check_anchor(anchor: &Anchor) -> Result<(), String> {
    relative("anchor path", &anchor.path)?;
    match anchor.lines {
        Some(range) => lines(range.start, range.end),
        None => Ok(()),
    }
}

/// Whether `anchor` names a path `change` changes and, with lines, lines
/// within one hunk of its diff.
fn anchor_in(anchor: &Anchor, change: &Change) -> Result<(), String> {
    if !change.changes(&anchor.path) {
        return Err(format!(
            "{} is not changed in this review; anchor to a changed path or omit the anchor for a review-level thread",
            anchor.path
        ));
    }
    let Some(Lines { side, start, end }) = anchor.lines else {
        return Ok(());
    };
    if change.patch.shows(&anchor.path, side, start, end) {
        return Ok(());
    }
    Err(format!(
        "{}:{start}-{end} ({}) is not within one hunk of the reviewed diff ({}); anchor to diff lines, to the whole file, or omit the anchor",
        anchor.path,
        match side {
            Side::New => "new side",
            Side::Old => "old side",
        },
        hunk_list(&change.patch, &anchor.path, side)
    ))
}

fn open(id: String, revision: usize, thread: NewThread, now: i64) -> Thread {
    Thread {
        id,
        revision,
        anchor: thread.anchor,
        status: Status::Open,
        title: thread.title,
        severity: thread.severity,
        pre_existing: thread.pre_existing,
        confidence: thread.confidence,
        angle: thread.angle,
        rule: thread.rule,
        comments: vec![Comment {
            author: thread.author,
            at: now,
            body: Some(thread.body),
            status: None,
        }],
        created_at: now,
        updated_at: now,
    }
}

/// A caller-given author: one line, never refresh's own name.
fn author(name: &str) -> Result<(), String> {
    text("author", name, 200, false)?;
    if AuthorKind::of(name) == AuthorKind::Refresh {
        return Err(format!("author {name:?} is reserved for refresh"));
    }
    Ok(())
}

/// Validate a reply or status change's author and optional body.
fn comment(name: &str, body: Option<&str>) -> Result<()> {
    let mut errors = Vec::new();
    if let Err(message) = author(name) {
        errors.push(message);
    }
    if let Some(body) = body
        && let Err(message) = text("body", body, BODY_LIMIT, true)
    {
        errors.push(message);
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(Error::Invalid(errors.join("; ")))
    }
}

fn hunk_list(patch: &Patch, changed: &str, side: Side) -> String {
    let ranges: Vec<String> = patch
        .files
        .get(changed)
        .map(|file| file.hunks.as_slice())
        .unwrap_or_default()
        .iter()
        .map(|hunk| match side {
            Side::New => (hunk.new_start, hunk.new_len),
            Side::Old => (hunk.old_start, hunk.old_len),
        })
        .filter(|&(_, len)| len > 0)
        .map(|(start, len)| format!("{start}-{}", start + len - 1))
        .collect();
    if ranges.is_empty() {
        "no lines on that side".into()
    } else {
        format!("hunks cover {}", ranges.join(", "))
    }
}

fn line_count(bytes: &[u8]) -> u32 {
    let newlines = bytes.split(|byte| *byte == b'\n').count() - 1;
    let trailing = usize::from(!bytes.is_empty() && !bytes.ends_with(b"\n"));
    u32::try_from(newlines + trailing).unwrap_or(u32::MAX)
}

fn short(id: &str) -> &str {
    id.get(..12).unwrap_or(id)
}

fn text(field: &str, value: &str, limit: usize, multiline: bool) -> Result<(), String> {
    if value.trim().is_empty() {
        return Err(format!("{field} is empty"));
    }
    if value.len() > limit {
        return Err(format!("{field} is {} bytes; at most {limit}", value.len()));
    }
    if !multiline && value.contains(['\n', '\r']) {
        return Err(format!("{field} must be one line"));
    }
    Ok(())
}

/// Lowercase ASCII letters, digits and hyphens, 1 to 64 characters.
fn name(field: &str, value: &str) -> Result<(), String> {
    if value.is_empty()
        || value.len() > 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    {
        return Err(format!(
            "{field} {value:?} must be 1-64 lowercase letters, digits or hyphens"
        ));
    }
    Ok(())
}

fn relative(field: &str, path: &str) -> Result<(), String> {
    if path.is_empty()
        || path.starts_with('/')
        || path
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err(format!(
            "{field} {path:?} must be repository-relative, like src/lib.rs"
        ));
    }
    Ok(())
}

fn lines(start: u32, end: u32) -> Result<(), String> {
    if start == 0 || end < start {
        return Err(format!(
            "lines {start}-{end} must start at 1 or later and not end before they start"
        ));
    }
    Ok(())
}
