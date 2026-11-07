//! Everything the picker does that is not talking to a terminal.
//!
//! Query in, rows out, lines out. No I/O, so alignment, grouping, scoping and
//! cursor behaviour are assertions rather than screenshots.

use std::borrow::Cow;
use std::collections::HashSet;
use std::fmt;

use nucleo::pattern::{CaseMatching, Normalization, Pattern};
use nucleo::{Config, Matcher, Utf32String};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use unicode_segmentation::UnicodeSegmentation as _;

use crate::{Cell, Column, Command, Entry, Error, Group, Palette, Width, columns, text};

/// Width of the pointer gutter, in cells. Every row reserves it so that a
/// column lands on the same screen position whether or not it is selected.
const GUTTER: usize = 2;
pub(crate) const COLUMN_GAP: usize = 2;
const POINTER: &str = "▎ ";
/// Cells each fill column keeps before content columns stop yielding to it.
const MIN_FILL: usize = 20;
/// Rule cells before a section's name, so the label is not flush to the edge.
const LEAD: usize = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Accept,
    Abort,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Row {
    /// A group's divider. Renders, never matches, never selected.
    ///
    /// It carries its own hit count rather than looking one up, so the number
    /// on screen cannot disagree with the rows beneath it.
    Section {
        group: usize,
        count: usize,
    },
    Headings(usize),
    Item(usize),
    Detail {
        item: usize,
        line: usize,
    },
}

/// An entry's drawn and searchable form, computed once.
///
/// `cells` is what the eye sees, `haystack` is what the matcher sees, and
/// `bases` ties them together: for each searchable cell, where its text starts
/// in the haystack. All three come out of one pass, which is what stops a
/// highlight from landing one position off the text it belongs to.
#[derive(Debug, PartialEq, Eq)]
struct Prepared {
    group: usize,
    id: Option<String>,
    cells: Vec<Cell>,
    details: Vec<Cell>,
    haystack: Utf32String,
    bases: Vec<Option<usize>>,
}

/// A group's drawing rules. Its key has done its job once entries are bucketed.
#[derive(Debug)]
struct Layout {
    label: String,
    columns: Vec<Column>,
    scope: Option<char>,
}

pub struct Model<T: Entry> {
    group_keys: Vec<T::Group>,
    items: Vec<T>,
    prepared: Vec<Prepared>,
    layouts: Vec<Layout>,
    grid: Grid,
    buckets: Vec<Vec<usize>>,
    palette: Palette,
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
    group_filter: Option<usize>,
}

#[expect(
    clippy::missing_fields_in_debug,
    reason = "the prepared rows and the matcher are bulk scratch state, and printing them buries the four fields that describe what the list is doing"
)]
impl<T: Entry> fmt::Debug for Model<T> {
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
    pub(crate) fn build(
        items: Vec<T>,
        groups: Vec<Group<T::Group>>,
        palette: Palette,
        match_paths: bool,
    ) -> Result<Self, Error> {
        validate_groups(&groups)?;

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
            let declared = groups[group].columns.len();
            let cells = item.cells(&palette);
            if cells.len() != declared {
                return Err(Error::Arity {
                    group: format!("{key:?}"),
                    declared,
                    drawn: cells.len(),
                });
            }
            let details = item.details(&palette);
            let (haystack, bases) =
                weave(cells.iter().chain(&details), item.hidden_terms().as_deref());
            buckets[group].push(index);
            prepared.push(Prepared {
                group,
                id: item.identity().map(Cow::into_owned),
                cells,
                details,
                haystack: Utf32String::from(haystack),
                bases,
            });
        }

        let mut config = Config::DEFAULT;
        if match_paths {
            config.set_match_paths();
        }
        let group_keys = groups.iter().map(|group| group.key).collect();
        let layouts: Vec<Layout> = groups
            .into_iter()
            .map(|group| Layout {
                label: group.label,
                columns: group.columns,
                scope: group.scope,
            })
            .collect();
        let grid = Grid::measure(&layouts, &prepared);

        let mut model = Self {
            group_keys,
            items,
            prepared,
            layouts,
            grid,
            buckets,
            palette,
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
            group_filter: None,
        };
        model.refresh();
        Ok(model)
    }
}

impl<T: Entry> Model<T> {
    fn group_index(&self, group: T::Group) -> Result<usize, Error> {
        self.group_keys
            .iter()
            .position(|key| *key == group)
            .ok_or_else(|| Error::UndeclaredGroup {
                group: format!("{group:?}"),
            })
    }

    /// Replaces every entry with `items`, as one atomic batch. Every entry
    /// needs an identity; the selection stays on the entry with the selected
    /// identity wherever it moved, including into another group. Returns whether
    /// presentation or preview commands changed; entry payloads always update.
    pub fn replace_all(&mut self, items: Vec<T>) -> Result<bool, Error> {
        let mut identities = HashSet::with_capacity(items.len());
        let mut replacement = Vec::with_capacity(items.len());
        for item in items {
            let index = self.group_index(item.group())?;
            replacement.push(self.prepare_entry(index, item, &mut identities)?);
        }
        let changed = self
            .prepared
            .iter()
            .ne(replacement.iter().map(|(_, prepared)| prepared))
            || self
                .items
                .iter()
                .map(Entry::preview)
                .ne(replacement.iter().map(|(entry, _)| entry.preview()));
        self.rebuild(replacement);
        Ok(changed)
    }

