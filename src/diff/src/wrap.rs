//! Display widths and indentation-preserving soft wrap.

use std::ops::Range;

use unicode_width::UnicodeWidthChar as _;

/// Cells a tab occupies. Fixed, not tab-stop aligned, so a line's width does
/// not depend on where it is drawn.
pub(crate) const TAB: u16 = 4;

pub(crate) fn char_width(c: char) -> u16 {
    if c == '\t' {
        TAB
    } else {
        u16::try_from(c.width().unwrap_or(0)).unwrap_or(0)
    }
}

/// One display line of a wrapped source line: a byte range of the source,
/// drawn after `indent` blank cells.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Segment {
    pub(crate) bytes: Range<usize>,
    pub(crate) indent: u16,
}

/// Splits `text` into display lines of at most `width` cells. Continuation
/// lines repeat the leading indentation so wrapped code keeps its shape,
/// unless that would leave less than half the width for text. Breaks prefer
/// the last space in the second half of a line.
pub(crate) fn wrap(text: &str, width: u16) -> Vec<Segment> {
    let mut out = Vec::new();
    wrap_into(text, width, &mut |segment| out.push(segment));
    out
}

/// How many display lines [`wrap`] produces, without allocating them.
pub(crate) fn wrapped_height(text: &str, width: u16) -> u32 {
    if text.len() <= usize::from(width) && text.is_ascii() && !text.contains('\t') {
        return 1;
    }
    let mut n = 0;
    wrap_into(text, width, &mut |_| n += 1);
    n
}

fn wrap_into(text: &str, width: u16, emit: &mut dyn FnMut(Segment)) {
    let width = width.max(1);
    let lead: u16 = text
        .chars()
        .take_while(|c| c.is_whitespace())
        .map(char_width)
        .sum();
    let lead_bytes = text.len() - text.trim_start().len();
    let indent = if width.saturating_sub(lead) >= (width / 2).max(1) {
        lead
    } else {
        0
    };
    let mut start = 0;
    let mut cur_indent = 0;
    loop {
        let avail = width - cur_indent;
        let mut used = 0u16;
        let mut cut = None;
        let mut last_space = None;
        for (i, c) in text[start..].char_indices() {
            let w = char_width(c);
            if used + w > avail && i > 0 {
                cut = Some(start + i);
                break;
            }
            used += w;
            if c == ' ' && used > avail / 2 && start + i >= lead_bytes {
                last_space = Some(start + i + 1);
            }
        }
        let Some(hard) = cut else {
            emit(Segment {
                bytes: start..text.len(),
                indent: cur_indent,
            });
            return;
        };
        let end = last_space.filter(|&s| s < hard).unwrap_or(hard);
        emit(Segment {
            bytes: start..end,
            indent: cur_indent,
        });
        start = end;
        cur_indent = indent;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(text: &str, width: u16) -> Vec<(&str, u16)> {
        wrap(text, width)
            .into_iter()
            .map(|s| (&text[s.bytes], s.indent))
            .collect()
    }

    #[test]
    fn short_lines_do_not_wrap() {
        assert_eq!(texts("fn main() {}", 20), vec![("fn main() {}", 0)]);
        assert_eq!(texts("", 20), vec![("", 0)]);
        assert_eq!(wrapped_height("abc", 3), 1);
    }

    #[test]
    fn continuation_keeps_indentation_and_prefers_spaces() {
        assert_eq!(
            texts("    let value = compute(alpha, beta);", 20),
            vec![
                ("    let value = ", 0),
                ("compute(alpha, ", 4),
                ("beta);", 4)
            ]
        );
        assert_eq!(
            wrapped_height("    let value = compute(alpha, beta);", 20),
            3
        );
    }

    #[test]
    fn deep_indentation_is_dropped_when_it_would_crowd_the_text() {
        let line = format!("{}abcdefghij", " ".repeat(8));
        assert_eq!(texts(&line, 10), vec![("        ab", 0), ("cdefghij", 0)]);
    }

    #[test]
    fn wide_glyphs_count_two_cells() {
        assert_eq!(texts("日本語", 4), vec![("日本", 0), ("語", 0)]);
        assert_eq!(wrapped_height("日本語", 4), 2);
    }
}
