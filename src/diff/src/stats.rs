//! Line counts for worktree summaries, cached across worktrees and runs.
//!
//! Counts between two blobs never change, and the worktrees of one
//! repository share most of their blobs, so counts are kept per blob pair
//! in an append-only file under the user cache directory. A worktree's
//! committed totals depend only on its base and HEAD trees and are kept
//! the same way; only its uncommitted files are counted afresh.

use std::collections::{BTreeMap, HashMap};
use std::fs::{self, File, OpenOptions};
use std::io::{BufWriter, Read as _, Write as _};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::SystemTime;

use anyhow::Result;

use crate::app::Summary;
use crate::diff;
use crate::repo::{self, Mode, Planned, Reader, Stamp, Target, TreeChange};
use crate::review;

/// Lines added and removed, or `None` for a binary file.
pub(crate) type Counts = Option<(u32, u32)>;

/// The cache file keeps at most this many records; a larger file is cut to
/// its newest half when loaded.
const MAX_RECORDS: usize = 400_000;
const RECORD: usize = 1 + 20 + 20 + 12;
const TAG_LINES: u8 = 1;
const TAG_TOTALS: u8 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Totals {
    pub(crate) added: u32,
    pub(crate) removed: u32,
    pub(crate) files: u32,
}

/// One cache entry as stored on disk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Record {
    Lines {
        old: gix::ObjectId,
        new: gix::ObjectId,
        counts: Counts,
    },
    Totals {
        target: Target,
        totals: Totals,
    },
}

impl Record {
    #[expect(
        clippy::many_single_char_names,
        reason = "the record's fields by position"
    )]
    pub(crate) fn encode(&self) -> [u8; RECORD] {
        let mut out = [0u8; RECORD];
        let (tag, a, b, x, y, z) = match *self {
            Record::Lines { old, new, counts } => {
                let (x, y, binary) = counts.map_or((0, 0, 1), |(a, r)| (a, r, 0));
                (TAG_LINES, old, new, x, y, binary)
            }
            Record::Totals { target, totals } => (
                TAG_TOTALS,
                target.base_tree,
                target.head_tree,
                totals.added,
                totals.removed,
                totals.files,
            ),
        };
        out[0] = tag;
        out[1..21].copy_from_slice(a.as_bytes());
        out[21..41].copy_from_slice(b.as_bytes());
        out[41..45].copy_from_slice(&x.to_le_bytes());
        out[45..49].copy_from_slice(&y.to_le_bytes());
        out[49..53].copy_from_slice(&z.to_le_bytes());
        out
    }

    #[expect(
        clippy::many_single_char_names,
        reason = "the record's fields by position"
    )]
    pub(crate) fn decode(bytes: &[u8]) -> Option<Self> {
        let bytes: &[u8; RECORD] = bytes.try_into().ok()?;
        let id = |r: Range| gix::ObjectId::try_from(&bytes[r]).ok();
        let num = |at: usize| {
            u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
        };
        let (a, b) = (id(1..21)?, id(21..41)?);
        let (x, y, z) = (num(41), num(45), num(49));
        match bytes[0] {
            TAG_LINES => Some(Record::Lines {
                old: a,
                new: b,
                counts: (z == 0).then_some((x, y)),
            }),
            TAG_TOTALS => Some(Record::Totals {
                target: Target {
                    base_tree: a,
                    head_tree: b,
                },
                totals: Totals {
                    added: x,
                    removed: y,
                    files: z,
                },
            }),
            _ => None,
        }
    }
}

type Range = std::ops::Range<usize>;

/// Decodes a cache file, stopping at a torn or foreign record, and keeps
/// the newest `max` records.
pub(crate) fn load_records(bytes: &[u8], max: usize) -> Vec<Record> {
    let mut records: Vec<Record> = bytes
        .as_chunks::<RECORD>()
        .0
        .iter()
        .map_while(|r| Record::decode(r))
        .collect();
    if records.len() > max {
        records.drain(..records.len() - max);
    }
    records
}

