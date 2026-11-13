//! Cell-aware text for the CLI's terminal views and previews.
use unicode_segmentation::UnicodeSegmentation as _;
use unicode_width::UnicodeWidthStr as _;

/// Text from the store, safe to draw: no control characters, tabs as spaces.
pub(crate) fn clean(text: &str) -> String {
    text.chars()
        .filter_map(|c| match c {
            '\t' => Some(' '),
            '\n' => Some('\n'),
            c if c.is_control() => None,
            c => Some(c),
        })
        .collect()
}

pub(crate) fn clean_line(text: &str) -> String {
    clean(text).replace('\n', " ")
}

pub(crate) fn width(text: &str) -> usize {
    text.width()
}

/// Word-wrapped lines of at most `max` cells; words longer than a line break
/// between graphemes.
pub(crate) fn wrap(text: &str, max: usize) -> Vec<String> {
    let max = max.max(1);
    let mut lines = Vec::new();
    for paragraph in clean(text).split('\n') {
        let mut line = String::new();
        let mut used = 0;
        for word in paragraph.split_word_bounds() {
            let cells = width(word);
            if used + cells <= max {
                line.push_str(word);
                used += cells;
                continue;
            }
            if !line.is_empty() {
                lines.push(line.trim_end().to_owned());
                line = String::new();
                used = 0;
            }
            if word.trim().is_empty() {
                continue;
            }
            for grapheme in word.graphemes(true) {
                let cells = grapheme.width();
                if used + cells > max {
                    lines.push(std::mem::take(&mut line));
                    used = 0;
                }
                line.push_str(grapheme);
                used += cells;
            }
        }
        lines.push(line.trim_end().to_owned());
    }
    lines
}
