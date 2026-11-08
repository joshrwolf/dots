//! Line diffs of one file through gix's imara-diff, line counts for
//! summaries, and within-line emphasis computed per hunk when it is first
//! drawn.

use std::ops::Range;
use std::sync::Arc;

use gix::diff::blob::{Algorithm, Diff, InternedInput};

/// A file's text split into lines. Line `i` spans `starts[i]..starts[i + 1]`
/// and includes its line terminator.
#[derive(Debug)]
pub(crate) struct Text {
    pub(crate) src: Arc<str>,
    starts: Vec<u32>,
}

impl Text {
    pub(crate) fn new(src: Arc<str>) -> Self {
        let mut starts = Vec::with_capacity(src.len() / 32 + 2);
        starts.push(0);
        starts.extend(
            src.bytes()
                .enumerate()
                .filter(|&(_, b)| b == b'\n')
                .map(|(i, _)| offset(i + 1)),
        );
        if starts.last().copied() != Some(offset(src.len())) {
            starts.push(offset(src.len()));
        }
        starts.shrink_to_fit();
        Self { src, starts }
    }

    pub(crate) fn from_bytes(bytes: &[u8]) -> Self {
        Self::new(Arc::from(String::from_utf8_lossy(bytes).as_ref()))
    }

    pub(crate) fn len(&self) -> u32 {
        offset(self.starts.len() - 1)
    }

    /// Memory held, for cache budgets.
    pub(crate) fn bytes(&self) -> usize {
        self.src.len() + self.starts.len() * 4
    }

    /// Line `i` with its terminator.
    fn raw(&self, i: u32) -> &str {
        let i = i as usize;
        &self.src[self.starts[i] as usize..self.starts[i + 1] as usize]
    }

    /// Line `i` without its terminator.
    pub(crate) fn line(&self, i: u32) -> &str {
        let raw = self.raw(i);
        let raw = raw.strip_suffix('\n').unwrap_or(raw);
        raw.strip_suffix('\r').unwrap_or(raw)
    }

    /// Byte offset of line `i` within `src`; `i == len()` gives the end.
    pub(crate) fn start(&self, i: u32) -> u32 {
        self.starts[i as usize]
    }
}

/// Byte offsets inside a file are `u32`; the loader refuses larger files.
fn offset(n: usize) -> u32 {
    u32::try_from(n).expect("file larger than 4 GiB passed the size guard")
}

/// One changed region: `old` lines were replaced by `new` lines.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Hunk {
    pub(crate) old: Range<u32>,
    pub(crate) new: Range<u32>,
}

/// What stays resident per file: hunks and sizes, not texts. Texts are
/// reloaded on demand by their blob ids.
#[derive(Debug)]
pub(crate) struct FileDiff {
    pub(crate) hunks: Vec<Hunk>,
    pub(crate) added: u32,
    pub(crate) removed: u32,
    pub(crate) old_len: u32,
    pub(crate) new_len: u32,
    /// The new side's content id: its blob, or the hash of the working-tree
    /// file in Git's form.
    pub(crate) new_id: Option<gix::ObjectId>,
}

impl FileDiff {
    pub(crate) fn compute(old: &Text, new: &Text, new_id: Option<gix::ObjectId>) -> Self {
        let (old_len, new_len) = (old.len(), new.len());
        let hunks = if old_len == 0 || new_len == 0 {
            // One-sided: no interning needed.
            if old_len == new_len {
                Vec::new()
            } else {
                vec![Hunk {
                    old: 0..old_len,
                    new: 0..new_len,
                }]
            }
        } else {
            let mut input = InternedInput::default();
            input.update_before((0..old_len).map(|i| old.raw(i)));
            input.update_after((0..new_len).map(|i| new.raw(i)));
            let mut diff = Diff::compute(Algorithm::Histogram, &input);
            diff.postprocess_lines(&input);
            diff.hunks()
                .map(|h| Hunk {
                    old: h.before,
                    new: h.after,
                })
                .collect()
        };
        let (added, removed) = hunks.iter().fold((0, 0), |(a, r), h| {
            (a + h.new.end - h.new.start, r + h.old.end - h.old.start)
        });
        Self {
            hunks,
            added,
            removed,
            old_len,
            new_len,
            new_id,
        }
    }