    /// Checks one replacement entry and lays out its cells.
    fn prepare_entry(
        &self,
        index: usize,
        entry: T,
        identities: &mut HashSet<String>,
    ) -> Result<(T, Prepared), Error> {
        let id = entry.identity().map(Cow::into_owned).unwrap_or_default();
        if id.trim().is_empty() || !identities.insert(id.clone()) {
            return Err(Error::InvalidIdentity { id });
        }
        let declared = self.columns_of(index).len();
        let cells = entry.cells(&self.palette);
        if cells.len() != declared {
            return Err(Error::Arity {
                group: format!("{:?}", entry.group()),
                declared,
                drawn: cells.len(),
            });
        }
        let details = entry.details(&self.palette);
        let (haystack, bases) = weave(
            cells.iter().chain(&details),
            entry.hidden_terms().as_deref(),
        );
        Ok((
            entry,
            Prepared {
                group: index,
                id: Some(id),
                cells,
                details,
                haystack: Utf32String::from(haystack),
                bases,
            },
        ))
    }

    /// Swaps in a complete validated batch, following the selected identity.
    fn rebuild(&mut self, replacement: Vec<(T, Prepared)>) {
        let selected_id = self
            .selected()
            .and_then(|item| self.prepared[item].id.as_ref());
        let preserved = selected_id.and_then(|id| {
            replacement
                .iter()
                .position(|(_, prepared)| prepared.id.as_ref() == Some(id))
        });
        (self.items, self.prepared) = replacement.into_iter().unzip();
        for bucket in &mut self.buckets {
            bucket.clear();
        }
        for (item, prepared) in self.prepared.iter().enumerate() {
            self.buckets[prepared.group].push(item);
        }
        self.grid = Grid::measure(&self.layouts, &self.prepared);
        self.refresh();
        self.cursor = preserved
            .and_then(|item| {
                self.selectable
                    .iter()
                    .position(|&row| self.rows[row] == Row::Item(item))
            })
            .unwrap_or(0);
    }

    #[must_use]
    pub fn palette(&self) -> Palette {
        self.palette
    }

    #[must_use]
    pub fn query(&self) -> &str {
        &self.query
    }

    /// Caret position in cells from the start of the query.
    #[must_use]
    pub fn caret_cells(&self) -> usize {
        columns::width(self.query.split_at(self.caret_byte()).0)
    }

    #[must_use]
    pub fn matched(&self) -> usize {
        self.matched
    }

    /// Entries the current query could match — those in groups the scope makes
    /// visible, not every entry the picker holds.
    #[must_use]
    pub fn candidates(&self) -> usize {
        self.candidates
    }

    /// The index of the entry under the cursor; `None` when nothing matches.
    #[must_use]
    pub fn selected(&self) -> Option<usize> {
        let row = *self.selectable.get(self.cursor)?;
        match self.rows[row] {
            Row::Item(index) => Some(index),
            Row::Section { .. } | Row::Headings(_) | Row::Detail { .. } => {
                unreachable!("only item rows are selectable")
            }
        }
    }

    pub(crate) fn select_line(&mut self, line: usize) {
        let row = self.offset.saturating_add(line);
        let row = match self.rows.get(row) {
            Some(Row::Detail { item, .. }) => self
                .rows
                .iter()
                .position(|candidate| *candidate == Row::Item(*item))
                .expect("detail belongs to a primary row"),
            _ => row,
        };
        if let Some(cursor) = self
            .selectable
            .iter()
            .position(|&candidate| candidate == row)
        {
            self.cursor = cursor;
        }
    }

    /// Selects an exact identity, revealing its group if that group is scoped.
    pub fn select_identity(&mut self, id: &str) -> Result<(), Error> {
        let index = self
            .prepared
            .iter()
            .position(|row| row.id.as_deref() == Some(id))
            .ok_or_else(|| Error::MissingSelection { id: id.to_owned() })?;
        if self.layouts[self.prepared[index].group].scope.is_some() {
            self.group_filter = Some(self.prepared[index].group);
            self.refresh();
        }
        let cursor = self
            .selectable
            .iter()
            .position(|&row| self.rows[row] == Row::Item(index))
            .ok_or_else(|| Error::MissingSelection { id: id.to_owned() })?;
        self.cursor = cursor;
        Ok(())
    }

    /// The entry under the cursor.
    #[must_use]
    pub fn selected_entry(&self) -> Option<&T> {
        Some(&self.items[self.selected()?])
    }

    /// Whether any entry has a preview, so a pane is worth drawing.
    pub fn has_previews(&self) -> bool {
        self.items.iter().any(|item| item.preview().is_some())
    }

