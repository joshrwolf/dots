//! `--graphics`: file headers drawn as kitty graphics images, pinned to the
//! header's cells (`c=`, `r=`) so every client scales them to its own cell
//! size. Code stays text. Each image is drawn once per distinct header and
//! re-placed as it moves; a new placement goes up before the old one is
//! deleted, so headers never flicker.

use std::collections::HashMap;
use std::hash::{Hash as _, Hasher as _};
use std::io::Write as _;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::Result;
use base64::Engine as _;
use cosmic_text::{Attrs, Buffer, Family, FontSystem, Metrics, Shaping, SwashCache, Weight};
use tiny_skia::{FillRule, Paint, PathBuilder, Pixmap, Stroke, Transform};

use crate::app::{App, Line};
use crate::repo::Status;
use crate::theme::{Rgb, mix};

struct Fonts {
    system: FontSystem,
    cache: SwashCache,
}

/// One header image's content; equal specs share an image.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct Spec {
    path: String,
    /// The base path of a rename.
    from: Option<String>,
    /// A mode change, as text.
    note: Option<String>,
    folded: bool,
    status: Status,
    counts: Option<(u32, u32)>,
    reviewed: bool,
    cols: u16,
    /// Foreground, muted, green, red, amber and background.
    colors: [Rgb; 6],
    cell: (u16, u16),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Placed {
    image: u32,
    placement: u32,
    x: u16,
    y: u16,
}

pub(crate) struct Graphics {
    fonts: Arc<Mutex<Option<Fonts>>>,
    images: HashMap<u64, (u32, Instant)>,
    placed: Vec<Placed>,
    next_id: u32,
    next_placement: u32,
    /// Time spent drawing and encoding images, for `--timings`.
    pub(crate) drawn: Vec<Duration>,
}

impl std::fmt::Debug for Graphics {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Graphics")
            .field("images", &self.images.len())
            .finish_non_exhaustive()
    }
}

/// Images kept uploaded; older ones are deleted from the terminal.
const KEEP: usize = 48;

impl Graphics {
    pub(crate) fn new() -> Self {
        let fonts = Arc::new(Mutex::new(None));
        let loading = Arc::clone(&fonts);
        // Loading system fonts takes a while; headers stay text until then.
        std::thread::spawn(move || {
            let loaded = Fonts {
                system: FontSystem::new(),
                cache: SwashCache::new(),
            };
            if let Ok(mut slot) = loading.lock() {
                *slot = Some(loaded);
            }
        });
        Self {
            fonts,
            images: HashMap::new(),
            placed: Vec::new(),
            next_id: 1,
            next_placement: 1,
            drawn: Vec::new(),
        }
    }

    /// True until fonts have loaded and a redraw can show images.
    pub(crate) fn pending(&self) -> bool {
        self.fonts.lock().is_ok_and(|f| f.is_none())
    }

    /// Forgets every image, after a theme change.
    pub(crate) fn invalidate(&mut self) {
        self.clear();
    }

    pub(crate) fn clear(&mut self) {
        let mut out = std::io::stdout().lock();
        let _ = out.write_all(b"\x1b_Ga=d,d=A,q=2\x1b\\");
        let _ = out.flush();
        self.images.clear();
        self.placed.clear();
    }

