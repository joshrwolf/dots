//! A picker described as JSON, for hosts that build views in another language.
//!
//! The format is strict: an unknown field or a reference to an undeclared
//! group is an error, so a typo in a view fails loudly instead of drawing
//! something subtly different. Lists also accept `{}`, because Lua encoders
//! cannot tell an empty array from an empty object.

use std::borrow::Cow;
use std::collections::HashMap;
use std::time::Duration;

use ratatui::style::Color;
use serde::{Deserialize, Deserializer};

use crate::{Cell, Column, Entry, Group, Palette, Picker, Shortcut};

/// Everything a host supplies to open a picker.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct View {
    #[serde(default)]
    pub prompt: Option<String>,
    /// Text already typed when the picker opens.
    #[serde(default)]
    pub query: Option<String>,
    /// What the host does with the row Enter chose.
    pub accept: String,
    /// A command that prints a line whenever the rows may have changed. The
    /// host re-reads the view on each line while the picker runs.
    #[serde(default, deserialize_with = "optional_list")]
    pub watch: Option<Vec<String>>,
    /// Re-run the selected row's preview this many seconds apart.
    #[serde(default)]
    pub preview_every: Option<f64>,
    /// Show the first or last lines of output when the preview is taller than its pane.
    #[serde(default)]
    pub preview_anchor: PreviewAnchor,
    /// Show the preview to the right of the list or below it.
    #[serde(default)]
    pub preview_position: PreviewPosition,
    #[serde(default, deserialize_with = "list")]
    pub shortcuts: Vec<ShortcutSpec>,
    #[serde(deserialize_with = "list")]
    pub groups: Vec<GroupSpec>,
    #[serde(deserialize_with = "list")]
    pub rows: Vec<Row>,
}

/// Which end of a preview stays visible when it overflows.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PreviewAnchor {
    Start,
    #[default]
    End,
}

/// Where the preview pane sits.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PreviewPosition {
    /// Beside the list, on terminals at least 80 cells wide.
    #[default]
    Right,
    /// Under the list, which keeps a fixed share of the body, so wide rows
    /// keep their columns.
    Below,
}

