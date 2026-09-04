//! Everything the picker does that is not talking to a terminal.
//!
//! Query in, rows out, lines out. No I/O, so alignment, grouping, scoping and
//! cursor behaviour are assertions rather than screenshots.

use std::fmt;

use nucleo::pattern::{CaseMatching, Normalization, Pattern};
use nucleo::{Config, Matcher, Utf32String};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use unicode_segmentation::UnicodeSegmentation as _;

use crate::Theme;
use crate::columns;

use super::{Cell, Column, Command, Entry, Error, Group, Width};

/// Width of the pointer gutter, in cells. Every row reserves it so that a
/// column lands on the same screen position whether or not it is selected.
const GUTTER: usize = 2;
const POINTER: &str = "◆ ";
/// Rule cells before a section's name, so the label is not flush to the edge.
const LEAD: usize = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Accept,
    Abort,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Row {
    /// A group's divider. Renders, never matches, never selected.
    ///
    /// It carries its own hit count rather than looking one up, so the number
    /// on screen cannot disagree with the rows beneath it.
    Section {
        group: usize,
        count: usize,
    },
    Item(usize),
}

/// An entry's drawn and searchable form, computed once.
///
/// `cells` is what the eye sees, `haystack` is what the matcher sees, and
/// `bases` ties them together: for each searchable cell, where its text starts
/// in the haystack. All three come out of one pass, which is what stops a
/// highlight from landing one position off the text it belongs to.
#[derive(Debug)]
struct Prepared {
    group: usize,
    cells: Vec<Cell>,
    haystack: Utf32String,
    bases: Vec<Option<usize>>,
}

/// A group's drawing rules. Its key has done its job once entries are bucketed.
#[derive(Debug)]
struct Layout {
    label: &'static str,
    columns: Vec<Column>,
    scope: Option<char>,
}

pub struct Model<T> {
    items: Vec<T>,
    prepared: Vec<Prepared>,
    layouts: Vec<Layout>,
    buckets: Vec<Vec<usize>>,
    theme: Theme,
    matcher: Matcher,
    pattern: Pattern,
    query: String,
    /// In grapheme clusters, the unit the caret moves and deletes by. A caret
    /// counted in codepoints can stop between a base letter and its combining
    /// mark, and the next insert splits them.
    caret: usize,
    rows: Vec<Row>,
    /// Positions in `rows` the cursor may occupy, so movement is arithmetic
    /// and landing on a divider is not representable.
    selectable: Vec<usize>,
    cursor: usize,
    offset: usize,
    matched: usize,
    candidates: usize,
}

#[expect(
    clippy::missing_fields_in_debug,
    reason = "the prepared rows and the matcher are bulk scratch state, and printing them buries the four fields that describe what the list is doing"
)]
impl<T> fmt::Debug for Model<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Model")
            .field("items", &self.items.len())
            .field("query", &self.query)
            .field("rows", &self.rows.len())
            .field("cursor", &self.cursor)
            .field("matched", &self.matched)
            .finish()
    }
}

impl<T: Entry> Model<T> {
    pub(super) fn build(
        items: Vec<T>,
        groups: Vec<Group<T::Group>>,
        theme: Theme,
        match_paths: bool,
    ) -> Result<Self, Error> {
        if groups.is_empty() {
            return Err(Error::NoGroups);
        }
        for (index, group) in groups.iter().enumerate() {
            if groups
                .iter()
                .take(index)
                .any(|earlier| earlier.key == group.key)
            {
                return Err(Error::DuplicateGroup {
                    group: format!("{:?}", group.key),
                });
            }
        }

        let mut prepared = Vec::with_capacity(items.len());
        let mut buckets = vec![Vec::new(); groups.len()];
        for (index, item) in items.iter().enumerate() {
            let key = item.group();
            let group = groups
                .iter()
                .position(|declared| declared.key == key)
                .ok_or_else(|| Error::UndeclaredGroup {
                    group: format!("{key:?}"),
                })?;
            let declared = groups.get(group).map_or(0, |g| g.columns.len());
            let cells = item.cells(&theme);
            if cells.len() != declared {
                return Err(Error::Arity {
                    group: format!("{key:?}"),
                    declared,
                    drawn: cells.len(),
                });
            }
            let (haystack, bases) = weave(&cells, item.hidden_terms().as_deref());
            if let Some(bucket) = buckets.get_mut(group) {
                bucket.push(index);
            }
            prepared.push(Prepared {
                group,
                cells,
                haystack: Utf32String::from(haystack),
                bases,
            });
        }

        let mut config = Config::DEFAULT;
        if match_paths {
            config.set_match_paths();
        }
        let layouts = groups
            .into_iter()
            .map(|group| Layout {
                label: group.label,
                columns: group.columns,
                scope: group.scope,
            })
            .collect();

        let mut model = Self {
            items,
            prepared,
            layouts,
            buckets,
            theme,
            matcher: Matcher::new(config),
            pattern: Pattern::default(),
            query: String::new(),
            caret: 0,
            rows: Vec::new(),
            selectable: Vec::new(),
            cursor: 0,
            offset: 0,
            matched: 0,
            candidates: 0,
        };
        model.refresh();
        Ok(model)
    }
}