    /// Consumes the model and returns the chosen entry.
    pub fn take_selected(mut self) -> Option<T> {
        let index = self.selected()?;
        Some(self.items.swap_remove(index))
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
            Command::NextScope => self.cycle_scope(false),
            Command::PreviousScope => self.cycle_scope(true),
            Command::Accept => {
                return self.selected().is_some().then_some(Outcome::Accept);
            }
            Command::Abort => return Some(Outcome::Abort),
        }
        None
    }

    /// Query sigils take precedence over the selected group filter.
    #[must_use]
    pub fn scope_label(&self) -> &str {
        let (scope, _) = split_scope(&self.query, &self.layouts);
        let shown = match scope {
            Some(sigil) => Some(
                self.layouts
                    .iter()
                    .position(|layout| layout.scope == Some(sigil))
                    .expect("a split sigil belongs to a layout"),
            ),
            None => self.group_filter,
        };
        shown.map_or("all", |index| self.layouts[index].label.as_str())
    }

    /// Each scoped group's sigil and label, for the help line.
    pub(crate) fn scope_help(&self) -> Vec<(String, String)> {
        self.layouts
            .iter()
            .filter_map(|layout| {
                layout
                    .scope
                    .map(|sigil| (sigil.to_string(), layout.label.clone()))
            })
            .collect()
    }

    fn cycle_scope(&mut self, backwards: bool) {
        let (sigil, needle) = split_scope(&self.query, &self.layouts);
        let current = sigil
            .and_then(|sigil| {
                self.layouts
                    .iter()
                    .position(|layout| layout.scope == Some(sigil))
            })
            .or(self.group_filter);
        let needle = needle.to_owned();
        if sigil.is_some() {
            self.query = needle;
            self.caret = self.caret.saturating_sub(1);
        }
        // Slot zero is the combined view; slot n + 1 shows layout n.
        let slots = self.layouts.len() + 1;
        let slot = current.map_or(0, |index| index + 1);
        let next = if backwards {
            (slot + slots - 1) % slots
        } else {
            (slot + 1) % slots
        };
        self.group_filter = next.checked_sub(1);
        self.offset = 0;
        self.refresh();
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
    ///
    /// A typed control follows the same rule as a pasted one; see
    /// [`text::input`].
    fn insert(&mut self, ch: char) {
        let Some(ch) = text::input(ch) else { return };
        let at = self.caret_byte();
        self.query.insert(at, ch);
        self.caret = columns::graphemes(self.query.split_at(at + ch.len_utf8()).0);
        self.refresh();
    }

    /// Inserts a bracketed paste as one edit and one matcher refresh.
    ///
    /// A query is one terminal line, so pasted terminal sequences cannot
    /// affect the picker surface; see [`text::input`].
    pub(crate) fn insert_text(&mut self, pasted: &str) {
        let pasted: String = pasted.chars().filter_map(text::input).collect();
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
        let item = self.selected().expect("the selected row exists");
        let last = row.saturating_add(self.prepared[item].details.len());
        if self.prepared[item].details.len() < height && last >= self.offset.saturating_add(height)
        {
            self.offset = last.saturating_add(1).saturating_sub(height);
        }
        // A group's first item says nothing about which group it is in, so pull
        // the divider that names it on screen when there is room for it.
        while self.offset > 0
            && matches!(
                self.rows[self.offset - 1],
                Row::Section { .. } | Row::Headings(_)
            )
            && last < self.offset.saturating_add(height).saturating_sub(1)
        {
            self.offset -= 1;
        }
        self.offset = self.offset.min(self.rows.len().saturating_sub(height));
    }

    /// The visible window, ready to draw.
    pub fn lines(&mut self, width: usize, height: usize) -> Vec<Line<'static>> {
        let selected = self.selectable.get(self.cursor).copied();
        let selected_item = self.selected();
        let end = self.offset.saturating_add(height).min(self.rows.len());
        // Scrolling and refreshing keep `offset` within the rows.
        let window: Vec<(usize, Row)> = self.rows[self.offset..end]
            .iter()
            .enumerate()
            .map(|(n, row)| (self.offset + n, *row))
            .collect();
        let sizes = self.grid.sizes(&self.layouts, width);
        window
            .into_iter()
            .map(|(index, row)| {
                let selected = selected == Some(index)
                    || matches!(row, Row::Detail { item, .. } if selected_item == Some(item));
                self.draw_row(row, selected, width, &sizes)
            })
            .collect()
    }

    #[cfg(test)]
    fn render_row(&mut self, row: Row, selected: bool, width: usize) -> Line<'static> {
        let sizes = self.grid.sizes(&self.layouts, width);
        self.draw_row(row, selected, width, &sizes)
    }

    /// One row of the list, its columns `sizes` wide (see [`Grid::sizes`]).
    fn draw_row(
        &mut self,
        row: Row,
        selected: bool,
        width: usize,
        sizes: &[usize],
    ) -> Line<'static> {
        match row {
            Row::Section { group, count } => self.section_line(group, count, width),
            Row::Headings(group) => self.heading_line(group, width, sizes),
            Row::Item(item) => self.item_line(item, selected, width, sizes),
            Row::Detail { item, line } => self.detail_line(item, line, selected, width),
        }
    }

    fn columns_of(&self, group: usize) -> &[Column] {
        &self.layouts[group].columns
    }

    /// A muted divider with the group name in bold and its matching count.
    fn section_line(&self, group: usize, count: usize, width: usize) -> Line<'static> {
        let rule = Style::default()
            .fg(self.palette.muted)
            .add_modifier(Modifier::DIM);
        let title = Style::default()
            .fg(self.palette.strong)
            .add_modifier(Modifier::BOLD);
        let count_style = Style::default().fg(self.palette.muted);
        let label = &self.layouts[group].label;

        let lead = LEAD.min(width);
        let mut spans = vec![Span::styled("─".repeat(lead), rule)];
        let mut drawn = lead;
        for (text, style) in [
            (format!(" {label} "), title),
            (format!("({count}) "), count_style),
        ] {
            let shown = columns::truncate(&text, width.saturating_sub(drawn)).into_owned();
            drawn += columns::width(&shown);
            spans.push(Span::styled(shown, style));
        }
        spans.push(Span::styled("─".repeat(width.saturating_sub(drawn)), rule));
        Line::from(spans).style(Style::default().bg(self.palette.background))
    }

    fn heading_line(&self, group: usize, width: usize, sizes: &[usize]) -> Line<'static> {
        let mut text = " ".repeat(GUTTER.min(width));
        let columns = self.columns_of(group);
        for (column, budget) in columns.iter().zip(budgets(columns, sizes, width)) {
            let shown = columns::truncate(&column.heading, budget.saturating_sub(COLUMN_GAP));
            text.push_str(&shown);
            text.push_str(&" ".repeat(budget.saturating_sub(columns::width(&shown))));
        }
        text.push_str(&" ".repeat(width.saturating_sub(columns::width(&text))));
        Line::styled(
            text,
            Style::default()
                .fg(self.palette.muted)
                .bg(self.palette.background),
        )
    }

    fn item_line(
        &mut self,
        item: usize,
        selected: bool,
        width: usize,
        sizes: &[usize],
    ) -> Line<'static> {
        let prepared = &self.prepared[item];
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
                Style::default().fg(self.palette.accent),
            )
        } else {
            Span::raw(" ".repeat(gutter))
        }];
        let mut drawn = gutter;

        for ((cell, base), budget) in prepared.cells.iter().zip(&prepared.bases).zip(budgets(
            self.columns_of(prepared.group),
            sizes,
            width,
        )) {
            let used = paint_cell(
                &mut spans,
                cell,
                budget.saturating_sub(COLUMN_GAP),
                *base,
                &indices,
                &self.palette,
            );
            spans.push(Span::raw(" ".repeat(budget.saturating_sub(used))));
            drawn += budget;
        }
        spans.push(Span::raw(" ".repeat(width.saturating_sub(drawn))));

        let background = if selected {
            self.palette.selection_bg
        } else {
            self.palette.background
        };
        Line::from(spans).style(Style::default().bg(background))
    }

    fn detail_line(
        &mut self,
        item: usize,
        line: usize,
        selected: bool,
        width: usize,
    ) -> Line<'static> {
        let prepared = &self.prepared[item];
        let mut indices = Vec::new();
        self.pattern
            .indices(prepared.haystack.slice(..), &mut self.matcher, &mut indices);
        indices.sort_unstable();
        indices.dedup();
        let gutter = GUTTER.min(width);
        let mut spans = vec![Span::raw(" ".repeat(gutter))];
        let used = paint_cell(
            &mut spans,
            &prepared.details[line],
            width - gutter,
            prepared.bases[prepared.cells.len() + line],
            &indices,
            &self.palette,
        );
        spans.push(Span::raw(" ".repeat(width.saturating_sub(gutter + used))));
        Line::from(spans).style(Style::default().bg(if selected {
            self.palette.selection_bg
        } else {
            self.palette.background
        }))
    }

    /// Number of entries in a page when entries have secondary lines.
    #[must_use]
    pub fn page_size(&self, height: usize) -> usize {
        if self.prepared.iter().all(|entry| entry.details.is_empty()) {
            return height;
        }
        let Some(&row) = self.selectable.get(self.cursor) else {
            return 1;
        };
        self.selectable
            .iter()
            .skip(self.cursor)
            .take_while(|&&next| next < row.saturating_add(height))
            .count()
            .max(1)
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
            let visible = if scope.is_some() {
                layout.scope == scope
            } else if let Some(group) = self.group_filter {
                index == group
            } else {
                layout.scope.is_none()
            };
            if !visible {
                continue;
            }
            let bucket = &self.buckets[index];
            self.candidates += bucket.len();
            hits.clear();
            for &item in bucket {
                let prepared = &self.prepared[item];
                let rank = self
                    .pattern
                    .score(prepared.haystack.slice(..), &mut self.matcher);
                if let Some(score) = rank {
                    hits.push((score, item));
                }
            }
            if hits.is_empty() {
                continue;
            }
            // Score descending, then index ascending: equal scores keep row order.
            hits.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
            self.rows.push(Row::Section {
                group: index,
                count: hits.len(),
            });
            if layout
                .columns
                .iter()
                .any(|column| !column.heading.is_empty())
            {
                self.rows.push(Row::Headings(index));
            }
            self.matched += hits.len();
            for &(_, item) in &hits {
                self.selectable.push(self.rows.len());
                self.rows.push(Row::Item(item));
                self.rows.extend(
                    (0..self.prepared[item].details.len()).map(|line| Row::Detail { item, line }),
                );
            }
        }

        // Keeping the cursor on the same entry means narrowing a query does not
        // move the selection out from under a decision already made.
        self.cursor = previous
            .and_then(|item| {
                self.selectable
                    .iter()
                    .position(|&row| self.rows[row] == Row::Item(item))
            })
            .unwrap_or(0);
        self.offset = 0;
    }
}