    /// Places an image over every header on screen, after the text frame.
    pub(crate) fn after_draw(&mut self, app: &App) -> Result<()> {
        let fonts_slot = Arc::clone(&self.fonts);
        let Ok(mut guard) = fonts_slot.lock() else {
            return Ok(());
        };
        let Some(fonts) = guard.as_mut() else {
            return Ok(());
        };
        if app.stream.is_none() {
            drop(guard);
            return self.place(Vec::new(), 0, 0);
        }
        let area = app.areas.diff;
        let rows = app.settings.header_rows;
        let cell = cell_pixels();
        let mut headers = Vec::new();
        if area.height >= rows {
            let pinned = app.pinned_header();
            if let Some(file) = pinned {
                headers.push((file, area.y));
            }
            let top = app.global_top();
            let first = if pinned.is_some() { rows } else { 0 };
            for y in first..area.height.saturating_sub(rows - 1) {
                if let Line::Header { file, part: 0 } = app.line_at(top + u32::from(y)) {
                    headers.push((file, area.y + y));
                }
            }
        }
        let mut out = Vec::new();
        let mut wanted = Vec::new();
        for (file, y) in headers {
            let spec = spec_for(app, file, area.width, cell);
            let mut hasher = std::hash::DefaultHasher::new();
            spec.hash(&mut hasher);
            let key = hasher.finish();
            let id = if let Some((id, used)) = self.images.get_mut(&key) {
                *used = Instant::now();
                *id
            } else {
                let started = Instant::now();
                let pixmap = render(fonts, &spec, rows);
                let id = self.next_id;
                self.next_id += 1;
                upload(&mut out, &pixmap, id);
                self.drawn.push(started.elapsed());
                self.images.insert(key, (id, Instant::now()));
                id
            };
            wanted.push((id, area.x, y));
        }
        drop(guard);
        let mut stdout = std::io::stdout().lock();
        stdout.write_all(&out)?;
        drop(stdout);
        self.place(wanted, area.width, rows)?;
        self.evict();
        Ok(())
    }

    fn place(&mut self, wanted: Vec<(u32, u16, u16)>, cols: u16, rows: u16) -> Result<()> {
        let mut out = Vec::new();
        let mut kept = Vec::new();
        for (image, x, y) in wanted {
            if let Some(p) = self
                .placed
                .iter()
                .find(|p| p.image == image && p.x == x && p.y == y)
            {
                kept.push(*p);
                continue;
            }
            let placement = self.next_placement;
            self.next_placement += 1;
            write!(
                out,
                "\x1b7\x1b[{};{}H\x1b_Ga=p,i={image},p={placement},c={cols},r={rows},C=1,q=2\x1b\\\x1b8",
                y + 1,
                x + 1
            )?;
            kept.push(Placed {
                image,
                placement,
                x,
                y,
            });
        }
        for old in &self.placed {
            if !kept.contains(old) {
                write!(
                    out,
                    "\x1b_Ga=d,d=i,i={},p={},q=2\x1b\\",
                    old.image, old.placement
                )?;
            }
        }
        self.placed = kept;
        let mut stdout = std::io::stdout().lock();
        stdout.write_all(&out)?;
        stdout.flush()?;
        Ok(())
    }

    fn evict(&mut self) {
        if self.images.len() <= KEEP {
            return;
        }
        let mut by_age: Vec<(u64, u32, Instant)> = self
            .images
            .iter()
            .map(|(k, (id, t))| (*k, *id, *t))
            .collect();
        by_age.sort_by_key(|e| e.2);
        let mut out = std::io::stdout().lock();
        for (key, id, _) in by_age.into_iter().take(self.images.len() - KEEP) {
            if self.placed.iter().any(|p| p.image == id) {
                continue;
            }
            let _ = write!(out, "\x1b_Ga=d,d=I,i={id},q=2\x1b\\");
            self.images.remove(&key);
        }
        let _ = out.flush();
    }
}

/// What the header image of `file` shows.
fn spec_for(app: &App, file: usize, cols: u16, cell: (u16, u16)) -> Spec {
    let stream = app
        .stream
        .as_ref()
        .expect("headers are drawn for a loaded stream");
    let f = &stream.files[file];
    let p = &app.palette.rgb;
    Spec {
        path: f.change.path.clone(),
        from: f.change.from.clone(),
        note: f.change.mode_note(),
        folded: f.folded,
        status: f.change.status,
        counts: f.counts(),
        reviewed: f.reviewed,
        cols,
        colors: [p.fg, p.muted, p.green, p.red, p.amber, p.header_bg],
        cell,
    }
}