impl<T> Model<T> {
    pub fn theme(&self) -> Theme {
        self.theme
    }

    pub fn query(&self) -> &str {
        &self.query
    }

    /// Caret position in cells from the start of the query.
    pub fn caret_cells(&self) -> usize {
        columns::width(self.query.split_at(self.caret_byte()).0)
    }

    pub fn matched(&self) -> usize {
        self.matched
    }

    /// Entries the current query could match — those in groups the scope makes
    /// visible, not every entry the picker holds.
    pub fn candidates(&self) -> usize {
        self.candidates
    }

    pub fn selected(&self) -> Option<usize> {
        let row = *self.selectable.get(self.cursor)?;
        match self.rows.get(row)? {
            Row::Item(index) => Some(*index),
            Row::Section { .. } => None,
        }
    }

    /// Consumes the model and returns the chosen entry.
    pub fn take_selected(mut self) -> Option<T> {
        let index = self.selected()?;
        (index < self.items.len()).then(|| self.items.swap_remove(index))
    }

    /// Applies one key's worth of intent. `Some` ends the loop.
    pub fn apply(&mut self, command: Command) -> Option<Outcome> {
        match command {
            Command::Insert(ch) => self.insert(ch),
            Command::DeleteBack => self.delete_back(),
            Command::DeleteWord => self.delete_word(),
            Command::ClearQuery => self.clear_query(),
            Command::CaretHome => self.caret = 0,
            Command::CaretEnd => self.caret = columns::graphemes(&self.query),
            Command::CaretLeft => self.caret = self.caret.saturating_sub(1),
            Command::CaretRight => {
                self.caret = self
                    .caret
                    .saturating_add(1)
                    .min(columns::graphemes(&self.query));
            }
            Command::Next(by) => {
                let last = self.selectable.len().saturating_sub(1);
                self.cursor = self.cursor.saturating_add(by).min(last);
            }
            Command::Previous(by) => self.cursor = self.cursor.saturating_sub(by),
            Command::Accept => return Some(Outcome::Accept),
            Command::Abort => return Some(Outcome::Abort),
        }
        None
    }

    fn caret_byte(&self) -> usize {
        self.query
            .grapheme_indices(true)
            .nth(self.caret)
            .map_or(self.query.len(), |(byte, _)| byte)
    }

    /// A combining mark typed after its base joins the cluster before it
    /// rather than becoming one of its own, so the caret is re-derived from
    /// the text instead of incremented.
    fn insert(&mut self, ch: char) {
        let at = self.caret_byte();
        self.query.insert(at, ch);
        self.caret = columns::graphemes(self.query.split_at(at + ch.len_utf8()).0);
        self.refresh();
    }

    /// Inserts a bracketed paste as one edit and one matcher refresh.
    ///
    /// A query is one terminal line. Whitespace controls become an ordinary
    /// space and other controls are discarded, so pasted terminal sequences
    /// cannot affect the picker surface.
    pub(super) fn insert_text(&mut self, text: &str) {
        let pasted: String = text
            .chars()
            .filter_map(|ch| {
                if !ch.is_control() {
                    Some(ch)
                } else if ch.is_whitespace() {
                    Some(' ')
                } else {
                    None
                }
            })
            .collect();
        if pasted.is_empty() {
            return;
        }
        let at = self.caret_byte();
        self.query.insert_str(at, &pasted);
        self.caret = columns::graphemes(self.query.split_at(at + pasted.len()).0);
        self.refresh();
    }

    fn delete_back(&mut self) {
        if self.caret == 0 {
            return;
        }
        let to = self.caret_byte();
        self.caret -= 1;
        let from = self.caret_byte();
        self.query.replace_range(from..to, "");
        self.refresh();
    }

    fn delete_word(&mut self) {
        let to = self.caret_byte();
        let before: Vec<(usize, &str)> = self.query.split_at(to).0.grapheme_indices(true).collect();
        let blank = |(_, g): &(usize, &str)| g.chars().all(char::is_whitespace);
        let keep = before.len() - before.iter().rev().take_while(|g| blank(g)).count();
        let keep = keep
            - before
                .iter()
                .take(keep)
                .rev()
                .take_while(|g| !blank(g))
                .count();
        let from = before.get(keep).map_or(to, |(byte, _)| *byte);
        self.query.replace_range(from..to, "");
        self.caret = keep;
        self.refresh();
    }

