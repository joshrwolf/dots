//! The worktrees of many repositories at a glance: each one's change against
//! its base and what its review waits on the user for.
//!
//! The inbox reads and never writes: it creates and refreshes no review,
//! captures no working tree, takes no lock, and runs Git with optional locks
//! off, so the index is not refreshed on disk either. Its signals therefore
//! describe the working tree as it is now, while a review's threads stay at
//! the review's current revision until something opens or refreshes it.
use std::collections::{BTreeMap, BTreeSet};
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use serde::Serialize;

use crate::base::Refs;
use crate::git::{Git, Listed};
use crate::store::Store;
use crate::{Basis, Error, Result, Review, SCHEMA, TargetSpec};

/// Many worktrees' changes and reviews.
#[derive(Debug, Serialize)]
pub struct Inbox {
    pub schema: u32,
    /// Most in need of the user's attention first: threads waiting on the
    /// user, then drafts, then files changed since the user last looked,
    /// then worktrees with a review, then larger changes; ties by path.
    pub worktrees: Vec<InboxEntry>,
    /// Repositories and worktrees that could not be read, and why.
    pub skipped: Vec<Skipped>,
}

/// One worktree.
#[derive(Debug, Serialize)]
pub struct InboxEntry {
    /// The repository's common Git directory.
    pub repository: PathBuf,
    /// Top level of the working tree.
    pub path: PathBuf,
    /// `None` when HEAD is detached.
    pub branch: Option<String>,
    /// The HEAD commit.
    pub head: String,
    /// The commit the worktree's review compares against, chosen as opening
    /// the review would choose it now.
    pub base: String,
    /// As opening the review records it, except that `candidates` leaves out
    /// those that could not be nearest: an earlier-preferred candidate
    /// contains their commit.
    pub basis: Basis,
    /// The working tree against `base`.
    pub diff: DiffSize,
    /// The worktree's own review, if it has one.
    pub review: Option<ReviewSignals>,
    /// How many other reviews, of other targets, the worktree holds.
    pub other_reviews: usize,
}

/// The size of a change, untracked files included.
#[derive(Debug, Default, Serialize)]
pub struct DiffSize {
    pub files: usize,
    pub added: u64,
    pub removed: u64,
}

/// What a worktree's review asks of the user.
#[derive(Debug, Serialize)]
pub struct ReviewSignals {
    pub id: String,
    /// The session working on the change, told when the user submits.
    pub owner: Option<String>,
    /// The review's current revision index.
    pub revision: usize,
    pub updated_at: i64,
    /// Threads waiting on the user; see [`crate::Thread::waits_on_user`].
    pub waiting: Vec<String>,
    /// How many drafts the user holds.
    pub drafts: usize,
    /// Paths whose change in the working tree differs from the revision the
    /// user last looked at; `None` when the user has not looked yet.
    pub changed_since_looked: Option<Vec<String>>,
}

/// A repository or worktree the inbox could not read.
#[derive(Debug, Serialize)]
pub struct Skipped {
    pub path: PathBuf,
    pub reason: String,
}

/// The inbox over the worktrees of `repositories`, each named by any
/// directory in it or its Git directory; a repository named twice is read
/// once. Worktrees with no change and no review are left out unless `all`.
pub fn inbox(repositories: &[PathBuf], all: bool) -> Inbox {
    let mut skipped = Vec::new();
    let mut seen = BTreeSet::new();
    let mut jobs = Vec::new();
    let mut repos = Vec::new();
    for path in repositories {
        let common = match Git::common_dir(path) {
            Ok(common) => common,
            Err(error) => {
                skipped.push(Skipped {
                    path: path.clone(),
                    reason: error.to_string(),
                });
                continue;
            }
        };
        if !seen.insert(common.clone()) {
            continue;
        }
        match repository(&common) {
            Ok(None) => {}
            Ok(Some((refs, listed))) => {
                let index = repos.len();
                repos.push((common, refs));
                jobs.extend(listed.into_iter().map(|listed| (index, listed)));
            }
            Err(error) => skipped.push(Skipped {
                path: common,
                reason: error.to_string(),
            }),
        }
    }
    let results = parallel(&jobs, |(index, listed)| {
        let (common, refs) = &repos[*index];
        entry(common, refs, listed)
    });
    let mut worktrees = Vec::new();
    for ((_, listed), result) in jobs.iter().zip(results) {
        match result {
            Ok(entry) if all || entry.diff.files > 0 || entry.review.is_some() => {
                worktrees.push(entry);
            }
            Ok(entry) if entry.other_reviews > 0 => worktrees.push(entry),
            Ok(_) => {}
            Err(error) => skipped.push(Skipped {
                path: listed.path.clone(),
                reason: error.to_string(),
            }),
        }
    }
    worktrees.sort_by(|left, right| {
        attention(right)
            .cmp(&attention(left))
            .then_with(|| left.path.cmp(&right.path))
    });
    Inbox {
        schema: SCHEMA,
        worktrees,
        skipped,
    }
}

