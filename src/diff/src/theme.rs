//! Colours derived from the terminal's own theme. The viewer asks for the
//! foreground, background and the first six palette colours (OSC 10, 11 and
//! 4) and mixes every tint from them, so it matches light and dark themes.

use std::fmt::Write as _;
use std::time::{Duration, Instant};

use ratatui::style::Color;

use crate::highlight::Role;

pub(crate) type Rgb = (u8, u8, u8);

/// What the terminal reported.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct Detected {
    pub(crate) fg: Option<Rgb>,
    pub(crate) bg: Option<Rgb>,
    /// Palette colours 1 to 6: red, green, yellow, blue, magenta, cyan.
    pub(crate) ansi: [Option<Rgb>; 6],
}

impl Detected {
    pub(crate) fn light() -> Self {
        Self {
            fg: Some((0x24, 0x29, 0x2f)),
            bg: Some((0xff, 0xff, 0xff)),
            ansi: [
                Some((0xcf, 0x22, 0x2e)),
                Some((0x1a, 0x7f, 0x37)),
                Some((0x9a, 0x67, 0x00)),
                Some((0x09, 0x69, 0xda)),
                Some((0x82, 0x50, 0xdf)),
                Some((0x1b, 0x7c, 0x83)),
            ],
        }
    }

    pub(crate) fn dark() -> Self {
        Self {
            fg: Some((0xe6, 0xed, 0xf3)),
            bg: Some((0x0d, 0x11, 0x17)),
            ansi: [
                Some((0xff, 0x7b, 0x72)),
                Some((0x3f, 0xb9, 0x50)),
                Some((0xd2, 0x99, 0x22)),
                Some((0x58, 0xa6, 0xff)),
                Some((0xbc, 0x8c, 0xff)),
                Some((0x39, 0xc5, 0xcf)),
            ],
        }
    }

    /// Fills whatever the terminal did not report from the built-in theme
    /// matching its background.
    fn complete(self) -> Self {
        let dark = self.bg.is_some_and(|bg| luma(bg) < 0.5);
        let fallback = if dark { Self::dark() } else { Self::light() };
        let mut ansi = self.ansi;
        for (slot, alt) in ansi.iter_mut().zip(fallback.ansi) {
            *slot = slot.or(alt);
        }
        Self {
            fg: self.fg.or(fallback.fg),
            bg: self.bg.or(fallback.bg),
            ansi,
        }
    }
}

/// Every colour the viewer paints.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Palette {
    pub(crate) dark: bool,
    pub(crate) bg: Color,
    pub(crate) fg: Color,
    pub(crate) muted: Color,
    pub(crate) faint: Color,
    pub(crate) rule: Color,
    pub(crate) panel: Color,
    pub(crate) selection: Color,
    pub(crate) accent: Color,
    pub(crate) green: Color,
    pub(crate) red: Color,
    pub(crate) yellow: Color,
    /// Modified files: yellow, deepened when too pale for the background.
    pub(crate) amber: Color,
    /// Quieter counts for lists.
    pub(crate) green_dim: Color,
    pub(crate) red_dim: Color,
    pub(crate) add_bg: Color,
    pub(crate) add_emph: Color,
    pub(crate) add_gutter: Color,
    pub(crate) del_bg: Color,
    pub(crate) del_emph: Color,
    pub(crate) del_gutter: Color,
    /// Moved code, in place of the added or removed tints.
    pub(crate) move_bg: Color,
    pub(crate) move_emph: Color,
    pub(crate) move_gutter: Color,
    pub(crate) move_fg: Color,
    pub(crate) gap_bg: Color,
    pub(crate) header_bg: Color,
    pub(crate) filler: Color,
    syntax: [Color; Role::COUNT],
    /// The raw values, for images drawn outside the cell grid.
    pub(crate) rgb: RawColors,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RawColors {
    pub(crate) bg: Rgb,
    pub(crate) fg: Rgb,
    pub(crate) muted: Rgb,
    pub(crate) green: Rgb,
    pub(crate) red: Rgb,
    pub(crate) amber: Rgb,
    pub(crate) accent: Rgb,
    pub(crate) header_bg: Rgb,
}

pub(crate) fn mix(a: Rgb, b: Rgb, t: f32) -> Rgb {
    let m = |x: u8, y: u8| {
        let v = f32::from(x) + (f32::from(y) - f32::from(x)) * t;
        // Clamped to 0..=255 first, so the cast cannot truncate.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let v = v.round().clamp(0.0, 255.0) as u8;
        v
    };
    (m(a.0, b.0), m(a.1, b.1), m(a.2, b.2))
}

pub(crate) fn luma(c: Rgb) -> f32 {
    (0.2126 * f32::from(c.0) + 0.7152 * f32::from(c.1) + 0.0722 * f32::from(c.2)) / 255.0
}

