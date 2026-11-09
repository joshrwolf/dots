//! Terminal input on its own thread. Every byte the terminal sends passes
//! one reader: replies to colour and graphics queries are taken out, and the
//! rest goes to
//! termina's parser as keys, mouse, paste and reports, so nothing typed
//! while the terminal answers a query is lost.

use std::io::IsTerminal as _;
use std::os::fd::{AsFd, OwnedFd};
use std::os::unix::net::UnixStream;
use std::sync::mpsc::Sender;
use std::time::Duration;

use anyhow::{Context as _, Result};
use ratatui::termina::escape::csi::{Csi, Device};
use ratatui::termina::event::{KeyCode, KeyEvent, Modifiers};
use ratatui::termina::{Event, Parser};
use rustix::event::{PollFd, PollFlags, Timespec};

use crate::theme::{self, Detected};

/// What the terminal sent.
#[derive(Debug)]
pub(crate) enum Input {
    Event(Event),
    /// The window changed size.
    Resized,
    /// The terminal finished answering a query.
    Replies(Replies),
}

/// What the terminal answered to a query, up to its device attributes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Replies {
    /// Its colours, when it reported any.
    pub(crate) colours: Option<Detected>,
    /// It accepted the kitty graphics query, when it was asked.
    pub(crate) graphics: bool,
}

const ESC: u8 = 0x1b;
const BEL: u8 = 0x07;
/// Longest reply; a longer sequence is not one.
const MAX_REPLY: usize = 256;
/// How long a lone Esc waits for the rest of a sequence.
const ESC_WAIT: Duration = Duration::from_millis(20);

/// Splits query replies out of terminal input and parses the rest.
#[derive(Debug, Default)]
pub(crate) struct Splitter {
    parser: Parser,
    /// An OSC or APC sequence being read, from its `ESC ]` or `ESC _`.
    reply: Option<Vec<u8>>,
    /// Colour replies to the current query.
    replies: Vec<u8>,
    /// The terminal accepted the graphics query.
    graphics: bool,
    /// A trailing Esc, which may start a sequence.
    esc: bool,
}

impl Splitter {
    /// Takes bytes read from the terminal and appends what they complete.
    pub(crate) fn feed(&mut self, bytes: &[u8], out: &mut Vec<Input>) {
        let mut plain = Vec::with_capacity(bytes.len());
        for &b in bytes {
            if let Some(reply) = &mut self.reply {
                reply.push(b);
                if b == BEL || reply.ends_with(&[ESC, b'\\']) {
                    let reply = self.reply.take().unwrap_or_default();
                    if let Some(ok) = graphics_reply(&reply) {
                        self.graphics |= ok;
                    } else if is_colour_reply(&reply) {
                        self.replies.extend_from_slice(&reply);
                    } else {
                        plain.extend_from_slice(&reply);
                    }
                } else if !could_be_reply(reply) {
                    // Alt+] or Alt+_, and what followed it.
                    let rest = reply.split_off(2);
                    let key = char::from(reply[1]);
                    self.reply = None;
                    self.parse(&plain, out);
                    plain.clear();
                    out.push(Input::Event(Event::Key(KeyEvent {
                        modifiers: Modifiers::ALT,
                        ..KeyEvent::from(KeyCode::Char(key))
                    })));
                    plain.extend_from_slice(&rest);
                }
                continue;
            }
            if std::mem::take(&mut self.esc) {
                if b == b']' || b == b'_' {
                    self.reply = Some(vec![ESC, b]);
                    continue;
                }
                plain.push(ESC);
            }
            if b == ESC {
                self.esc = true;
            } else {
                plain.push(b);
            }
        }
        self.parse(&plain, out);
    }

    /// Whether a lone Esc waits for what follows it.
    pub(crate) fn waiting(&self) -> bool {
        self.esc
    }