/// Draws the header image of the file at the top of the view to a PNG,
/// to check it without a terminal.
pub(crate) fn header_picture(app: &App, cols: u16, path: &std::path::Path) -> Result<()> {
    let mut fonts = Fonts {
        system: FontSystem::new(),
        cache: SwashCache::new(),
    };
    let spec = spec_for(app, app.top.file, cols, (10, 20));
    let pixmap = render(&mut fonts, &spec, 2);
    pixmap.save_png(path)?;
    Ok(())
}

/// Pixels per cell, from the terminal; a common size when it does not say.
fn cell_pixels() -> (u16, u16) {
    match ratatui::crossterm::terminal::window_size() {
        Ok(s) if s.width > 0 && s.columns > 0 && s.rows > 0 => {
            (s.width / s.columns, s.height / s.rows)
        }
        _ => (10, 20),
    }
}

fn skia(c: Rgb) -> tiny_skia::Color {
    tiny_skia::Color::from_rgba8(c.0, c.1, c.2, 255)
}

fn text_color(c: Rgb) -> cosmic_text::Color {
    cosmic_text::Color::rgb(c.0, c.1, c.2)
}

/// Draws a header: a status dot, the path in a proportional font, counts,
/// and a check mark once the file is reviewed.
#[allow(clippy::too_many_lines)]
fn render(fonts: &mut Fonts, spec: &Spec, rows: u16) -> Pixmap {
    // Draw at twice the cell size when the terminal reports small cells, so
    // high-density screens stay sharp.
    let scale = if spec.cell.1 < 24 { 2.0 } else { 1.0 };
    let cw = f32::from(spec.cell.0) * scale;
    let w_px = (f32::from(spec.cols) * cw).max(1.0);
    let h_px = (f32::from(spec.cell.1 * rows) * scale).max(1.0);
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let mut pixmap = Pixmap::new(w_px as u32, h_px as u32).expect("header size is positive");
    let [fg, muted, green, red, amber, header_bg] = spec.colors;
    pixmap.fill(skia(header_bg));
    // A hairline under the header separates it from code.
    let rule = mix(header_bg, fg, 0.12);
    if let Some(r) = tiny_skia::Rect::from_xywh(0.0, h_px - scale, w_px, scale) {
        pixmap.fill_rect(r, &paint(rule), Transform::identity(), None);
    }
    let status_rgb = match spec.status {
        Status::Added => green,
        Status::Deleted => red,
        Status::Modified => amber,
    };
    let mid = h_px / 2.0;
    let font = h_px * 0.36;
    let dot_r = h_px * 0.1;
    // A chevron: down while open, right while folded.
    let chevron = h_px * 0.09;
    let cx = cw;
    let mut pb = PathBuilder::new();
    if spec.folded {
        pb.move_to(cx - chevron * 0.5, mid - chevron);
        pb.line_to(cx + chevron * 0.5, mid);
        pb.line_to(cx - chevron * 0.5, mid + chevron);
    } else {
        pb.move_to(cx - chevron, mid - chevron * 0.5);
        pb.line_to(cx, mid + chevron * 0.5);
        pb.line_to(cx + chevron, mid - chevron * 0.5);
    }
    if let Some(path) = pb.finish() {
        let stroke = Stroke {
            width: 1.4 * scale,
            line_cap: tiny_skia::LineCap::Round,
            line_join: tiny_skia::LineJoin::Round,
            ..Stroke::default()
        };
        pixmap.stroke_path(&path, &paint(muted), &stroke, Transform::identity(), None);
    }
    let dot_x = cw * 2.0 + dot_r;
    if let Some(dot) = PathBuilder::from_circle(dot_x, mid, dot_r) {
        pixmap.fill_path(
            &dot,
            &paint(status_rgb),
            FillRule::Winding,
            Transform::identity(),
            None,
        );
    }
    let mut right = w_px - cw;
    if spec.reviewed {
        let size = h_px * 0.3;
        right -= size;
        check(&mut pixmap, right, mid, size, scale, green);
        right -= cw;
    }
    if let Some((a, r)) = spec.counts {
        let removed = format!("−{r}");
        let added = format!("+{a}");
        let quiet = |c| mix(c, header_bg, 0.2);
        let rw = measure(fonts, &removed, font, Weight::NORMAL);
        right -= rw;
        draw_text(
            fonts,
            &mut pixmap,
            &[(&removed, quiet(red), Weight::NORMAL)],
            right,
            mid,
            font,
            header_bg,
        );
        let aw = measure(fonts, &added, font, Weight::NORMAL);
        right -= aw + cw * 0.8;
        draw_text(
            fonts,
            &mut pixmap,
            &[(&added, quiet(green), Weight::NORMAL)],
            right,
            mid,
            font,
            header_bg,
        );
    }
    // Path: directory muted, name bold; the directory is cut first. A
    // rename leads with its old path, given at most a third.
    let mut x0 = dot_x + dot_r + cw * 0.9;
    if let Some(from) = &spec.from {
        let budget = (right - cw - x0) / 3.0;
        let mut from = from.clone();
        while from.chars().count() > 2 && measure(fonts, &from, font, Weight::NORMAL) > budget {
            let cut: String = from.chars().skip(2).collect();
            from = format!("…{}", cut.trim_start_matches('…'));
        }
        let from = format!("{from}  →  ");
        draw_text(
            fonts,
            &mut pixmap,
            &[(&from, muted, Weight::NORMAL)],
            x0,
            mid,
            font,
            header_bg,
        );
        x0 += measure(fonts, &from, font, Weight::NORMAL);
    }
    let note = spec.note.as_ref().map(|n| format!("   {n}"));
    let note_w = note
        .as_ref()
        .map_or(0.0, |n| measure(fonts, n, font * 0.9, Weight::NORMAL));
    let avail = (right - cw - x0 - note_w).max(0.0);
    let (dir, name) = spec
        .path
        .rsplit_once('/')
        .map_or(("", spec.path.as_str()), |(d, n)| (d, n));
    let mut dir = if dir.is_empty() {
        String::new()
    } else {
        format!("{dir}/")
    };
    let name_w = measure(fonts, name, font * 1.05, Weight::SEMIBOLD);
    while !dir.is_empty() && measure(fonts, &dir, font, Weight::NORMAL) + name_w > avail {
        let cut: String = dir.chars().skip(2).collect();
        dir = format!("…{}", cut.trim_start_matches('…'));
        if dir.chars().count() <= 2 {
            dir.clear();
        }
    }
    let dw = measure(fonts, &dir, font, Weight::NORMAL);
    draw_text(
        fonts,
        &mut pixmap,
        &[(&dir, muted, Weight::NORMAL)],
        x0,
        mid,
        font,
        header_bg,
    );
    let name_color = if spec.reviewed { muted } else { fg };
    draw_text(
        fonts,
        &mut pixmap,
        &[(name, name_color, Weight::SEMIBOLD)],
        x0 + dw,
        mid,
        font * 1.05,
        header_bg,
    );
    if let Some(note) = &note {
        draw_text(
            fonts,
            &mut pixmap,
            &[(note, muted, Weight::NORMAL)],
            x0 + dw + name_w,
            mid,
            font * 0.9,
            header_bg,
        );
    }
    pixmap
}

