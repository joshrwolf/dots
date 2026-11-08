//! Line diffs of one file through gix's imara-diff, line counts for
//! summaries, and word emphasis computed per hunk when it is first drawn.

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

    /// Lines `range` as one slice, terminators included.
    fn span(&self, range: &Range<u32>) -> &str {
        &self.src[self.start(range.start) as usize..self.start(range.end) as usize]
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

/// Word-level emphasis of one hunk: changed byte ranges of each side, as
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

/// Hunks larger than this many lines per side get no word emphasis: big
/// rewrites read better as plain removals and additions.
const EMPHASIS_MAX_LINES: u32 = 8;
const EMPHASIS_MAX_BYTES: usize = 4096;
const EMPHASIS_MAX_WORDS: usize = 1000;
/// Above this many emphasised runs on one line the emphasis is noise.
const EMPHASIS_MAX_ALTERNATION: usize = 3;

/// Word emphasis for `hunk`, or `None` when it would not help: one-sided,
/// large or unequal hunks, mostly rewritten text, or emphasis alternating
/// too often within a line.
pub(crate) fn hunk_emphasis(old: &Text, new: &Text, hunk: &Hunk) -> Option<HunkEmphasis> {
    let (o, n) = (hunk.old.len(), hunk.new.len());
    let lines_ok = |x: usize| (1..=EMPHASIS_MAX_LINES as usize).contains(&x);
    if !lines_ok(o) || !lines_ok(n) || o.max(n) > 2 * o.min(n) {
        return None;
    }
    let (a, b) = (old.span(&hunk.old), new.span(&hunk.new));
    if a.len() > EMPHASIS_MAX_BYTES || b.len() > EMPHASIS_MAX_BYTES {
        return None;
    }
    let (a_tokens, b_tokens) = (tokens(a), tokens(b));
    if a_tokens.len() + b_tokens.len() > EMPHASIS_MAX_WORDS {
        return None;
    }
    let mut input = InternedInput::default();
    input.update_before(a_tokens.iter().copied());
    input.update_after(b_tokens.iter().copied());
    let mut diff = Diff::compute(Algorithm::Myers, &input);
    diff.postprocess_no_heuristic(&input);
    let collect = |tokens: &[&str], base: &str, file_start: u32, changed: &dyn Fn(u32) -> bool| {
        let mut ranges = Vec::new();
        let mut same = 0usize;
        for (i, t) in (0u32..).zip(tokens) {
            if changed(i) {
                if t.trim().is_empty() {
                    continue;
                }
                let at = offset(t.as_ptr() as usize - base.as_ptr() as usize) + file_start;
                push_range(&mut ranges, at..at + offset(t.len()));
            } else {
                same += t.trim().len();
            }
        }
        (ranges, same)
    };
    let (old_ranges, same) = collect(&a_tokens, a, old.start(hunk.old.start), &|i| {
        diff.is_removed(i)
    });
    let (new_ranges, _) = collect(&b_tokens, b, new.start(hunk.new.start), &|i| {
        diff.is_added(i)
    });
    let longest = a.trim().len().max(b.trim().len()).max(1);
    // Mostly rewritten text reads better as plain removals and additions.
    if same * 10 < longest * 4 {
        return None;
    }
    let emphasis = HunkEmphasis {
        old: old_ranges,
        new: new_ranges,
    };
    let noisy = |text: &Text, lines: &Range<u32>, new_side: bool| {
        lines.clone().any(|l| {
            emphasis
                .within(new_side, text.start(l)..text.start(l + 1))
                .len()
                > EMPHASIS_MAX_ALTERNATION
        })
    };
    if noisy(old, &hunk.old, false) || noisy(new, &hunk.new, true) {
        return None;
    }
    Some(emphasis)
}

/// Splits text into identifier runs, whitespace runs (newlines apart) and
/// single symbols.
fn tokens(text: &str) -> Vec<&str> {
    let class = |c: char| {
        if c.is_alphanumeric() || c == '_' {
            0
        } else if c == '\n' {
            3
        } else if c.is_whitespace() {
            1
        } else {
            2
        }
    };
    let mut out = Vec::new();
    let mut start = 0;
    let mut prev = None;
    for (i, c) in text.char_indices() {
        let k = class(c);
        if i > start && (prev != Some(k) || k >= 2) {
            out.push(&text[start..i]);
            start = i;
        }
        prev = Some(k);
    }
    if start < text.len() {
        out.push(&text[start..]);
    }
    out
}

fn push_range(ranges: &mut Vec<Range<u32>>, range: Range<u32>) {
    match ranges.last_mut() {
        Some(last) if last.end == range.start => last.end = range.end,
        _ => ranges.push(range),
    }
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

    #[test]
    fn emphasises_changed_words_in_small_similar_hunks() {
        let (old, new) = (text("let x = foo(1);\n"), text("let x = bar(1);\n"));
        let hunk = Hunk {
            old: 0..1,
            new: 0..1,
        };
        let e = hunk_emphasis(&old, &new, &hunk).expect("similar lines");
        assert_eq!(e.old, vec![8..11]);
        assert_eq!(e.new, vec![8..11]);
        assert_eq!(e.within(true, 0..16), vec![8..11]);
    }

    #[test]
    fn skips_emphasis_for_rewrites_noise_and_large_hunks() {
        let rewrite = (
            text("completely different\n"),
            text("nothing alike here at all\n"),
        );
        let one = Hunk {
            old: 0..1,
            new: 0..1,
        };
        assert_eq!(hunk_emphasis(&rewrite.0, &rewrite.1, &one), None);
        // Many small edits along one line alternate too often.
        let noisy = (
            text("alpha beta gamma delta epsilon zeta eta theta\n"),
            text("alpha Beta gamma Delta epsilon Zeta eta Theta\n"),
        );
        assert_eq!(hunk_emphasis(&noisy.0, &noisy.1, &one), None);
        let big = (0..20).fold(String::new(), |s, i| s + "line " + &i.to_string() + "\n");
        let changed = big.replace("line", "Line");
        let all = Hunk {
            old: 0..20,
            new: 0..20,
        };
        assert_eq!(hunk_emphasis(&text(&big), &text(&changed), &all), None);
        let pure = Hunk {
            old: 0..0,
            new: 0..1,
        };
        assert_eq!(hunk_emphasis(&text(""), &text("x\n"), &pure), None);
    }
}
