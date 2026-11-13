//! Pictures of a drawn frame: as text, and as a PNG.

use std::path::Path;

use anyhow::{Context as _, Result};
use ratatui::buffer::Buffer;
use ratatui::style::{Color, Modifier};

/// The buffer as lines of text, with SGR colours when `ansi`.
#[cfg(any(test, feature = "dev"))]
pub(crate) fn dump(buf: &Buffer, ansi: bool) -> String {
    use std::fmt::Write as _;
    let mut out = String::new();
    let area = buf.area;
    for y in area.top()..area.bottom() {
        let mut last = None;
        for x in area.left()..area.right() {
            let cell = &buf[(x, y)];
            if ansi {
                let style = (cell.fg, cell.bg, cell.modifier);
                if last != Some(style) {
                    out.push_str("\x1b[0");
                    if let Color::Rgb(r, g, b) = cell.fg {
                        let _ = write!(out, ";38;2;{r};{g};{b}");
                    }
                    if let Color::Rgb(r, g, b) = cell.bg {
                        let _ = write!(out, ";48;2;{r};{g};{b}");
                    }
                    if cell.modifier.contains(Modifier::BOLD) {
                        out.push_str(";1");
                    }
                    if cell.modifier.contains(Modifier::ITALIC) {
                        out.push_str(";3");
                    }
                    out.push('m');
                    last = Some(style);
                }
            }
            out.push_str(cell.symbol());
        }
        if ansi {
            out.push_str("\x1b[0m");
        }
        let trimmed = out.trim_end_matches(' ').len();
        if !ansi {
            out.truncate(trimmed);
        }
        out.push('\n');
    }
    out
}

/// Draws the buffer as a PNG-like RGBA picture for eyeballing colours
/// without a terminal: cells as rectangles, text in a monospace font.
#[expect(clippy::many_single_char_names, reason = "cell and pixel coordinates")]
pub(crate) fn picture(buf: &Buffer, path: &Path) -> Result<()> {
    use cosmic_text::{Attrs, Buffer as TextBuffer, Family, Metrics, Shaping, SwashCache, Weight};
    let (cw, ch) = (9u32, 19u32);
    let area = buf.area;
    let (w, h) = (u32::from(area.width) * cw, u32::from(area.height) * ch);
    let mut pixmap = tiny_skia::Pixmap::new(w, h).context("picture size")?;
    let rgb = |c: Color, fallback: (u8, u8, u8)| match c {
        Color::Rgb(r, g, b) => (r, g, b),
        _ => fallback,
    };
    for y in area.top()..area.bottom() {
        for x in area.left()..area.right() {
            let (r, g, b) = rgb(buf[(x, y)].bg, (255, 255, 255));
            let rect =
                tiny_skia::Rect::from_xywh(f32::from(x) * 9.0, f32::from(y) * 19.0, 9.0, 19.0)
                    .context("cell")?;
            let mut paint = tiny_skia::Paint::default();
            paint.set_color_rgba8(r, g, b, 255);
            pixmap.fill_rect(rect, &paint, tiny_skia::Transform::identity(), None);
        }
    }
    let mut fonts = crate::graphics::font_system();
    let mut cache = SwashCache::new();
    let data_w = pixmap.width();
    for y in area.top()..area.bottom() {
        for x in area.left()..area.right() {
            let cell = &buf[(x, y)];
            let symbol = cell.symbol();
            if symbol.trim().is_empty() {
                continue;
            }
            let (r, g, b) = rgb(cell.fg, (0, 0, 0));
            let weight = if cell.modifier.contains(Modifier::BOLD) {
                Weight::BOLD
            } else {
                Weight::NORMAL
            };
            let mut text = TextBuffer::new(&mut fonts, Metrics::new(14.0, 19.0));
            text.set_size(None, None);
            text.set_text(
                symbol,
                &Attrs::new().family(Family::Monospace).weight(weight),
                Shaping::Advanced,
                None,
            );
            text.shape_until_scroll(&mut fonts, false);
            let (ox, oy) = (i32::from(x) * 9, i32::from(y) * 19);
            let data = pixmap.data_mut();
            text.draw(
                &mut fonts,
                &mut cache,
                cosmic_text::Color::rgb(r, g, b),
                |gx, gy, gw, gh, color| {
                    let a = u32::from(color.a());
                    for yy in oy + gy..oy + gy + i32::try_from(gh).unwrap_or(0) {
                        for xx in ox + gx..ox + gx + i32::try_from(gw).unwrap_or(0) {
                            let (Ok(ux), Ok(uy)) = (u32::try_from(xx), u32::try_from(yy)) else {
                                continue;
                            };
                            if ux >= data_w || uy >= h {
                                continue;
                            }
                            let i = ((uy * data_w + ux) * 4) as usize;
                            let blend = |d: u8, s: u8| {
                                u8::try_from((u32::from(s) * a + u32::from(d) * (255 - a)) / 255)
                                    .unwrap_or(255)
                            };
                            data[i] = blend(data[i], color.r());
                            data[i + 1] = blend(data[i + 1], color.g());
                            data[i + 2] = blend(data[i + 2], color.b());
                        }
                    }
                },
            );
        }
    }
    pixmap
        .save_png(path)
        .with_context(|| format!("writing {}", path.display()))?;
    Ok(())
}