/// The base refs and usable worktrees of the repository at `common`, if it
/// has any.
fn repository(common: &Path) -> Result<Option<(Refs, Vec<Listed>)>> {
    let listed = Git::worktrees(common)?;
    // Refs are shared, so any worktree reads them.
    let Some(first) = listed.first() else {
        return Ok(None);
    };
    let git = Git::checkout(&first.path, common)?;
    let mut refs = Refs::read(&git)?;
    refs.prune(&git)?;
    Ok(Some((refs, listed)))
}

/// Run `work` on each job on a few threads, keeping the jobs' order.
fn parallel<J: Sync, T: Send>(jobs: &[J], work: impl Fn(&J) -> T + Sync) -> Vec<T> {
    let threads = std::thread::available_parallelism()
        .map_or(4, usize::from)
        .clamp(1, 4)
        .min(jobs.len().max(1));
    let next = AtomicUsize::new(0);
    let mut results: Vec<(usize, T)> = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..threads)
            .map(|_| {
                scope.spawn(|| {
                    let mut done = Vec::new();
                    loop {
                        let index = next.fetch_add(1, Ordering::Relaxed);
                        let Some(job) = jobs.get(index) else {
                            return done;
                        };
                        done.push((index, work(job)));
                    }
                })
            })
            .collect();
        handles
            .into_iter()
            .flat_map(|handle| {
                handle
                    .join()
                    .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
            })
            .collect()
    });
    results.sort_by_key(|(index, _)| *index);
    results.into_iter().map(|(_, result)| result).collect()
}

fn attention(entry: &InboxEntry) -> (usize, usize, usize, bool, usize) {
    let review = entry.review.as_ref();
    (
        review.map_or(0, |review| review.waiting.len()),
        review.map_or(0, |review| review.drafts),
        review
            .and_then(|review| review.changed_since_looked.as_ref())
            .map_or(0, Vec::len),
        review.is_some(),
        entry.diff.files,
    )
}

fn entry(common: &Path, refs: &Refs, listed: &Listed) -> Result<InboxEntry> {
    let Some(head) = &listed.head else {
        return Err(Error::Invalid("HEAD has no commit yet".into()));
    };
    let git = Git::checkout(&listed.path, common)?;
    let reviews = Store::new(git.own_dir.join("dots/reviews")).all()?;
    let (own, others): (Vec<Review>, Vec<Review>) = reviews
        .into_iter()
        .partition(|review| matches!(review.target, TargetSpec::Worktree { .. }));
    let own = own.into_iter().max_by_key(|review| review.sequence);
    let kept = match own.as_ref().map(|review| &review.target) {
        Some(TargetSpec::Worktree { base }) => base.as_deref(),
        _ => None,
    };
    let chosen = refs.choose(&git, head, listed.branch.as_deref(), kept)?;
    let untracked = git.untracked()?;
    let (diff, changed) = measure(&git, &chosen.base, &untracked)?;
    let review = match own {
        None => None,
        Some(review) => Some(signals(&git, &review, &chosen.base, &untracked, &changed)?),
    };
    Ok(InboxEntry {
        repository: common.to_owned(),
        path: listed.path.clone(),
        branch: listed.branch.clone(),
        head: head.clone(),
        base: chosen.base,
        basis: chosen.basis,
        diff,
        review,
        other_reviews: others.len(),
    })
}

