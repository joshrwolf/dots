//! The terminal the viewer draws on: set up on start and restored on exit
//! or panic.

use std::io::Write as _;

use anyhow::Result;
use ratatui::backend::TerminaBackend;
use ratatui::termina::escape::csi::{
    Csi, DecPrivateMode, DecPrivateModeCode, Keyboard, KittyKeyboardFlags, Mode,
};
use ratatui::termina::{PlatformTerminal, Terminal as _};

pub(crate) type Tui = ratatui::Terminal<TerminaBackend<PlatformTerminal>>;

/// Modes the viewer turns on: the alternate screen, mouse reports of
/// clicks, drags and motion in SGR form, bracketed paste, and theme change
/// reports.
const MODES: [DecPrivateModeCode; 7] = [
    DecPrivateModeCode::ClearAndEnableAlternateScreen,
    DecPrivateModeCode::MouseTracking,
    DecPrivateModeCode::ButtonEventMouse,
    DecPrivateModeCode::AnyEventMouse,
    DecPrivateModeCode::SGRMouse,
    DecPrivateModeCode::BracketedPaste,
    DecPrivateModeCode::Theme,
];

fn modes(on: bool) -> String {
    MODES
        .iter()
        .map(|&code| {
            let mode = DecPrivateMode::Code(code);
            Csi::Mode(if on {
                Mode::SetDecPrivateMode(mode)
            } else {
                Mode::ResetDecPrivateMode(mode)
            })
            .to_string()
        })
        .collect()
}

fn setup() -> String {
    // Unambiguous keys tell Ctrl+Enter from Enter where the terminal can.
    let keys = Keyboard::PushFlags(KittyKeyboardFlags::DISAMBIGUATE_ESCAPE_CODES);
    format!("{}{}", modes(true), Csi::Keyboard(keys))
}

fn restore() -> String {
    let show_cursor = Mode::SetDecPrivateMode(DecPrivateMode::Code(DecPrivateModeCode::ShowCursor));
    format!(
        "{}{}{}",
        Csi::Keyboard(Keyboard::PopFlags(1)),
        modes(false),
        Csi::Mode(show_cursor)
    )
}

/// Takes over the terminal. It returns to cooked mode when dropped.
pub(crate) fn start() -> Result<Tui> {
    let mut out = PlatformTerminal::new()?;
    out.enter_raw_mode()?;
    out.set_panic_hook(|handle| {
        let _ = write!(handle, "{}", restore());
        let _ = handle.flush();
    });
    write!(out, "{}", setup())?;
    out.flush()?;
    Ok(ratatui::Terminal::new(TerminaBackend::new(out))?)
}

/// Gives the terminal back as it was before [`start`].
pub(crate) fn stop(terminal: &mut Tui) -> Result<()> {
    let backend = terminal.backend_mut();
    write!(backend, "{}", restore())?;
    backend.flush()?;
    Ok(())
}

/// Writes `text` to the terminal now.
pub(crate) fn send(terminal: &mut Tui, text: &str) -> Result<()> {
    let backend = terminal.backend_mut();
    backend.write_all(text.as_bytes())?;
    backend.flush()?;
    Ok(())
}

/// Clears the screen and draws every cell at the next draw. Unlike
/// `Terminal::clear`, it does not ask the terminal where the cursor is: the
/// input thread reads the replies.
pub(crate) fn repaint(terminal: &mut Tui) -> Result<()> {
    let size = terminal.size()?;
    terminal.resize(ratatui::layout::Rect::from((
        ratatui::layout::Position::ORIGIN,
        size,
    )))?;
    Ok(())
}