    /// The hunk containing `line` of one side, if any.
    pub(crate) fn hunk_of(&self, new_side: bool, line: u32) -> Option<usize> {
        let side = |h: &Hunk| {
            if new_side {
                h.new.clone()
            } else {
                h.old.clone()
            }
        };
        let i = self.hunks.partition_point(|h| side(h).end <= line);
        self.hunks
            .get(i)
            .filter(|h| side(h).contains(&line))
            .map(|_| i)
    }
}

/// True when the first 8000 bytes hold a NUL, as git decides.
pub(crate) fn is_binary(bytes: &[u8]) -> bool {
    bytes[..bytes.len().min(8000)].contains(&0)
}

/// Lines added and removed between two contents, or `None` for binary
/// files. Only counts: no slider postprocessing (it moves hunks without
/// changing counts) and no interning for one-sided changes.
pub(crate) fn line_counts(old: &[u8], new: &[u8]) -> Option<(u32, u32)> {
    if is_binary(old) || is_binary(new) {
        return None;
    }
    let lines = |b: &[u8]| {
        u32::try_from(gix::diff::blob::sources::byte_lines(b).count()).unwrap_or(u32::MAX)
    };
    if old.is_empty() || new.is_empty() {
        return Some((lines(new), lines(old)));
    }
    let input = InternedInput::new(
        gix::diff::blob::sources::byte_lines(old),
        gix::diff::blob::sources::byte_lines(new),
    );
    let diff = Diff::compute(Algorithm::Histogram, &input);
    Some((diff.count_additions(), diff.count_removals()))
}

/// Within-line emphasis of one hunk: changed byte ranges of each side, as
/// offsets into the file, sorted.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct HunkEmphasis {
    pub(crate) old: Vec<Range<u32>>,
    pub(crate) new: Vec<Range<u32>>,
}

impl HunkEmphasis {
    /// The emphasised ranges of one line spanning `line` bytes of the file,
    /// relative to the line's start.
    pub(crate) fn within(&self, new_side: bool, line: Range<u32>) -> Vec<Range<u32>> {
        let ranges = if new_side { &self.new } else { &self.old };
        let first = ranges.partition_point(|r| r.end <= line.start);
        ranges[first..]
            .iter()
            .take_while(|r| r.start < line.end)
            .map(|r| r.start.max(line.start) - line.start..r.end.min(line.end) - line.start)
            .filter(|r| !r.is_empty())
            .collect()
    }
}

/// Lines longer than this get no within-line emphasis.
const INLINE_MAX_BYTES: usize = 1000;
/// Hunks with more line pairs than this get none either; the emphasis of a
/// hunk is computed at once when it first shows.
const INLINE_MAX_PAIRS: usize = 1000;
/// Pairs sharing less than this share of their text are rewrites, shown as
/// plain removals and additions.
const INLINE_MIN_SAME: f32 = 0.4;
/// Unchanged text between two changes up to this many bytes, or only
/// whitespace, is emphasised with them, so emphasis does not flicker.
const INLINE_MERGE_GAP: u32 = 2;

/// Within-line emphasis for `hunk`: for each pair of an old and a new line,
/// the changed parts of each, snapped to whole words. `None` when no pair
/// has any.
pub(crate) fn hunk_emphasis(old: &Text, new: &Text, hunk: &Hunk) -> Option<HunkEmphasis> {
    let pairs = hunk.old.clone().zip(hunk.new.clone());
    if pairs.len() > INLINE_MAX_PAIRS {
        return None;
    }
    let mut emphasis = HunkEmphasis::default();
    for (o, n) in pairs {
        let Some((a, b)) = line_emphasis(old.line(o), new.line(n)) else {
            continue;
        };
        let shift = |ranges: Vec<Range<u32>>, start: u32| {
            ranges
                .into_iter()
                .map(move |r| r.start + start..r.end + start)
        };
        emphasis.old.extend(shift(a, old.start(o)));
        emphasis.new.extend(shift(b, new.start(n)));
    }
    (!emphasis.old.is_empty() || !emphasis.new.is_empty()).then_some(emphasis)
}

/// Byte ranges within one line.
type Ranges = Vec<Range<u32>>;