#[derive(Debug, Default)]
struct Inner {
    lines: HashMap<(gix::ObjectId, gix::ObjectId), Counts>,
    totals: HashMap<Target, Totals>,
    /// Working-tree files already hashed, by size and modification time.
    stamps: HashMap<(PathBuf, Stamp), gix::ObjectId>,
    log: Option<BufWriter<File>>,
}

/// The caches of one repository, shared by every worktree and thread.
#[derive(Debug, Default)]
pub(crate) struct Stats {
    inner: Mutex<Inner>,
}

impl Stats {
    /// Loads the cache of the repository `worktree` belongs to. Without a
    /// usable cache directory it stays in memory.
    pub(crate) fn open(worktree: &Path) -> Self {
        let stats = Self::default();
        let common = gix::open(worktree).map(|r| r.common_dir().to_owned());
        if let Some(path) = common.ok().as_deref().and_then(cache_path) {
            // A cache that cannot be read or written only costs speed.
            let _ = stats.load(&path);
        }
        stats
    }

    fn load(&self, path: &Path) -> Result<()> {
        let mut bytes = Vec::new();
        if let Ok(mut file) = File::open(path) {
            file.read_to_end(&mut bytes)?;
        }
        let total = bytes.len() / RECORD;
        let keep = if total > MAX_RECORDS {
            MAX_RECORDS / 2
        } else {
            MAX_RECORDS
        };
        let records = load_records(&bytes, keep);
        let clean = records.len() == total && bytes.len() % RECORD == 0;
        if !clean {
            // Rewrite: cut to the newest records, or drop a torn tail.
            if let Some(dir) = path.parent() {
                fs::create_dir_all(dir)?;
            }
            let tmp = path.with_extension("tmp");
            let mut out = BufWriter::new(File::create(&tmp)?);
            for r in &records {
                out.write_all(&r.encode())?;
            }
            out.flush()?;
            fs::rename(&tmp, path)?;
        }
        let mut inner = self.lock();
        for r in records {
            match r {
                Record::Lines { old, new, counts } => {
                    inner.lines.insert((old, new), counts);
                }
                Record::Totals { target, totals } => {
                    inner.totals.insert(target, totals);
                }
            }
        }
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let file = OpenOptions::new().create(true).append(true).open(path)?;
        inner.log = Some(BufWriter::new(file));
        Ok(())
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        // A panic while holding the lock leaves only cache entries behind.
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn append(inner: &mut Inner, record: Record) {
        if let Some(log) = &mut inner.log
            && log.write_all(&record.encode()).is_err()
        {
            inner.log = None;
        }
    }

    pub(crate) fn lines(&self, old: gix::ObjectId, new: gix::ObjectId) -> Option<Counts> {
        self.lock().lines.get(&(old, new)).copied()
    }

    pub(crate) fn put_lines(&self, old: gix::ObjectId, new: gix::ObjectId, counts: Counts) {
        let mut inner = self.lock();
        if inner.lines.insert((old, new), counts).is_none() {
            Self::append(&mut inner, Record::Lines { old, new, counts });
        }
    }

    pub(crate) fn totals(&self, target: Target) -> Option<Totals> {
        self.lock().totals.get(&target).copied()
    }

    pub(crate) fn put_totals(&self, target: Target, totals: Totals) {
        let mut inner = self.lock();
        if inner.totals.insert(target, totals).is_none() {
            Self::append(&mut inner, Record::Totals { target, totals });
        }
    }

    pub(crate) fn flush(&self) {
        if let Some(log) = &mut self.lock().log {
            let _ = log.flush();
        }
    }

    fn stamp(&self, path: &Path, stamp: Stamp) -> Option<gix::ObjectId> {
        self.lock().stamps.get(&(path.to_owned(), stamp)).copied()
    }

    fn put_stamp(&self, path: PathBuf, stamp: Stamp, id: gix::ObjectId) {
        self.lock().stamps.insert((path, stamp), id);
    }

    /// Counts between two blobs, from the cache or by reading them.
    fn blob_counts(
        &self,
        reader: &Reader,
        old: Option<gix::ObjectId>,
        new: Option<gix::ObjectId>,
    ) -> Result<Counts> {
        let null = gix::ObjectId::null(gix::hash::Kind::Sha1);
        let key = (old.unwrap_or(null), new.unwrap_or(null));
        if let Some(c) = self.lines(key.0, key.1) {
            return Ok(c);
        }
        let read = |id: Option<gix::ObjectId>| id.map_or(Ok(Vec::new()), |id| reader.blob(id));
        let counts = diff::line_counts(&read(old)?, &read(new)?);
        self.put_lines(key.0, key.1, counts);
        Ok(counts)
    }

    /// Counts between a blob and a working-tree file in Git's form, with
    /// the file's blob id (`None` once deleted) and modification time.
    fn worktree_counts(
        &self,
        reader: &mut Reader,
        root: &Path,
        path: &str,
        old: Option<gix::ObjectId>,
    ) -> Result<Option<Counted>> {
        let full = root.join(path);
        let Ok(meta) = fs::symlink_metadata(&full) else {
            return Ok(Some(Counted {
                counts: self.blob_counts(reader, old, None)?,
                id: None,
                modified: None,
            }));
        };
        if meta.is_dir() {
            return Ok(None);
        }
        let stamp = Stamp {
            modified: meta.modified().ok(),
            len: meta.len(),
        };
        let null = gix::ObjectId::null(gix::hash::Kind::Sha1);
        if let Some(id) = self.stamp(&full, stamp)
            && let Some(counts) = self.lines(old.unwrap_or(null), id)
        {
            return Ok(Some(Counted {
                counts,
                id: Some(id),
                modified: stamp.modified,
            }));
        }
        let data = reader.worktree_file(path)?;
        let id = gix::objs::compute_hash(gix::hash::Kind::Sha1, gix::object::Kind::Blob, &data)?;
        self.put_stamp(full, stamp, id);
        let old_data = old.map_or(Ok(Vec::new()), |id| reader.blob(id))?;
        let counts = diff::line_counts(&old_data, &data);
        self.put_lines(old.unwrap_or(null), id, counts);
        Ok(Some(Counted {
            counts,
            id: Some(id),
            modified: stamp.modified,
        }))
    }
}

/// A working-tree file's counts against its base.
struct Counted {
    counts: Counts,
    id: Option<gix::ObjectId>,
    modified: Option<SystemTime>,
}

fn cache_path(common_dir: &Path) -> Option<PathBuf> {
    use std::hash::{Hash as _, Hasher as _};
    let dir = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::home_dir().map(|h| h.join(".cache")))?;
    let common = common_dir
        .canonicalize()
        .unwrap_or_else(|_| common_dir.to_owned());
    let mut hasher = std::hash::DefaultHasher::new();
    common.hash(&mut hasher);
    Some(
        dir.join("dots-diff")
            .join(format!("{:016x}.lines", hasher.finish())),
    )
}