fn validate_groups<G: Copy + Eq + fmt::Debug>(groups: &[Group<G>]) -> Result<(), Error> {
    if groups.is_empty() {
        return Err(Error::NoGroups);
    }
    for (index, group) in groups.iter().enumerate() {
        let earlier = &groups[..index];
        if let Some(sigil) = group.scope
            && (sigil.is_control()
                || sigil.is_whitespace()
                || earlier.iter().any(|earlier| earlier.scope == Some(sigil)))
        {
            return Err(Error::InvalidScope { sigil });
        }
        if earlier.iter().any(|earlier| earlier.key == group.key) {
            return Err(Error::DuplicateGroup {
                group: format!("{:?}", group.key),
            });
        }
    }
    Ok(())
}

/// Builds the haystack and records where each searchable cell starts in it.
///
/// Offsets are in grapheme clusters, because that is the unit `nucleo` indexes
/// a non-ASCII haystack in: `Utf32String` collects one entry per cluster, so a
/// codepoint count drifts by one for every combining mark or emoji sequence
/// ahead of a match.
fn weave<'a>(
    cells: impl IntoIterator<Item = &'a Cell>,
    hidden: Option<&str>,
) -> (String, Vec<Option<usize>>) {
    let mut haystack = String::new();
    let mut bases = Vec::new();
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

fn paint_cell(
    spans: &mut Vec<Span<'static>>,
    cell: &Cell,
    budget: usize,
    base: Option<usize>,
    indices: &[u32],
    palette: &Palette,
) -> usize {
    let prefix = columns::truncate(&cell.prefix, budget);
    let prefix_width = columns::width(&prefix);
    if !prefix.is_empty() {
        spans.push(Span::styled(
            prefix.into_owned(),
            Style::default().fg(palette.muted),
        ));
    }
    let available = budget.saturating_sub(prefix_width);
    if available == 0 {
        return prefix_width;
    }
    let ellipsis = Span::styled("…", Style::default().fg(cell.fg));
    let body_width = if columns::width(&cell.text) <= available {
        paint(spans, &cell.text, base, indices, cell.fg, palette.matched);
        columns::width(&cell.text)
    } else if cell.tail {
        let (suffix, offset) = columns::suffix(&cell.text, available - 1);
        spans.push(ellipsis);
        paint(
            spans,
            suffix,
            base.map(|base| base + offset),
            indices,
            cell.fg,
            palette.matched,
        );
        1 + columns::width(suffix)
    } else {
        let shown = columns::truncate(&cell.text, available);
        let kept = shown.strip_suffix('…').unwrap_or(&shown);
        paint(spans, kept, base, indices, cell.fg, palette.matched);
        spans.push(ellipsis);
        columns::width(&shown)
    };
    prefix_width + body_width
}

/// Cells each column position takes, shared by every group.
///
/// A group's fixed columns take their position's shared size, so the same
/// column lands on the same screen cells in every group that has it, and
/// rows of different kinds read as one table. Fills still share whatever
/// their own group's row leaves. Measured once across all entries, visible
/// or not, so columns stay put while filtering and scoping.
#[derive(Debug, Default)]
struct Grid {
    /// What each position asks for: the widest `Cells` there, or the widest
    /// content or heading of a `Content` column there, within its budget.
    natural: Vec<usize>,
    /// The widest `Cells` at each position, which no squeeze takes.
    floor: Vec<usize>,
}

impl Grid {
    fn measure(layouts: &[Layout], prepared: &[Prepared]) -> Self {
        let positions = layouts
            .iter()
            .map(|layout| layout.columns.len())
            .max()
            .unwrap_or(0);
        let mut grid = Self {
            natural: vec![0; positions],
            floor: vec![0; positions],
        };
        // Each group's content first, so a `Content` budget caps only its own.
        let mut content: Vec<Vec<usize>> = layouts
            .iter()
            .map(|layout| {
                layout
                    .columns
                    .iter()
                    .map(|column| match columns::width(&column.heading) {
                        0 => 0,
                        heading => heading.saturating_add(COLUMN_GAP),
                    })
                    .collect()
            })
            .collect();
        for row in prepared {
            for (wide, cell) in content[row.group].iter_mut().zip(&row.cells) {
                *wide = (*wide).max(
                    columns::width(&cell.text)
                        .saturating_add(columns::width(&cell.prefix))
                        .saturating_add(COLUMN_GAP),
                );
            }
        }
        for (layout, wide) in layouts.iter().zip(&content) {
            for (position, (column, wide)) in layout.columns.iter().zip(wide).enumerate() {
                let size = match column.width {
                    Width::Cells(cells) => {
                        grid.floor[position] = grid.floor[position].max(cells);
                        cells
                    }
                    Width::Content(max) => (*wide).min(max),
                    Width::Fill => 0,
                };
                grid.natural[position] = grid.natural[position].max(size);
            }
        }
        grid
    }

    /// Each position's cells in a row `width` wide.
    ///
    /// On a small terminal, content positions yield cells, widest first, until
    /// every group's fixed columns leave each of its fills [`MIN_FILL`] cells.
    /// The squeeze shrinks the shared sizes, so it never breaks alignment;
    /// `Cells` keep their contract.
    fn sizes(&self, layouts: &[Layout], width: usize) -> Vec<usize> {
        let content = width.saturating_sub(GUTTER);
        let mut sizes = self.natural.clone();
        loop {
            let mut shrunk = false;
            for layout in layouts {
                let fills = fills(&layout.columns);
                let target = content.saturating_sub(fills.saturating_mul(MIN_FILL));
                if fixed(&layout.columns, &sizes) <= target {
                    continue;
                }
                let widest = layout
                    .columns
                    .iter()
                    .enumerate()
                    .filter(|&(position, column)| {
                        matches!(column.width, Width::Content(_))
                            && sizes[position] > self.floor[position].max(1)
                    })
                    .max_by_key(|&(position, _)| sizes[position])
                    .map(|(position, _)| position);
                if let Some(position) = widest {
                    sizes[position] -= 1;
                    shrunk = true;
                }
            }
            if !shrunk {
                return sizes;
            }
        }
    }
}

fn fills(columns: &[Column]) -> usize {
    columns
        .iter()
        .filter(|column| column.width == Width::Fill)
        .count()
}

/// Cells a group's fixed columns take at `sizes`.
fn fixed(columns: &[Column], sizes: &[usize]) -> usize {
    columns
        .iter()
        .zip(sizes)
        .filter(|(column, _)| column.width != Width::Fill)
        .fold(0usize, |total, (_, size)| total.saturating_add(*size))
}

/// Cells each column of a row gets, given the row's total width and the
/// grid's `sizes` for it.
///
/// Every row of every group comes through here with the same sizes, so
/// alignment is one calculation rather than a per-row decision that could
/// drift.
fn budgets(columns: &[Column], sizes: &[usize], width: usize) -> Vec<usize> {
    let content = width.saturating_sub(GUTTER);
    let fills = fills(columns);
    let flexible = content.saturating_sub(fixed(columns, sizes));
    let share = flexible.checked_div(fills).unwrap_or(0);

    let mut out = Vec::with_capacity(columns.len());
    let mut remaining = content;
    let mut fills_left = fills;
    for (column, size) in columns.iter().zip(sizes) {
        let want = match column.width {
            Width::Cells(_) | Width::Content(_) => *size,
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
    use crate::Picker;

    #[test]
    fn clipped_paths_preserve_suffixes_and_original_match_positions() {
        let palette = Palette::default();
        let text = "/long/prefix/cafe\u{301}/👨‍👩‍👧/file.rs";
        let offset = columns::graphemes(text.split_once("file").unwrap().0);
        let indices: Vec<_> = (offset..offset + 4)
            .map(|i| u32::try_from(i + 7).unwrap())
            .collect();
        let cell = Cell::path(text, palette.strong).prefix("└─ ");
        assert_eq!(weave(std::slice::from_ref(&cell), None).0, text);
        for budget in 0..=80 {
            let mut spans = Vec::new();
            let used = paint_cell(&mut spans, &cell, budget, Some(7), &indices, &palette);
            assert!(used <= budget);
            assert_eq!(
                used,
                spans
                    .iter()
                    .map(|span| columns::width(&span.content))
                    .sum::<usize>()
            );
            if budget >= 15 {
                let highlighted: String = spans
                    .iter()
                    .filter(|span| span.style.fg == Some(palette.matched))
                    .map(|span| span.content.as_ref())
                    .collect();
                assert_eq!(highlighted, "file");
                let drawn: String = spans.iter().map(|span| span.content.as_ref()).collect();
                assert!(drawn.ends_with("file.rs"));
                assert!(drawn.starts_with("└─ "));
                assert!(!drawn.contains("…\u{301}"));
            }
        }
    }

    #[test]
    fn content_widths_are_measured_once_and_yield_space_to_primary_text() {
        let mut model = Picker::new(vec![
            Thing::fruit("cafe\u{301} 👨‍👩‍👧", "a useful task"),
            Thing::fruit("pear", "another task"),
        ])
        .groups(vec![Group::new(
            Kind::Fruit,
            "fruit",
            vec![
                Column::new(6),
                Column::content(30).heading("name"),
                Column::fill(),
            ],
        )])
        .build()
        .unwrap();
        assert_eq!(model.budgets(0, 40), [6, 9, 23]);
        for ch in "pear".chars() {
            model.apply(Command::Insert(ch));
        }
        assert_eq!(model.budgets(0, 40), [6, 9, 23]);
        assert_eq!(model.budgets(0, 32), [6, 4, 20]);
        for width in 0..=120 {
            for line in model.lines(width, 10) {
                assert_eq!(line.width(), width);
            }
        }
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Kind {
        Fruit,
        Veg,
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

        fn veg(name: &'static str, note: &'static str) -> Self {
            Self {
                kind: Kind::Veg,
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

    impl<T: Entry> Model<T> {
        fn budgets(&self, group: usize, width: usize) -> Vec<usize> {
            let sizes = self.grid.sizes(&self.layouts, width);
            budgets(self.columns_of(group), &sizes, width)
        }
    }

    /// Budgets for one group's columns with no rows to measure.
    fn alone(columns: Vec<Column>, width: usize) -> Vec<usize> {
        let layouts = [Layout {
            label: String::new(),
            columns,
            scope: None,
        }];
        let sizes = Grid::measure(&layouts, &[]).sizes(&layouts, width);
        budgets(&layouts[0].columns, &sizes, width)
    }

    impl Entry for Thing {
        type Group = Kind;

        fn group(&self) -> Kind {
            self.kind
        }

        fn cells(&self, _palette: &Palette) -> Vec<Cell> {
            let white = Color::White;
            match self.kind {
                Kind::Fruit => vec![
                    Cell::tag("fruit", white),
                    Cell::new(self.name, white),
                    Cell::new(self.note, white),
                ],
                Kind::Veg => vec![
                    Cell::tag("vegetable", white),
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
            .palette(Palette::default())
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
                Row::Section { .. } | Row::Headings(_) | Row::Detail { .. } => None,
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
    fn enter_needs_a_row() {
        assert_eq!(model("zzzzzz").apply(Command::Accept), None);
        assert_eq!(model("apple").apply(Command::Accept), Some(Outcome::Accept));
    }

    #[test]
    fn a_scope_matches_every_word_after_its_sigil() {
        assert_eq!(item_names(&model("/apri fix it")), Vec::<&str>::new());
        assert_eq!(item_names(&model("/apri cot")), ["/tmp/apricot"]);
    }

    #[test]
    fn changing_scope_keeps_the_text_after_the_word() {
        let mut model = model("/apri fix it");
        model.apply(Command::NextScope);
        assert_eq!(model.query(), "apri fix it");
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
                Row::Headings(_) | Row::Detail { .. } => {}
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
        assert_eq!(item_names(&model("fruits")).len(), 0);
    }

    #[test]
    fn scores_order_within_a_group_not_across_the_whole_list() {
        assert_eq!(item_names(&model("ap")), ["apple", "apricot"]);
    }

    #[test]
    fn a_tag_cell_is_drawn_but_never_matched() {
        // Every fruit row draws the word "fruit", so a query for it would
        // match all three if tag cells were searchable.
        assert_eq!(item_names(&model("fruit")).len(), 0);
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

    /// Fruit and vegetables share every column; paths share the first and
    /// fill the rest. Every group measures to its own content and the widest.
    fn mixed() -> Model<Thing> {
        let pair = || vec![Column::content(20), Column::content(20), Column::fill()];
        typed(
            vec![
                Thing::fruit("fig", "sweet"),
                Thing::veg("artichoke", "green"),
                Thing::path("/tmp"),
            ],
            vec![
                Group::new(Kind::Fruit, "fruits", pair()),
                Group::new(Kind::Veg, "vegetables", pair()),
                Group::new(
                    Kind::Path,
                    "paths",
                    vec![Column::content(20), Column::fill()],
                )
                .scoped('/'),
            ],
            "",
        )
    }

    #[test]
    fn a_column_lands_on_the_same_cells_in_every_group_that_has_it() {
        let mut model = mixed();
        let fruit = starts(&model.render_row(Row::Item(0), false, 80));
        let veg = starts(&model.render_row(Row::Item(1), false, 80));
        let path = model.render_row(Row::Item(2), false, 80);
        // "vegetable" sets the first column and "artichoke" the second for
        // fruit too; the path's fill starts where both second columns do.
        assert_eq!(fruit, [2, 13, 24]);
        assert_eq!(veg, fruit);
        assert_eq!(starts(&path), [2, 13]);
        assert_eq!(drawn_width(&path), 80, "the fill takes the rest");
    }

    #[test]
    fn squeezing_a_narrow_row_keeps_columns_aligned_across_groups() {
        let mut model = mixed();
        for width in [24, 30, 40, 50] {
            let fruit = model.budgets(0, width);
            assert_eq!(model.budgets(1, width), fruit, "at {width}");
            assert_eq!(model.budgets(2, width)[0], fruit[0], "at {width}");
            for item in 0..3 {
                let line = model.render_row(Row::Item(item), false, width);
                assert_eq!(drawn_width(&line), width, "item {item} at {width}");
            }
        }
        assert_eq!(
            model.budgets(0, 40),
            [9, 9, 20],
            "both content columns yield"
        );
    }

    #[test]
    fn a_fixed_column_widens_to_the_widest_at_its_position() {
        let model = typed(
            vec![Thing::fruit("fig", "sweet"), Thing::path("/tmp")],
            vec![
                Group::new(
                    Kind::Fruit,
                    "fruits",
                    vec![Column::new(3), Column::content(20), Column::fill()],
                ),
                Group::new(Kind::Path, "paths", vec![Column::new(8), Column::fill()]),
            ],
            "",
        );
        assert_eq!(model.budgets(0, 40), [8, 5, 25]);
        assert_eq!(model.budgets(1, 40), [8, 30]);
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

    #[derive(Debug)]
    struct Card(&'static str, &'static str);

    impl Entry for Card {
        type Group = ();
        fn group(&self) {}
        fn cells(&self, palette: &Palette) -> Vec<Cell> {
            vec![Cell::new(self.0, palette.strong)]
        }
        fn details(&self, palette: &Palette) -> Vec<Cell> {
            vec![
                Cell::new("fixture", palette.muted),
                Cell::new(self.1, palette.strong),
            ]
        }
        fn identity(&self) -> Option<Cow<'_, str>> {
            Some(Cow::Borrowed(self.0))
        }
    }

    #[test]
    fn secondary_lines_match_highlight_and_select_their_primary_entry() {
        let mut model = Model::build(
            vec![Card("first", "選択 👩‍💻"), Card("second", "Other question")],
            vec![Group::new((), "requests", vec![Column::fill()])],
            Palette::default(),
            false,
        )
        .unwrap();
        model.select_line(3);
        assert_eq!(model.selected(), Some(0));
        model.select_line(6);
        assert_eq!(model.selected(), Some(1));
        model.scroll_into_view(4);
        let lines = model.lines(40, 4);
        assert_eq!(
            lines
                .iter()
                .filter(|line| line.style.bg == Some(model.palette.selection_bg))
                .count(),
            3
        );
        model.insert_text("選択");
        assert_eq!(model.matched(), 1);
        assert_eq!(model.selected(), Some(0));
        for width in 0..50 {
            let line = model.render_row(Row::Detail { item: 0, line: 1 }, true, width);
            assert_eq!(drawn_width(&line), width, "secondary width {width}");
            if width > 6 {
                assert!(
                    line.spans
                        .iter()
                        .any(|span| span.style.fg == Some(model.palette.matched)),
                    "{line:?}"
                );
            }
        }
    }

    #[test]
    fn secondary_lines_affect_refresh_and_page_size_without_extra_selections() {
        let mut model = Model::build(
            vec![Card("first", "Question"), Card("second", "Another")],
            vec![Group::new((), "requests", vec![Column::fill()])],
            Palette::default(),
            false,
        )
        .unwrap();
        assert_eq!(model.matched(), 2);
        assert_eq!(model.page_size(3), 1);
        assert_eq!(model.page_size(6), 2);
        assert!(
            !model
                .replace_all(vec![Card("first", "Question"), Card("second", "Another")])
                .unwrap()
        );
        assert!(
            model
                .replace_all(vec![
                    Card("first", "New question"),
                    Card("second", "Another")
                ])
                .unwrap()
        );
        model.insert_text("New question");
        assert_eq!(model.matched(), 1);
        assert_eq!(model.selected(), Some(0));
    }

    #[test]
    fn rows_explicitly_paint_the_palette_background() {
        let mut model = model("");
        let plain = model.render_row(Row::Item(0), false, 40);
        let selected = model.render_row(Row::Item(0), true, 40);
        let section = model.render_row(Row::Section { group: 0, count: 3 }, false, 40);
        assert_eq!(plain.style.bg, Some(model.palette.background));
        assert_eq!(selected.style.bg, Some(model.palette.selection_bg));
        assert_eq!(section.style.bg, Some(model.palette.background));
    }

    fn highlighted(model: &mut Model<Thing>) -> String {
        model
            .render_row(Row::Item(0), false, 60)
            .spans
            .iter()
            .filter(|span| span.style.fg == Some(Palette::default().matched))
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
            fn cells(&self, _palette: &Palette) -> Vec<Cell> {
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
    fn a_typed_control_follows_the_paste_rule() {
        let mut model = model("a");
        for ch in ['\n', '\u{1b}', '\u{9b}', '\t', 'b'] {
            model.apply(Command::Insert(ch));
        }
        assert_eq!(model.query(), "a  b");
        assert_eq!(model.caret_cells(), 4);
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
        assert_eq!(model.rows.len(), 0);
        assert_eq!(model.selected(), None);
        assert_eq!(model.matched(), 0);
    }

    #[test]
    fn a_fixed_column_after_a_fill_keeps_its_cells() {
        let columns = vec![Column::new(8), Column::fill(), Column::new(2)];
        let given = alone(columns, 40);
        assert_eq!(given, [8, 28, 2]);
        assert_eq!(given.iter().sum::<usize>(), 40 - GUTTER);
    }

    #[test]
    fn two_fills_split_the_remainder_without_losing_a_cell() {
        let given = alone(vec![Column::fill(), Column::fill()], 41);
        assert_eq!(given.iter().sum::<usize>(), 41 - GUTTER);
    }

    #[test]
    fn columns_wider_than_the_terminal_are_cut_off_rather_than_overflowing() {
        let columns = vec![Column::new(20), Column::new(20), Column::fill()];
        let given = alone(columns, 24);
        assert_eq!(given, [20, 2, 0]);
        assert_eq!(given.iter().sum::<usize>(), 24 - GUTTER);
    }

    #[test]
    fn impossible_fixed_width_totals_saturate_instead_of_overflowing() {
        let columns = vec![Column::new(usize::MAX), Column::new(usize::MAX)];
        assert_eq!(alone(columns, 12), [10, 0]);
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
    fn two_groups_cannot_share_a_scope() {
        let result = Picker::new(Vec::<Thing>::new())
            .groups([fruit_group().scoped('/'), path_group()])
            .build();
        assert!(
            matches!(result, Err(Error::InvalidScope { sigil: '/' })),
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
