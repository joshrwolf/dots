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
    status: Status,
    counts: Option<(u32, u32)>,
    reviewed: bool,
    progress: (usize, usize),
    cols: u16,
    colors: [Rgb; 7],
    cell: (u16, u16),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Placed {
    image: u32,
    placement: u32,
    x: u16,
    y: u16,
}

pub struct Graphics {
    fonts: Arc<Mutex<Option<Fonts>>>,
    images: HashMap<u64, (u32, Instant)>,
    placed: Vec<Placed>,
    next_id: u32,
    next_placement: u32,
    /// Time spent drawing and encoding images, for `--timings`.
    pub drawn: Vec<Duration>,
}

impl std::fmt::Debug for Graphics {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Graphics").field("images", &self.images.len()).finish_non_exhaustive()
    }
}

/// Images kept uploaded; older ones are deleted from the terminal.
const KEEP: usize = 48;

impl Graphics {
    pub fn new() -> Self {
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
    pub fn pending(&self) -> bool {
        self.fonts.lock().map_or(false, |f| f.is_none())
    }

    /// Forgets every image, after a theme change.
    pub fn invalidate(&mut self) {
        self.clear();
    }

    pub fn clear(&mut self) {
        let mut out = std::io::stdout().lock();
        let _ = out.write_all(b"\x1b_Ga=d,d=A,q=2\x1b\\");
        let _ = out.flush();
        self.images.clear();
        self.placed.clear();
    }

    /// Places an image over every header on screen, after the text frame.
    pub fn after_draw(&mut self, app: &App) -> Result<()> {
        let fonts_slot = Arc::clone(&self.fonts);
        let Ok(mut fonts) = fonts_slot.lock() else { return Ok(()) };
        let Some(fonts) = fonts.as_mut() else { return Ok(()) };
        let Some(stream) = &app.stream else {
            drop(fonts);
            return self.place(Vec::new(), 0, 0);
        };
        let area = app.areas.diff;
        let rows = app.settings.header_rows;
        let cell = cell_pixels();
        let (reviewed, total) = (stream.files.iter().filter(|f| f.reviewed).count(), stream.files.len());
        let p = &app.palette.rgb;
        let colors = [p.bg, p.fg, p.muted, p.green, p.red, p.accent, p.header_bg];
        let mut headers = Vec::new();
        if !stream.files.is_empty() && area.height >= rows {
            headers.push((app.top.file, area.y));
            let top = app.global_top();
            for y in rows..area.height.saturating_sub(rows - 1) {
                if let Line::Header { file, part: 0 } = app.line_at(top + u32::from(y)) {
                    headers.push((file, area.y + y));
                }
            }
        }
        let mut out = Vec::new();
        let mut wanted = Vec::new();
        for (file, y) in headers {
            let f = &stream.files[file];
            let spec = Spec {
                path: f.change.path.clone(),
                status: f.change.status,
                counts: f.counts(),
                reviewed: f.reviewed,
                progress: (reviewed, total),
                cols: area.width,
                colors,
                cell,
            };
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
        drop(fonts);
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
            if let Some(p) = self.placed.iter().find(|p| p.image == image && p.x == x && p.y == y) {
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
            kept.push(Placed { image, placement, x, y });
        }
        for old in &self.placed {
            if !kept.contains(old) {
                write!(out, "\x1b_Ga=d,d=i,i={},p={},q=2\x1b\\", old.image, old.placement)?;
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
        let mut by_age: Vec<(u64, u32, Instant)> = self.images.iter().map(|(k, (id, t))| (*k, *id, *t)).collect();
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

/// Pixels per cell, from the terminal; a common size when it does not say.
fn cell_pixels() -> (u16, u16) {
    match ratatui::crossterm::terminal::window_size() {
        Ok(s) if s.width > 0 && s.columns > 0 && s.rows > 0 => (s.width / s.columns, s.height / s.rows),
        _ => (10, 20),
    }
}

fn skia(c: Rgb) -> tiny_skia::Color {
    tiny_skia::Color::from_rgba8(c.0, c.1, c.2, 255)
}

fn text_color(c: Rgb) -> cosmic_text::Color {
    cosmic_text::Color::rgb(c.0, c.1, c.2)
}

/// Draws a header: status pill, path in a proportional font, counts, and a
/// ring showing how many of the worktree's files are reviewed.
fn render(fonts: &mut Fonts, spec: &Spec, rows: u16) -> Pixmap {
    // Draw at twice the cell size when the terminal reports small cells, so
    // high-density screens stay sharp.
    let scale = if spec.cell.1 < 24 { 2.0 } else { 1.0 };
    let cw = f32::from(spec.cell.0) * scale;
    let w_px = (f32::from(spec.cols) * cw).max(1.0);
    let h_px = (f32::from(spec.cell.1 * rows) * scale).max(1.0);
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let mut pixmap = Pixmap::new(w_px as u32, h_px as u32).expect("header size is positive");
    let [bg, fg, muted, green, red, accent, header_bg] = spec.colors;
    pixmap.fill(skia(header_bg));
    // A hairline under the header separates it from code.
    let rule = mix(header_bg, fg, 0.12);
    if let Some(r) = tiny_skia::Rect::from_xywh(0.0, h_px - scale, w_px, scale) {
        pixmap.fill_rect(r, &paint(rule), Transform::identity(), None);
    }
    let status_rgb = match spec.status {
        Status::Added => green,
        Status::Deleted => red,
        Status::Modified => mix(accent, red, 0.5),
    };
    let mid = h_px / 2.0;
    let font = h_px * 0.36;
    // Status pill.
    let pill_h = h_px * 0.56;
    let pill_w = pill_h * 1.15;
    let pill_x = cw;
    rounded(&mut pixmap, pill_x, mid - pill_h / 2.0, pill_w, pill_h, pill_h * 0.3, mix(header_bg, status_rgb, 0.22));
    let letter = spec.status.letter().to_string();
    let lw = measure(fonts, &letter, font, Weight::BOLD);
    draw_text(fonts, &mut pixmap, &[(&letter, status_rgb, Weight::BOLD)], pill_x + (pill_w - lw) / 2.0, mid, font, header_bg);
    // Right side: counts, then the review ring.
    let ring_d = h_px * 0.5;
    let ring_x = w_px - cw - ring_d;
    ring(&mut pixmap, ring_x + ring_d / 2.0, mid, ring_d / 2.0, spec, scale, (accent, green, mix(header_bg, fg, 0.18)));
    let mut right = ring_x - cw;
    if let Some((a, r)) = spec.counts {
        let removed = format!("−{r}");
        let added = format!("+{a}");
        let rw = measure(fonts, &removed, font, Weight::SEMIBOLD);
        right -= rw;
        draw_text(fonts, &mut pixmap, &[(&removed, red, Weight::SEMIBOLD)], right, mid, font, header_bg);
        let aw = measure(fonts, &added, font, Weight::SEMIBOLD);
        right -= aw + cw * 0.8;
        draw_text(fonts, &mut pixmap, &[(&added, green, Weight::SEMIBOLD)], right, mid, font, header_bg);
    }
    // Path: directory muted, name bold, cut from the left to fit.
    let x0 = pill_x + pill_w + cw * 0.9;
    let avail = (right - cw - x0).max(0.0);
    let (dir, name) = spec.path.rsplit_once('/').map_or(("", spec.path.as_str()), |(d, n)| (d, n));
    let mut dir = if dir.is_empty() { String::new() } else { format!("{dir}/") };
    let name_w = measure(fonts, name, font * 1.05, Weight::BOLD);
    while !dir.is_empty() && measure(fonts, &dir, font, Weight::NORMAL) + name_w > avail {
        let cut: String = dir.chars().skip(2).collect();
        dir = format!("…{}", cut.trim_start_matches('…'));
        if dir.chars().count() <= 2 {
            dir.clear();
        }
    }
    let dw = measure(fonts, &dir, font, Weight::NORMAL);
    draw_text(fonts, &mut pixmap, &[(&dir, muted, Weight::NORMAL)], x0, mid, font, header_bg);
    let name_color = if spec.reviewed { muted } else { fg };
    draw_text(fonts, &mut pixmap, &[(name, name_color, Weight::BOLD)], x0 + dw, mid, font * 1.05, header_bg);
    let _ = bg;
    pixmap
}

fn paint(c: Rgb) -> Paint<'static> {
    let mut p = Paint::default();
    p.set_color(skia(c));
    p.anti_alias = true;
    p
}

fn rounded(pixmap: &mut Pixmap, x: f32, y: f32, w: f32, h: f32, r: f32, c: Rgb) {
    let mut pb = PathBuilder::new();
    pb.move_to(x + r, y);
    pb.line_to(x + w - r, y);
    pb.quad_to(x + w, y, x + w, y + r);
    pb.line_to(x + w, y + h - r);
    pb.quad_to(x + w, y + h, x + w - r, y + h);
    pb.line_to(x + r, y + h);
    pb.quad_to(x, y + h, x, y + h - r);
    pb.line_to(x, y + r);
    pb.quad_to(x, y, x + r, y);
    pb.close();
    if let Some(path) = pb.finish() {
        pixmap.fill_path(&path, &paint(c), FillRule::Winding, Transform::identity(), None);
    }
}

/// The review ring: an arc for the share of reviewed files, and a check
/// mark when this file is reviewed.
fn ring(pixmap: &mut Pixmap, cx: f32, cy: f32, r: f32, spec: &Spec, scale: f32, colors: (Rgb, Rgb, Rgb)) {
    let (accent, green, track) = colors;
    let stroke = Stroke {
        width: 2.0 * scale,
        ..Stroke::default()
    };
    if let Some(circle) = PathBuilder::from_circle(cx, cy, r) {
        pixmap.stroke_path(&circle, &paint(track), &stroke, Transform::identity(), None);
    }
    let (done, total) = spec.progress;
    if total > 0 && done > 0 {
        #[allow(clippy::cast_precision_loss)]
        let share = done as f32 / total as f32;
        let mut pb = PathBuilder::new();
        let steps = 48;
        for i in 0..=steps {
            #[allow(clippy::cast_precision_loss)]
            let a = -std::f32::consts::FRAC_PI_2 + share * std::f32::consts::TAU * (i as f32 / steps as f32);
            let (x, y) = (cx + r * a.cos(), cy + r * a.sin());
            if i == 0 {
                pb.move_to(x, y);
            } else {
                pb.line_to(x, y);
            }
        }
        if let Some(arc) = pb.finish() {
            pixmap.stroke_path(&arc, &paint(accent), &stroke, Transform::identity(), None);
        }
    }
    if spec.reviewed {
        let mut pb = PathBuilder::new();
        pb.move_to(cx - r * 0.45, cy);
        pb.line_to(cx - r * 0.1, cy + r * 0.35);
        pb.line_to(cx + r * 0.5, cy - r * 0.35);
        if let Some(check) = pb.finish() {
            pixmap.stroke_path(&check, &paint(green), &stroke, Transform::identity(), None);
        }
    }
}

fn buffer(fonts: &mut Fonts, spans: &[(&str, Rgb, Weight)], size: f32) -> Buffer {
    let mut buffer = Buffer::new(&mut fonts.system, Metrics::new(size, size * 1.25));
    buffer.set_size(None, None);
    let spans = spans
        .iter()
        .map(|(t, c, w)| (*t, Attrs::new().family(Family::SansSerif).weight(*w).color(text_color(*c))));
    buffer.set_rich_text(spans, &Attrs::new().family(Family::SansSerif), Shaping::Advanced, None);
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

/// Draws text with its vertical centre at `mid`, blending glyph coverage
/// over the opaque background.
fn draw_text(fonts: &mut Fonts, pixmap: &mut Pixmap, spans: &[(&str, Rgb, Weight)], x: f32, mid: f32, size: f32, bg: Rgb) {
    let mut b = buffer(fonts, spans, size);
    let line_h = size * 1.25;
    let top = mid - line_h / 2.0;
    let (w, h) = (pixmap.width(), pixmap.height());
    let data = pixmap.data_mut();
    let _ = bg;
    b.draw(&mut fonts.system, &mut fonts.cache, text_color((0, 0, 0)), |gx, gy, gw, gh, color| {
        let a = u32::from(color.a());
        if a == 0 {
            return;
        }
        #[allow(clippy::cast_possible_truncation)]
        let (ox, oy) = (x as i32 + gx, top as i32 + gy);
        for yy in oy..oy + i32::try_from(gh).unwrap_or(0) {
            for xx in ox..ox + i32::try_from(gw).unwrap_or(0) {
                let (Ok(ux), Ok(uy)) = (u32::try_from(xx), u32::try_from(yy)) else { continue };
                if ux >= w || uy >= h {
                    continue;
                }
                let i = ((uy * w + ux) * 4) as usize;
                let blend = |d: u8, s: u8| u8::try_from((u32::from(s) * a + u32::from(d) * (255 - a)) / 255).unwrap_or(255);
                data[i] = blend(data[i], color.r());
                data[i + 1] = blend(data[i + 1], color.g());
                data[i + 2] = blend(data[i + 2], color.b());
                data[i + 3] = 255;
            }
        }
    });
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
