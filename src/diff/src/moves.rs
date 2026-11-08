//! Moved code: removed blocks that reappear as added blocks, in the same
//! file or another, possibly with small edits along the way.
//!
//! Exact runs of trimmed lines seed a block; it then grows over neighbouring
//! lines that are alike, so edits inside moved code keep it one block and
//! get within-line emphasis against their originals.

use std::collections::HashMap;
use std::ops::Range;
use std::sync::Arc;

use crate::diff::{self, FileDiff, Hunk, Text};
use crate::repo::{FileChange, Reader};

/// Fewest lines with real content a block needs to count as moved.
const MIN_LINES: usize = 3;
/// Fewest such lines that must match exactly to start a block.
const MIN_SEED: usize = 2;
/// Most changed lines searched; larger changes are not searched at all.
pub(crate) const MAX_LINES: u64 = 100_000;
/// Removed copies of one line tried as a block's start.
const MAX_CANDIDATES: usize = 32;
/// How alike an edited line must be to its original to stay in a block.
const MIN_SIMILARITY: f32 = 0.6;

/// Which contents of a file were searched: base blob and new content id.
pub(crate) type Key = (Option<gix::ObjectId>, Option<gix::ObjectId>);

/// Consecutive changed lines of one side of a hunk.
#[derive(Debug)]
pub(crate) struct Run {
    pub(crate) hunk: usize,
    /// The first line's 0-based number.
    pub(crate) start: u32,
    pub(crate) lines: Vec<String>,
}

/// One file's changed lines.
#[derive(Debug)]
pub(crate) struct Changed {
    pub(crate) path: String,
    pub(crate) key: Key,
    pub(crate) removed: Vec<Run>,
    pub(crate) added: Vec<Run>,
}

/// Where a block's lines are: file, run and position in the run.
type At = (usize, usize, usize);

/// Removed lines that reappear as added ones, line for line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Block {
    old: At,
    new: At,
    len: usize,
}

/// A line of a moved block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Moved {
    /// On a block's first line: the path and 0-based line where the other
    /// end of the move starts.
    pub(crate) other: Option<(Arc<str>, u32)>,
    /// Byte ranges that differ from the line at the other end.
    pub(crate) emphasis: Vec<Range<u32>>,
}

#[derive(Debug, Default)]
struct FileMoves {
    key: Key,
    old: HashMap<u32, Moved>,
    new: HashMap<u32, Moved>,
}

/// Moved lines of every file, by path.
#[derive(Debug, Default)]
pub(crate) struct Moves {
    files: HashMap<String, FileMoves>,
    pub(crate) blocks: usize,
    /// The change was too large to search.
    pub(crate) skipped: bool,
}

impl Moves {
    pub(crate) fn skipped() -> Self {
        Self {
            skipped: true,
            ..Self::default()
        }
    }

    /// Finds moved blocks among `files` and indexes their lines.
    pub(crate) fn find(files: &[Changed]) -> Self {
        let blocks = detect(files);
        let mut moves = Self {
            blocks: blocks.len(),
            ..Self::default()
        };
        let paths: Vec<Arc<str>> = files.iter().map(|f| Arc::from(f.path.as_str())).collect();
        for b in &blocks {
            let old_run = &files[b.old.0].removed[b.old.1];
            let new_run = &files[b.new.0].added[b.new.1];
            let line = |run: &Run, pos: usize| run.start + u32::try_from(pos).unwrap_or(u32::MAX);
            for i in 0..b.len {
                let (a, c) = (&old_run.lines[b.old.2 + i], &new_run.lines[b.new.2 + i]);
                let (ea, ec) = diff::line_emphasis(a, c).unwrap_or_default();
                let first = |path: usize, run: &Run, pos: usize| {
                    (i == 0).then(|| (Arc::clone(&paths[path]), line(run, pos)))
                };
                moves.entry(files, b.old.0).old.insert(
                    line(old_run, b.old.2 + i),
                    Moved {
                        other: first(b.new.0, new_run, b.new.2),
                        emphasis: ea,
                    },
                );
                moves.entry(files, b.new.0).new.insert(
                    line(new_run, b.new.2 + i),
                    Moved {
                        other: first(b.old.0, old_run, b.old.2),
                        emphasis: ec,
                    },
                );
            }
        }
        moves
    }

    fn entry(&mut self, files: &[Changed], file: usize) -> &mut FileMoves {
        let f = &files[file];
        self.files
            .entry(f.path.clone())
            .or_insert_with(|| FileMoves {
                key: f.key,
                ..FileMoves::default()
            })
    }

