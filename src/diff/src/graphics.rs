//! `--graphics`: file headers drawn as kitty graphics images through
//! Unicode placeholders. Each image is transmitted once with a virtual
//! placement; the header's cells then hold placeholder characters that
//! name the image and the cell's row and column in it, so the terminal
//! draws the image wherever those cells are. Headers scroll, clip and
//! disappear with the text around them. Code stays text.

use std::collections::HashMap;
use std::io::Write as _;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use base64::Engine as _;
use cosmic_text::{Attrs, Buffer, Family, FontSystem, Metrics, Shaping, SwashCache, Weight};
use ratatui::style::Color;
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

/// The character a placeholder cell holds.
const PLACEHOLDER: char = '\u{10EEEE}';

/// The combining marks that number a placeholder cell's row and column
/// in its image, in order, as the kitty graphics protocol lists them.
const DIACRITICS: [char; 297] = [
    '\u{0305}',
    '\u{030D}',
    '\u{030E}',
    '\u{0310}',
    '\u{0312}',
    '\u{033D}',
    '\u{033E}',
    '\u{033F}',
    '\u{0346}',
    '\u{034A}',
    '\u{034B}',
    '\u{034C}',
    '\u{0350}',
    '\u{0351}',
    '\u{0352}',
    '\u{0357}',
    '\u{035B}',
    '\u{0363}',
    '\u{0364}',
    '\u{0365}',
    '\u{0366}',
    '\u{0367}',
    '\u{0368}',
    '\u{0369}',
    '\u{036A}',
    '\u{036B}',
    '\u{036C}',
    '\u{036D}',
    '\u{036E}',
    '\u{036F}',
    '\u{0483}',
    '\u{0484}',
    '\u{0485}',
    '\u{0486}',
    '\u{0487}',
    '\u{0592}',
    '\u{0593}',
    '\u{0594}',
    '\u{0595}',
    '\u{0597}',
    '\u{0598}',
    '\u{0599}',
    '\u{059C}',
    '\u{059D}',
    '\u{059E}',
    '\u{059F}',
    '\u{05A0}',
    '\u{05A1}',
    '\u{05A8}',
    '\u{05A9}',
    '\u{05AB}',
    '\u{05AC}',
    '\u{05AF}',
    '\u{05C4}',
    '\u{0610}',
    '\u{0611}',
    '\u{0612}',
    '\u{0613}',
    '\u{0614}',
    '\u{0615}',
    '\u{0616}',
    '\u{0617}',
    '\u{0657}',
    '\u{0658}',
    '\u{0659}',
    '\u{065A}',
    '\u{065B}',
    '\u{065D}',
    '\u{065E}',
    '\u{06D6}',
    '\u{06D7}',
    '\u{06D8}',
    '\u{06D9}',
    '\u{06DA}',
    '\u{06DB}',
    '\u{06DC}',
    '\u{06DF}',
    '\u{06E0}',
    '\u{06E1}',
    '\u{06E2}',
    '\u{06E4}',
    '\u{06E7}',
    '\u{06E8}',
    '\u{06EB}',
    '\u{06EC}',
    '\u{0730}',
    '\u{0732}',
    '\u{0733}',
    '\u{0735}',
    '\u{0736}',
    '\u{073A}',
    '\u{073D}',
    '\u{073F}',
    '\u{0740}',
    '\u{0741}',
    '\u{0743}',
    '\u{0745}',
    '\u{0747}',
    '\u{0749}',
    '\u{074A}',
    '\u{07EB}',
    '\u{07EC}',
    '\u{07ED}',
    '\u{07EE}',
    '\u{07EF}',
    '\u{07F0}',
    '\u{07F1}',
    '\u{07F3}',
    '\u{0816}',
    '\u{0817}',
    '\u{0818}',
    '\u{0819}',
    '\u{081B}',
    '\u{081C}',
    '\u{081D}',
    '\u{081E}',
    '\u{081F}',
    '\u{0820}',
    '\u{0821}',
    '\u{0822}',
    '\u{0823}',
    '\u{0825}',
    '\u{0826}',
    '\u{0827}',
    '\u{0829}',
    '\u{082A}',
    '\u{082B}',
    '\u{082C}',
    '\u{082D}',
    '\u{0951}',
    '\u{0953}',
    '\u{0954}',
    '\u{0F82}',
    '\u{0F83}',
    '\u{0F86}',
    '\u{0F87}',
    '\u{135D}',
    '\u{135E}',
    '\u{135F}',
    '\u{17DD}',
    '\u{193A}',
    '\u{1A17}',
    '\u{1A75}',
    '\u{1A76}',
    '\u{1A77}',
    '\u{1A78}',
    '\u{1A79}',
    '\u{1A7A}',
    '\u{1A7B}',
    '\u{1A7C}',
    '\u{1B6B}',
    '\u{1B6D}',
    '\u{1B6E}',
    '\u{1B6F}',
    '\u{1B70}',
    '\u{1B71}',
    '\u{1B72}',
    '\u{1B73}',
    '\u{1CD0}',
    '\u{1CD1}',
    '\u{1CD2}',
    '\u{1CDA}',
    '\u{1CDB}',
    '\u{1CE0}',
    '\u{1DC0}',
    '\u{1DC1}',
    '\u{1DC3}',
    '\u{1DC4}',
    '\u{1DC5}',
    '\u{1DC6}',
    '\u{1DC7}',
    '\u{1DC8}',
    '\u{1DC9}',
    '\u{1DCB}',
    '\u{1DCC}',
    '\u{1DD1}',
    '\u{1DD2}',
    '\u{1DD3}',
    '\u{1DD4}',
    '\u{1DD5}',
    '\u{1DD6}',
    '\u{1DD7}',
    '\u{1DD8}',
    '\u{1DD9}',
    '\u{1DDA}',
    '\u{1DDB}',
    '\u{1DDC}',
    '\u{1DDD}',
    '\u{1DDE}',
    '\u{1DDF}',
    '\u{1DE0}',
    '\u{1DE1}',
    '\u{1DE2}',
    '\u{1DE3}',
    '\u{1DE4}',
    '\u{1DE5}',
    '\u{1DE6}',
    '\u{1DFE}',
    '\u{20D0}',
    '\u{20D1}',
    '\u{20D4}',
    '\u{20D5}',
    '\u{20D6}',
    '\u{20D7}',
    '\u{20DB}',
    '\u{20DC}',
    '\u{20E1}',
    '\u{20E7}',
    '\u{20E9}',
    '\u{20F0}',
    '\u{2CEF}',
    '\u{2CF0}',
    '\u{2CF1}',
    '\u{2DE0}',
    '\u{2DE1}',
    '\u{2DE2}',
    '\u{2DE3}',
    '\u{2DE4}',
    '\u{2DE5}',
    '\u{2DE6}',
    '\u{2DE7}',
    '\u{2DE8}',
    '\u{2DE9}',
    '\u{2DEA}',
    '\u{2DEB}',
    '\u{2DEC}',
    '\u{2DED}',
    '\u{2DEE}',
    '\u{2DEF}',
    '\u{2DF0}',
    '\u{2DF1}',
    '\u{2DF2}',
    '\u{2DF3}',
    '\u{2DF4}',
    '\u{2DF5}',
    '\u{2DF6}',
    '\u{2DF7}',
    '\u{2DF8}',
    '\u{2DF9}',
    '\u{2DFA}',
    '\u{2DFB}',
    '\u{2DFC}',
    '\u{2DFD}',
    '\u{2DFE}',
    '\u{2DFF}',
    '\u{A66F}',
    '\u{A67C}',
    '\u{A67D}',
    '\u{A6F0}',
    '\u{A6F1}',
    '\u{A8E0}',
    '\u{A8E1}',
    '\u{A8E2}',
    '\u{A8E3}',
    '\u{A8E4}',
    '\u{A8E5}',
    '\u{A8E6}',
    '\u{A8E7}',
    '\u{A8E8}',
    '\u{A8E9}',
    '\u{A8EA}',
    '\u{A8EB}',
    '\u{A8EC}',
    '\u{A8ED}',
    '\u{A8EE}',
    '\u{A8EF}',
    '\u{A8F0}',
    '\u{A8F1}',
    '\u{AAB0}',
    '\u{AAB2}',
    '\u{AAB3}',
    '\u{AAB7}',
    '\u{AAB8}',
    '\u{AABE}',
    '\u{AABF}',
    '\u{AAC1}',
    '\u{FE20}',
    '\u{FE21}',
    '\u{FE22}',
    '\u{FE23}',
    '\u{FE24}',
    '\u{FE25}',
    '\u{FE26}',
    '\u{10A0F}',
    '\u{10A38}',
    '\u{1D185}',
    '\u{1D186}',
    '\u{1D187}',
    '\u{1D188}',
    '\u{1D189}',
    '\u{1D1AA}',
    '\u{1D1AB}',
    '\u{1D1AC}',
    '\u{1D1AD}',
    '\u{1D242}',
    '\u{1D243}',
    '\u{1D244}',
];

