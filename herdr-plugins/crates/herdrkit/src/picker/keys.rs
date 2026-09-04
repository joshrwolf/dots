//! Keys, as a pure mapping.
//!
//! Separating this from the event loop is what makes "the bindings are fzf's,
//! exactly" a claim with a test behind it rather than an assertion.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

/// One edit or movement, independent of which key produced it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    Insert(char),
    DeleteBack,
    DeleteWord,
    ClearQuery,
    CaretHome,
    CaretEnd,
    CaretLeft,
    CaretRight,
    Next(usize),
    Previous(usize),
    Accept,
    Abort,
}

/// fzf's bindings. `page` is how far a page key moves, which is whatever is on
/// screen.
///
/// A popup receives *all* terminal input including Escape, so no herdr
/// keybinding can intercept anything — every key is this mapping's to handle,
/// and any key it declines is simply dead.
pub fn command(key: KeyEvent, page: usize) -> Option<Command> {
    // Windows reports a release for every press, and a held key repeats.
    if !matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) {
        return None;
    }
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let alt = key.modifiers.contains(KeyModifiers::ALT);

    let command = match key.code {
        KeyCode::Char(ch) if ctrl => match ch {
            'c' | 'g' | 'q' => Command::Abort,
            'j' | 'n' => Command::Next(1),
            'k' | 'p' => Command::Previous(1),
            'h' => Command::DeleteBack,
            'u' => Command::ClearQuery,
            'w' => Command::DeleteWord,
            'a' => Command::CaretHome,
            'e' => Command::CaretEnd,
            'b' => Command::CaretLeft,
            'f' => Command::CaretRight,
            _ => return None,
        },
        KeyCode::Char(ch) if !alt => Command::Insert(ch),
        KeyCode::Backspace => Command::DeleteBack,
        KeyCode::Left => Command::CaretLeft,
        KeyCode::Right => Command::CaretRight,
        KeyCode::Home => Command::CaretHome,
        KeyCode::End => Command::CaretEnd,
        KeyCode::Down => Command::Next(1),
        KeyCode::Up => Command::Previous(1),
        KeyCode::PageDown => Command::Next(page),
        KeyCode::PageUp => Command::Previous(page),
        KeyCode::Enter => Command::Accept,
        KeyCode::Esc => Command::Abort,
        _ => return None,
    };
    Some(command)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press(code: KeyCode, modifiers: KeyModifiers) -> Option<Command> {
        command(KeyEvent::new(code, modifiers), 10)
    }

    fn plain(code: KeyCode) -> Option<Command> {
        press(code, KeyModifiers::NONE)
    }

    fn ctrl(ch: char) -> Option<Command> {
        press(KeyCode::Char(ch), KeyModifiers::CONTROL)
    }

    #[test]
    fn movement_matches_fzf() {
        for down in [ctrl('j'), ctrl('n'), plain(KeyCode::Down)] {
            assert_eq!(down, Some(Command::Next(1)));
        }
        for up in [ctrl('k'), ctrl('p'), plain(KeyCode::Up)] {
            assert_eq!(up, Some(Command::Previous(1)));
        }
        assert_eq!(plain(KeyCode::PageDown), Some(Command::Next(10)));
        assert_eq!(plain(KeyCode::PageUp), Some(Command::Previous(10)));
    }

    #[test]
    fn editing_matches_fzf() {
        assert_eq!(ctrl('h'), Some(Command::DeleteBack));
        assert_eq!(plain(KeyCode::Backspace), Some(Command::DeleteBack));
        assert_eq!(ctrl('u'), Some(Command::ClearQuery));
        assert_eq!(ctrl('w'), Some(Command::DeleteWord));
        assert_eq!(ctrl('a'), Some(Command::CaretHome));
        assert_eq!(ctrl('e'), Some(Command::CaretEnd));
        assert_eq!(ctrl('b'), Some(Command::CaretLeft));
        assert_eq!(ctrl('f'), Some(Command::CaretRight));
    }

    #[test]
    fn every_abort_key_aborts() {
        assert_eq!(plain(KeyCode::Esc), Some(Command::Abort));
        for ch in ['c', 'g', 'q'] {
            assert_eq!(ctrl(ch), Some(Command::Abort));
        }
    }

    #[test]
    fn a_shifted_letter_is_typed_and_a_control_letter_is_not() {
        assert_eq!(
            press(KeyCode::Char('A'), KeyModifiers::SHIFT),
            Some(Command::Insert('A'))
        );
        assert_eq!(plain(KeyCode::Char('q')), Some(Command::Insert('q')));
    }

    /// Alt chords belong to the terminal and the window manager; swallowing
    /// them as text would type a letter the user did not ask for.
    #[test]
    fn alt_chords_are_declined() {
        assert_eq!(press(KeyCode::Char('f'), KeyModifiers::ALT), None);
    }

    #[test]
    fn a_release_is_not_a_press() {
        let mut key = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
        key.kind = KeyEventKind::Release;
        assert_eq!(command(key, 10), None);
        key.kind = KeyEventKind::Repeat;
        assert_eq!(command(key, 10), Some(Command::Accept));
    }

    #[test]
    fn an_unmapped_key_is_declined_rather_than_swallowed() {
        assert_eq!(plain(KeyCode::F(5)), None);
        assert_eq!(plain(KeyCode::Tab), None);
        assert_eq!(ctrl('z'), None);
    }
}