/// A worktree's changes against its default-branch merge base, counted
/// through the caches: committed totals by tree pair, then the
/// uncommitted files git reports. Also names the changes' content, so a
/// later summary tells whether anything changed since, and reads when the
/// user last looked at them.
pub(crate) fn summarize(stats: &Stats, worktree: &Path) -> Result<Summary> {
    let (repo, _, target) = repo::open(worktree, Mode::Base)?;
    let entries = repo::status(&repo, worktree, &[])?;
    let root = repo
        .workdir()
        .map_or_else(|| worktree.to_owned(), Path::to_owned);
    let mut reader = Reader::new(repo.clone(), root.clone());
    let diverged = target.base_tree != target.head_tree;
    let tree = if diverged {
        repo::tree_changes(&repo, target.base_tree, target.head_tree)?
    } else {
        Vec::new()
    };
    // Each changed path's base and new content.
    let mut contents: BTreeMap<String, (Option<gix::ObjectId>, String)> = tree
        .iter()
        .map(|c| (c.path.clone(), (c.old, review::content_key(c.new))))
        .collect();
    let committed = committed_totals(stats, &reader, target, &tree)?;
    let (mut added, mut removed, mut files) = (
        i64::from(committed.added),
        i64::from(committed.removed),
        i64::from(committed.files),
    );
    let mut changed = None;
    let trees = if diverged {
        Some((
            repo.find_tree(target.base_tree)?,
            repo.find_tree(target.head_tree)?,
        ))
    } else {
        None
    };
    for entry in entries {
        // Replace the path's committed contribution by base→worktree.
        let mut committed_change = Vec::new();
        if let Some((base, head)) = &trees {
            let (old, new) = (
                repo::blob_at(base, &entry.path)?,
                repo::blob_at(head, &entry.path)?,
            );
            if old != new {
                let (a, r) = stats.blob_counts(&reader, old, new)?.unwrap_or((0, 0));
                added -= i64::from(a);
                removed -= i64::from(r);
                files -= 1;
                contents.remove(&entry.path);
                committed_change.push(TreeChange {
                    path: entry.path.clone(),
                    old,
                    new,
                    ..TreeChange::default()
                });
            }
        }
        for repo::Assembled { path, old, new, .. } in repo::assemble(committed_change, vec![entry])
        {
            let (counts, id) = match new {
                Planned::Blob(id) => (stats.blob_counts(&reader, old, Some(id))?, Some(id)),
                Planned::Missing => (stats.blob_counts(&reader, old, None)?, None),
                Planned::Worktree => {
                    let Some(c) = stats.worktree_counts(&mut reader, &root, &path, old)? else {
                        continue;
                    };
                    changed = changed.max(c.modified);
                    (c.counts, c.id)
                }
            };
            let (a, r) = counts.unwrap_or((0, 0));
            added += i64::from(a);
            removed += i64::from(r);
            files += 1;
            contents.insert(path, (old, review::content_key(id)));
        }
    }
    stats.flush();
    // An unreadable review counts as never looked at; opening the worktree
    // reports why it cannot be read.
    let looked = review::State::load(repo.git_dir())
        .ok()
        .and_then(|s| s.looked);
    let clamp = |n: i64| u32::try_from(n.max(0)).unwrap_or(u32::MAX);
    Ok(Summary {
        added: clamp(added),
        removed: clamp(removed),
        files: usize::try_from(files.max(0)).unwrap_or(0),
        content: Some(digest(&contents)?),
        changed,
        looked,
    })
}

