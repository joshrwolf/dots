//! What a picker does with a choice: the host's action for Enter on a row or
//! for a Ctrl+letter shortcut, run while the picker stays on screen.

use std::future::Future;
use std::pin::Pin;

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use crate::{Entry, Error, Model, keys};

/// What the user chose, as the host's action sees it.
#[derive(Debug)]
pub enum Choice<'a, T, A> {
    /// Enter, on the selected row.
    Accept(&'a T),
    /// A shortcut's value, with what was chosen when it was pressed.
    Shortcut(&'a A, Chosen<'a, T>),
}

/// The row under the cursor, if any, and the typed line for a shortcut that
/// takes it.
#[derive(Debug, PartialEq, Eq)]
pub struct Chosen<'a, T> {
    pub entry: Option<&'a T>,
    /// The whole input line, never blank; `None` unless the shortcut takes
    /// the line.
    pub line: Option<&'a str>,
}

/// The work an action does. `Err` carries the message to show; the picker
/// stays open with it.
/// How a completed host action changes the current picker.
#[derive(Debug)]
pub enum Action<T: Entry> {
    Close,
    Stay,
    Push(crate::Picker<T>),
    Replace(crate::Picker<T>),
    Back,
}

pub type Acting<T> = Pin<Box<dyn Future<Output = Result<Action<T>, String>> + Send>>;

/// Starts the host's action for a choice. It runs on the calling thread and
/// returns the work, which the picker's background runtime drives.
pub type Act<T, A> = Box<dyn Fn(Choice<'_, T, A>) -> Acting<T>>;

/// A picker's actions: its shortcuts and what any choice does.
pub struct Actions<T: Entry, A> {
    pub shortcuts: Vec<Shortcut<A>>,
    pub act: Act<T, A>,
}

impl<T: Entry, A: std::fmt::Debug> std::fmt::Debug for Actions<T, A> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Actions")
            .field("shortcuts", &self.shortcuts)
            .finish_non_exhaustive()
    }
}

/// A control-letter shortcut with a typed value and a generated help entry.
#[derive(Debug)]
pub struct Shortcut<A> {
    key: char,
    label: String,
    takes_line: bool,
    hint: Option<String>,
    pub(super) value: A,
}

impl<A> Shortcut<A> {
    pub fn control(key: char, label: impl Into<String>, value: A) -> Self {
        Self {
            key,
            label: label.into(),
            takes_line: false,
            hint: None,
            value,
        }
    }

    /// The typed line is this shortcut's input: its action receives the line,
    /// and a blank line asks for input instead of running it. `hint` names
    /// what to type, such as `URL`, in the help and in that request.
    #[must_use]
    pub fn takes_line(mut self, hint: Option<String>) -> Self {
        self.takes_line = true;
        self.hint = hint;
        self
    }

    pub(super) fn key_label(&self) -> String {
        format!("ctrl+{}", self.key)
    }

    pub(super) fn label(&self) -> &str {
        &self.label
    }

    pub(super) fn hint(&self) -> Option<&str> {
        self.hint.as_deref()
    }

    pub(super) fn matches(&self, key: KeyEvent) -> bool {
        key.kind != KeyEventKind::Release
            && key.code == KeyCode::Char(self.key)
            && key.modifiers == KeyModifiers::CONTROL
    }

    /// What the action receives for the model's state, or `None` when the
    /// shortcut takes the line and it is blank.
    pub(super) fn chosen<'a, T: Entry>(&self, model: &'a Model<T>) -> Option<Chosen<'a, T>> {
        let line = if self.takes_line {
            let query = model.query();
            if query.trim().is_empty() {
                return None;
            }
            Some(query)
        } else {
            None
        };
        Some(Chosen {
            entry: model.selected_entry(),
            line,
        })
    }

    /// Why a blank line did not run the shortcut.
    pub(super) fn needs_input(&self) -> String {
        let what = match &self.hint {
            Some(hint) => format!("‹{hint}›"),
            None => "input".to_owned(),
        };
        format!("{} {} needs {what}", self.key_label(), self.label)
    }
}