fn rgb(c: Rgb) -> Color {
    Color::Rgb(c.0, c.1, c.2)
}

impl Palette {
    pub(crate) fn new(detected: Detected) -> Self {
        let d = detected.complete();
        let (Some(bg), Some(fg)) = (d.bg, d.fg) else {
            unreachable!("complete() fills both")
        };
        let [red, green, yellow, blue, magenta, cyan] = d.ansi.map(|c| c.unwrap_or(fg));
        let dark = luma(bg) < 0.5;
        // Dark backgrounds need more colour for a tint to read. Whole lines
        // get a soft tint and their changed parts a strong one.
        let (row, emph, gutter) = if dark {
            (0.11, 0.38, 0.22)
        } else {
            (0.065, 0.28, 0.14)
        };
        let muted = mix(fg, bg, 0.45);
        // Pale terminal yellows vanish on light backgrounds; deepen only those.
        let amber = if !dark && luma(yellow) > 0.5 {
            mix(yellow, fg, 0.35)
        } else {
            yellow
        };
        let header_bg = mix(bg, fg, if dark { 0.08 } else { 0.045 });
        let syntax_of = |role: Role| -> Rgb {
            match role {
                Role::Keyword => magenta,
                Role::String => green,
                Role::Comment => muted,
                Role::Function => blue,
                Role::Type | Role::Attribute => yellow,
                Role::Constant | Role::Macro => cyan,
                Role::Operator | Role::Punctuation => mix(fg, bg, 0.25),
                Role::Plain | Role::Property | Role::Variable => fg,
                Role::Tag => red,
            }
        };
        let mut syntax = [Color::Reset; Role::COUNT];
        for role in Role::ALL {
            syntax[role as usize] = rgb(syntax_of(role));
        }
        Self {
            dark,
            bg: rgb(bg),
            fg: rgb(fg),
            muted: rgb(muted),
            faint: rgb(mix(fg, bg, 0.65)),
            rule: rgb(mix(fg, bg, 0.85)),
            panel: rgb(mix(bg, fg, if dark { 0.04 } else { 0.025 })),
            selection: rgb(mix(bg, blue, if dark { 0.25 } else { 0.14 })),
            accent: rgb(blue),
            green: rgb(green),
            red: rgb(red),
            yellow: rgb(yellow),
            amber: rgb(amber),
            green_dim: rgb(mix(green, bg, 0.3)),
            red_dim: rgb(mix(red, bg, 0.3)),
            add_bg: rgb(mix(bg, green, row)),
            add_emph: rgb(mix(bg, green, emph)),
            add_gutter: rgb(mix(bg, green, gutter)),
            del_bg: rgb(mix(bg, red, row)),
            del_emph: rgb(mix(bg, red, emph)),
            del_gutter: rgb(mix(bg, red, gutter)),
            move_bg: rgb(mix(bg, magenta, row)),
            move_emph: rgb(mix(bg, magenta, emph)),
            move_gutter: rgb(mix(bg, magenta, gutter)),
            move_fg: rgb(magenta),
            gap_bg: rgb(mix(bg, blue, if dark { 0.10 } else { 0.06 })),
            header_bg: rgb(header_bg),
            filler: rgb(mix(bg, fg, if dark { 0.03 } else { 0.02 })),
            syntax,
            rgb: RawColors {
                bg,
                fg,
                muted,
                green,
                red,
                amber,
                accent: blue,
                header_bg,
            },
        }
    }

    pub(crate) fn syntax(&self, role: Role) -> Color {
        self.syntax[role as usize]
    }
}

/// The query: colours, then primary device attributes, which every terminal
/// answers, so a reader knows when the replies are complete.
pub(crate) fn query() -> String {
    let mut q = String::from("\x1b]10;?\x1b\\\x1b]11;?\x1b\\");
    for i in 1..=6 {
        let _ = write!(q, "\x1b]4;{i};?\x1b\\");
    }
    q.push_str("\x1b[c");
    q
}

/// True once `buf` holds the device attributes reply ending a [`query`].
pub(crate) fn complete(buf: &[u8]) -> bool {
    buf.windows(3)
        .enumerate()
        .filter(|(_, w)| *w == b"\x1b[?")
        .any(|(i, _)| {
            buf[i + 3..]
                .iter()
                .find(|b| !(b.is_ascii_digit() || **b == b';'))
                .is_some_and(|&b| b == b'c')
        })
}