    fn clear_query(&mut self) {
        self.query.clear();
        self.caret = 0;
        self.refresh();
    }

    /// Scrolls so the cursor is on screen. Must run before [`Self::lines`].
    pub fn scroll_into_view(&mut self, height: usize) {
        if height == 0 {
            self.offset = 0;
            return;
        }
        let Some(&row) = self.selectable.get(self.cursor) else {
            self.offset = 0;
            return;
        };
        if row < self.offset {
            self.offset = row;
        } else if row >= self.offset.saturating_add(height) {
            self.offset = row.saturating_add(1).saturating_sub(height);
        }
        // A group's first item says nothing about which group it is in, so pull
        // the divider that names it on screen when there is room for it.
        if self.offset > 0
            && matches!(self.rows.get(self.offset), Some(Row::Item(_)))
            && matches!(self.rows.get(self.offset - 1), Some(Row::Section { .. }))
            && row < self.offset.saturating_add(height).saturating_sub(1)
        {
            self.offset -= 1;
        }
        self.offset = self.offset.min(self.rows.len().saturating_sub(height));
    }

    /// The visible window, ready to draw.
    pub fn lines(&mut self, width: usize, height: usize) -> Vec<Line<'static>> {
        let selected = self.selectable.get(self.cursor).copied();
        let end = self.offset.saturating_add(height).min(self.rows.len());
        let window: Vec<(usize, Row)> = self
            .rows
            .get(self.offset..end)
            .unwrap_or_default()
            .iter()
            .enumerate()
            .map(|(n, row)| (self.offset + n, *row))
            .collect();
        window
            .into_iter()
            .map(|(index, row)| self.render_row(row, selected == Some(index), width))
            .collect()
    }

    fn render_row(&mut self, row: Row, selected: bool, width: usize) -> Line<'static> {
        match row {
            Row::Section { group, count } => self.section_line(group, count, width),
            Row::Item(item) => self.item_line(item, selected, width),
        }
    }

    fn columns_of(&self, group: usize) -> &[Column] {
        self.layouts
            .get(group)
            .map(|layout| layout.columns.as_slice())
            .unwrap_or_default()
    }

    /// A rule spanning the row, with the group's name and hit count set into
    /// it, starting at column zero rather than past the gutter.
    ///
    /// A rule rather than a row of column labels: groups here run one to three
    /// entries, so label rows outnumbered the entries they described, and a
    /// muted row of words costs a line while still reading as data. A rule
    /// reads as a boundary at a glance and the count is worth the space.
    fn section_line(&self, group: usize, count: usize, width: usize) -> Line<'static> {
        let rule = Style::default().fg(self.theme.muted);
        let label = self.layouts.get(group).map_or("", |layout| layout.label);

        let lead = LEAD.min(width);
        let mut spans = vec![Span::styled("─".repeat(lead), rule)];
        let mut drawn = lead;
        for (text, style) in [
            (format!(" {label} "), Style::default().fg(self.theme.accent)),
            (format!("({count}) "), rule),
        ] {
            let shown = columns::truncate(&text, width.saturating_sub(drawn)).into_owned();
            drawn += columns::width(&shown);
            spans.push(Span::styled(shown, style));
        }
        spans.push(Span::styled("─".repeat(width.saturating_sub(drawn)), rule));
        Line::from(spans).style(Style::default().bg(self.theme.background))
    }

    fn item_line(&mut self, item: usize, selected: bool, width: usize) -> Line<'static> {
        let Some(prepared) = self.prepared.get(item) else {
            return Line::default();
        };
        let mut indices: Vec<u32> = Vec::new();
        self.pattern
            .indices(prepared.haystack.slice(..), &mut self.matcher, &mut indices);
        indices.sort_unstable();
        indices.dedup();

        let gutter = GUTTER.min(width);
        let mut spans = vec![if selected {
            let pointer = columns::truncate(POINTER.trim_end(), gutter);
            let padding = gutter.saturating_sub(columns::width(&pointer));
            Span::styled(
                format!("{pointer}{}", " ".repeat(padding)),
                Style::default().fg(self.theme.accent),
            )
        } else {
            Span::raw(" ".repeat(gutter))
        }];
        let mut drawn = gutter;

        for ((cell, base), budget) in prepared
            .cells
            .iter()
            .zip(&prepared.bases)
            .zip(budgets(self.columns_of(prepared.group), width))
        {
            let shown = columns::truncate(&cell.text, budget.saturating_sub(1));
            paint(
                &mut spans,
                &shown,
                *base,
                &indices,
                cell.fg,
                self.theme.matched,
            );
            spans.push(Span::raw(
                " ".repeat(budget.saturating_sub(columns::width(&shown))),
            ));
            drawn += budget;
        }
        spans.push(Span::raw(" ".repeat(width.saturating_sub(drawn))));

        let background = if selected {
            self.theme.selection_bg
        } else {
            self.theme.background
        };
        Line::from(spans).style(Style::default().bg(background))
    }

    fn refresh(&mut self) {
        let previous = self.selected();
        let (scope, needle) = split_scope(&self.query, &self.layouts);
        self.pattern
            .reparse(needle, CaseMatching::Smart, Normalization::Smart);

        self.rows.clear();
        self.selectable.clear();
        self.matched = 0;
        self.candidates = 0;

        let mut hits: Vec<(u32, usize)> = Vec::new();
        for (index, layout) in self.layouts.iter().enumerate() {
            if layout.scope != scope {
                continue;
            }
            let bucket = self
                .buckets
                .get(index)
                .map(Vec::as_slice)
                .unwrap_or_default();
            self.candidates += bucket.len();
            hits.clear();
            for &item in bucket {
                let Some(prepared) = self.prepared.get(item) else {
                    continue;
                };
                if let Some(score) = self
                    .pattern
                    .score(prepared.haystack.slice(..), &mut self.matcher)
                {
                    hits.push((score, item));
                }
            }
            if hits.is_empty() {
                continue;
            }
            // Score descending, then index ascending: equal scores keep the
            // order the source produced them in.
            hits.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
            self.rows.push(Row::Section {
                group: index,
                count: hits.len(),
            });
            self.matched += hits.len();
            for &(_, item) in &hits {
                self.selectable.push(self.rows.len());
                self.rows.push(Row::Item(item));
            }
        }

        // Keeping the cursor on the same entry means narrowing a query does not
        // move the selection out from under a decision already made.
        self.cursor = previous
            .and_then(|item| {
                self.selectable
                    .iter()
                    .position(|&row| self.rows.get(row) == Some(&Row::Item(item)))
            })
            .unwrap_or(0);
        self.offset = 0;
    }
}