    /// Line `n` of one side of `path`, if it moved and the file still has
    /// the contents that were searched.
    pub(crate) fn line(&self, path: &str, key: Key, new_side: bool, n: u32) -> Option<&Moved> {
        let f = self.files.get(path).filter(|f| f.key == key)?;
        if new_side { &f.new } else { &f.old }.get(&n)
    }
}

/// Reads the changed lines of `files` and finds the moves among them,
/// unless there are too many lines to search.
pub(crate) fn search(reader: &mut Reader, files: &[(FileChange, Arc<FileDiff>)]) -> Moves {
    let total: u64 = files
        .iter()
        .map(|(_, d)| u64::from(d.added) + u64::from(d.removed))
        .sum();
    if total > MAX_LINES {
        return Moves::skipped();
    }
    let mut changed = Vec::with_capacity(files.len());
    for (change, diff) in files {
        // A file that changed since it was diffed waits for the next search.
        let Ok(contents) = reader.contents(change) else {
            continue;
        };
        if contents.new_id != diff.new_id {
            continue;
        }
        let (old, new) = (
            Text::from_bytes(&contents.old),
            Text::from_bytes(&contents.new),
        );
        let runs = |text: &Text, side: fn(&Hunk) -> &Range<u32>| -> Vec<Run> {
            diff.hunks
                .iter()
                .enumerate()
                .filter(|(_, h)| !side(h).is_empty())
                .map(|(i, h)| Run {
                    hunk: i,
                    start: side(h).start,
                    lines: side(h).clone().map(|n| text.line(n).to_owned()).collect(),
                })
                .collect()
        };
        changed.push(Changed {
            path: change.path.clone(),
            key: (change.old, diff.new_id),
            removed: runs(&old, |h| &h.old),
            added: runs(&new, |h| &h.new),
        });
    }
    Moves::find(&changed)
}

/// Whether a line carries enough content to anchor a move.
fn solid(line: &str) -> bool {
    line.chars().filter(|c| c.is_alphanumeric()).count() >= 3
}

fn alike(a: &str, b: &str) -> bool {
    a.trim() == b.trim() || diff::line_similarity(a, b) >= MIN_SIMILARITY
}

