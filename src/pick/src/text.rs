//! The picker's single-line text contract.
//!
//! Every row, label, heading and the prompt is one terminal line, and so is the
//! query. C0/C1 controls can move the cursor, create extra rows, or inject a
//! terminal escape sequence, so none reaches the screen. Host display text
//! shows where a control was; typed and pasted input drops it.

/// Host-supplied display text as one line: a tab becomes a space and any other
/// control a replacement character.
pub(crate) fn display(text: String) -> String {
    if !text.chars().any(char::is_control) {
        return text;
    }
    text.chars()
        .map(|ch| {
            if ch == '\t' {
                ' '
            } else if ch.is_control() {
                '\u{fffd}'
            } else {
                ch
            }
        })
        .collect()
}

/// One character of query input: whitespace controls become an ordinary space
/// and other controls are discarded.
pub(crate) fn input(ch: char) -> Option<char> {
    if !ch.is_control() {
        Some(ch)
    } else if ch.is_whitespace() {
        Some(' ')
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_text_cannot_inject_rows_or_terminal_commands() {
        assert_eq!(
            display("one\ntwo\r\u{1b}[31m\tend\u{9b}".to_owned()),
            "one\u{fffd}two\u{fffd}\u{fffd}[31m end\u{fffd}"
        );
    }

    #[test]
    fn input_keeps_one_line_and_drops_other_controls() {
        let typed: String = "a\nb\tc\u{1b}d\u{9b}e 一"
            .chars()
            .filter_map(input)
            .collect();
        assert_eq!(typed, "a b cde 一");
    }
}