pub(super) fn validate<A>(shortcuts: &[Shortcut<A>]) -> Result<(), Error> {
    let mut seen = std::collections::HashSet::new();
    let shown = |text: &str| !text.trim().is_empty() && !text.chars().any(char::is_control);
    for shortcut in shortcuts {
        let key = KeyEvent::new(KeyCode::Char(shortcut.key), KeyModifiers::CONTROL);
        if !shortcut.key.is_ascii_lowercase()
            || keys::command(key, 1).is_some()
            || !seen.insert(shortcut.key)
            || !shown(&shortcut.label)
            || !shortcut.hint.as_deref().is_none_or(shown)
        {
            return Err(Error::InvalidShortcut { key: shortcut.key });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use ratatui::style::Color;

    use super::*;
    use crate::{Cell, Column, Group, Palette, Picker};

    #[test]
    fn shortcuts_cannot_shadow_editing_navigation_or_each_other() {
        for key in ['j', 'k', 'u', 'c', 'r', 'R'] {
            assert_eq!(
                validate(&[Shortcut::control(key, "action", ())]).is_ok(),
                key == 'r'
            );
        }
        assert!(
            validate(&[
                Shortcut::control('r', "one", ()),
                Shortcut::control('r', "two", ())
            ])
            .is_err()
        );
        assert!(validate(&[Shortcut::control('r', "", ())]).is_err());
        assert!(validate(&[Shortcut::control('r', "bad\nlabel", ())]).is_err());
        let hinted = |hint: &str| Shortcut::control('r', "PR", ()).takes_line(Some(hint.into()));
        assert!(validate(&[hinted("URL")]).is_ok());
        assert!(validate(&[hinted(" ")]).is_err());
        assert!(validate(&[hinted("U\u{1b}RL")]).is_err());
    }

    #[test]
    fn control_shortcut_leaves_plain_typing_and_key_releases_alone() {
        let shortcut = Shortcut::control('r', "review URL", 42);
        let mut key = KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL);
        assert!(shortcut.matches(key));
        key.kind = KeyEventKind::Release;
        assert!(!shortcut.matches(key));
        assert!(!shortcut.matches(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE)));
        assert_eq!(
            (shortcut.key_label().as_str(), shortcut.label()),
            ("ctrl+r", "review URL")
        );
    }

    #[derive(Debug, PartialEq, Eq)]
    struct Fruit(&'static str);

    impl Entry for Fruit {
        type Group = ();
        fn group(&self) {}
        fn cells(&self, _palette: &Palette) -> Vec<Cell> {
            vec![Cell::new(self.0, Color::White)]
        }
    }

    fn typed(query: &str) -> Model<Fruit> {
        Picker::new(vec![Fruit("apple")])
            .group(Group::new((), "fruit", vec![Column::fill()]))
            .query(query)
            .build()
            .unwrap()
    }

    #[test]
    fn a_shortcut_that_takes_the_line_gets_it_with_the_row_if_any() {
        let line = Shortcut::control('r', "review", ()).takes_line(Some("URL".into()));
        let model = typed("apple");
        assert_eq!(
            line.chosen(&model),
            Some(Chosen {
                entry: Some(&Fruit("apple")),
                line: Some("apple"),
            })
        );
        let model = typed("fix the login bug");
        assert_eq!(
            line.chosen(&model),
            Some(Chosen {
                entry: None,
                line: Some("fix the login bug"),
            })
        );
    }

    #[test]
    fn a_blank_line_asks_for_the_input_the_hint_names() {
        let line = Shortcut::control('r', "review PR", ()).takes_line(Some("URL".into()));
        assert_eq!(line.chosen(&typed("  ")), None);
        assert_eq!(line.needs_input(), "ctrl+r review PR needs ‹URL›");
        let bare = Shortcut::control('s', "new", ()).takes_line(None);
        assert_eq!(bare.needs_input(), "ctrl+s new needs input");
    }

    #[test]
    fn other_shortcuts_get_only_the_row_and_run_on_a_blank_line() {
        let open = Shortcut::control('o', "open", ());
        assert_eq!(
            open.chosen(&typed("")),
            Some(Chosen {
                entry: Some(&Fruit("apple")),
                line: None,
            })
        );
    }
}
