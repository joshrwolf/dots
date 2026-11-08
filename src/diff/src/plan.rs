//! Row planning: which lines of a file diff are shown, in what order, and
//! how many display lines each row takes at a given width.

use std::collections::HashSet;

use crate::diff::Hunk;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Layout {
    Unified,
    Split,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    Context,
    Delete,
    Insert,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Row {
    /// Unified: one line of one side, or a context line of both.
    Line {
        kind: Kind,
        old: Option<u32>,
        new: Option<u32>,
    },
    /// Split: the old line on the left and the new one on the right. A
    /// changed pair shows deletions left and insertions right.
    Pair {
        old: Option<u32>,
        new: Option<u32>,
        changed: bool,
    },
    /// Unchanged lines folded away; `id` names the region for expansion.
    Gap { id: u32, hidden: u32 },
    /// Rows past the per-file cap.
    Truncated { hidden: u32 },
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct Options {
    pub(crate) layout: Layout,
    pub(crate) context: u32,
    /// The most rows one file plans; the rest become a [`Row::Truncated`].
    pub(crate) cap: usize,
}

/// Plans the rows of one file. `old_len` and `new_len` are line counts;
/// `expanded` holds the ids of unchanged regions shown in full.
pub(crate) fn plan(
    hunks: &[Hunk],
    old_len: u32,
    new_len: u32,
    expanded: &HashSet<u32>,
    options: Options,
) -> Vec<Row> {
    let mut rows = Vec::new();
    if hunks.is_empty() {
        return rows;
    }
    let ctx = options.context;
    let (mut old_at, mut new_at) = (0u32, 0u32);
    for (id, hunk) in (0u32..).zip(hunks.iter().map(Some).chain([None])) {
        let (old_end, new_end) = hunk.map_or((old_len, new_len), |h| (h.old.start, h.new.start));
        let len = old_end - old_at;
        debug_assert_eq!(len, new_end - new_at, "unchanged regions match in length");
        let (show_head, show_tail) = if expanded.contains(&id) {
            (len, 0)
        } else {
            let head = if id == 0 { 0 } else { ctx };
            let tail = if hunk.is_none() { 0 } else { ctx };
            if head + tail + 1 >= len {
                (len, 0)
            } else {
                (head, tail)
            }
        };
        let context = |rows: &mut Vec<Row>, i: u32| {
            let (old, new) = (Some(old_at + i), Some(new_at + i));
            rows.push(match options.layout {
                Layout::Unified => Row::Line {
                    kind: Kind::Context,
                    old,
                    new,
                },
                Layout::Split => Row::Pair {
                    old,
                    new,
                    changed: false,
                },
            });
        };
        for i in 0..show_head {
            context(&mut rows, i);
        }
        let hidden = len - show_head - show_tail;
        if hidden > 0 {
            rows.push(Row::Gap { id, hidden });
        }
        for i in len - show_tail..len {
            context(&mut rows, i);
        }
        let Some(hunk) = hunk else { break };
        match options.layout {
            Layout::Unified => {
                rows.extend(hunk.old.clone().map(|o| Row::Line {
                    kind: Kind::Delete,
                    old: Some(o),
                    new: None,
                }));
                rows.extend(hunk.new.clone().map(|n| Row::Line {
                    kind: Kind::Insert,
                    old: None,
                    new: Some(n),
                }));
            }
            Layout::Split => {
                let (dels, ins) = (hunk.old.len(), hunk.new.len());
                for i in 0..dels.max(ins) {
                    let i = u32::try_from(i).expect("hunk lengths are u32");
                    rows.push(Row::Pair {
                        old: (i < hunk.old.end - hunk.old.start).then(|| hunk.old.start + i),
                        new: (i < hunk.new.end - hunk.new.start).then(|| hunk.new.start + i),
                        changed: true,
                    });
                }
            }
        }
        old_at = hunk.old.end;
        new_at = hunk.new.end;
    }
    if rows.len() > options.cap {
        let hidden = u32::try_from(rows.len() - options.cap).unwrap_or(u32::MAX);
        rows.truncate(options.cap);
        rows.push(Row::Truncated { hidden });
    }
    rows
}

/// Prefix sums of row heights: `offsets[i]` is the first display line of
/// row `i`, and the last entry is the total.
pub(crate) fn offsets(heights: impl Iterator<Item = u32>) -> Vec<u32> {
    let mut out = Vec::with_capacity(heights.size_hint().0 + 1);
    let mut total = 0u32;
    out.push(0);
    for h in heights {
        total += h;
        out.push(total);
    }
    out
}

/// The row containing display line `line`, and the line's index within the
/// row. `offsets` comes from [`offsets`]; lines past the end land on the
/// last row.
pub(crate) fn locate(offsets: &[u32], line: u32) -> (usize, u32) {
    let rows = offsets.len().saturating_sub(1);
    if rows == 0 {
        return (0, 0);
    }
    let row = offsets.partition_point(|&o| o <= line).saturating_sub(1);
    let row = row.min(rows - 1);
    (row, line - offsets[row])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn options(layout: Layout) -> Options {
        Options {
            layout,
            context: 2,
            cap: 1000,
        }
    }

    fn hunk(old: std::ops::Range<u32>, new: std::ops::Range<u32>) -> Hunk {
        Hunk { old, new }
    }

    fn line(kind: Kind, old: Option<u32>, new: Option<u32>) -> Row {
        Row::Line { kind, old, new }
    }

    #[test]
    fn folds_unchanged_regions_around_a_change() {
        // 10 lines; line 5 replaced.
        let rows = plan(
            &[hunk(5..6, 5..6)],
            10,
            10,
            &HashSet::new(),
            options(Layout::Unified),
        );
        let ctx = |i| line(Kind::Context, Some(i), Some(i));
        assert_eq!(
            rows,
            vec![
                Row::Gap { id: 0, hidden: 3 },
                ctx(3),
                ctx(4),
                line(Kind::Delete, Some(5), None),
                line(Kind::Insert, None, Some(5)),
                ctx(6),
                ctx(7),
                Row::Gap { id: 1, hidden: 2 },
            ]
        );
    }

    #[test]
    fn expanding_a_gap_shows_every_line() {
        let expanded = HashSet::from([0]);
        let rows = plan(
            &[hunk(5..6, 5..6)],
            10,
            10,
            &expanded,
            options(Layout::Unified),
        );
        assert_eq!(rows[0], line(Kind::Context, Some(0), Some(0)));
        assert!(!rows.iter().any(|r| matches!(r, Row::Gap { id: 0, .. })));
    }

    #[test]
    fn never_folds_a_single_line() {
        // Two changes with exactly 2*ctx+1 unchanged lines between them.
        let rows = plan(
            &[hunk(0..1, 0..1), hunk(6..7, 6..7)],
            7,
            7,
            &HashSet::new(),
            options(Layout::Unified),
        );
        assert!(!rows.iter().any(|r| matches!(r, Row::Gap { .. })));
        assert_eq!(rows.len(), 2 + 5 + 2);
    }

    #[test]
    fn split_pairs_deletions_with_insertions() {
        let rows = plan(
            &[hunk(0..2, 0..1)],
            2,
            1,
            &HashSet::new(),
            options(Layout::Split),
        );
        assert_eq!(
            rows,
            vec![
                Row::Pair {
                    old: Some(0),
                    new: Some(0),
                    changed: true
                },
                Row::Pair {
                    old: Some(1),
                    new: None,
                    changed: true
                },
            ]
        );
    }

    #[test]
    fn caps_rows_per_file() {
        let mut opts = options(Layout::Unified);
        opts.cap = 3;
        let rows = plan(&[hunk(0..0, 0..10)], 0, 10, &HashSet::new(), opts);
        assert_eq!(rows.len(), 4);
        assert_eq!(rows[3], Row::Truncated { hidden: 7 });
    }

    #[test]
    fn locates_display_lines_in_rows() {
        let offs = offsets([1, 3, 1].into_iter());
        assert_eq!(offs, vec![0, 1, 4, 5]);
        assert_eq!(locate(&offs, 0), (0, 0));
        assert_eq!(locate(&offs, 2), (1, 1));
        assert_eq!(locate(&offs, 4), (2, 0));
        assert_eq!(locate(&offs, 99), (2, 95));
        assert_eq!(locate(&[0], 3), (0, 0));
    }
}