fn paint(c: Rgb) -> Paint<'static> {
    let mut p = Paint::default();
    p.set_color(skia(c));
    p.anti_alias = true;
    p
}

/// A check mark `size` wide starting at `x`, centred on `mid`.
fn check(pixmap: &mut Pixmap, x: f32, mid: f32, size: f32, scale: f32, color: Rgb) {
    let stroke = Stroke {
        width: 1.6 * scale,
        line_cap: tiny_skia::LineCap::Round,
        line_join: tiny_skia::LineJoin::Round,
        ..Stroke::default()
    };
    let mut pb = PathBuilder::new();
    pb.move_to(x, mid);
    pb.line_to(x + size * 0.35, mid + size * 0.32);
    pb.line_to(x + size, mid - size * 0.38);
    if let Some(path) = pb.finish() {
        pixmap.stroke_path(&path, &paint(color), &stroke, Transform::identity(), None);
    }
}

fn buffer(fonts: &mut Fonts, spans: &[(&str, Rgb, Weight)], size: f32) -> Buffer {
    let mut buffer = Buffer::new(&mut fonts.system, Metrics::new(size, size * 1.25));
    buffer.set_size(None, None);
    let spans = spans.iter().map(|(t, c, w)| {
        (
            *t,
            Attrs::new()
                .family(Family::SansSerif)
                .weight(*w)
                .color(text_color(*c)),
        )
    });
    buffer.set_rich_text(
        spans,
        &Attrs::new().family(Family::SansSerif),
        Shaping::Advanced,
        None,
    );
    buffer.shape_until_scroll(&mut fonts.system, false);
    buffer
}

