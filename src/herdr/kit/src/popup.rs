//! Conveniences for a plugin whose surface is a herdr popup.

use std::io::{self, Read as _, Write as _};

use ratatui::crossterm::event::{self, Event, KeyEventKind};
use ratatui::crossterm::terminal;

/// Prints `error` and waits for one keypress.
///
/// A popup that exits silently is indistinguishable from a dead keybinding, so
/// a failure has to hold the window open long enough to be read.
pub(crate) fn report(error: &dyn std::fmt::Display) {
    let mut stderr = io::stderr();
    let _ = writeln!(stderr, "{error}");
    let _ = write!(stderr, "\n(press any key to close)");
    let _ = stderr.flush();
    hold_for_key();
}

/// Blocks until a key is pressed.
///
/// Raw mode is what makes this true to the prompt: in canonical mode the
/// terminal buffers a line, so a bare read would sit there until Enter.
/// Falling back to a line read keeps the pause when raw mode is unavailable.
fn hold_for_key() {
    if terminal::enable_raw_mode().is_err() {
        let _ = io::stdin().read(&mut [0u8; 1]);
        return;
    }
    while let Ok(read) = event::read() {
        if closes_popup(&read) {
            break;
        }
    }
    let _ = terminal::disable_raw_mode();
}

fn closes_popup(event: &Event) -> bool {
    matches!(
        event,
        Event::Key(key) if matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat)
    )
}

#[cfg(test)]
mod tests {
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    use super::*;

    #[test]
    fn a_key_release_does_not_immediately_close_the_error() {
        let mut key = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
        key.kind = KeyEventKind::Release;
        assert!(!closes_popup(&Event::Key(key)));

        key.kind = KeyEventKind::Press;
        assert!(closes_popup(&Event::Key(key)));
    }
}