    /// Nothing followed a lone Esc: it was the Escape key.
    pub(crate) fn idle(&mut self, out: &mut Vec<Input>) {
        if std::mem::take(&mut self.esc) {
            self.parse(&[ESC], out);
        }
    }

    fn parse(&mut self, bytes: &[u8], out: &mut Vec<Input>) {
        if bytes.is_empty() {
            return;
        }
        self.parser.parse(bytes, false);
        while let Some(event) = self.parser.pop() {
            match event {
                // Every terminal answers device attributes, and in order,
                // so the replies before it are complete.
                Event::Csi(Csi::Device(Device::DeviceAttributes(()))) => {
                    let d = theme::parse(&std::mem::take(&mut self.replies));
                    out.push(Input::Replies(Replies {
                        colours: (d.bg.is_some() || d.fg.is_some()).then_some(d),
                        graphics: std::mem::take(&mut self.graphics),
                    }));
                }
                event => out.push(Input::Event(event)),
            }
        }
    }
}

/// A complete OSC 4, 10 or 11 reply.
fn is_colour_reply(osc: &[u8]) -> bool {
    [&b"4;"[..], b"10;", b"11;"]
        .iter()
        .any(|head| osc[2..].starts_with(head))
}

/// A complete reply to the graphics query: whether the terminal accepted it.
fn graphics_reply(apc: &[u8]) -> Option<bool> {
    let body = apc[2..].strip_prefix(crate::graphics::QUERY_REPLY)?;
    Some(body.starts_with(b"OK"))
}

/// Whether an unfinished sequence can still be a reply up to the length
/// limit: an OSC number then text, or an APC graphics reply.
fn could_be_reply(sequence: &[u8]) -> bool {
    let body = &sequence[2..];
    if sequence.len() > MAX_REPLY {
        return false;
    }
    if sequence[1] == b'_' {
        return body.first().is_none_or(|&b| b == b'G');
    }
    let digits = body.iter().take_while(|b| b.is_ascii_digit()).count();
    digits == body.len() || (digits > 0 && body[digits] == b';')
}