/// The changed byte ranges of two versions of a line, or `None` when they
/// are too long, equal, or mostly rewritten.
fn line_emphasis(a: &str, b: &str) -> Option<(Ranges, Ranges)> {
    if a == b || a.len() > INLINE_MAX_BYTES || b.len() > INLINE_MAX_BYTES {
        return None;
    }
    let (ac, bc) = (chars(a), chars(b));
    let mut input = InternedInput::default();
    input.update_before(ac.iter().copied());
    input.update_after(bc.iter().copied());
    let diff = Diff::compute(Algorithm::Myers, &input);
    let bytes = |cs: &[&str], s: &str, r: Range<u32>| {
        let at = |i: u32| {
            cs.get(i as usize).map_or(offset(s.len()), |c| {
                offset(c.as_ptr() as usize - s.as_ptr() as usize)
            })
        };
        at(r.start)..at(r.end)
    };
    let (mut ra, mut rb) = (Vec::new(), Vec::new());
    let hunks: Vec<_> = diff.hunks().collect();
    for (i, h) in hunks.iter().enumerate() {
        let (a_range, b_range) = (
            bytes(&ac, a, h.before.clone()),
            bytes(&bc, b, h.after.clone()),
        );
        // Text between changes is the same on both sides, so a change is
        // widened by the same amount on each, to whole words on either.
        let room_left = i.checked_sub(1).map_or(a_range.start, |p| {
            a_range.start - bytes(&ac, a, hunks[p].before.clone()).end
        });
        let room_right = hunks.get(i + 1).map_or(offset(a.len()) - a_range.end, |n| {
            bytes(&ac, a, n.before.clone()).start - a_range.end
        });
        let left = word_left(a, a_range.start)
            .max(word_left(b, b_range.start))
            .min(room_left);
        let right = word_right(a, a_range.end)
            .max(word_right(b, b_range.end))
            .min(room_right);
        ra.push(a_range.start - left..a_range.end + right);
        rb.push(b_range.start - left..b_range.end + right);
    }
    let same = |s: &str, ranges: &[Range<u32>]| {
        let changed: usize = ranges
            .iter()
            .map(|r| s[r.start as usize..r.end as usize].trim().len())
            .sum();
        s.trim().len().saturating_sub(changed)
    };
    let longest = a.trim().len().max(b.trim().len()).max(1);
    #[allow(clippy::cast_precision_loss)]
    let share = same(a, &ra).min(same(b, &rb)) as f32 / longest as f32;
    if share < INLINE_MIN_SAME {
        return None;
    }
    Some((settle(a, ra), settle(b, rb)))
}