/// Builds the haystack and records where each searchable cell starts in it.
///
/// Offsets are in grapheme clusters, because that is the unit `nucleo` indexes
/// a non-ASCII haystack in: `Utf32String` collects one entry per cluster, so a
/// codepoint count drifts by one for every combining mark or emoji sequence
/// ahead of a match.
fn weave(cells: &[Cell], hidden: Option<&str>) -> (String, Vec<Option<usize>>) {
    let mut haystack = String::new();
    let mut bases = Vec::with_capacity(cells.len());
    let mut at = 0;
    for cell in cells {
        if !cell.searchable {
            bases.push(None);
            continue;
        }
        if !haystack.is_empty() {
            haystack.push(' ');
            at += 1;
        }
        bases.push(Some(at));
        at += columns::graphemes(&cell.text);
        haystack.push_str(&cell.text);
    }
    if let Some(extra) = hidden {
        if !haystack.is_empty() {
            haystack.push(' ');
        }
        haystack.push_str(extra);
    }
    (haystack, bases)
}

/// Cells each column of a row gets, given the row's total width.
///
/// Every row of a group comes through here with the same columns, so alignment
/// within the group is one calculation rather than a per-row decision that
/// could drift.
fn budgets(columns: &[Column], width: usize) -> Vec<usize> {
    let content = width.saturating_sub(GUTTER);
    let fixed = columns
        .iter()
        .fold(0usize, |total, column| total.saturating_add(column.fixed()));
    let fills = columns.iter().filter(|c| c.width == Width::Fill).count();
    let flexible = content.saturating_sub(fixed);
    let share = if fills == 0 { 0 } else { flexible / fills };

    let mut out = Vec::with_capacity(columns.len());
    let mut remaining = content;
    let mut fills_left = fills;
    for column in columns {
        let want = match column.width {
            Width::Cells(cells) => cells,
            Width::Fill => {
                fills_left -= 1;
                // The last fill absorbs the division remainder, so the row
                // always adds up to exactly the width asked for.
                if fills_left == 0 {
                    share + flexible % fills
                } else {
                    share
                }
            }
        };
        let give = want.min(remaining);
        remaining -= give;
        out.push(give);
    }
    out
}

/// Splits a leading scope sigil off the query. A sigil no group claims is an
/// ordinary character, so typing `/` in a picker with no scoped group searches
/// for a slash.
fn split_scope<'q>(query: &'q str, layouts: &[Layout]) -> (Option<char>, &'q str) {
    let mut chars = query.chars();
    if let Some(first) = chars.next()
        && layouts.iter().any(|layout| layout.scope == Some(first))
    {
        return (Some(first), chars.as_str());
    }
    (None, query)
}

