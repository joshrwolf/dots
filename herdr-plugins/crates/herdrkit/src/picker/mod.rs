//! The interactive list every plugin that asks the user to choose goes
//! through.
//!
//! It runs in this process and returns typed values without serializing rows.
//! Optional sources run on one managed async worker thread; terminal state and
//! selection remain on the calling thread. See [`Source`] for the provider contract.
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
mod shortcut;
mod source;

use std::borrow::Cow;
use std::fmt;
use std::io;
use std::num::NonZeroUsize;

use ratatui::style::Color;

use crate::Theme;

pub use keys::{Command, command};
pub use model::{Model, Outcome};
pub use shortcut::{Selection, Shortcut};
pub use source::{Source, SourceResult};

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
    prefix: String,
    tail: bool,
}

impl Cell {
    /// A cell that is both drawn and searched.
    pub fn new(text: impl Into<String>, fg: Color) -> Self {
        Self {
            text: single_line(text.into()),
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
            text: single_line(text.into()),
            fg,
            searchable: false,
            prefix: String::new(),
            tail: false,
        }
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    /// A directory or file path: keep its distinctive suffix when clipped.
    pub fn path(text: impl Into<String>, fg: Color) -> Self {
        Self {
            tail: true,
            ..Self::new(text, fg)
        }
    }

    /// Muted, unsearchable context before the cell's primary text.
    #[must_use]
    pub fn prefix(mut self, text: impl Into<String>) -> Self {
        self.prefix = single_line(text.into());
        self
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
    /// Fit the group's content and heading up to this cell budget.
    Content(usize),
    /// Shares whatever the fixed columns leave, wherever it sits in the row —
    /// so a group can put a narrow marker column after its widest one.
    Fill,
}

/// A column with an optional heading, aligned using the same width as its cells.
#[derive(Debug, Clone)]
#[must_use]
pub struct Column {
    width: Width,
    heading: String,
    measured: usize,
}

impl Column {
    pub fn new(cells: usize) -> Self {
        Self {
            width: Width::Cells(cells),
            heading: String::new(),
            measured: 0,
        }
    }

    pub fn fill() -> Self {
        Self {
            width: Width::Fill,
            heading: String::new(),
            measured: 0,
        }
    }

    pub fn heading(mut self, text: impl Into<String>) -> Self {
        self.heading = single_line(text.into());
        self
    }

    /// Measure once across all entries, keeping columns stable while filtering.
    pub fn content(max: usize) -> Self {
        Self {
            width: Width::Content(max),
            ..Self::new(0)
        }
    }

    /// Cells this column always needs, ignoring whatever a fill would take.
    pub(crate) fn fixed(&self) -> usize {
        match self.width {
            Width::Cells(cells) => cells,
            Width::Content(max) => self.measured.min(max),
            Width::Fill => 0,
        }
    }
}

/// A provider row with an identity stable across result refreshes.
#[derive(Debug)]
pub struct DynamicEntry<T> {
    pub id: String,
    pub value: T,
}

impl<T> DynamicEntry<T> {
    pub fn new(id: impl Into<String>, value: T) -> Self {
        Self {
            id: id.into(),
            value,
        }
    }
}

/// Inert source feedback, displayed independently of selectable entries.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum SourceStatus {
    #[default]
    Ready,
    Loading,
    /// Informational feedback alongside results, such as a pagination limit.
    Notice(String),
    /// The source found no domain results; connection actions may still be present.
    Empty(String),
    Error(String),
}

/// How entries are matched and ordered within a group.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Matching {
    #[default]
    Fuzzy,
    /// Keep provider order and display every supplied row without local highlights.
    ProviderRanked,
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
    exclusive: bool,
    matching: Matching,
}

