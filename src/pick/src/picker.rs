//! The picker builder and its entry points.

use std::time::Duration;

use crate::live::{self, Fetch};
use crate::model::Model;
use crate::run::Ended;
use crate::spec::{PreviewAnchor, PreviewPosition};
use crate::{Actions, Entry, Error, Group, Palette, run, shortcut, text};

/// How a picker running actions closed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Closed {
    /// An action succeeded.
    Acted,
    /// The user closed it without choosing.
    Cancelled,
}

#[derive(Debug)]
#[must_use]
pub struct Picker<T: Entry> {
    items: Vec<T>,
    groups: Vec<Group<T::Group>>,
    prompt: String,
    query: String,
    selected: Option<String>,
    palette: Palette,
    match_paths: bool,
    live: run::Live<T>,
}

impl<T: Entry> Picker<T> {
    pub fn new(items: Vec<T>) -> Self {
        Self {
            items,
            groups: Vec::new(),
            prompt: "> ".to_owned(),
            query: String::new(),
            selected: None,
            // Loading the theme adapter can fail, so callers opt in with
            // `.palette(Palette::load()?)`.
            palette: Palette::default(),
            match_paths: false,
            live: run::Live::default(),
        }
    }

    /// Keep the rows current while the picker runs: every line `watch`
    /// prints, its first included, makes the picker `fetch` the rows again and
    /// replace them. Entries need identities so the selection survives a
    /// refresh; a failed or invalid refresh keeps the rows shown and says so.
    pub fn live(mut self, watch: Vec<String>, fetch: Fetch<T>) -> Self {
        self.live.feed = Some((live::Trigger::Command(watch), fetch));
        self
    }

    /// Refresh rows on a timer while this page is visible.
    pub fn refresh_every(mut self, interval: Duration, fetch: Fetch<T>) -> Self {
        self.live.feed = Some((live::Trigger::Interval(interval), fetch));
        self
    }

    /// Re-run the selected entry's preview this often, so it follows a
    /// changing screen.
    pub fn preview_every(mut self, interval: Duration) -> Self {
        self.live.preview_every = Some(interval);
        self
    }

    /// Which end of a preview stays visible when its output is taller than the pane.
    pub fn preview_anchor(mut self, anchor: PreviewAnchor) -> Self {
        self.live.preview_layout.anchor = anchor;
        self
    }

    /// Where the preview pane sits: beside the list or below it.
    pub fn preview_position(mut self, position: PreviewPosition) -> Self {
        self.live.preview_layout.position = position;
        self
    }

    pub fn group(mut self, group: Group<T::Group>) -> Self {
        self.groups.push(group);
        self
    }

    pub fn groups(mut self, groups: impl IntoIterator<Item = Group<T::Group>>) -> Self {
        self.groups.extend(groups);
        self
    }

    /// The text before the query, kept to one line like every other label.
    pub fn prompt(mut self, prompt: impl Into<String>) -> Self {
        self.prompt = text::display(prompt.into());
        self
    }

    /// Start with a query, sanitized like typed and pasted input.
    pub fn query(mut self, query: impl Into<String>) -> Self {
        self.query = query.into();
        self
    }

    /// Preselect an exact row identity, revealing its scope rather than choosing another row.
    pub fn selected(mut self, id: impl Into<String>) -> Self {
        self.selected = Some(id.into());
        self
    }

    pub fn palette(mut self, palette: Palette) -> Self {
        self.palette = palette;
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
        let mut model = Model::build(self.items, self.groups, self.palette, self.match_paths)?;
        model.insert_text(&self.query);
        if let Some(id) = self.selected {
            model.select_identity(&id)?;
        }
        Ok(model)
    }

    pub(crate) fn prepare(mut self) -> Result<(Model<T>, String, run::Live<T>), Error> {
        let prompt = self.prompt.clone();
        let live = std::mem::take(&mut self.live);
        Ok((self.build()?, prompt, live))
    }

    /// Draws the list and blocks until the user chooses or aborts.
    ///
    /// `Ok(None)` is an abort.
    ///
    /// # Panics
    ///
    /// When a live feed or preview job panics. The panic resumes on this
    /// thread once the terminal has been restored.
    pub fn run(mut self) -> Result<Option<T>, Error>
    where
        T: Send + 'static,
    {
        let prompt = self.prompt.clone();
        let live = std::mem::take(&mut self.live);
        let model = self.build()?;
        Ok(
            match run::run::<T, std::convert::Infallible>(model, &prompt, None, live)? {
                Ended::Chosen(entry) => Some(entry),
                Ended::Acted | Ended::Cancelled => None,
            },
        )
    }

    /// Runs `actions` in place: Enter or a shortcut starts the host's action
    /// while the picker stays on screen. Navigation preserves pending input;
    /// closing or failing actions discard it. Actions can close, push or replace a page, or go back; a failure shows its message and
    /// keeps the query, selection, and scope. Back restores the parent page. Returns once an action closes
    /// or the user cancels, and says which. Shortcuts work even when a query matches nothing;
    /// normal editing keys are reserved.
    ///
    /// # Panics
    ///
    /// As [`Self::run`], and when an action's work panics.
    pub fn run_actions<A>(mut self, actions: &Actions<T, A>) -> Result<Closed, Error>
    where
        T: Send + 'static,
    {
        shortcut::validate(&actions.shortcuts)?;
        let prompt = self.prompt.clone();
        let live = std::mem::take(&mut self.live);
        let model = self.build()?;
        Ok(match run::run(model, &prompt, Some(actions), live)? {
            Ended::Acted => Closed::Acted,
            Ended::Cancelled => Closed::Cancelled,
            Ended::Chosen(_) => unreachable!("with actions, Enter runs the accept action"),
        })
    }
}

#[cfg(test)]
mod tests {
    use ratatui::style::Color;

    use super::*;
    use crate::{Cell, Column};

    #[derive(Debug)]
    struct Item;

    impl Entry for Item {
        type Group = ();
        fn group(&self) {}
        fn cells(&self, _palette: &Palette) -> Vec<Cell> {
            vec![Cell::new("item", Color::White)]
        }

        fn identity(&self) -> Option<std::borrow::Cow<'_, str>> {
            Some(std::borrow::Cow::Borrowed("item"))
        }
    }

    #[test]
    fn exact_selection_reveals_a_scoped_row_and_rejects_missing_identity() {
        let model = Picker::new(vec![Item])
            .group(Group::new((), "completed", vec![Column::fill()]).scoped('='))
            .selected("item")
            .build()
            .unwrap();
        assert_eq!(model.selected(), Some(0));
        assert_eq!(model.scope_label(), "completed");
        assert!(matches!(
            Picker::new(vec![Item])
                .group(Group::new((), "items", vec![Column::fill()]))
                .selected("absent")
                .build(),
            Err(Error::MissingSelection { .. })
        ));
    }

    #[test]
    fn an_equivalent_live_snapshot_does_not_invalidate_the_preview() {
        let mut model = Picker::new(vec![Item])
            .group(Group::new((), "items", vec![Column::fill()]))
            .build()
            .unwrap();
        assert!(!model.replace_all(vec![Item]).unwrap());
        assert!(model.replace_all(Vec::new()).unwrap());
    }

    #[test]
    fn a_prompt_stays_on_one_line() {
        let picker = Picker::new(vec![Item]).prompt("go\n\u{1b}[2J\t> ");
        assert_eq!(picker.prompt, "go\u{fffd}\u{fffd}[2J > ");
    }
}