/// A Ctrl+letter key that runs its own action.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShortcutSpec {
    pub key: char,
    pub label: String,
    pub action: String,
    /// The typed line is the action's input: it receives the line as
    /// `query`, and the selected row's `id` only when a row is selected. See
    /// [`arguments`]. A blank line asks for input instead of running it.
    #[serde(default)]
    pub query: bool,
    /// What to type for a shortcut that takes the line, such as `URL`.
    #[serde(default)]
    pub hint: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GroupSpec {
    pub id: String,
    pub label: String,
    /// Columns line up by position across groups: each fixed column gets the
    /// widest size any group's fixed column at its position asks for.
    #[serde(deserialize_with = "list")]
    pub columns: Vec<ColumnSpec>,
    /// Hide the group until the query starts with this character.
    #[serde(default)]
    pub scope: Option<char>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ColumnSpec {
    pub width: WidthSpec,
    #[serde(default)]
    pub heading: Option<String>,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum WidthSpec {
    /// At least this many cells of text; the gap before the next column is extra.
    Cells(usize),
    /// The widest cell or heading at this position in any group, up to this
    /// many cells.
    Content(usize),
    /// Whatever the other columns leave.
    Fill,
}

/// One selectable row.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Row {
    /// Handed back to the host when the row is chosen.
    pub id: String,
    pub group: String,
    #[serde(deserialize_with = "list")]
    pub cells: Vec<CellSpec>,
    /// Search terms that are never drawn.
    #[serde(default)]
    pub search: Option<String>,
    /// A command whose output previews the row.
    #[serde(default, deserialize_with = "optional_list")]
    pub preview: Option<Vec<String>>,
    #[serde(skip)]
    group_index: usize,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CellSpec {
    pub text: String,
    #[serde(default)]
    pub role: Role,
    #[serde(default)]
    pub kind: CellKind,
    /// Muted, unsearchable text before the cell's own.
    #[serde(default)]
    pub prefix: Option<String>,
}

/// A palette role. Hosts name what a cell means, never a colour.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    #[default]
    Strong,
    Muted,
    Accent,
    Blue,
    Green,
    Red,
    Yellow,
}

impl Role {
    fn color(self, palette: &Palette) -> Color {
        match self {
            Self::Strong => palette.strong,
            Self::Muted => palette.muted,
            Self::Accent => palette.accent,
            Self::Blue => palette.blue,
            Self::Green => palette.green,
            Self::Red => palette.red,
            Self::Yellow => palette.yellow,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CellKind {
    /// Drawn and searched.
    #[default]
    Text,
    /// Drawn, never searched: a status glyph or a kind label.
    Tag,
    /// A path that keeps its end when clipped.
    Path,
}

impl Entry for Row {
    type Group = usize;

    fn group(&self) -> usize {
        self.group_index
    }

    fn cells(&self, palette: &Palette) -> Vec<Cell> {
        self.cells
            .iter()
            .map(|spec| {
                let color = spec.role.color(palette);
                let cell = match spec.kind {
                    CellKind::Text => Cell::new(spec.text.clone(), color),
                    CellKind::Tag => Cell::tag(spec.text.clone(), color),
                    CellKind::Path => Cell::path(spec.text.clone(), color),
                };
                match &spec.prefix {
                    Some(prefix) => cell.prefix(prefix.clone()),
                    None => cell,
                }
            })
            .collect()
    }

    fn hidden_terms(&self) -> Option<Cow<'_, str>> {
        self.search.as_deref().map(Cow::Borrowed)
    }

    fn identity(&self) -> Option<Cow<'_, str>> {
        Some(Cow::Borrowed(&self.id))
    }

    fn preview(&self) -> Option<&[String]> {
        self.preview.as_deref()
    }
}

/// A view the picker cannot show.
#[derive(Debug, thiserror::Error)]
pub enum SpecError {
    #[error("the view is not valid JSON for a picker")]
    Json(#[from] serde_json::Error),
    #[error("group {0:?} is declared twice")]
    DuplicateGroup(String),
    #[error("row {row:?} belongs to group {group:?}, which is not declared")]
    UndeclaredGroup { row: String, group: String },
    #[error("row id {0:?} is empty or repeated")]
    InvalidRowId(String),
    #[error("shortcut ctrl+{0} has an empty action")]
    EmptyAction(char),
    #[error("shortcut ctrl+{0} has a hint but does not take the line (query)")]
    HintWithoutQuery(char),
    #[error("the view's accept action is empty")]
    EmptyAccept,
    #[error("preview_every must be between 0.1 and 3600 seconds, not {0}")]
    PreviewInterval(f64),
    #[error("a column of {0} cells is wider than any terminal")]
    ColumnWidth(usize),
}

/// A parsed view, ready to run.
#[derive(Debug)]
#[must_use]
pub struct Prepared {
    pub picker: Picker<Row>,
    /// Each shortcut's value is its action's name.
    pub shortcuts: Vec<Shortcut<String>>,
    pub accept: String,
    pub watch: Option<Vec<String>>,
    /// Resolves the rows of a refreshed view against this picker's groups.
    pub groups: Groups,
}

/// Group ids of a prepared view, for resolving the rows of a refreshed one.
#[derive(Debug, Clone)]
pub struct Groups(HashMap<String, usize>);

impl Groups {
    /// `rows`, resolved against the groups the picker was built with. A
    /// refreshed view may change rows, not groups.
    pub fn rows(&self, mut rows: Vec<Row>) -> Result<Vec<Row>, SpecError> {
        let mut ids = std::collections::HashSet::new();
        for row in &mut rows {
            if row.id.is_empty() || !ids.insert(row.id.clone()) {
                return Err(SpecError::InvalidRowId(row.id.clone()));
            }
            row.group_index =
                *self
                    .0
                    .get(&row.group)
                    .ok_or_else(|| SpecError::UndeclaredGroup {
                        row: row.id.clone(),
                        group: row.group.clone(),
                    })?;
        }
        Ok(rows)
    }
}

impl View {
    pub fn parse(json: &str) -> Result<Self, SpecError> {
        Ok(serde_json::from_str(json)?)
    }

    /// Validates references that the format alone cannot express and builds
    /// the picker. Layout errors, such as a cell count that disagrees with
    /// the columns, surface from `Picker::build` or `Picker::run`.
    pub fn prepare(self, palette: Palette) -> Result<Prepared, SpecError> {
        if self.accept.trim().is_empty() {
            return Err(SpecError::EmptyAccept);
        }
        let preview_every = match self.preview_every {
            None => None,
            Some(seconds) if (0.1..=3600.0).contains(&seconds) => {
                Some(Duration::from_secs_f64(seconds))
            }
            Some(seconds) => return Err(SpecError::PreviewInterval(seconds)),
        };
        let mut indices = HashMap::new();
        for (index, group) in self.groups.iter().enumerate() {
            if indices.insert(group.id.clone(), index).is_some() {
                return Err(SpecError::DuplicateGroup(group.id.clone()));
            }
        }
        let groups = Groups(indices);
        let rows = groups.rows(self.rows)?;
        let shortcuts = self
            .shortcuts
            .into_iter()
            .map(|spec| {
                if spec.action.trim().is_empty() {
                    return Err(SpecError::EmptyAction(spec.key));
                }
                let shortcut = Shortcut::control(spec.key, spec.label, spec.action);
                match (spec.query, spec.hint) {
                    (true, hint) => Ok(shortcut.takes_line(hint)),
                    (false, None) => Ok(shortcut),
                    (false, Some(_)) => Err(SpecError::HintWithoutQuery(spec.key)),
                }
            })
            .collect::<Result<_, _>>()?;

        let declared = self
            .groups
            .into_iter()
            .enumerate()
            .map(|(index, spec)| {
                let columns = spec
                    .columns
                    .into_iter()
                    .map(ColumnSpec::column)
                    .collect::<Result<_, _>>()?;
                let mut group = Group::new(index, spec.label, columns);
                if let Some(sigil) = spec.scope {
                    group = group.scoped(sigil);
                }
                Ok(group)
            })
            .collect::<Result<Vec<_>, SpecError>>()?;
        let mut picker = Picker::new(rows).groups(declared).palette(palette);
        if let Some(prompt) = self.prompt {
            picker = picker.prompt(prompt);
        }
        if let Some(query) = self.query {
            picker = picker.query(query);
        }
        if let Some(interval) = preview_every {
            picker = picker.preview_every(interval);
        }
        picker = picker
            .preview_anchor(self.preview_anchor)
            .preview_position(self.preview_position);
        Ok(Prepared {
            picker,
            shortcuts,
            accept: self.accept,
            watch: self.watch,
            groups,
        })
    }
}

/// The arguments an action receives: the row's `id`, if one was selected,
/// and `query`, the whole input line, if the action takes it. The accept
/// action and other shortcuts get only `{ id }`, so they can declare exactly
/// that: the schema Rex builds from an action's `args` forbids other fields.
pub fn arguments(row: Option<&Row>, query: Option<&str>) -> serde_json::Value {
    let mut args = serde_json::Map::new();
    if let Some(row) = row {
        args.insert("id".to_owned(), row.id.clone().into());
    }
    if let Some(query) = query {
        args.insert("query".to_owned(), query.into());
    }
    args.into()
}

impl ColumnSpec {
    fn column(self) -> Result<Column, SpecError> {
        let column = match self.width {
            WidthSpec::Cells(cells) => Column::new(
                cells
                    .checked_add(crate::model::COLUMN_GAP)
                    .ok_or(SpecError::ColumnWidth(cells))?,
            ),
            WidthSpec::Content(max) => Column::content(max),
            WidthSpec::Fill => Column::fill(),
        };
        Ok(match self.heading {
            Some(heading) => column.heading(heading),
            None => column,
        })
    }
}

/// A JSON array, or `{}` for an empty one. Errors inside the array keep their
/// own message, such as an unknown role, rather than a generic mismatch.
fn list<'de, D, T>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    struct List<T>(std::marker::PhantomData<T>);

    impl<'de, T: Deserialize<'de>> serde::de::Visitor<'de> for List<T> {
        type Value = Vec<T>;

        fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str("a list, or {} for an empty one")
        }

        fn visit_seq<A: serde::de::SeqAccess<'de>>(self, mut seq: A) -> Result<Vec<T>, A::Error> {
            let mut items = Vec::with_capacity(seq.size_hint().unwrap_or(0).min(4096));
            while let Some(item) = seq.next_element()? {
                items.push(item);
            }
            Ok(items)
        }

        fn visit_map<A: serde::de::MapAccess<'de>>(self, mut map: A) -> Result<Vec<T>, A::Error> {
            match map.next_key::<serde::de::IgnoredAny>()? {
                None => Ok(Vec::new()),
                Some(_) => Err(serde::de::Error::custom(
                    "expected a list, got a non-empty object",
                )),
            }
        }
    }

    deserializer.deserialize_any(List(std::marker::PhantomData))
}

