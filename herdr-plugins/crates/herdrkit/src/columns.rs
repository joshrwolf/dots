//! Column measurement in terminal cells.
//!
//! Every width here is a display width, never a byte or codepoint count, and
//! every cut lands on a grapheme boundary. Agent rows carry task titles taken
//! from terminal titles, which contain wide glyphs, combining marks and emoji
//! sequences; measuring or cutting per codepoint shifts every column to the
//! right of one of them and the drift is invisible in a diff.

use std::borrow::Cow;

use unicode_segmentation::UnicodeSegmentation as _;
use unicode_width::UnicodeWidthStr;

/// Cells `text` occupies when printed.
pub(crate) fn width(text: &str) -> usize {
    UnicodeWidthStr::width(text)
}

/// Grapheme clusters in `text`.
///
/// This is the unit `nucleo` indexes a haystack in, so it is also the unit a
/// match position means.
pub(crate) fn graphemes(text: &str) -> usize {
    text.graphemes(true).count()
}

/// `text` clipped to `max` cells, ending in `…` when anything was dropped.
///
/// A grapheme that straddles the limit is dropped whole, so the result is never
/// wider than `max`, and the kept part is always a grapheme-prefix of `text` —
/// which is what lets a match position computed over `text` be applied to the
/// clipped form.
pub(crate) fn truncate(text: &str, max: usize) -> Cow<'_, str> {
    if width(text) <= max {
        return Cow::Borrowed(text);
    }
    if max == 0 {
        return Cow::Borrowed("");
    }
    let budget = max - 1;
    let mut out = String::with_capacity(text.len());
    let mut used = 0;
    for grapheme in text.graphemes(true) {
        let cells = width(grapheme);
        if used + cells > budget {
            break;
        }
        used += cells;
        out.push_str(grapheme);
    }
    out.push('…');
    Cow::Owned(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A family emoji: five codepoints, one grapheme, two cells.
    const FAMILY: &str = "\u{1f468}\u{200d}\u{1f469}\u{200d}\u{1f467}";
    /// `e` plus a combining acute: two codepoints, one grapheme, one cell.
    const COMBINING: &str = "cafe\u{301}";

    #[test]
    fn text_that_already_fits_is_not_copied() {
        assert!(matches!(truncate("short", 20), Cow::Borrowed(_)));
    }

    #[test]
    fn a_wide_glyph_straddling_the_limit_is_dropped_whole() {
        let out = truncate("一二三四五", 5);
        assert_eq!(out, "一二…");
        assert_eq!(width(&out), 5);
    }

    /// Measuring per codepoint counts this family as six cells rather than two,
    /// which under-fills the column by four for every sequence in it.
    #[test]
    fn an_emoji_sequence_is_measured_as_one_grapheme_not_five_codepoints() {
        assert_eq!(width(FAMILY), 2);
        assert_eq!(graphemes(FAMILY), 1);
        let three = FAMILY.repeat(3);
        let text = format!("{three}abc");
        let out = truncate(&text, 8);
        assert_eq!(width(&out), 8);
        // Measured per codepoint each family counts six cells, so only one of
        // the three would have survived an 8-cell budget.
        assert!(out.starts_with(&three), "{out:?} lost a family");
    }

    #[test]
    fn a_combining_mark_never_splits_from_its_base() {
        assert_eq!(graphemes(COMBINING), 4);
        for cells in 1..8 {
            let out = truncate(COMBINING, cells);
            assert!(
                !out.starts_with('\u{301}') && !out.contains("e\u{301}\u{301}"),
                "{out:?} split a cluster at {cells}"
            );
        }
    }

    #[test]
    fn the_kept_part_is_always_a_grapheme_prefix() {
        let text = format!("{FAMILY}x{COMBINING}");
        for cells in 1..20 {
            let out = truncate(&text, cells);
            let kept = out.strip_suffix('…').unwrap_or(&out);
            assert!(text.starts_with(kept), "{kept:?} is not a prefix");
            assert_eq!(
                graphemes(kept),
                text.graphemes(true)
                    .take(graphemes(kept))
                    .map(str::len)
                    .count(),
                "kept part is whole graphemes at {cells}"
            );
        }
    }

    #[test]
    fn a_zero_width_joiner_does_not_consume_a_cell() {
        assert_eq!(width("a\u{200d}b"), 2);
    }
}
