//! Line diffs of one file, with word-level emphasis inside paired changed lines.

use std::collections::HashMap;
use std::hash::{Hash as _, Hasher as _};
use std::ops::Range;
use std::sync::Arc;

use imara_diff::{Algorithm, Diff, InternedInput};

/// A file's text split into lines. Line `i` spans `starts[i]..starts[i + 1]`
/// and includes its line terminator.
#[derive(Debug)]
pub(crate) struct Text {
    pub(crate) src: Arc<str>,
    starts: Vec<u32>,
    /// Identifies the content for highlight caching.
    pub(crate) hash: u64,
}

impl Text {
    pub(crate) fn new(src: Arc<str>) -> Self {
        let mut starts = Vec::with_capacity(src.len() / 32 + 2);
        starts.push(0);
        for (i, b) in src.bytes().enumerate() {
            if b == b'\n' {
                starts.push(offset(i + 1));
            }
        }
        if starts.last().copied() != Some(offset(src.len())) {
            starts.push(offset(src.len()));
        }
        let mut hasher = std::hash::DefaultHasher::new();
        src.hash(&mut hasher);
        Self {
            src,
            starts,
            hash: hasher.finish(),
        }
    }

    pub(crate) fn len(&self) -> u32 {
        offset(self.starts.len() - 1)
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

    /// Byte offset of line `i` within `src`.
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

/// Emphasised byte ranges within single lines, keyed by line number.
pub(crate) type Emphasis = HashMap<u32, Vec<Range<u32>>>;

#[derive(Debug)]
pub(crate) struct FileDiff {
    pub(crate) old: Arc<Text>,
    pub(crate) new: Arc<Text>,
    pub(crate) hunks: Vec<Hunk>,
    pub(crate) added: u32,
    pub(crate) removed: u32,
    pub(crate) emph_old: Emphasis,
    pub(crate) emph_new: Emphasis,
}

/// Lines longer than this get no word emphasis; the diff would cost more than it shows.
const EMPHASIS_MAX_LINE: usize = 400;

impl FileDiff {
    pub(crate) fn compute(old: Text, new: Text) -> Self {
        let mut input = InternedInput::default();
        input.update_before((0..old.len()).map(|i| old.raw(i)));
        input.update_after((0..new.len()).map(|i| new.raw(i)));
        let mut diff = Diff::compute(Algorithm::Histogram, &input);
        diff.postprocess_lines(&input);
        let hunks: Vec<Hunk> = diff
            .hunks()
            .map(|h| Hunk {
                old: h.before,
                new: h.after,
            })
            .collect();
        let mut emph_old = Emphasis::new();
        let mut emph_new = Emphasis::new();
        for hunk in &hunks {
            for (o, n) in hunk.old.clone().zip(hunk.new.clone()) {
                if let Some([a, b]) = word_emphasis(old.line(o), new.line(n)) {
                    emph_old.insert(o, a);
                    emph_new.insert(n, b);
                }
            }
        }
        Self {
            added: diff.count_additions(),
            removed: diff.count_removals(),
            old: Arc::new(old),
            new: Arc::new(new),
            hunks,
            emph_old,
            emph_new,
        }
    }
}

/// The changed tokens of a paired line, or `None` when the lines share too
/// little for word emphasis to help (the whole line is the change).
pub(crate) fn word_emphasis(old: &str, new: &str) -> Option<[Vec<Range<u32>>; 2]> {
    if old.len() > EMPHASIS_MAX_LINE || new.len() > EMPHASIS_MAX_LINE {
        return None;
    }
    let (a_tokens, b_tokens) = (tokens(old), tokens(new));
    let ops = similar::capture_diff_slices(similar::Algorithm::Myers, &a_tokens, &b_tokens);
    let (mut a, mut b, mut same) = (Vec::new(), Vec::new(), 0usize);
    let span = |tokens: &[&str], range: Range<usize>, line: &str| -> Range<u32> {
        let start = tokens[range.clone()]
            .first()
            .map_or(line.len(), |t| t.as_ptr() as usize - line.as_ptr() as usize);
        let len: usize = tokens[range].iter().map(|t| t.len()).sum();
        offset(start)..offset(start + len)
    };
    for op in ops {
        let (tag, ar, br) = op.as_tag_tuple();
        if tag == similar::DiffTag::Equal {
            same += a_tokens[ar].iter().map(|t| t.trim().len()).sum::<usize>();
        } else {
            if !ar.is_empty() {
                push_range(&mut a, span(&a_tokens, ar, old));
            }
            if !br.is_empty() {
                push_range(&mut b, span(&b_tokens, br, new));
            }
        }
    }
    let longest = old.trim().len().max(new.trim().len()).max(1);
    // Mostly rewritten lines read better as plain removals and additions.
    (same * 10 >= longest * 4).then_some([a, b])
}

/// Splits a line into identifier runs, whitespace runs and single symbols.
fn tokens(line: &str) -> Vec<&str> {
    let class = |c: char| {
        if c.is_alphanumeric() || c == '_' {
            0
        } else if c.is_whitespace() {
            1
        } else {
            2
        }
    };
    let mut out = Vec::new();
    let mut start = 0;
    let mut prev = None;
    for (i, c) in line.char_indices() {
        let k = class(c);
        if i > start && (prev != Some(k) || k == 2) {
            out.push(&line[start..i]);
            start = i;
        }
        prev = Some(k);
    }
    if start < line.len() {
        out.push(&line[start..]);
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
        let d = FileDiff::compute(text("a\nb\nc\nd\n"), text("a\nB\nc\nd\ne\n"));
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
    }

    #[test]
    fn emphasises_changed_words_only_when_lines_are_similar() {
        let [a, b] = word_emphasis("let x = foo(1);", "let x = bar(1);").expect("similar");
        assert_eq!(a, vec![8..11]);
        assert_eq!(b, vec![8..11]);
        assert!(word_emphasis("completely different", "nothing alike here at all").is_none());
    }
}