/// Reads the terminal on a thread, sending what it says until the receiver
/// is gone.
pub(crate) fn spawn<M: Send + 'static>(tx: Sender<M>, wrap: fn(Input) -> M) -> Result<()> {
    let tty: OwnedFd = if std::io::stdin().is_terminal() {
        rustix::io::dup(rustix::stdio::stdin())?
    } else {
        std::fs::File::open("/dev/tty")
            .context("opening /dev/tty")?
            .into()
    };
    let (resized, resized_tx) = UnixStream::pair()?;
    resized.set_nonblocking(true)?;
    signal_hook::low_level::pipe::register(signal_hook::consts::SIGWINCH, resized_tx)?;
    std::thread::spawn(move || {
        let mut splitter = Splitter::default();
        let mut out = Vec::new();
        let mut buf = [0u8; 4096];
        loop {
            let mut fds = [
                PollFd::new(&tty, PollFlags::IN),
                PollFd::new(&resized, PollFlags::IN),
            ];
            let wait = splitter
                .waiting()
                .then(|| Timespec::try_from(ESC_WAIT).ok())
                .flatten();
            match rustix::event::poll(&mut fds, wait.as_ref()) {
                Ok(0) => splitter.idle(&mut out),
                Ok(_) => {
                    let (tty_ready, resized_ready) =
                        (!fds[0].revents().is_empty(), !fds[1].revents().is_empty());
                    if resized_ready {
                        while rustix::io::read(resized.as_fd(), &mut [0u8; 64]).is_ok_and(|n| n > 0)
                        {
                        }
                        out.push(Input::Resized);
                    }
                    if tty_ready {
                        match rustix::io::read(&tty, &mut buf) {
                            Ok(0) | Err(_) => return,
                            Ok(n) => splitter.feed(&buf[..n], &mut out),
                        }
                    }
                }
                Err(rustix::io::Errno::INTR) => {}
                Err(_) => return,
            }
            for input in out.drain(..) {
                if tx.send(wrap(input)).is_err() {
                    return;
                }
            }
        }
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feed(chunks: &[&[u8]]) -> Vec<String> {
        let mut s = Splitter::default();
        let mut out = Vec::new();
        for chunk in chunks {
            s.feed(chunk, &mut out);
        }
        s.idle(&mut out);
        out.iter()
            .map(|i| match i {
                Input::Event(Event::Key(k)) if k.modifiers == Modifiers::ALT => {
                    format!("alt {:?}", k.code)
                }
                Input::Event(Event::Key(k)) => format!("{:?}", k.code),
                Input::Event(Event::Paste(p)) => format!("paste {p}"),
                Input::Event(e) => format!("{e:?}"),
                Input::Resized => "resized".to_owned(),
                Input::Replies(Replies {
                    colours: Some(d),
                    graphics,
                }) => format!(
                    "colours {:?} {:?} {:?}{}",
                    d.bg,
                    d.fg,
                    d.ansi[1],
                    if *graphics { " graphics" } else { "" }
                ),
                Input::Replies(Replies { graphics, .. }) => format!("replies graphics {graphics}"),
            })
            .collect()
    }

    #[test]
    fn keys_typed_during_a_colour_reply_are_all_delivered() {
        let reply: &[u8] =
            b"\x1b]11;rgb:ffff/ffff/ffff\x1b\\\x1b]10;rgb:24/29/2f\x07\x1b]4;2;rgb:1a1a/7f7f/3737\x1b\\\x1b[?62;22c";
        let mut typed = b"ab".to_vec();
        typed.extend_from_slice(&reply[..20]);
        let mut rest = reply[20..].to_vec();
        rest.extend_from_slice(b"\x1b[Ac\x1b");
        assert_eq!(
            feed(&[&typed, &rest]),
            [
                "Char('a')",
                "Char('b')",
                "colours Some((255, 255, 255)) Some((36, 41, 47)) Some((26, 127, 55))",
                "Up",
                "Char('c')",
                "Escape",
            ]
        );
    }

    #[test]
    fn a_reply_split_anywhere_is_still_taken_out() {
        let reply: &[u8] = b"x\x1b]11;rgb:0/0/0\x1b\\\x1b[?1c\x1b[200~p\x1b[201~";
        for at in 1..reply.len() {
            assert_eq!(
                feed(&[&reply[..at], &reply[at..]]),
                ["Char('x')", "colours Some((0, 0, 0)) None None", "paste p"],
                "split at {at}"
            );
        }
    }

    #[test]
    fn alt_bracket_and_other_osc_sequences_reach_the_parser() {
        assert_eq!(feed(&[b"\x1b]q"]), ["alt Char(']')", "Char('q')"]);
        // A theme report from a terminal following mode 2031.
        let report = feed(&[b"\x1b[?997;1n"]);
        assert_eq!(report.len(), 1);
        assert!(report[0].contains("ReportTheme(Dark)"), "{report:?}");
        // Device attributes with no colours report that they finished.
        assert_eq!(feed(&[b"\x1b[?62c"]), ["replies graphics false"]);
        assert_eq!(feed(&[b"\x1b_x"]), ["alt Char('_')", "Char('x')"]);
    }

    #[test]
    fn a_graphics_reply_is_taken_out_whole_and_split() {
        let reply: &[u8] = b"a\x1b_Gi=31;OK\x1b\\\x1b[?62;22cb";
        for at in 1..reply.len() {
            assert_eq!(
                feed(&[&reply[..at], &reply[at..]]),
                ["Char('a')", "replies graphics true", "Char('b')"],
                "split at {at}"
            );
        }
        let refused = feed(&[b"\x1b_Gi=31;ENOTSUPPORTED:no\x1b\\\x1b[?62c"]);
        assert_eq!(refused, ["replies graphics false"]);
    }
}