/// Emits `text` as spans, colouring the graphemes the matcher hit.
fn paint(
    spans: &mut Vec<Span<'static>>,
    text: &str,
    base: Option<usize>,
    indices: &[u32],
    fg: Color,
    matched: Color,
) {
    let Some(base) = base.filter(|_| !indices.is_empty()) else {
        spans.push(Span::styled(text.to_owned(), Style::default().fg(fg)));
        return;
    };
    let mut run = String::new();
    let mut hot = false;
    for (offset, grapheme) in text.graphemes(true).enumerate() {
        let position = u32::try_from(base + offset).unwrap_or(u32::MAX);
        let is_match = indices.binary_search(&position).is_ok();
        if is_match != hot && !run.is_empty() {
            let colour = if hot { matched } else { fg };
            spans.push(Span::styled(
                std::mem::take(&mut run),
                Style::default().fg(colour),
            ));
        }
        hot = is_match;
        run.push_str(grapheme);
    }
    if !run.is_empty() {
        let colour = if hot { matched } else { fg };
        spans.push(Span::styled(run, Style::default().fg(colour)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::picker::Picker;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Kind {
        Fruit,
        Path,
    }

    #[derive(Debug, Clone)]
    struct Thing {
        kind: Kind,
        name: &'static str,
        note: &'static str,
    }

    impl Thing {
        fn fruit(name: &'static str, note: &'static str) -> Self {
            Self {
                kind: Kind::Fruit,
                name,
                note,
            }
        }

        fn path(name: &'static str) -> Self {
            Self {
                kind: Kind::Path,
                name,
                note: "",
            }
        }
    }

    impl Entry for Thing {
        type Group = Kind;

        fn group(&self) -> Kind {
            self.kind
        }

        fn cells(&self, _theme: &Theme) -> Vec<Cell> {
            let white = Color::White;
            match self.kind {
                Kind::Fruit => vec![
                    Cell::tag("fruit", white),
                    Cell::new(self.name, white),
                    Cell::new(self.note, white),
                ],
                Kind::Path => vec![Cell::tag("dir", white), Cell::new(self.name, white)],
            }
        }
    }

    fn fruit_group() -> Group<Kind> {
        Group::new(
            Kind::Fruit,
            "fruits",
            vec![Column::new(8), Column::new(12), Column::fill()],
        )
    }

    fn path_group() -> Group<Kind> {
        Group::new(Kind::Path, "paths", vec![Column::new(8), Column::fill()]).scoped('/')
    }

    fn typed(items: Vec<Thing>, groups: Vec<Group<Kind>>, query: &str) -> Model<Thing> {
        let mut model = Picker::new(items)
            .groups(groups)
            .theme(Theme::default())
            .build()
            .unwrap();
        for ch in query.chars() {
            model.apply(Command::Insert(ch));
        }
        model
    }

    fn model(query: &str) -> Model<Thing> {
        let items = vec![
            Thing::fruit("apple", "red"),
            Thing::fruit("apricot", "orange"),
            Thing::fruit("banana", "yellow"),
            Thing::path("/tmp/apricot"),
            Thing::path("/var/log"),
        ];
        typed(items, vec![fruit_group(), path_group()], query)
    }

    fn item_names(model: &Model<Thing>) -> Vec<&'static str> {
        model
            .rows
            .iter()
            .filter_map(|row| match row {
                Row::Item(index) => model.items.get(*index).map(|t| t.name),
                Row::Section { .. } => None,
            })
            .collect()
    }

    #[test]
    fn an_empty_query_shows_unscoped_groups_only() {
        let model = model("");
        assert_eq!(item_names(&model), ["apple", "apricot", "banana"]);
        assert_eq!(model.matched(), 3);
        assert_eq!(model.candidates(), 3, "the scoped group is not a candidate");
    }

    #[test]
    fn the_sigil_swaps_to_the_scoped_group_and_is_not_matched() {
        let model = model("/apri");
        assert_eq!(item_names(&model), ["/tmp/apricot"]);
        assert_eq!(model.candidates(), 2);
    }

    #[test]
    fn a_bare_sigil_shows_the_whole_scoped_group() {
        assert_eq!(item_names(&model("/")), ["/tmp/apricot", "/var/log"]);
    }

    #[test]
    fn every_group_gets_one_divider_and_only_when_it_has_hits() {
        let dividers = model("")
            .rows
            .iter()
            .filter(|row| matches!(row, Row::Section { .. }))
            .count();
        assert_eq!(dividers, 1);
    }

    /// The count is carried by the row rather than looked up, so the only way
    /// it can be wrong is if it disagrees with the items that follow it.
    #[test]
    fn a_divider_counts_exactly_the_items_under_it() {
        let model = model("ap");
        let mut expected = vec![];
        let mut under = 0;
        for row in &model.rows {
            match row {
                Row::Section { count, .. } => {
                    expected.push(*count);
                    under = 0;
                }
                Row::Item(_) => under += 1,
            }
        }
        assert_eq!(expected, [2], "apple and apricot");
        assert_eq!(under, 2);
    }

    #[test]
    fn a_divider_names_its_group_and_shows_the_count() {
        let mut model = model("");
        let drawn = model
            .render_row(Row::Section { group: 0, count: 3 }, false, 40)
            .to_string();
        assert!(drawn.starts_with("─── fruits (3) ─"), "{drawn:?}");
    }

    /// A group label is chrome. Typing it must not filter, or every row of the
    /// group would match a word that is not in any of them.
    #[test]
    fn a_group_label_is_not_matchable() {
        assert!(item_names(&model("fruits")).is_empty());
    }

    #[test]
    fn scores_order_within_a_group_not_across_the_whole_list() {
        assert_eq!(item_names(&model("ap")), ["apple", "apricot"]);
    }

    #[test]
    fn a_tag_cell_is_drawn_but_never_matched() {
        // Every fruit row draws the word "fruit", so a query for it would
        // match all three if tag cells were searchable.
        assert!(item_names(&model("fruit")).is_empty());
    }

    #[test]
    fn the_cursor_never_lands_on_a_header() {
        let mut model = model("");
        for command in [Command::Next(1), Command::Previous(1)] {
            for _ in 0..20 {
                model.apply(command);
                assert!(model.selected().is_some());
            }
        }
    }

    #[test]
    fn arbitrarily_large_navigation_saturates_at_the_last_item() {
        let mut model = model("");
        model.apply(Command::Next(usize::MAX));
        let selected = model.selected().and_then(|index| model.items.get(index));
        assert_eq!(selected.map(|item| item.name), Some("banana"));
    }

    #[test]
    fn narrowing_the_query_keeps_the_cursor_on_the_same_entry() {
        let mut model = model("");
        model.apply(Command::Next(1));
        let name = |m: &Model<Thing>| m.selected().and_then(|i| m.items.get(i)).map(|t| t.name);
        assert_eq!(name(&model), Some("apricot"));
        for ch in "apr".chars() {
            model.apply(Command::Insert(ch));
        }
        assert_eq!(name(&model), Some("apricot"));
    }

    /// Cell offset at which each run of visible text begins. Column alignment
    /// is exactly this being equal between a group's header and its rows.
    fn starts(line: &Line<'_>) -> Vec<usize> {
        let flat: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        let mut out = vec![];
        let mut at = 0;
        let mut blank = true;
        for grapheme in flat.graphemes(true) {
            if grapheme != " " && blank {
                out.push(at);
            }
            blank = grapheme == " ";
            at += columns::width(grapheme);
        }
        out
    }

    fn drawn_width(line: &Line<'_>) -> usize {
        line.spans.iter().map(|s| columns::width(&s.content)).sum()
    }

    #[test]
    fn every_row_of_a_group_lands_its_columns_on_the_same_cells() {
        let mut model = model("");
        let first = model.render_row(Row::Item(0), false, 60);
        assert_eq!(starts(&first), [2, 10, 22]);
        for item in 1..3 {
            let row = model.render_row(Row::Item(item), false, 60);
            assert_eq!(starts(&row), starts(&first), "item {item}");
        }
    }

    /// The divider is the one row that starts at column zero: it is a boundary
    /// across the whole list, not an entry in the gutter-indented column grid.
    #[test]
    fn a_divider_spans_the_row_from_column_zero() {
        let mut model = model("");
        let drawn = model.render_row(Row::Section { group: 0, count: 1 }, false, 60);
        assert_eq!(starts(&drawn).first(), Some(&0));
        assert_eq!(drawn_width(&drawn), 60);
    }

    #[test]
    fn column_offsets_are_the_same_whether_a_row_is_selected() {
        let mut model = model("");
        let plain = model.render_row(Row::Item(0), false, 60);
        let picked = model.render_row(Row::Item(0), true, 60);
        // The pointer occupies the gutter only when selected; everything past
        // it must not move, which is the reason the gutter is always reserved.
        let past_gutter = |line: &Line<'_>| {
            starts(line)
                .into_iter()
                .filter(|at| *at >= GUTTER)
                .collect::<Vec<_>>()
        };
        assert_eq!(past_gutter(&plain), past_gutter(&picked));
    }

    #[test]
    fn every_rendered_row_is_exactly_the_requested_width() {
        let mut model = model("");
        for width in [0, 1, 2, 4, 20, 40, 60, 120] {
            for row in model.rows.clone() {
                assert_eq!(
                    drawn_width(&model.render_row(row, false, width)),
                    width,
                    "{row:?} at width {width}"
                );
            }
        }
    }

    #[test]
    fn a_selected_row_is_still_exactly_the_requested_width() {
        let mut model = model("");
        for width in [0, 1, 2, 40] {
            assert_eq!(
                drawn_width(&model.render_row(Row::Item(0), true, width)),
                width
            );
        }
    }

    #[test]
    fn rows_explicitly_paint_the_theme_background() {
        let mut model = model("");
        let plain = model.render_row(Row::Item(0), false, 40);
        let selected = model.render_row(Row::Item(0), true, 40);
        let section = model.render_row(Row::Section { group: 0, count: 3 }, false, 40);
        assert_eq!(plain.style.bg, Some(model.theme.background));
        assert_eq!(selected.style.bg, Some(model.theme.selection_bg));
        assert_eq!(section.style.bg, Some(model.theme.background));
    }

    fn highlighted(model: &mut Model<Thing>) -> String {
        model
            .render_row(Row::Item(0), false, 60)
            .spans
            .iter()
            .filter(|span| span.style.fg == Some(Theme::default().matched))
            .map(|span| span.content.to_string())
            .collect()
    }

    #[test]
    fn matched_characters_are_painted_and_unmatched_ones_are_not() {
        assert_eq!(highlighted(&mut model("appl")), "appl");
    }

    /// `nucleo` indexes a non-ASCII haystack per grapheme cluster. Counting
    /// codepoints instead shifts the highlight one place left for every extra
    /// codepoint ahead of the match.
    #[test]
    fn a_combining_mark_before_the_match_does_not_shift_the_highlight() {
        let items = vec![Thing::fruit("cafe\u{301}", "red")];
        let mut model = typed(items, vec![fruit_group()], "red");
        assert_eq!(highlighted(&mut model), "red");
    }

    #[test]
    fn an_emoji_sequence_before_the_match_does_not_shift_the_highlight() {
        let items = vec![Thing::fruit(
            "\u{1f468}\u{200d}\u{1f469}\u{200d}\u{1f467} x",
            "main",
        )];
        let mut model = typed(items, vec![fruit_group()], "main");
        assert_eq!(highlighted(&mut model), "main");
    }

    #[test]
    fn an_empty_leading_searchable_cell_does_not_shift_the_highlight() {
        let items = vec![Thing::fruit("", "plum")];
        let mut model = typed(items, vec![fruit_group()], "plum");
        assert_eq!(highlighted(&mut model), "plum");
    }

    #[test]
    fn a_hidden_term_matches_without_being_drawn() {
        #[derive(Debug)]
        struct Hidden;
        impl Entry for Hidden {
            type Group = ();
            fn group(&self) {}
            fn cells(&self, _theme: &Theme) -> Vec<Cell> {
                vec![Cell::new("visible", Color::White)]
            }
            fn hidden_terms(&self) -> Option<std::borrow::Cow<'_, str>> {
                Some("w2:p1".into())
            }
        }
        let mut model = Picker::new(vec![Hidden])
            .group(Group::new((), "things", vec![Column::fill()]))
            .build()
            .unwrap();
        for ch in "w2:p1".chars() {
            model.apply(Command::Insert(ch));
        }
        assert_eq!(model.matched(), 1);
        let drawn = model.render_row(Row::Item(0), false, 20).to_string();
        assert!(!drawn.contains("w2"), "{drawn:?}");
    }

    #[test]
    fn deleting_a_word_stops_at_the_previous_boundary() {
        let mut model = model("one two");
        model.apply(Command::DeleteWord);
        assert_eq!(model.query(), "one ");
        model.apply(Command::DeleteWord);
        assert_eq!(model.query(), "");
    }

    #[test]
    fn editing_in_the_middle_of_a_query_inserts_at_the_caret() {
        let mut model = model("ac");
        model.apply(Command::CaretLeft);
        model.apply(Command::Insert('b'));
        assert_eq!(model.query(), "abc");
        assert_eq!(model.caret_cells(), 2);
    }

    #[test]
    fn a_paste_is_inserted_once_and_cannot_inject_terminal_controls() {
        let mut model = model("ac");
        model.apply(Command::CaretLeft);
        model.insert_text("b\nwide 一\t\u{1b}[31m");
        assert_eq!(model.query(), "ab wide 一 [31mc");
        assert_eq!(model.caret_cells(), columns::width("ab wide 一 [31m"));
    }

    #[test]
    fn a_multibyte_query_edits_on_character_boundaries() {
        let mut model = model("日本語");
        assert_eq!(model.caret_cells(), 6, "three wide characters");
        model.apply(Command::CaretLeft);
        model.apply(Command::Insert('x'));
        assert_eq!(model.query(), "日本x語");
        model.apply(Command::DeleteBack);
        assert_eq!(model.query(), "日本語");
        model.apply(Command::CaretHome);
        model.apply(Command::DeleteBack);
        assert_eq!(
            model.query(),
            "日本語",
            "nothing to delete before the start"
        );
    }

    /// Typing `e` then a combining acute produces one cluster. A caret counted
    /// in codepoints would then sit *inside* it, and the next letter typed
    /// would land between the base and its mark.
    #[test]
    fn the_caret_never_stops_inside_a_grapheme_cluster() {
        let mut model = model("cafe\u{301}");
        assert_eq!(model.caret_cells(), 4);
        model.apply(Command::CaretLeft);
        model.apply(Command::Insert('x'));
        assert_eq!(model.query(), "cafxe\u{301}", "the cluster stayed whole");
        model.apply(Command::CaretEnd);
        model.apply(Command::DeleteBack);
        assert_eq!(
            model.query(),
            "cafx",
            "one deletion removes the whole cluster"
        );
    }

    #[test]
    fn deleting_a_word_counts_a_cluster_as_one_unit() {
        let mut model = model("a cafe\u{301} b");
        model.apply(Command::CaretLeft);
        model.apply(Command::CaretLeft);
        model.apply(Command::DeleteWord);
        assert_eq!(model.query(), "a  b");
        assert_eq!(model.caret_cells(), 2);
    }

    #[test]
    fn scrolling_keeps_the_cursor_in_the_window() {
        let mut model = model("");
        model.apply(Command::Next(10));
        model.scroll_into_view(2);
        assert_eq!(model.lines(40, 2).len(), 2);
    }

    #[test]
    fn a_query_matching_nothing_leaves_no_rows_and_no_selection() {
        let model = model("zzzzzz");
        assert!(model.rows.is_empty());
        assert_eq!(model.selected(), None);
        assert_eq!(model.matched(), 0);
    }

    #[test]
    fn a_fixed_column_after_a_fill_keeps_its_cells() {
        let columns = [Column::new(8), Column::fill(), Column::new(2)];
        let given = budgets(&columns, 40);
        assert_eq!(given, [8, 28, 2]);
        assert_eq!(given.iter().sum::<usize>(), 40 - GUTTER);
    }

    #[test]
    fn two_fills_split_the_remainder_without_losing_a_cell() {
        let given = budgets(&[Column::fill(), Column::fill()], 41);
        assert_eq!(given.iter().sum::<usize>(), 41 - GUTTER);
    }

    #[test]
    fn columns_wider_than_the_terminal_are_cut_off_rather_than_overflowing() {
        let columns = [Column::new(20), Column::new(20), Column::fill()];
        let given = budgets(&columns, 24);
        assert_eq!(given, [20, 2, 0]);
        assert_eq!(given.iter().sum::<usize>(), 24 - GUTTER);
    }

    #[test]
    fn impossible_fixed_width_totals_saturate_instead_of_overflowing() {
        let columns = [Column::new(usize::MAX), Column::new(usize::MAX)];
        assert_eq!(budgets(&columns, 12), [10, 0]);
    }

    #[test]
    fn a_picker_with_no_groups_is_rejected() {
        let picker: Picker<Thing> = Picker::new(vec![Thing::fruit("apple", "red")]);
        assert!(matches!(picker.build(), Err(Error::NoGroups)));
    }

    #[test]
    fn an_entry_in_an_undeclared_group_is_rejected_rather_than_dropped() {
        let result = Picker::new(vec![Thing::path("/tmp")])
            .group(fruit_group())
            .build();
        assert!(
            matches!(result, Err(Error::UndeclaredGroup { .. })),
            "{result:?}"
        );
    }

    #[test]
    fn a_group_declared_twice_is_rejected() {
        let result = Picker::new(Vec::<Thing>::new())
            .groups([fruit_group(), fruit_group()])
            .build();
        assert!(
            matches!(result, Err(Error::DuplicateGroup { .. })),
            "{result:?}"
        );
    }

    #[test]
    fn cells_that_disagree_with_the_columns_are_rejected() {
        let result = Picker::new(vec![Thing::fruit("apple", "red")])
            .group(Group::new(Kind::Fruit, "fruits", vec![Column::fill()]))
            .build();
        assert!(
            matches!(
                result,
                Err(Error::Arity {
                    declared: 1,
                    drawn: 3,
                    ..
                })
            ),
            "{result:?}"
        );
    }
}