/// The widest image placeholders can show, in cells.
#[expect(clippy::cast_possible_truncation, reason = "the protocol numbers 297")]
pub(crate) const MAX_COLS: u16 = DIACRITICS.len() as u16;

/// The text of the cell at column `col` of a one-row image shown through
/// placeholders, or `None` past the last numbered column.
pub(crate) fn placeholder(col: u16) -> Option<String> {
    let col = DIACRITICS.get(usize::from(col))?;
    Some([PLACEHOLDER, DIACRITICS[0], *col].into_iter().collect())
}

/// The foreground colour that names image `id` in its placeholder cells:
/// its low 24 bits as red, green and blue.
pub(crate) fn image_color(id: u32) -> Color {
    let [_, r, g, b] = id.to_be_bytes();
    Color::Rgb(r, g, b)
}

/// Raw bytes of images kept transmitted; the least recently shown beyond
/// these are deleted from the terminal. Rex keeps 10 MB of images per pane
/// and silently drops the oldest beyond that, which would blank headers.
const KEEP_BYTES: usize = 8 << 20;

/// The header images transmitted to the terminal, by what they show.
#[derive(Debug, Clone, Copy)]
struct Image {
    id: u32,
    bytes: usize,
    /// The last frame that showed it.
    shown: u64,
}