/// Each character of `s` as its own slice.
fn chars(s: &str) -> Vec<&str> {
    s.char_indices()
        .map(|(i, c)| &s[i..i + c.len_utf8()])
        .collect()
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// For a change starting at `at` inside a word, the bytes back to the
/// word's start.
fn word_left(s: &str, at: u32) -> u32 {
    let (head, tail) = s.split_at(at as usize);
    if tail.starts_with(is_word) {
        offset(head.len() - head.trim_end_matches(is_word).len())
    } else {
        0
    }
}

/// For a change ending at `at` inside a word, the bytes on to the word's
/// end.
fn word_right(s: &str, at: u32) -> u32 {
    let (head, tail) = s.split_at(at as usize);
    if head.ends_with(is_word) {
        offset(tail.len() - tail.trim_start_matches(is_word).len())
    } else {
        0
    }
}

/// Merges changed ranges of `s` across small unchanged gaps and drops
/// empty ones.
fn settle(s: &str, ranges: Vec<Range<u32>>) -> Vec<Range<u32>> {
    let mut out: Vec<Range<u32>> = Vec::new();
    for range in ranges.into_iter().filter(|r| !r.is_empty()) {
        match out.last_mut() {
            Some(last)
                if range.start <= last.end
                    || range.start - last.end <= INLINE_MERGE_GAP
                    || s[last.end as usize..range.start as usize].trim().is_empty() =>
            {
                last.end = last.end.max(range.end);
            }
            _ => out.push(range),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(s: &str) -> Text {
        Text::new(Arc::from(s))
    }

    #[test]
    fn splits_lines_with_and_without_trailing_newline() {
        let t = text("a\nb\r\nc");
        assert_eq!(t.len(), 3);
        assert_eq!([t.line(0), t.line(1), t.line(2)], ["a", "b", "c"]);
        assert_eq!(text("a\n").len(), 1);
        assert_eq!(text("").len(), 0);
    }

    #[test]
    fn computes_hunks_and_counts() {
        let d = FileDiff::compute(&text("a\nb\nc\nd\n"), &text("a\nB\nc\nd\ne\n"), None);
        assert_eq!(
            d.hunks,
            vec![
                Hunk {
                    old: 1..2,
                    new: 1..2
                },
                Hunk {
                    old: 4..4,
                    new: 4..5
                }
            ]
        );
        assert_eq!((d.added, d.removed), (2, 1));
        assert_eq!(d.hunk_of(true, 4), Some(1));
        assert_eq!(d.hunk_of(true, 2), None);
        assert_eq!(d.hunk_of(false, 1), Some(0));
    }

    #[test]
    fn one_sided_files_are_one_hunk() {
        let d = FileDiff::compute(&text(""), &text("a\nb\n"), None);
        assert_eq!(
            d.hunks,
            vec![Hunk {
                old: 0..0,
                new: 0..2
            }]
        );
        assert_eq!((d.added, d.removed), (2, 0));
    }

    #[test]
    fn counts_lines_without_a_full_diff() {
        assert_eq!(line_counts(b"a\nb\nc\n", b"a\nB\nc\nd"), Some((2, 1)));
        assert_eq!(line_counts(b"", b"x\ny"), Some((2, 0)));
        assert_eq!(line_counts(b"x\n", b""), Some((0, 1)));
        assert_eq!(line_counts(b"a\0b", b"a"), None);
    }

    fn emph<'a>(a: &'a str, b: &'a str) -> Option<(Vec<&'a str>, Vec<&'a str>)> {
        let (ra, rb) = line_emphasis(a, b)?;
        let cut = |s: &'a str, r: &[Range<u32>]| -> Vec<&'a str> {
            r.iter()
                .map(|r| &s[r.start as usize..r.end as usize])
                .collect()
        };
        Some((cut(a, &ra), cut(b, &rb)))
    }

    #[test]
    fn changes_snap_to_whole_words() {
        // A character diff finds "oo" -> "a"; the emphasis takes the words.
        assert_eq!(
            emph("let x = foo(1);", "let x = fa(1);"),
            Some((vec!["foo"], vec!["fa"]))
        );
        // Both sides widen together: an insertion inside a word marks the
        // old word too.
        assert_eq!(
            emph("pub host: String,", "pub bind_host: String,"),
            Some((vec!["host"], vec!["bind_host"]))
        );
        assert_eq!(
            emph("let a = count;", "let a = count_all;"),
            Some((vec!["count"], vec!["count_all"]))
        );
        // Punctuation changes stay narrow.
        assert_eq!(
            emph("call(a, b);", "call(a, b)?;"),
            Some((vec![], vec!["?"]))
        );
    }

    #[test]
    fn small_unchanged_gaps_merge() {
        // Two changed words around one space read as one change.
        assert_eq!(
            emph("let value = old thing here;", "let value = new stuff here;"),
            Some((vec!["old thing"], vec!["new stuff"]))
        );
        // A real unchanged word between changes keeps them apart.
        assert_eq!(
            emph("alpha middle omega", "ALPHA middle OMEGA"),
            Some((vec!["alpha", "omega"], vec!["ALPHA", "OMEGA"]))
        );
    }

    #[test]
    fn rewrites_and_long_lines_get_no_emphasis() {
        assert_eq!(
            emph("completely different", "nothing alike here at all"),
            None
        );
        assert_eq!(emph("same", "same"), None);
        let long = "x".repeat(INLINE_MAX_BYTES + 1);
        assert_eq!(emph(&long, "x"), None);
    }

    #[test]
    fn hunk_emphasis_is_in_file_offsets() {
        let (old, new) = (
            text("keep\nlet x = foo(1);\n"),
            text("keep\nlet x = bar(1);\n"),
        );
        let hunk = Hunk {
            old: 1..2,
            new: 1..2,
        };
        let e = hunk_emphasis(&old, &new, &hunk).expect("similar lines");
        assert_eq!(e.old, vec![13..16]);
        assert_eq!(e.within(true, 5..21), vec![8..11]);
        let pure = Hunk {
            old: 0..0,
            new: 0..1,
        };
        assert_eq!(hunk_emphasis(&text(""), &text("x\n"), &pure), None);
    }
}
