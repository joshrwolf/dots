//! The interactive list every plugin that asks the user to choose goes
//! through.
//!
//! It runs in this process. Nothing is spawned, nothing is serialised, and the
//! value handed back is the value handed in — so there is no text format
//! between the index and the action to keep in step.
//!
//! Owning the loop is what buys the three properties everything here is built
//! on:
//!
//! - **Match text is independent of displayed text.** A [`Cell`] can be drawn
//!   without being searched, and an entry can contribute search terms it never
//!   draws.
//! - **Rows can be inert.** A group's divider is a row that renders, never
//!   matches, and that the cursor steps over, so `Enter` cannot land on a
//!   non-choice.
//! - **It is a pure function** of items, query, cursor and width, which is to
//!   say it is a table test rather than something verified by screen-scraping
//!   a pane and counting character offsets by hand.

mod keys;
mod model;
mod run;

use std::borrow::Cow;
use std::fmt;
use std::io;

use ratatui::style::Color;

use crate::Theme;

pub use keys::{Command, command};
pub use model::{Model, Outcome};

/// One selectable row.
pub trait Entry {
    /// Which declared group this entry belongs to.
    type Group: Copy + Eq + fmt::Debug;

    fn group(&self) -> Self::Group;

    /// One cell per column of this entry's group, in order.
    ///
    /// Called once per entry, when the picker is built — not per frame. The
    /// theme is handed in rather than captured so that a row cannot end up
    /// painted from a different palette than the chrome around it.
    fn cells(&self, theme: &Theme) -> Vec<Cell>;

    /// Search terms that are never drawn — an id, an alias, a full path whose
    /// row only shows the tail.
    fn hidden_terms(&self) -> Option<Cow<'_, str>> {
        None
    }
}

/// One drawn cell. A colour is required because a cell left to the terminal
/// default has nothing to recede against: if that default is a muted grey then
/// a row's primary content renders as secondary.
#[derive(Debug, Clone)]
pub struct Cell {
    text: String,
    fg: Color,
    searchable: bool,
}

impl Cell {
    /// A cell that is both drawn and searched.
    pub fn new(text: impl Into<String>, fg: Color) -> Self {
        Self {
            text: single_line(text.into()),
            fg,
            searchable: true,
        }
    }

    /// A cell that is drawn but never searched — a status glyph, a kind label,
    /// a focus marker. Nothing the user would type to find the row.
    pub fn tag(text: impl Into<String>, fg: Color) -> Self {
        Self {
            text: single_line(text.into()),
            fg,
            searchable: false,
        }
    }

    pub fn text(&self) -> &str {
        &self.text
    }
}

