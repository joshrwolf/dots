//! What a picker row is made of: the entry trait and its drawn cells.

use std::borrow::Cow;
use std::fmt;

use ratatui::style::Color;

use crate::{Palette, text};

/// One selectable entry, optionally with secondary lines.
pub trait Entry {
    /// Which declared group this entry belongs to.
    type Group: Copy + Eq + fmt::Debug + Send + 'static;

    fn group(&self) -> Self::Group;

    /// One cell per column of this entry's group, in order.
    ///
    /// Called once per entry, when the picker is built — not per frame. The
    /// palette is handed in rather than captured so that a row cannot end up
    /// painted from a different palette than the chrome around it.
    fn cells(&self, palette: &Palette) -> Vec<Cell>;

    /// Additional single-line cells beneath the primary row, also searchable.
    /// They share this entry's selection and identity, not separate list items.
    fn details(&self, _palette: &Palette) -> Vec<Cell> {
        Vec::new()
    }

    /// Search terms that are never drawn — an id, an alias, a full path whose
    /// row only shows the tail.
    fn hidden_terms(&self) -> Option<Cow<'_, str>> {
        None
    }

    /// A stable identity, so a refreshed list keeps the selection on this entry.
    fn identity(&self) -> Option<Cow<'_, str>> {
        None
    }

    /// A command whose output previews this entry while it is selected.
    fn preview(&self) -> Option<&[String]> {
        None
    }
}

/// One drawn cell. A colour is required because a cell left to the terminal
/// default has nothing to recede against: if that default is a muted grey then
/// a row's primary content renders as secondary.
///
/// Terminal titles and Unix paths are untrusted display text, so a cell keeps
/// to one line: a tab becomes a space and any other control character a
/// replacement character.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cell {
    pub(crate) text: String,
    pub(crate) fg: Color,
    pub(crate) searchable: bool,
    pub(crate) prefix: String,
    pub(crate) tail: bool,
}

impl Cell {
    /// A cell that is both drawn and searched.
    pub fn new(text: impl Into<String>, fg: Color) -> Self {
        Self {
            text: text::display(text.into()),
            fg,
            searchable: true,
            prefix: String::new(),
            tail: false,
        }
    }

    /// A cell that is drawn but never searched — a status glyph, a kind label,
    /// a focus marker. Nothing the user would type to find the row.
    pub fn tag(text: impl Into<String>, fg: Color) -> Self {
        Self {
            searchable: false,
            ..Self::new(text, fg)
        }
    }

    /// A directory or file path: keep its distinctive suffix when clipped.
    pub fn path(text: impl Into<String>, fg: Color) -> Self {
        Self {
            tail: true,
            ..Self::new(text, fg)
        }
    }

    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Muted, unsearchable context before the cell's primary text.
    #[must_use]
    pub fn prefix(mut self, text: impl Into<String>) -> Self {
        self.prefix = text::display(text.into());
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cell_text_cannot_inject_rows_or_terminal_commands() {
        let cell = Cell::new("one\ntwo\r\u{1b}[31m\tend", Color::White);
        assert_eq!(cell.text(), "one�two��[31m end");
    }
}