fn optional_list<'de, D, T>(deserializer: D) -> Result<Option<Vec<T>>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    list(deserializer).map(|items| (!items.is_empty()).then_some(items))
}

#[cfg(test)]
mod tests {
    use super::*;

    const VIEW: &str = r#"{
        "prompt": "go> ",
        "accept": "go_to",
        "shortcuts": [{ "key": "o", "label": "open", "action": "go_open" }],
        "groups": [
            { "id": "sessions", "label": "sessions",
              "columns": [{ "width": { "content": 30 } }, { "width": "fill", "heading": "path" }] },
            { "id": "dirs", "label": "directories", "columns": [{ "width": "fill" }], "scope": "/" }
        ],
        "rows": [
            { "id": "session:1", "group": "sessions",
              "cells": [{ "text": "dots" }, { "text": "~/dots", "kind": "path", "role": "muted" }],
              "search": "session:1", "preview": ["rex", "capture", "-s", "session:1"] },
            { "id": "/tmp", "group": "dirs", "cells": [{ "text": "/tmp", "kind": "path" }], "preview": {} }
        ]
    }"#;

    #[test]
    fn a_view_builds_a_picker_with_its_groups_and_shortcuts() {
        let prepared = View::parse(VIEW)
            .unwrap()
            .prepare(Palette::default())
            .unwrap();
        assert_eq!(prepared.accept, "go_to");
        assert_eq!(prepared.shortcuts.len(), 1);
        let model = prepared.picker.build().unwrap();
        assert_eq!(model.scope_label(), "all");
    }

    #[test]
    fn rows_resolve_groups_search_terms_and_previews() {
        let view = View::parse(VIEW).unwrap();
        let dirs = view.rows.get(1).unwrap();
        assert_eq!(dirs.preview, None, "an empty Lua table is no preview");
        let prepared = view.prepare(Palette::default()).unwrap();
        let model = prepared.picker.build().unwrap();
        let row = model.take_selected().unwrap();
        assert_eq!(row.group(), 0);
        assert_eq!(row.hidden_terms().as_deref(), Some("session:1"));
        assert_eq!(
            row.preview.as_deref(),
            Some(&["rex", "capture", "-s", "session:1"].map(String::from)[..])
        );
    }

    #[test]
    fn roles_paint_from_the_palette() {
        let palette = Palette::default();
        let view = View::parse(VIEW).unwrap();
        let cells = view.rows.first().unwrap().cells(&palette);
        assert_eq!(cells.first().map(Cell::text), Some("dots"));
        assert_eq!(Role::Muted.color(&palette), palette.muted);
    }

    #[test]
    fn a_fixed_column_shows_all_of_its_cells() {
        let view = View::parse(
            r#"{ "accept": "a",
                 "groups": [{ "id": "g", "label": "g",
                              "columns": [{ "width": { "cells": 1 } }, { "width": "fill" }] }],
                 "rows": [{ "id": "r", "group": "g",
                            "cells": [{ "text": "●", "kind": "tag" }, { "text": "agent" }] }] }"#,
        )
        .unwrap();
        let mut model = view
            .prepare(Palette::default())
            .unwrap()
            .picker
            .build()
            .unwrap();
        let text: String = model
            .lines(40, 10)
            .iter()
            .flat_map(|line| line.spans.iter().map(|span| span.content.to_string()))
            .collect();
        assert!(text.contains("●  agent"), "{text:?}");
    }

    fn rows(json: &str, groups: &Groups) -> Vec<Row> {
        groups.rows(View::parse(json).unwrap().rows).unwrap()
    }

    const REFRESHED: &str = r#"{ "accept": "go_to", "groups": [],
        "rows": [
            { "id": "session:2", "group": "sessions",
              "cells": [{ "text": "api" }, { "text": "~/api" }] },
            { "id": "session:1", "group": "sessions",
              "cells": [{ "text": "dots" }, { "text": "~/dots" }] }
        ] }"#;

    #[test]
    fn a_refresh_keeps_the_selected_row_by_id() {
        let prepared = View::parse(VIEW)
            .unwrap()
            .prepare(Palette::default())
            .unwrap();
        let groups = prepared.groups.clone();
        let mut model = prepared.picker.build().unwrap();
        assert_eq!(
            model.selected_entry().map(|row| row.id.as_str()),
            Some("session:1")
        );
        model.replace_all(rows(REFRESHED, &groups)).unwrap();
        assert_eq!(
            model.selected_entry().map(|row| row.id.as_str()),
            Some("session:1")
        );
        assert_eq!(model.candidates(), 2, "the old directory row is gone");
    }

    #[test]
    fn a_refresh_fills_a_group_that_started_empty() {
        let group = |id: &str| {
            format!(r#"{{ "id": "{id}", "label": "{id}", "columns": [{{ "width": "fill" }}] }}"#)
        };
        let row = |id: &str, group: &str| {
            format!(r#"{{ "id": "{id}", "group": "{group}", "cells": [{{ "text": "{id}" }}] }}"#)
        };
        let view = |rows: &[String]| {
            format!(
                r#"{{ "accept": "a", "groups": [{}, {}], "rows": [{}] }}"#,
                group("attention"),
                group("working"),
                rows.join(",")
            )
        };
        let first = view(&[row("counter", "working")]);
        let prepared = View::parse(&first)
            .unwrap()
            .prepare(Palette::default())
            .unwrap();
        let groups = prepared.groups.clone();
        let mut model = prepared.picker.build().unwrap();
        assert_eq!(model.candidates(), 1);
        let second = view(&[row("probe", "attention"), row("counter", "working")]);
        model.replace_all(rows(&second, &groups)).unwrap();
        assert_eq!(model.candidates(), 2);
        assert_eq!(
            model.selected_entry().map(|row| row.id.as_str()),
            Some("counter")
        );
    }

    #[test]
    fn the_selection_follows_a_row_into_another_group_and_back() {
        let group = |id: &str| {
            format!(r#"{{ "id": "{id}", "label": "{id}", "columns": [{{ "width": "fill" }}] }}"#)
        };
        let row = |id: &str, group: &str| {
            format!(r#"{{ "id": "{id}", "group": "{group}", "cells": [{{ "text": "{id}" }}] }}"#)
        };
        let view = |rows: &[String]| {
            format!(
                r#"{{ "accept": "a", "groups": [{}, {}], "rows": [{}] }}"#,
                group("attention"),
                group("working"),
                rows.join(",")
            )
        };
        let working = view(&[
            row("blocked", "attention"),
            row("one", "working"),
            row("agent", "working"),
            row("two", "working"),
        ]);
        let prepared = View::parse(&working)
            .unwrap()
            .prepare(Palette::default())
            .unwrap();
        let groups = prepared.groups.clone();
        let mut model = prepared.picker.build().unwrap();
        model.apply(crate::Command::Next(2));
        let selected = |model: &crate::Model<Row>| model.selected_entry().map(|row| row.id.clone());
        assert_eq!(selected(&model).as_deref(), Some("agent"));

        let attention = view(&[
            row("blocked", "attention"),
            row("agent", "attention"),
            row("one", "working"),
            row("two", "working"),
        ]);
        model.replace_all(rows(&attention, &groups)).unwrap();
        assert_eq!(selected(&model).as_deref(), Some("agent"), "into attention");
        model.replace_all(rows(&working, &groups)).unwrap();
        assert_eq!(
            selected(&model).as_deref(),
            Some("agent"),
            "back to working"
        );
    }

    #[test]
    fn out_of_range_numbers_are_errors_not_panics() {
        let huge_interval =
            r#"{ "accept": "a", "groups": [], "rows": [], "preview_every": 1e300 }"#;
        assert!(matches!(
            View::parse(huge_interval)
                .unwrap()
                .prepare(Palette::default()),
            Err(SpecError::PreviewInterval(_))
        ));
        let wide = format!(
            r#"{{ "accept": "a", "rows": [],
                 "groups": [{{ "id": "g", "label": "g", "columns": [{{ "width": {{ "cells": {} }} }}] }}] }}"#,
            usize::MAX
        );
        assert!(matches!(
            View::parse(&wide).unwrap().prepare(Palette::default()),
            Err(SpecError::ColumnWidth(_))
        ));
    }

    #[test]
    fn a_rejected_refresh_leaves_the_list_alone() {
        let prepared = View::parse(VIEW)
            .unwrap()
            .prepare(Palette::default())
            .unwrap();
        let groups = prepared.groups.clone();
        let mut model = prepared.picker.build().unwrap();
        let before = model.candidates();
        let wrong_arity = r#"{ "accept": "a", "groups": [],
            "rows": [{ "id": "session:9", "group": "sessions", "cells": [{ "text": "one" }] }] }"#;
        assert!(model.replace_all(rows(wrong_arity, &groups)).is_err());
        assert_eq!(model.candidates(), before);
        assert_eq!(
            model.selected_entry().map(|row| row.id.as_str()),
            Some("session:1")
        );
        let undeclared = r#"{ "accept": "a", "groups": [],
            "rows": [{ "id": "x", "group": "elsewhere", "cells": [] }] }"#;
        assert!(matches!(
            groups.rows(View::parse(undeclared).unwrap().rows),
            Err(SpecError::UndeclaredGroup { .. })
        ));
    }

    #[test]
    fn watch_commands_and_preview_intervals_are_part_of_a_view() {
        let live = r#"{ "accept": "a", "groups": [], "rows": [],
            "watch": ["rex", "do", "/watch.lua", "--all"], "preview_every": 1 }"#;
        let prepared = View::parse(live)
            .unwrap()
            .prepare(Palette::default())
            .unwrap();
        assert_eq!(
            prepared.watch.as_deref(),
            Some(&["rex", "do", "/watch.lua", "--all"].map(String::from)[..])
        );
        let too_fast = r#"{ "accept": "a", "groups": [], "rows": [], "preview_every": 0.01 }"#;
        assert!(matches!(
            View::parse(too_fast).unwrap().prepare(Palette::default()),
            Err(SpecError::PreviewInterval(_))
        ));
        let none = r#"{ "accept": "a", "groups": [], "rows": [], "watch": {} }"#;
        assert!(View::parse(none).unwrap().watch.is_none());
    }

    #[test]
    fn preview_anchor_defaults_to_end_and_accepts_both_values() {
        assert_eq!(
            View::parse(VIEW).unwrap().preview_anchor,
            PreviewAnchor::End
        );
        for (value, anchor) in [("start", PreviewAnchor::Start), ("end", PreviewAnchor::End)] {
            let json = format!(
                r#"{{ "accept": "a", "groups": [], "rows": [], "preview_anchor": "{value}" }}"#
            );
            let view = View::parse(&json).unwrap();
            assert_eq!(view.preview_anchor, anchor);
            let _ = view.prepare(Palette::default()).unwrap();
        }
        let invalid = r#"{ "accept": "a", "groups": [], "rows": [], "preview_anchor": "middle" }"#;
        let error = View::parse(invalid).unwrap_err();
        let SpecError::Json(cause) = error else {
            panic!("expected a JSON value error: {error}");
        };
        let message = cause.to_string();
        assert!(
            message.contains("start") && message.contains("end"),
            "{message}"
        );
    }

    #[test]
    fn preview_position_defaults_to_right_and_rejects_other_values() {
        assert_eq!(
            View::parse(VIEW).unwrap().preview_position,
            PreviewPosition::Right
        );
        for (value, position) in [
            ("right", PreviewPosition::Right),
            ("below", PreviewPosition::Below),
        ] {
            let json = format!(
                r#"{{ "accept": "a", "groups": [], "rows": [], "preview_position": "{value}" }}"#
            );
            let view = View::parse(&json).unwrap();
            assert_eq!(view.preview_position, position);
            let _ = view.prepare(Palette::default()).unwrap();
        }
        let invalid = r#"{ "accept": "a", "groups": [], "rows": [], "preview_position": "left" }"#;
        let error = View::parse(invalid).unwrap_err();
        let SpecError::Json(cause) = error else {
            panic!("expected a JSON value error: {error}");
        };
        let message = cause.to_string();
        assert!(
            message.contains("right") && message.contains("below"),
            "{message}"
        );
    }

    const START: &str = r#"{
        "prompt": "go› ", "accept": "go_to",
        "shortcuts": [
            { "key": "t", "label": "issue", "action": "go_issue", "query": true, "hint": "ID" },
            { "key": "o", "label": "open", "action": "go_open" }
        ],
        "groups": [
            { "id": "tabs", "label": "tabs", "columns": [{ "width": "fill" }] },
            { "id": "dirs", "label": "directories", "columns": [{ "width": "fill" }], "scope": "@" }
        ],
        "rows": [
            { "id": "tab|dots", "group": "tabs", "cells": [{ "text": "dots" }] },
            { "id": "dir|/src/api", "group": "dirs", "cells": [{ "text": "/src/api" }] }
        ]
    }"#;

    fn starting(query: &str) -> (Vec<Shortcut<String>>, crate::Model<Row>) {
        let prepared = View::parse(START)
            .unwrap()
            .prepare(Palette::default())
            .unwrap();
        let mut model = prepared.picker.build().unwrap();
        model.insert_text(query);
        (prepared.shortcuts, model)
    }

    /// What the `index`th shortcut's action receives, or `None` when it does
    /// not run.
    fn received(index: usize, query: &str) -> Option<serde_json::Value> {
        let (shortcuts, model) = starting(query);
        let shortcut = shortcuts.get(index).unwrap();
        shortcut
            .chosen(&model)
            .map(|chosen| arguments(chosen.entry, chosen.line))
    }

    #[test]
    fn a_shortcut_that_takes_the_line_gets_it_with_or_without_a_row() {
        let (shortcuts, model) = starting("ABC-123 fix it");
        assert_eq!(model.selected_entry().map(|row| row.id.as_str()), None);
        let issue = shortcuts.first().unwrap();
        assert_eq!(
            (issue.value.as_str(), issue.hint()),
            ("go_issue", Some("ID"))
        );
        assert_eq!(
            received(0, "ABC-123 fix it"),
            Some(serde_json::json!({ "query": "ABC-123 fix it" }))
        );
        assert_eq!(
            received(0, "dots"),
            Some(serde_json::json!({ "id": "tab|dots", "query": "dots" }))
        );
        assert_eq!(received(0, " "), None, "a blank line asks for input");
    }

    #[test]
    fn other_shortcuts_and_enter_get_only_the_row() {
        assert_eq!(
            received(1, "dots"),
            Some(serde_json::json!({ "id": "tab|dots" }))
        );
        let (_, model) = starting("dots");
        assert_eq!(
            arguments(model.selected_entry(), None),
            serde_json::json!({ "id": "tab|dots" })
        );
        assert_eq!(received(1, "zzz"), Some(serde_json::json!({})));
    }

    #[test]
    fn a_hint_needs_a_shortcut_that_takes_the_line() {
        let hinted = |query: bool| {
            format!(
                r#"{{ "accept": "a", "groups": [], "rows": [],
                     "shortcuts": [{{ "key": "r", "label": "PR", "action": "pr",
                                      "query": {query}, "hint": "URL" }}] }}"#
            )
        };
        assert!(
            View::parse(&hinted(true))
                .unwrap()
                .prepare(Palette::default())
                .is_ok()
        );
        assert!(matches!(
            View::parse(&hinted(false))
                .unwrap()
                .prepare(Palette::default()),
            Err(SpecError::HintWithoutQuery('r'))
        ));
    }

    #[test]
    fn a_scope_still_matches_the_whole_line_in_a_view_with_line_shortcuts() {
        let (_, model) = starting("@api");
        assert_eq!(
            model.selected_entry().map(|row| row.id.as_str()),
            Some("dir|/src/api")
        );
        let (_, model) = starting("@api fix-login");
        assert_eq!(model.matched(), 0, "every word filters");
    }

    #[test]
    fn enter_needs_a_row() {
        let (_, mut model) = starting("ABC-123");
        assert_eq!(model.apply(crate::Command::Accept), None);
        let (_, mut model) = starting("dots");
        assert_eq!(
            model.apply(crate::Command::Accept),
            Some(crate::Outcome::Accept)
        );
    }

    #[test]
    fn empty_lua_tables_are_empty_lists() {
        let view =
            View::parse(r#"{ "accept": "a", "shortcuts": {}, "groups": {}, "rows": {} }"#).unwrap();
        assert!(view.shortcuts.is_empty() && view.groups.is_empty() && view.rows.is_empty());
    }

    #[test]
    fn unknown_fields_and_values_are_rejected() {
        for json in [
            r#"{ "accept": "a", "groups": [], "rows": [], "colour": "red" }"#,
            r#"{ "accept": "a", "groups": [], "rows": [], "shortcuts": { "o": 1 } }"#,
            r#"{ "accept": "a", "groups": [{ "id": "g", "label": "g", "columns": [{ "width": "wide" }] }], "rows": [] }"#,
            r#"{ "accept": "a", "groups": [{ "id": "g", "label": "g", "columns": [] }],
                 "rows": [{ "id": "r", "group": "g", "cells": [{ "text": "x", "role": "purple" }] }] }"#,
        ] {
            assert!(View::parse(json).is_err(), "{json}");
        }
    }

    #[test]
    fn references_are_checked_before_drawing() {
        let undeclared = r#"{ "accept": "a", "groups": [],
            "rows": [{ "id": "r", "group": "missing", "cells": [] }] }"#;
        let duplicate_row = r#"{ "accept": "a", "groups": [{ "id": "g", "label": "g", "columns": [] }],
            "rows": [{ "id": "r", "group": "g", "cells": [] }, { "id": "r", "group": "g", "cells": [] }] }"#;
        let duplicate_group = r#"{ "accept": "a", "rows": [],
            "groups": [{ "id": "g", "label": "g", "columns": [] }, { "id": "g", "label": "h", "columns": [] }] }"#;
        let no_accept = r#"{ "accept": " ", "groups": [], "rows": [] }"#;
        let prepare = |json| View::parse(json).unwrap().prepare(Palette::default());
        assert!(matches!(
            prepare(undeclared),
            Err(SpecError::UndeclaredGroup { .. })
        ));
        assert!(matches!(
            prepare(duplicate_row),
            Err(SpecError::InvalidRowId(_))
        ));
        assert!(matches!(
            prepare(duplicate_group),
            Err(SpecError::DuplicateGroup(_))
        ));
        assert!(matches!(prepare(no_accept), Err(SpecError::EmptyAccept)));
    }

    #[test]
    fn two_groups_with_one_scope_are_rejected_before_drawing() {
        let shared = r#"{ "accept": "a", "rows": [],
            "groups": [{ "id": "g", "label": "g", "columns": [], "scope": "@" },
                       { "id": "h", "label": "h", "columns": [], "scope": "@" }] }"#;
        let result = View::parse(shared)
            .unwrap()
            .prepare(Palette::default())
            .unwrap()
            .picker
            .build();
        assert!(
            matches!(result, Err(crate::Error::InvalidScope { sigil: '@' })),
            "{result:?}"
        );
    }
}