#[derive(Debug)]
struct Images {
    by_spec: HashMap<Spec, Image>,
    /// Raw bytes of all images.
    bytes: usize,
    next_id: u32,
    frame: u64,
}

impl Images {
    fn new() -> Self {
        Self {
            by_spec: HashMap::new(),
            bytes: 0,
            next_id: 1,
            frame: 0,
        }
    }

    /// Starts a frame; images it shows are not evicted at its end.
    fn begin(&mut self) {
        self.frame += 1;
    }

    /// The id of the image showing `spec`, drawn with `draw` and
    /// transmitted into `out` the first time.
    fn get(&mut self, spec: &Spec, out: &mut String, draw: impl FnOnce(&Spec) -> Pixmap) -> u32 {
        if let Some(image) = self.by_spec.get_mut(spec) {
            image.shown = self.frame;
            return image.id;
        }
        let id = self.next_id;
        // Ids fit the 24 bits a foreground colour carries, and never 0.
        self.next_id = self.next_id % 0x00FF_FFFF + 1;
        let pixmap = draw(spec);
        transmit(out, &pixmap, id, spec.cols);
        let image = Image {
            id,
            bytes: pixmap.data().len(),
            shown: self.frame,
        };
        self.bytes += image.bytes;
        self.by_spec.insert(spec.clone(), image);
        id
    }

    /// Deletes the least recently shown images until those kept fit
    /// [`KEEP_BYTES`], never one the current frame shows.
    fn evict(&mut self, out: &mut String) {
        if self.bytes <= KEEP_BYTES {
            return;
        }
        let mut by_age: Vec<Image> = self
            .by_spec
            .values()
            .filter(|image| image.shown != self.frame)
            .copied()
            .collect();
        by_age.sort_unstable_by_key(|image| image.shown);
        let mut gone = Vec::new();
        for image in by_age {
            if self.bytes <= KEEP_BYTES {
                break;
            }
            delete(out, image.id);
            self.bytes -= image.bytes;
            gone.push(image.id);
        }
        self.by_spec.retain(|_, image| !gone.contains(&image.id));
    }

    /// Deletes every image.
    fn clear(&mut self, out: &mut String) {
        for image in self.by_spec.values() {
            delete(out, image.id);
        }
        self.by_spec.clear();
        self.bytes = 0;
    }
}

pub(crate) struct Graphics {
    fonts: Arc<Mutex<Option<Fonts>>>,
    images: Images,
    /// Time spent drawing and encoding images, for `--timings`.
    pub(crate) drawn: Vec<Duration>,
}