fn measure(fonts: &mut Fonts, text: &str, size: f32, weight: Weight) -> f32 {
    if text.is_empty() {
        return 0.0;
    }
    let b = buffer(fonts, &[(text, (0, 0, 0), weight)], size);
    b.layout_runs().map(|r| r.line_w).fold(0.0, f32::max)
}

#[allow(clippy::many_single_char_names)]
/// Draws text with its vertical centre at `mid`, blending glyph coverage
/// over the opaque background.
fn draw_text(
    fonts: &mut Fonts,
    pixmap: &mut Pixmap,
    spans: &[(&str, Rgb, Weight)],
    x: f32,
    mid: f32,
    size: f32,
    bg: Rgb,
) {
    let mut b = buffer(fonts, spans, size);
    let line_h = size * 1.25;
    let top = mid - line_h / 2.0;
    let (w, h) = (pixmap.width(), pixmap.height());
    let data = pixmap.data_mut();
    let _ = bg;
    b.draw(
        &mut fonts.system,
        &mut fonts.cache,
        text_color((0, 0, 0)),
        |gx, gy, gw, gh, color| {
            let a = u32::from(color.a());
            if a == 0 {
                return;
            }
            #[allow(clippy::cast_possible_truncation)]
            let (ox, oy) = (x as i32 + gx, top as i32 + gy);
            for yy in oy..oy + i32::try_from(gh).unwrap_or(0) {
                for xx in ox..ox + i32::try_from(gw).unwrap_or(0) {
                    let (Ok(ux), Ok(uy)) = (u32::try_from(xx), u32::try_from(yy)) else {
                        continue;
                    };
                    if ux >= w || uy >= h {
                        continue;
                    }
                    let i = ((uy * w + ux) * 4) as usize;
                    let blend = |d: u8, s: u8| {
                        u8::try_from((u32::from(s) * a + u32::from(d) * (255 - a)) / 255)
                            .unwrap_or(255)
                    };
                    data[i] = blend(data[i], color.r());
                    data[i + 1] = blend(data[i + 1], color.g());
                    data[i + 2] = blend(data[i + 2], color.b());
                    data[i + 3] = 255;
                }
            }
        },
    );
}

/// Transmits `pixmap` as image `id` (raw RGBA, zlib-compressed) without placing it.
fn upload(out: &mut Vec<u8>, pixmap: &Pixmap, id: u32) {
    use flate2::write::ZlibEncoder;
    let mut z = ZlibEncoder::new(Vec::new(), flate2::Compression::fast());
    let _ = z.write_all(pixmap.data());
    let compressed = z.finish().unwrap_or_default();
    let data = base64::engine::general_purpose::STANDARD.encode(compressed);
    let chunks: Vec<&[u8]> = data.as_bytes().chunks(4096).collect();
    for (i, chunk) in chunks.iter().enumerate() {
        let more = u8::from(i + 1 < chunks.len());
        if i == 0 {
            let _ = write!(
                out,
                "\x1b_Ga=t,f=32,o=z,s={},v={},i={id},q=2,m={more};",
                pixmap.width(),
                pixmap.height()
            );
        } else {
            let _ = write!(out, "\x1b_Gm={more};");
        }
        out.extend_from_slice(chunk);
        out.extend_from_slice(b"\x1b\\");
    }
}
