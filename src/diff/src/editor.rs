//! The comment editor's text: lines of characters and a cursor. Keys map
//! to these edits in the runtime; drawing reads `lines` and `cursor`.

use crate::review::{Kind, Place};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Buffer {
    lines: Vec<String>,
    /// Line, and character within it.
    row: usize,
    col: usize,
}

impl Buffer {
    pub(crate) fn new(text: &str) -> Self {
        let lines: Vec<String> = text.split('\n').map(str::to_owned).collect();
        let row = lines.len() - 1;
        let col = lines[row].chars().count();
        Self { lines, row, col }
    }

    pub(crate) fn text(&self) -> String {
        self.lines.join("\n")
    }

    pub(crate) fn lines(&self) -> &[String] {
        &self.lines
    }

    pub(crate) fn cursor(&self) -> (usize, usize) {
        (self.row, self.col)
    }

    fn byte(&self, col: usize) -> usize {
        self.lines[self.row]
            .char_indices()
            .nth(col)
            .map_or(self.lines[self.row].len(), |(i, _)| i)
    }

    fn line_len(&self) -> usize {
        self.lines[self.row].chars().count()
    }

    /// Inserts text, which may hold newlines, as a paste does.
    pub(crate) fn insert(&mut self, text: &str) {
        let text = text.replace("\r\n", "\n").replace('\r', "\n");
        let at = self.byte(self.col);
        let rest = self.lines[self.row].split_off(at);
        let mut parts = text.split('\n');
        if let Some(first) = parts.next() {
            self.lines[self.row].push_str(first);
        }
        for part in parts {
            self.row += 1;
            self.lines.insert(self.row, part.to_owned());
        }
        self.col = self.line_len();
        self.lines[self.row].push_str(&rest);
    }

    pub(crate) fn newline(&mut self) {
        self.insert("\n");
    }

    pub(crate) fn backspace(&mut self) {
        if self.col > 0 {
            let start = self.byte(self.col - 1);
            let end = self.byte(self.col);
            self.lines[self.row].replace_range(start..end, "");
            self.col -= 1;
        } else if self.row > 0 {
            let line = self.lines.remove(self.row);
            self.row -= 1;
            self.col = self.line_len();
            self.lines[self.row].push_str(&line);
        }
    }

    pub(crate) fn delete(&mut self) {
        if self.col < self.line_len() {
            let start = self.byte(self.col);
            let end = self.byte(self.col + 1);
            self.lines[self.row].replace_range(start..end, "");
        } else if self.row + 1 < self.lines.len() {
            let next = self.lines.remove(self.row + 1);
            self.lines[self.row].push_str(&next);
        }
    }

    /// Deletes back to the start of the word before the cursor, with the
    /// spaces after it; at a line start, joins the line to the previous.
    pub(crate) fn delete_word(&mut self) {
        if self.col == 0 {
            self.backspace();
            return;
        }
        let chars: Vec<char> = self.lines[self.row].chars().collect();
        let mut start = self.col;
        while start > 0 && chars[start - 1].is_whitespace() {
            start -= 1;
        }
        let word = |c: char| c.is_alphanumeric() || c == '_';
        if start > 0 && word(chars[start - 1]) {
            while start > 0 && word(chars[start - 1]) {
                start -= 1;
            }
        } else {
            start = start.saturating_sub(1);
        }
        let (a, b) = (self.byte(start), self.byte(self.col));
        self.lines[self.row].replace_range(a..b, "");
        self.col = start;
    }

    /// Deletes back to the start of the line.
    pub(crate) fn delete_to_start(&mut self) {
        let b = self.byte(self.col);
        self.lines[self.row].replace_range(..b, "");
        self.col = 0;
    }

    pub(crate) fn left(&mut self) {
        if self.col > 0 {
            self.col -= 1;
        } else if self.row > 0 {
            self.row -= 1;
            self.col = self.line_len();
        }
    }

    pub(crate) fn right(&mut self) {
        if self.col < self.line_len() {
            self.col += 1;
        } else if self.row + 1 < self.lines.len() {
            self.row += 1;
            self.col = 0;
        }
    }

    pub(crate) fn up(&mut self) {
        if self.row > 0 {
            self.row -= 1;
            self.col = self.col.min(self.line_len());
        }
    }

    pub(crate) fn down(&mut self) {
        if self.row + 1 < self.lines.len() {
            self.row += 1;
            self.col = self.col.min(self.line_len());
        }
    }

    pub(crate) fn home(&mut self) {
        self.col = 0;
    }

    pub(crate) fn end(&mut self) {
        self.col = self.line_len();
    }
}

/// An open editor: what it writes and where.
#[derive(Debug, Clone)]
pub(crate) struct Editor {
    pub(crate) buffer: Buffer,
    pub(crate) kind: Kind,
    pub(crate) place: Place,
    /// Editing replacement code rather than prose.
    pub(crate) suggestion: bool,
    /// The comment being edited, or `None` for a new one.
    pub(crate) editing: Option<u64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edits_across_lines() {
        let mut b = Buffer::new("");
        b.insert("héllo wörld");
        b.newline();
        b.insert("next");
        assert_eq!(b.text(), "héllo wörld\nnext");
        b.up();
        assert_eq!(b.cursor(), (0, 4));
        b.end();
        b.delete_word();
        assert_eq!(b.text(), "héllo \nnext");
        b.delete();
        assert_eq!(b.text(), "héllo next");
        b.home();
        b.backspace();
        b.right();
        b.backspace();
        assert_eq!(b.text(), "éllo next");
        b.end();
        b.delete_to_start();
        assert_eq!(b.text(), "");
    }

    #[test]
    fn pastes_keep_their_lines_and_the_rest_of_the_line() {
        let mut b = Buffer::new("ab");
        b.left();
        b.insert("1\r\n2\n3");
        assert_eq!(b.text(), "a1\n2\n3b");
        assert_eq!(b.cursor(), (2, 1));
        b.home();
        b.backspace();
        assert_eq!(b.text(), "a1\n23b");
        b.home();
        b.delete_word();
        assert_eq!(b.text(), "a123b");
    }

    #[test]
    fn word_deletion_stops_at_punctuation() {
        let mut b = Buffer::new("call(foo_bar,  ");
        b.delete_word();
        assert_eq!(b.text(), "call(foo_bar");
        b.delete_word();
        assert_eq!(b.text(), "call(");
        b.delete_word();
        assert_eq!(b.text(), "call");
    }
}