impl std::fmt::Debug for Graphics {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Graphics")
            .field("images", &self.images.by_spec.len())
            .finish_non_exhaustive()
    }
}

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
            images: Images::new(),
            drawn: Vec::new(),
        }
    }

    /// True until fonts have loaded and a redraw can show images.
    pub(crate) fn pending(&self) -> bool {
        self.fonts.lock().is_ok_and(|f| f.is_none())
    }

    /// Sets the images of the headers the laid-out view shows in
    /// `app.header_images`. Returns what to send the terminal before the
    /// frame: images it has not seen, and deletions of old ones.
    pub(crate) fn prepare(&mut self, app: &mut App) -> String {
        app.header_images.clear();
        let mut out = String::new();
        let fonts_slot = Arc::clone(&self.fonts);
        let Ok(mut guard) = fonts_slot.lock() else {
            return out;
        };
        let Some(fonts) = guard.as_mut() else {
            return out;
        };
        self.images.begin();
        let cell = cell_pixels();
        let cols = app.areas.diff.width.min(MAX_COLS);
        for file in shown_headers(app) {
            let spec = spec_for(app, file, cols, cell);
            let id = self.images.get(&spec, &mut out, |spec| {
                let started = Instant::now();
                let pixmap = render(fonts, spec);
                self.drawn.push(started.elapsed());
                pixmap
            });
            app.header_images.insert(file, id);
        }
        self.images.evict(&mut out);
        out
    }

    /// Deletes every image this viewer transmitted.
    pub(crate) fn clear(&mut self) -> String {
        let mut out = String::new();
        self.images.clear(&mut out);
        out
    }
}

