//! Groups of rows and the columns they lay out.

use crate::text;

/// How wide a column is. Columns line up by position across groups: every
/// group's fixed column at one position gets the same cells, the widest any
/// of them asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Width {
    /// At least this many cells, the gap to the next column included.
    Cells(usize),
    /// Fit the widest content and heading at this position, in any group, up
    /// to this cell budget.
    Content(usize),
    /// Shares whatever the fixed columns leave, wherever it sits in the row —
    /// so a group can put a narrow marker column after its widest one.
    Fill,
}

/// A column with an optional heading, aligned using the same width as its cells.
#[derive(Debug, Clone)]
#[must_use]
pub struct Column {
    pub(crate) width: Width,
    pub(crate) heading: String,
}

impl Column {
    pub fn new(cells: usize) -> Self {
        Self {
            width: Width::Cells(cells),
            heading: String::new(),
        }
    }

    pub fn fill() -> Self {
        Self {
            width: Width::Fill,
            ..Self::new(0)
        }
    }

    /// Measure once across all entries, keeping columns stable while filtering.
    pub fn content(max: usize) -> Self {
        Self {
            width: Width::Content(max),
            ..Self::new(0)
        }
    }

    pub fn heading(mut self, text: impl Into<String>) -> Self {
        self.heading = text::display(text.into());
        self
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
/// searchable puts it in [`crate::Entry::hidden_terms`].
#[derive(Debug, Clone)]
#[must_use]
pub struct Group<G> {
    pub(crate) key: G,
    pub(crate) label: String,
    pub(crate) columns: Vec<Column>,
    pub(crate) scope: Option<char>,
}

impl<G> Group<G> {
    pub fn new(key: G, label: impl Into<String>, columns: Vec<Column>) -> Self {
        Self {
            key,
            label: text::display(label.into()),
            columns,
            scope: None,
        }
    }

    /// Hides this group until the query starts with `sigil`, and shows only
    /// scoped groups once it does. The sigil is stripped before matching, so
    /// it cannot skew results. Each scoped group needs its own sigil.
    ///
    /// This is how a large, low-value source stays out of the way: hundreds of
    /// directories against a handful of open workspaces would otherwise be the
    /// whole list. The default view stays the rows that describe live state.
    pub fn scoped(mut self, sigil: char) -> Self {
        self.scope = Some(sigil);
        self
    }
}