/// A worktree's committed totals: base→HEAD changes `tree`, counted once
/// per tree pair.
fn committed_totals(
    stats: &Stats,
    reader: &Reader,
    target: Target,
    tree: &[TreeChange],
) -> Result<Totals> {
    if let Some(t) = stats.totals(target) {
        return Ok(t);
    }
    let mut t = Totals {
        added: 0,
        removed: 0,
        files: 0,
    };
    for change in tree {
        let (a, r) = stats
            .blob_counts(reader, change.old, change.new)?
            .unwrap_or((0, 0));
        t.added += a;
        t.removed += r;
        t.files += 1;
    }
    stats.put_totals(target, t);
    Ok(t)
}

/// Names a set of changes by their content: equal for equal changes,
/// stable across runs.
fn digest(contents: &BTreeMap<String, (Option<gix::ObjectId>, String)>) -> Result<String> {
    let mut text = Vec::new();
    for (path, (old, new)) in contents {
        for part in [path.as_str(), &review::content_key(*old), new] {
            text.extend_from_slice(part.as_bytes());
            text.push(0);
        }
        text.pop();
        text.push(b'\n');
    }
    Ok(gix::objs::compute_hash(gix::hash::Kind::Sha1, gix::object::Kind::Blob, &text)?.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(n: u8) -> gix::ObjectId {
        gix::ObjectId::from_bytes_or_panic(&[n; 20])
    }

    #[test]
    fn records_round_trip() {
        let lines = Record::Lines {
            old: id(1),
            new: id(2),
            counts: Some((3, 4)),
        };
        let binary = Record::Lines {
            old: id(1),
            new: id(3),
            counts: None,
        };
        let totals = Record::Totals {
            target: Target {
                base_tree: id(5),
                head_tree: id(6),
            },
            totals: Totals {
                added: 7,
                removed: 8,
                files: 9,
            },
        };
        for r in [lines, binary, totals] {
            assert_eq!(Record::decode(&r.encode()), Some(r));
        }
    }

    #[test]
    fn loading_keeps_the_newest_records_and_stops_at_a_torn_tail() {
        let record = |n| Record::Lines {
            old: id(n),
            new: id(n),
            counts: Some((u32::from(n), 0)),
        };
        let mut bytes: Vec<u8> = (1..=5).flat_map(|n| record(n).encode()).collect();
        bytes.extend_from_slice(&[TAG_LINES, 9, 9]);
        assert_eq!(load_records(&bytes, 10).len(), 5);
        assert_eq!(load_records(&bytes, 2), vec![record(4), record(5)]);
        let mut foreign = record(1).encode().to_vec();
        foreign[0] = 0xff;
        foreign.extend_from_slice(&record(2).encode());
        assert_eq!(load_records(&foreign, 10), vec![]);
    }

    #[test]
    fn caches_by_blob_pair_and_tree_pair() {
        let stats = Stats::default();
        assert_eq!(stats.lines(id(1), id(2)), None);
        stats.put_lines(id(1), id(2), Some((3, 4)));
        assert_eq!(stats.lines(id(1), id(2)), Some(Some((3, 4))));
        assert_eq!(stats.lines(id(2), id(1)), None, "pairs are ordered");
        let target = Target {
            base_tree: id(1),
            head_tree: id(2),
        };
        let totals = Totals {
            added: 1,
            removed: 2,
            files: 3,
        };
        stats.put_totals(target, totals);
        assert_eq!(stats.totals(target), Some(totals));
    }

    #[test]
    fn digests_name_changes_by_content_and_stay_stable() {
        let mut contents =
            BTreeMap::from([("a.rs".to_owned(), (Some(id(1)), "missing".to_owned()))]);
        let first = digest(&contents).expect("digest");
        // Stored in review state, so the name never changes for the same
        // changes: `printf 'a.rs\0<id>\0missing\n' | git hash-object --stdin`.
        assert_eq!(first, "586fe90fce9dee690b9d6589be163800dc0b638b");
        assert_eq!(digest(&contents.clone()).expect("digest"), first);
        contents.insert("b.rs".into(), (None, review::content_key(Some(id(2)))));
        assert_ne!(digest(&contents).expect("digest"), first);
    }

    #[test]
    fn a_summary_names_its_content_and_reads_the_last_look() {
        let dir = std::env::temp_dir().join(format!("dots-diff-summary-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("dir");
        let git = |args: &[&str]| {
            let out = std::process::Command::new("git")
                .args([
                    "-c",
                    "user.name=t",
                    "-c",
                    "user.email=t@t",
                    "-c",
                    "commit.gpgsign=false",
                ])
                .args(args)
                .current_dir(&dir)
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .output()
                .expect("git");
            assert!(
                out.status.success(),
                "git {args:?}: {}",
                String::from_utf8_lossy(&out.stderr)
            );
        };
        git(&["init", "-q", "-b", "main"]);
        std::fs::write(dir.join("a.txt"), "one\n").expect("write");
        git(&["add", "."]);
        git(&["commit", "-qm", "base"]);
        git(&["switch", "-qc", "feature"]);
        std::fs::write(dir.join("a.txt"), "one\ntwo\n").expect("write");
        let stats = Stats::default();
        let first = summarize(&stats, &dir).expect("summary");
        assert_eq!((first.files, first.added), (1, 1));
        assert!(first.changed.is_some(), "an uncommitted file's time");
        assert_eq!(first.looked, None);
        // Committing changes nothing the user sees.
        git(&["commit", "-qam", "two"]);
        let committed = summarize(&stats, &dir).expect("summary");
        assert_eq!(committed.content, first.content);
        std::fs::write(dir.join("a.txt"), "one\nthree\n").expect("write");
        let edited = summarize(&stats, &dir).expect("summary");
        assert_ne!(edited.content, first.content);
        let git_dir = dir.join(".git");
        let mut review = review::State::default();
        review.looked.clone_from(&edited.content);
        review.save(&git_dir).expect("save");
        assert_eq!(
            summarize(&stats, &dir).expect("summary").looked,
            edited.content
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
