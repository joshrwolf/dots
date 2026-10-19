//! Explicit exits from a picker, independent of the selected row or query.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use super::{Error, command};

#[derive(Debug, PartialEq, Eq)]
pub enum Selection<T, A> {
    Entry(T),
    Shortcut(A),
    Cancelled,
}

/// A control-letter shortcut with a typed return value and a generated hint.
#[derive(Debug)]
pub struct Shortcut<A> {
    key: char,
    label: String,
    pub(super) value: A,
}

impl<A> Shortcut<A> {
    pub fn control(key: char, label: impl Into<String>, value: A) -> Self {
        Self {
            key,
            label: label.into(),
            value,
        }
    }

    pub(super) fn hint(&self) -> String {
        format!("ctrl+{} {}", self.key, self.label)
    }

    pub(super) fn matches(&self, key: KeyEvent) -> bool {
        key.kind != KeyEventKind::Release
            && key.code == KeyCode::Char(self.key)
            && key.modifiers == KeyModifiers::CONTROL
    }
}

pub(super) fn validate<A>(shortcuts: &[Shortcut<A>]) -> Result<(), Error> {
    let mut seen = std::collections::HashSet::new();
    for shortcut in shortcuts {
        let key = KeyEvent::new(KeyCode::Char(shortcut.key), KeyModifiers::CONTROL);
        if !shortcut.key.is_ascii_lowercase()
            || command(key, 1).is_some()
            || !seen.insert(shortcut.key)
            || shortcut.label.trim().is_empty()
            || shortcut.label.chars().any(char::is_control)
        {
            return Err(Error::InvalidShortcut { key: shortcut.key });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

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
    }

    #[test]
    fn control_shortcut_leaves_plain_typing_and_key_releases_alone() {
        let shortcut = Shortcut::control('r', "review URL", 42);
        let mut key = KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL);
        assert!(shortcut.matches(key));
        key.kind = KeyEventKind::Release;
        assert!(!shortcut.matches(key));
        assert!(!shortcut.matches(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE)));
        assert_eq!(shortcut.hint(), "ctrl+r review URL");
    }
}