/// Finds moved blocks: each added line belongs to at most one, as does
/// each removed line. A block never pairs a hunk's removed lines with its
/// own added lines; that is an edit, not a move.
fn detect(files: &[Changed]) -> Vec<Block> {
    let mut index: HashMap<&str, Vec<At>> = HashMap::new();
    for (fi, f) in files.iter().enumerate() {
        for (ri, run) in f.removed.iter().enumerate() {
            for (pi, line) in run.lines.iter().enumerate() {
                if solid(line) {
                    let at = index.entry(line.trim()).or_default();
                    if at.len() < MAX_CANDIDATES {
                        at.push((fi, ri, pi));
                    }
                }
            }
        }
    }
    let flags = |runs: &[Run]| -> Vec<Vec<bool>> {
        runs.iter().map(|r| vec![false; r.lines.len()]).collect()
    };
    let mut used_old: Vec<Vec<Vec<bool>>> = files.iter().map(|f| flags(&f.removed)).collect();
    let mut used_new: Vec<Vec<Vec<bool>>> = files.iter().map(|f| flags(&f.added)).collect();
    let mut blocks = Vec::new();
    for (fi, f) in files.iter().enumerate() {
        for (ri, run) in f.added.iter().enumerate() {
            let new = &run.lines;
            let mut j = 0;
            while j < new.len() {
                let line = &new[j];
                let seed = if used_new[fi][ri][j] || !solid(line) {
                    None
                } else {
                    // The longest exact run among the removed copies.
                    let mut best: Option<(At, usize, usize)> = None;
                    for &(gf, gr, gp) in index.get(line.trim()).map_or(&[][..], Vec::as_slice) {
                        let source = &files[gf].removed[gr];
                        if gf == fi && source.hunk == run.hunk {
                            continue;
                        }
                        let old = &source.lines;
                        let (mut len, mut solid_lines) = (0, 0);
                        while j + len < new.len()
                            && gp + len < old.len()
                            && !used_new[fi][ri][j + len]
                            && !used_old[gf][gr][gp + len]
                            && old[gp + len].trim() == new[j + len].trim()
                        {
                            solid_lines += usize::from(solid(&new[j + len]));
                            len += 1;
                        }
                        if best.is_none_or(|(_, _, s)| solid_lines > s) {
                            best = Some(((gf, gr, gp), len, solid_lines));
                        }
                    }
                    best.filter(|&(_, _, s)| s >= MIN_SEED)
                };
                let Some(((gf, gr, gp), len, _)) = seed else {
                    j += 1;
                    continue;
                };
                let old = &files[gf].removed[gr].lines;
                let free = |used_old: &[Vec<Vec<bool>>],
                            used_new: &[Vec<Vec<bool>>],
                            o: usize,
                            n: usize| {
                    !used_old[gf][gr][o] && !used_new[fi][ri][n] && alike(&old[o], &new[n])
                };
                let (mut o, mut n) = (gp, j);
                while o > 0 && n > 0 && free(&used_old, &used_new, o - 1, n - 1) {
                    o -= 1;
                    n -= 1;
                }
                let mut end = len + (j - n);
                while o + end < old.len()
                    && n + end < new.len()
                    && free(&used_old, &used_new, o + end, n + end)
                {
                    end += 1;
                }
                if new[n..n + end].iter().filter(|l| solid(l)).count() < MIN_LINES {
                    j += 1;
                    continue;
                }
                for k in 0..end {
                    used_old[gf][gr][o + k] = true;
                    used_new[fi][ri][n + k] = true;
                }
                blocks.push(Block {
                    old: (gf, gr, o),
                    new: (fi, ri, n),
                    len: end,
                });
                j = n + end;
            }
        }
    }
    blocks
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(hunk: usize, start: u32, lines: &[&str]) -> Run {
        Run {
            hunk,
            start,
            lines: lines.iter().map(|&l| l.to_owned()).collect(),
        }
    }

    fn file(path: &str, removed: Vec<Run>, added: Vec<Run>) -> Changed {
        Changed {
            path: path.to_owned(),
            key: (None, None),
            removed,
            added,
        }
    }

    const BODY: [&str; 5] = [
        "fn retry_delay(attempt: u32) -> u64 {",
        "    let base = 100;",
        "    let capped = attempt.min(6);",
        "    base * 2u64.pow(capped)",
        "}",
    ];

    #[test]
    fn finds_a_block_moved_to_another_file() {
        let files = [
            file("a.rs", vec![run(0, 20, &BODY)], vec![]),
            file("b.rs", vec![], vec![run(0, 3, &BODY)]),
        ];
        let moves = Moves::find(&files);
        assert_eq!(moves.blocks, 1);
        let first = moves.line("a.rs", (None, None), false, 20).expect("moved");
        assert_eq!(first.other, Some((Arc::from("b.rs"), 3)));
        assert_eq!(
            moves
                .line("b.rs", (None, None), true, 3)
                .expect("moved")
                .other,
            Some((Arc::from("a.rs"), 20))
        );
        for n in 3..8 {
            assert!(
                moves.line("b.rs", (None, None), true, n).is_some(),
                "line {n}"
            );
        }
        assert!(moves.line("b.rs", (None, None), true, 8).is_none());
        // Moves apply only to the contents they were found in.
        let other = Some(gix::ObjectId::null(gix::hash::Kind::Sha1));
        assert!(moves.line("b.rs", (None, other), true, 3).is_none());
    }

    #[test]
    fn edited_lines_stay_in_the_block_with_emphasis() {
        let mut edited = BODY;
        edited[1] = "    let base = 250;";
        let files = [file(
            "a.rs",
            vec![run(0, 0, &BODY)],
            vec![run(1, 40, &edited)],
        )];
        let moves = Moves::find(&files);
        assert_eq!(moves.blocks, 1);
        let line = moves.line("a.rs", (None, None), true, 41).expect("moved");
        assert_eq!(line.other, None);
        assert_eq!(line.emphasis, vec![15..18]);
        let unchanged = moves.line("a.rs", (None, None), true, 42).expect("moved");
        assert_eq!(unchanged.emphasis, Vec::<Range<u32>>::new());
    }

    #[test]
    fn short_or_trivial_blocks_are_not_moves() {
        let trivial = ["}", "", "    }", "{"];
        let short = ["let alpha = 1;", "let beta = 2;"];
        let files = [file(
            "a.rs",
            vec![run(0, 0, &trivial), run(1, 10, &short)],
            vec![run(2, 30, &trivial), run(3, 40, &short)],
        )];
        assert_eq!(Moves::find(&files).blocks, 0);
    }

    #[test]
    fn a_hunk_rewriting_its_own_lines_is_not_a_move() {
        let files = [file("a.rs", vec![run(0, 5, &BODY)], vec![run(0, 5, &BODY)])];
        assert_eq!(Moves::find(&files).blocks, 0);
    }

    #[test]
    fn each_line_moves_once() {
        // One removal, two copies added: only one copy is the move.
        let files = [
            file("a.rs", vec![run(0, 0, &BODY)], vec![]),
            file("b.rs", vec![], vec![run(0, 0, &BODY), run(1, 10, &BODY)]),
        ];
        let moves = Moves::find(&files);
        assert_eq!(moves.blocks, 1);
        assert!(moves.line("b.rs", (None, None), true, 0).is_some());
        assert!(moves.line("b.rs", (None, None), true, 10).is_none());
    }
}