/// Reads OSC colour replies terminated by BEL or ST.
pub(crate) fn parse(reply: &[u8]) -> Detected {
    let text = String::from_utf8_lossy(reply);
    let mut d = Detected::default();
    for part in text.split("\x1b]").skip(1) {
        let body = &part[..part.find(['\x07', '\x1b']).unwrap_or(part.len())];
        let Some((head, spec)) = body.split_once(";rgb:") else {
            continue;
        };
        let Some(color) = parse_rgb(spec) else {
            continue;
        };
        match head {
            "10" => d.fg = Some(color),
            "11" => d.bg = Some(color),
            other => {
                if let Some(i) = other
                    .strip_prefix("4;")
                    .and_then(|i| i.parse::<usize>().ok())
                    && (1..=6).contains(&i)
                {
                    d.ansi[i - 1] = Some(color);
                }
            }
        }
    }
    d
}

/// An X11 colour spec such as `ffff/8080/0000`, one to four hex digits each.
fn parse_rgb(spec: &str) -> Option<Rgb> {
    let mut parts = spec.split('/').map(|h| {
        if h.is_empty() || h.len() > 4 {
            return None;
        }
        let v = u32::from_str_radix(h, 16).ok()?;
        let max = (1u32 << (4 * h.len())) - 1;
        u8::try_from((v * 255 + max / 2) / max).ok()
    });
    let c = (parts.next()??, parts.next()??, parts.next()??);
    parts.next().is_none().then_some(c)
}

/// Asks the terminal for its colours over the controlling terminal and
/// waits up to `timeout` for the answer. The caller must hold raw mode and
/// keep every other reader off standard input meanwhile.
pub(crate) fn ask(timeout: Duration) -> std::io::Result<Option<Detected>> {
    use std::io::Write as _;
    let mut out = std::io::stdout();
    out.write_all(query().as_bytes())?;
    out.flush()?;
    let deadline = Instant::now() + timeout;
    let stdin = rustix::stdio::stdin();
    let mut buf = Vec::new();
    while !complete(&buf) {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            break;
        }
        let mut fds = [rustix::event::PollFd::new(
            &stdin,
            rustix::event::PollFlags::IN,
        )];
        let wait = rustix::event::Timespec::try_from(left).unwrap_or(rustix::event::Timespec {
            tv_sec: 0,
            tv_nsec: 50_000_000,
        });
        if rustix::event::poll(&mut fds, Some(&wait))? == 0 {
            break;
        }
        let mut chunk = [0u8; 1024];
        let n = rustix::io::read(stdin, &mut chunk)?;
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..n]);
    }
    let d = parse(&buf);
    Ok((d.bg.is_some() || d.fg.is_some()).then_some(d))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mixes_colours_linearly() {
        assert_eq!(mix((0, 0, 0), (255, 255, 255), 0.5), (128, 128, 128));
        assert_eq!(mix((10, 20, 30), (10, 20, 30), 0.7), (10, 20, 30));
        assert_eq!(mix((255, 0, 0), (0, 0, 255), 0.0), (255, 0, 0));
    }

    #[test]
    fn tints_stay_close_to_the_background_in_both_themes() {
        for (detected, dark) in [(Detected::light(), false), (Detected::dark(), true)] {
            let p = Palette::new(detected);
            assert_eq!(p.dark, dark);
            let Color::Rgb(r, g, b) = p.add_bg else {
                panic!("rgb")
            };
            let bg = detected.bg.expect("set");
            let distance = |c: Rgb| (luma(c) - luma(bg)).abs();
            assert!(distance((r, g, b)) < 0.2, "add tint is subtle");
            let Color::Rgb(r2, g2, b2) = p.add_emph else {
                panic!("rgb")
            };
            assert!(
                distance((r2, g2, b2)) > distance((r, g, b)),
                "emphasis is stronger"
            );
        }
    }

    #[test]
    fn parses_osc_replies_in_both_terminators() {
        let reply = b"\x1b]11;rgb:ffff/ffff/ffff\x1b\\\x1b]10;rgb:24/29/2f\x07\x1b]4;2;rgb:1a1a/7f7f/3737\x1b\\\x1b[?62;22c";
        assert!(complete(reply));
        let d = parse(reply);
        assert_eq!(d.bg, Some((255, 255, 255)));
        assert_eq!(d.fg, Some((0x24, 0x29, 0x2f)));
        assert_eq!(d.ansi[1], Some((0x1a, 0x7f, 0x37)));
        assert!(!complete(b"\x1b]11;rgb:0/0/0\x07"));
    }

    #[test]
    fn missing_colours_fall_back_by_background() {
        let p = Palette::new(Detected {
            bg: Some((0, 0, 0)),
            ..Detected::default()
        });
        assert!(p.dark);
        assert_eq!(
            p,
            Palette::new(Detected {
                bg: Some((0, 0, 0)),
                ..Detected::dark()
            })
        );
    }
}