impl<G> Group<G> {
    pub fn new(key: G, label: &'static str, columns: Vec<Column>) -> Self {
        Self {
            key,
            label,
            columns,
            scope: None,
            exclusive: false,
            matching: Matching::Fuzzy,
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
        self.exclusive = false;
        self.scope = Some(sigil);
        self
    }

    /// Require a typed sigil; ordinary Tab navigation never activates this group.
    pub fn exclusive(mut self, sigil: char) -> Self {
        self.scope = Some(sigil);
        self.exclusive = true;
        self
    }

    pub fn matching(mut self, matching: Matching) -> Self {
        self.matching = matching;
        self
    }
}

/// A picker that cannot be drawn.
///
/// Layout and source configuration errors are reported rather than tolerated: an entry in an undeclared group
/// would otherwise be silently absent from the list, and a cell count that
/// disagrees with its columns drops content or leaves a blank column that
/// reads as missing data. Neither is visible on screen.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    #[error("invalid, reserved, or duplicate picker shortcut ctrl+{key}")]
    InvalidShortcut { key: char },

    #[error("invalid or ambiguous picker scope {sigil:?}")]
    InvalidScope { sigil: char },

    #[error("invalid picker source configuration: {message}")]
    SourceConfiguration { message: String },

    #[error("a picker source worker panicked")]
    SourcePanicked,

    #[error("the terminal could not be driven")]
    Terminal(#[from] io::Error),

    #[error("no groups were declared, so no entry could ever be shown")]
    NoGroups,

    #[error("group replacement has {count} entries, exceeding its limit of {limit}")]
    EntryLimit { count: usize, limit: usize },

    #[error("group replacement contains an empty or duplicate entry identity: {id:?}")]
    InvalidIdentity { id: String },

    #[error("group replacement for {expected} contains an entry from {actual}")]
    ReplacementGroup { expected: String, actual: String },

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
    query: String,
    theme: Theme,
    match_paths: bool,
    sources: Vec<source::Registration<T>>,
}

impl<T: Entry> Picker<T> {
    pub fn new(items: Vec<T>) -> Self {
        Self {
            items,
            groups: Vec::new(),
            prompt: "> ".to_owned(),
            query: String::new(),
            // A picker should agree with herdr without every plugin having to
            // remember the same opt-in. Callers can still provide a theme for
            // previews, tests, or a deliberately distinct embedded surface.
            theme: Theme::load(),
            match_paths: false,
            sources: Vec::new(),
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

    /// Start with a query, applying the same control-character sanitization as paste.
    pub fn query(mut self, query: impl Into<String>) -> Self {
        self.query = query.into();
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

    /// Register an inert provider for an exclusive group. Only typing that
    /// group's sigil starts a request. The limit bounds each atomic batch;
    /// oversized or invalid batches become an inert error instead of partial rows.
    pub fn source(
        mut self,
        group: T::Group,
        limit: NonZeroUsize,
        provider: impl Source<T> + 'static,
    ) -> Self {
        self.sources.push(source::Registration {
            group,
            limit,
            provider: Box::new(provider),
        });
        self
    }

    /// Validates the layout and lays out every row, without a terminal.
    pub fn build(self) -> Result<Model<T>, Error> {
        let mut model = Model::build(self.items, self.groups, self.theme, self.match_paths)?;
        source::validate(&self.sources, &model)?;
        model.insert_text(&self.query);
        Ok(model)
    }

    /// Draws the list and blocks until the user chooses or aborts.
    ///
    /// `Ok(None)` is an abort, which is an ordinary outcome and not an error.
    pub fn run(self) -> Result<Option<T>, Error>
    where
        T: Send + 'static,
    {
        match self.run_with_shortcuts(Vec::<Shortcut<std::convert::Infallible>>::new())? {
            Selection::Entry(entry) => Ok(Some(entry)),
            Selection::Cancelled => Ok(None),
            Selection::Shortcut(never) => match never {},
        }
    }

    /// Returns an entry, a typed shortcut action, or cancellation. Shortcuts
    /// work even when a query matches nothing; normal editing keys are reserved.
    pub fn run_with_shortcuts<A>(
        mut self,
        shortcuts: Vec<Shortcut<A>>,
    ) -> Result<Selection<T, A>, Error>
    where
        T: Send + 'static,
    {
        shortcut::validate(&shortcuts)?;
        let prompt = self.prompt.clone();
        let registrations = std::mem::take(&mut self.sources);
        let model = self.build()?;
        let sources = source::Sources::start(registrations, &model)?;
        run::run(model, &prompt, sources, shortcuts)
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