/// Terminal titles and Unix paths are untrusted display text. C0/C1 controls
/// can move the cursor, create extra rows, or inject a terminal escape
/// sequence, all of which violate the picker's one-entry-per-line layout.
fn single_line(text: String) -> String {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Width {
    Cells(usize),
    /// Shares whatever the fixed columns leave, wherever it sits in the row —
    /// so a group can put a narrow marker column after its widest one.
    Fill,
}

/// A column carries no label of its own: a group is titled once, by its own
/// divider, and the content of a row of names and paths says what it is.
#[derive(Debug, Clone)]
#[must_use]
pub struct Column {
    width: Width,
}

impl Column {
    pub fn new(cells: usize) -> Self {
        Self {
            width: Width::Cells(cells),
        }
    }

    pub fn fill() -> Self {
        Self { width: Width::Fill }
    }

    /// Cells this column always needs, ignoring whatever a fill would take.
    pub(crate) fn fixed(&self) -> usize {
        match self.width {
            Width::Cells(cells) => cells,
            Width::Fill => 0,
        }
    }
}

/// A run of entries sharing a set of columns.
///
/// Groups keep their declared order and entries sort by score *within* their
/// group. Sorting the whole list by score instead would interleave the kinds,
/// so a handful of live agents would vanish among hundreds of directories.
///
/// `label` titles the group's divider. It is chrome, never part of a haystack,
/// so it cannot be typed to filter — an entry that wants its kind to be
/// searchable puts it in [`Entry::hidden_terms`].
#[derive(Debug, Clone)]
#[must_use]
pub struct Group<G> {
    key: G,
    label: &'static str,
    columns: Vec<Column>,
    scope: Option<char>,
}

impl<G> Group<G> {
    pub fn new(key: G, label: &'static str, columns: Vec<Column>) -> Self {
        Self {
            key,
            label,
            columns,
            scope: None,
        }
    }

    /// Hides this group until the query starts with `sigil`, and shows only
    /// scoped groups once it does. The sigil is stripped before matching, so
    /// it cannot skew results.
    ///
    /// This is how a large, low-value source stays out of the way: hundreds of
    /// directories against a handful of open workspaces would otherwise be the
    /// whole list. The default view stays the rows that describe live state.
    pub fn scoped(mut self, sigil: char) -> Self {
        self.scope = Some(sigil);
        self
    }
}

/// A picker that cannot be drawn.
///
/// Every variant but [`Error::Terminal`] is a programming error in the calling
/// plugin, reported rather than tolerated: an entry in an undeclared group
/// would otherwise be silently absent from the list, and a cell count that
/// disagrees with its columns drops content or leaves a blank column that
/// reads as missing data. Neither is visible on screen.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    #[error("the terminal could not be driven")]
    Terminal(#[from] io::Error),

    #[error("no groups were declared, so no entry could ever be shown")]
    NoGroups,

    #[error("group {group} is declared twice")]
    DuplicateGroup { group: String },

    #[error("an entry belongs to group {group}, which is not declared")]
    UndeclaredGroup { group: String },

    #[error("group {group} declares {declared} columns but an entry drew {drawn} cells")]
    Arity {
        group: String,
        declared: usize,
        drawn: usize,
    },
}

#[derive(Debug)]
#[must_use]
pub struct Picker<T: Entry> {
    items: Vec<T>,
    groups: Vec<Group<T::Group>>,
    prompt: String,
    theme: Theme,
    match_paths: bool,
}

impl<T: Entry> Picker<T> {
    pub fn new(items: Vec<T>) -> Self {
        Self {
            items,
            groups: Vec::new(),
            prompt: "> ".to_owned(),
            // A picker should agree with herdr without every plugin having to
            // remember the same opt-in. Callers can still provide a theme for
            // previews, tests, or a deliberately distinct embedded surface.
            theme: Theme::load(),
            match_paths: false,
        }
    }

    pub fn group(mut self, group: Group<T::Group>) -> Self {
        self.groups.push(group);
        self
    }

    pub fn groups(mut self, groups: impl IntoIterator<Item = Group<T::Group>>) -> Self {
        self.groups.extend(groups);
        self
    }

    pub fn prompt(mut self, prompt: impl Into<String>) -> Self {
        self.prompt = prompt.into();
        self
    }

    pub fn theme(mut self, theme: Theme) -> Self {
        self.theme = theme;
        self
    }

    /// Scores `/` as a path separator, so a query matching a path segment
    /// boundary ranks above one matching mid-segment.
    pub fn match_paths(mut self) -> Self {
        self.match_paths = true;
        self
    }

    /// Validates the layout and lays out every row, without a terminal.
    pub fn build(self) -> Result<Model<T>, Error> {
        Model::build(self.items, self.groups, self.theme, self.match_paths)
    }

    /// Draws the list and blocks until the user chooses or aborts.
    ///
    /// `Ok(None)` is an abort, which is an ordinary outcome and not an error.
    pub fn run(self) -> Result<Option<T>, Error> {
        let prompt = self.prompt.clone();
        run::run(self.build()?, &prompt)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug)]
    struct Nothing;

    impl Entry for Nothing {
        type Group = ();

        fn group(&self) {}

        fn cells(&self, _theme: &Theme) -> Vec<Cell> {
            Vec::new()
        }
    }

    #[test]
    fn a_new_picker_uses_herdrs_configured_theme() {
        let loaded = Theme::load();
        let picker = Picker::new(vec![Nothing]);
        assert_eq!(picker.theme.indicators, loaded.indicators);
        assert_eq!(picker.theme.accent, loaded.accent);
        assert_eq!(picker.theme.blue, loaded.blue);
    }

    #[test]
    fn cell_text_cannot_inject_rows_or_terminal_commands() {
        let cell = Cell::new("one\ntwo\r\u{1b}[31m\tend", Color::White);
        assert_eq!(cell.text(), "one�two��[31m end");
    }
}