/// The files whose header shows, wholly or in part, in the laid-out view.
fn shown_headers(app: &App) -> Vec<usize> {
    if app.stream.is_none() {
        return Vec::new();
    }
    let mut files: Vec<usize> = app.pinned_header().into_iter().collect();
    let top = app.global_top();
    for y in 0..app.areas.diff.height {
        if let Line::Header { file, .. } = app.line_at(top + u32::from(y))
            && !files.contains(&file)
        {
            files.push(file);
        }
    }
    files
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
pub(crate) fn header_picture(app: &App, cols: u16, path: &std::path::Path) -> anyhow::Result<()> {
    let mut fonts = Fonts {
        system: FontSystem::new(),
        cache: SwashCache::new(),
    };
    let spec = spec_for(app, app.top.file, cols, cell_pixels());
    let pixmap = render(&mut fonts, &spec);
    pixmap.save_png(path)?;
    Ok(())
}

/// Pixels per cell, from the terminal; a common size when it does not say.
fn cell_pixels() -> (u16, u16) {
    match rustix::termios::tcgetwinsize(rustix::stdio::stdout()) {
        Ok(s) if s.ws_xpixel > 0 && s.ws_col > 0 && s.ws_row > 0 => {
            (s.ws_xpixel / s.ws_col, s.ws_ypixel / s.ws_row)
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
#[expect(clippy::too_many_lines, reason = "one header drawn top to bottom")]
fn render(fonts: &mut Fonts, spec: &Spec) -> Pixmap {
    // Draw at twice the cell size when the terminal reports small cells, so
    // high-density screens stay sharp.
    let scale = if spec.cell.1 < 24 { 2.0 } else { 1.0 };
    let cw = f32::from(spec.cell.0) * scale;
    let w_px = (f32::from(spec.cols) * cw).max(1.0);
    let h_px = (f32::from(spec.cell.1) * scale).max(1.0);
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "pixel sizes are positive and small"
    )]
    let mut pixmap = Pixmap::new(w_px as u32, h_px as u32).expect("header size is positive");
    let [fg, muted, green, red, amber, header_bg] = spec.colors;
    pixmap.fill(skia(header_bg));
    // A rule along the top edge separates the file from the one above,
    // in place of the blank line text headers get.
    let rule = mix(header_bg, fg, 0.22);
    let rule_h = scale.max((h_px * 0.06).round());
    if let Some(r) = tiny_skia::Rect::from_xywh(0.0, 0.0, w_px, rule_h) {
        pixmap.fill_rect(r, &paint(rule), Transform::identity(), None);
    }
    let status_rgb = match spec.status {
        Status::Added => green,
        Status::Deleted => red,
        Status::Modified => amber,
    };
    // Sized to the cell: text as large as the terminal's own, centred in
    // the space under the rule.
    let mid = f32::midpoint(h_px, rule_h);
    let font = h_px * 0.6;
    let dot_r = h_px * 0.15;
    // A chevron: down while open, right while folded.
    let chevron = h_px * 0.14;
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
        let size = h_px * 0.42;
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
    );
    let name_color = if spec.reviewed { muted } else { fg };
    draw_text(
        fonts,
        &mut pixmap,
        &[(name, name_color, Weight::SEMIBOLD)],
        x0 + dw,
        mid,
        font * 1.05,
    );
    if let Some(note) = &note {
        draw_text(
            fonts,
            &mut pixmap,
            &[(note, muted, Weight::NORMAL)],
            x0 + dw + name_w,
            mid,
            font * 0.9,
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

#[expect(clippy::many_single_char_names, reason = "glyph coordinates")]
/// Draws text with its vertical centre at `mid`, blending glyph coverage
/// over the opaque background.
fn draw_text(
    fonts: &mut Fonts,
    pixmap: &mut Pixmap,
    spans: &[(&str, Rgb, Weight)],
    x: f32,
    mid: f32,
    size: f32,
) {
    let mut b = buffer(fonts, spans, size);
    let line_h = size * 1.25;
    let top = mid - line_h / 2.0;
    let (w, h) = (pixmap.width(), pixmap.height());
    let data = pixmap.data_mut();
    b.draw(
        &mut fonts.system,
        &mut fonts.cache,
        text_color((0, 0, 0)),
        |gx, gy, gw, gh, color| {
            let a = u32::from(color.a());
            if a == 0 {
                return;
            }
            #[expect(
                clippy::cast_possible_truncation,
                reason = "glyph positions are within the header"
            )]
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

/// Transmits `pixmap` as image `id` (raw RGBA, zlib-compressed) with a
/// virtual placement `cols` cells wide and one tall, for placeholders to
/// show.
fn transmit(out: &mut String, pixmap: &Pixmap, id: u32, cols: u16) {
    use flate2::write::ZlibEncoder;
    use std::fmt::Write as _;
    let mut z = ZlibEncoder::new(Vec::new(), flate2::Compression::fast());
    let _ = z.write_all(pixmap.data());
    let compressed = z.finish().unwrap_or_default();
    let data = base64::engine::general_purpose::STANDARD.encode(compressed);
    let mut chunks = data.as_bytes().chunks(4096).peekable();
    let mut first = true;
    while let Some(chunk) = chunks.next() {
        let more = u8::from(chunks.peek().is_some());
        if first {
            let _ = write!(
                out,
                "\x1b_Ga=T,U=1,f=32,o=z,s={},v={},i={id},c={cols},r=1,q=2,m={more};",
                pixmap.width(),
                pixmap.height()
            );
            first = false;
        } else {
            let _ = write!(out, "\x1b_Gm={more};");
        }
        // Base64 is ASCII.
        out.push_str(std::str::from_utf8(chunk).unwrap_or_default());
        out.push_str("\x1b\\");
    }
}

/// Deletes image `id` and its placements.
fn delete(out: &mut String, id: u32) {
    use std::fmt::Write as _;
    let _ = write!(out, "\x1b_Ga=d,d=I,i={id},q=2\x1b\\");
}

#[cfg(test)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    use super::*;
    use crate::app::fixture::app_with;

    fn spec(path: &str) -> Spec {
        Spec {
            path: path.into(),
            from: None,
            note: None,
            folded: false,
            status: Status::Modified,
            counts: Some((1, 2)),
            reviewed: false,
            cols: 40,
            colors: [(0, 0, 0); 6],
            cell: (10, 20),
        }
    }

    fn pixmap(_: &Spec) -> Pixmap {
        Pixmap::new(4, 2).expect("pixmap")
    }

    #[test]
    fn placeholders_number_row_zero_and_their_column_with_diacritics() {
        assert_eq!(placeholder(0).as_deref(), Some("\u{10EEEE}\u{305}\u{305}"));
        assert_eq!(placeholder(2).as_deref(), Some("\u{10EEEE}\u{305}\u{30E}"));
        assert_eq!(
            placeholder(MAX_COLS - 1).as_deref(),
            Some("\u{10EEEE}\u{305}\u{1D244}")
        );
        assert_eq!(placeholder(MAX_COLS), None);
        assert_eq!(image_color(0x0001_0203), Color::Rgb(1, 2, 3));
        // One cell wide, so ratatui lays the row out as the terminal does.
        let cell = placeholder(7).expect("placeholder");
        assert_eq!(unicode_width::UnicodeWidthStr::width(cell.as_str()), 1);
    }

    #[test]
    fn an_image_is_transmitted_once_with_a_virtual_placement() {
        let mut images = Images::new();
        let mut out = String::new();
        let mut drawn = 0;
        images.begin();
        let a = images.get(&spec("a.rs"), &mut out, |s| {
            drawn += 1;
            pixmap(s)
        });
        assert!(
            out.starts_with(&format!(
                "\x1b_Ga=T,U=1,f=32,o=z,s=4,v=2,i={a},c=40,r=1,q=2,m=0;"
            )),
            "{out:?}"
        );
        out.clear();
        images.begin();
        let again = images.get(&spec("a.rs"), &mut out, |s| {
            drawn += 1;
            pixmap(s)
        });
        assert_eq!((again, drawn, out.as_str()), (a, 1, ""));
    }

    #[test]
    fn a_new_theme_cell_size_or_width_draws_a_new_image() {
        let mut images = Images::new();
        let mut out = String::new();
        let base = spec("a.rs");
        let themed = Spec {
            colors: [(255, 255, 255); 6],
            ..base.clone()
        };
        let finer = Spec {
            cell: (20, 40),
            ..base.clone()
        };
        let wider = Spec {
            cols: 80,
            ..base.clone()
        };
        let ids: Vec<u32> = [&base, &themed, &finer, &wider]
            .into_iter()
            .map(|s| images.get(s, &mut out, pixmap))
            .collect();
        assert_eq!(ids, [1, 2, 3, 4]);
    }

    #[test]
    fn eviction_deletes_the_least_recently_shown_but_never_what_is_on_screen() {
        // Each image is a quarter of what is kept.
        let quarter = |_: &Spec| Pixmap::new(1024, 512).expect("pixmap");
        let mut images = Images::new();
        let mut out = String::new();
        images.begin();
        let first = images.get(&spec("first"), &mut out, quarter);
        for i in 0..3 {
            images.begin();
            images.get(&spec(&format!("{i}")), &mut out, quarter);
        }
        images.evict(&mut out);
        assert_eq!(images.by_spec.len(), 4, "they fit");
        // The oldest is shown again in the frame that overflows the cache.
        images.begin();
        images.get(&spec("first"), &mut out, quarter);
        images.get(&spec("new"), &mut out, quarter);
        images.get(&spec("newer"), &mut out, quarter);
        out.clear();
        images.evict(&mut out);
        assert_eq!(images.bytes, KEEP_BYTES);
        assert!(images.by_spec.values().any(|image| image.id == first));
        // The images of "0" and "1" were shown least recently.
        assert_eq!(
            out,
            "\x1b_Ga=d,d=I,i=2,q=2\x1b\\\x1b_Ga=d,d=I,i=3,q=2\x1b\\"
        );
    }

    #[test]
    fn large_images_go_in_chunks_that_carry_the_keys_once() {
        let mut out = String::new();
        let mut big = Pixmap::new(400, 200).expect("pixmap");
        // Noise, so compression leaves several chunks.
        for (i, b) in big.data_mut().iter_mut().enumerate() {
            *b = u8::try_from(i.wrapping_mul(2_654_435_761) >> 24 & 0xFF).unwrap_or(0);
        }
        transmit(&mut out, &big, 7, 40);
        let chunks: Vec<&str> = out.split_terminator("\x1b\\").collect();
        assert!(chunks.len() > 2, "{}", chunks.len());
        assert!(chunks[0].starts_with("\x1b_Ga=T,U=1,") && chunks[0].contains(",m=1;"));
        assert!(
            chunks[1..chunks.len() - 1]
                .iter()
                .all(|c| c.starts_with("\x1b_Gm=1;"))
        );
        assert!(chunks[chunks.len() - 1].starts_with("\x1b_Gm=0;"));
    }

    #[test]
    fn a_header_with_an_image_is_drawn_only_as_placeholders() {
        let mut app = app_with(&[("a.rs", "a\n", "b\n"), ("b.rs", "x\n", "y\n")]);
        app.settings.headers = crate::app::Headers::Images;
        app.relayout();
        app.header_images.insert(0, 0x0001_0203);
        let mut terminal = Terminal::new(TestBackend::new(100, 12)).expect("terminal");
        terminal
            .draw(|f| crate::ui::draw(f, &mut app))
            .expect("draw");
        let buf = terminal.backend().buffer();
        let diff = app.areas.diff;
        let cell = &buf[(diff.x + 3, diff.y)];
        assert_eq!(Some(cell.symbol().to_owned()), placeholder(3));
        assert_eq!(cell.fg, Color::Rgb(1, 2, 3));
        let text = crate::render::dump(buf, false);
        assert_eq!(
            text.matches("a.rs").count(),
            1,
            "only the sidebar names a.rs:\n{text}"
        );
        assert_eq!(
            text.matches("b.rs").count(),
            2,
            "b.rs has no image:\n{text}"
        );
    }
}