/// The working tree's change against `base`, and the paths it changes.
fn measure(git: &Git, base: &str, untracked: &[String]) -> Result<(DiffSize, BTreeSet<String>)> {
    let mut size = DiffSize::default();
    let mut changed = BTreeSet::new();
    let numstat = git.numstat(base)?;
    for record in numstat.split(|byte| *byte == 0) {
        let Ok(record) = std::str::from_utf8(record) else {
            continue;
        };
        let mut fields = record.splitn(3, '\t');
        let (Some(added), Some(removed), Some(path)) =
            (fields.next(), fields.next(), fields.next())
        else {
            continue;
        };
        // Binary files count "-" lines.
        size.added += added.parse::<u64>().unwrap_or(0);
        size.removed += removed.parse::<u64>().unwrap_or(0);
        changed.insert(path.to_owned());
    }
    for path in untracked {
        size.added += lines(&git.toplevel.join(path));
        changed.insert(path.clone());
    }
    size.files = changed.len();
    Ok((size, changed))
}

/// Most bytes of an untracked file read to count its lines.
const LINE_LIMIT: u64 = 8 * 1024 * 1024;

/// Lines of an untracked file as Git would count them for a diff: none for
/// a binary file, one for a symbolic link, at most [`LINE_LIMIT`] bytes
/// counted. Unreadable files count none.
fn lines(path: &Path) -> u64 {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => return 1,
        Ok(metadata) if metadata.is_file() => {}
        _ => return 0,
    }
    let mut bytes = Vec::new();
    if std::fs::File::open(path)
        .and_then(|file| file.take(LINE_LIMIT).read_to_end(&mut bytes))
        .is_err()
    {
        return 0;
    }
    if bytes.iter().take(8000).any(|byte| *byte == 0) {
        return 0;
    }
    u64::from(crate::line_count(&bytes))
}

/// What `review` waits on the user for, against the working tree whose
/// change from `base` touches `changed`, `untracked` among them.
fn signals(
    git: &Git,
    review: &Review,
    base: &str,
    untracked: &[String],
    changed: &BTreeSet<String>,
) -> Result<ReviewSignals> {
    let changed_since_looked = match &review.looked {
        None => None,
        Some(looked) => {
            let revision = review.revisions.get(looked.revision).ok_or_else(|| {
                Error::Invalid(format!(
                    "review {} last looked at a revision it lacks",
                    review.id
                ))
            })?;
            Some(since(
                git,
                &revision.snapshot,
                &revision.head,
                &revision.base,
                base,
                untracked,
                changed,
            )?)
        }
    };
    Ok(ReviewSignals {
        id: review.id.clone(),
        owner: review.owner.clone(),
        revision: review.revisions.len() - 1,
        updated_at: review.updated_at,
        waiting: review
            .threads
            .iter()
            .filter(|thread| thread.waits_on_user())
            .map(|thread| thread.id.clone())
            .collect(),
        drafts: review.drafts.len(),
        changed_since_looked,
    })
}

/// Paths whose change differs between the revision the user looked at
/// (pinned by `snapshot`, whose tree is `head`, against `looked_base`) and
/// the working tree against `base`: the working tree's content differs from
/// the looked-at head, or the base moved under a path the change touches.
fn since(
    git: &Git,
    snapshot: &str,
    head: &str,
    looked_base: &str,
    base: &str,
    untracked: &[String],
    changed: &BTreeSet<String>,
) -> Result<Vec<String>> {
    let untracked_set: BTreeSet<&str> = untracked.iter().map(String::as_str).collect();
    // Untracked files show as removed against the snapshot; they are
    // compared by content below.
    let mut paths: BTreeSet<String> = git
        .changed_since(snapshot)?
        .into_iter()
        .filter(|path| !untracked_set.contains(path.as_str()))
        .collect();
    if !untracked.is_empty() {
        let looked: BTreeMap<String, String> = git.blobs(head, untracked)?;
        let present: Vec<String> = untracked
            .iter()
            .filter(|path| looked.contains_key(*path))
            .cloned()
            .collect();
        let hashes = git.hash_files(&present)?;
        for path in untracked {
            if !looked.contains_key(path) {
                paths.insert(path.clone());
            }
        }
        for (path, hash) in present.iter().zip(hashes) {
            if looked.get(path) != Some(&hash) {
                paths.insert(path.clone());
            }
        }
    }
    if looked_base != base {
        paths.extend(
            git.changed_between(looked_base, base)?
                .into_iter()
                .filter(|path| changed.contains(path)),
        );
    }
    Ok(paths.into_iter().collect())
}
