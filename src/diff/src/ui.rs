//! Drawing. Only visible rows are touched: each display line of the diff
//! pane is located in the stream and painted straight into the buffer.

use std::ops::{Range, RangeInclusive};

use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};

use crate::app::{App, Body, FileTexts, FileView, Focus, Line, Mode};
use crate::cards::{self, Placed};
use crate::diff::FileDiff;
use crate::graphics;
use crate::highlight::{Lines, Role, Run};
use crate::moves::Moved;
use crate::plan::{Kind, Row};
use crate::repo::Status;
use crate::review;
use crate::theme::Palette;
use crate::wrap::{self, Segment};

mod overlay;
mod sidebar;
mod switcher;

use overlay::{draw_editor, draw_help, draw_picker};
use sidebar::draw_sidebar;
use switcher::draw_switcher;

/// Sidebar width when the user has not chosen one: a fifth of the screen.
const SIDEBAR_MIN: u16 = 26;
const SIDEBAR_MAX: u16 = 32;
/// Below this width the diff gets the whole screen.
const SIDEBAR_HIDE_BELOW: u16 = 80;
/// The status mark of a file, coloured by its status.
pub(crate) const STATUS_MARK: &str = "●";
/// A reviewed file, or a folder whose files are all reviewed.
pub(crate) const REVIEWED_MARK: &str = "✓";

/// The sidebar's width on a screen `width` columns wide; 0 when hidden.
pub(crate) fn sidebar_width(app: &App, width: u16) -> u16 {
    if !app.settings.sidebar || width < SIDEBAR_HIDE_BELOW {
        return 0;
    }
    app.settings
        .sidebar_width
        .unwrap_or_else(|| (width / 5).clamp(SIDEBAR_MIN, SIDEBAR_MAX))
        .min(width / 2)
}

/// Splits the screen into sidebar, diff pane and status line.
pub(crate) fn areas(app: &App, area: Rect) -> (Rect, Rect, Rect) {
    let status = Rect::new(
        area.x,
        area.bottom().saturating_sub(1),
        area.width,
        1.min(area.height),
    );
    let height = area.height.saturating_sub(1);
    let side = sidebar_width(app, area.width);
    let files = Rect::new(area.x, area.y, side, height);
    let diff = Rect::new(area.x + side, area.y, area.width - side, height);
    (files, diff, status)
}

/// Lays the view out for a frame of `full`; drawing does it too.
pub(crate) fn lay_out(app: &mut App, full: Rect) {
    let (files, diff, _) = areas(app, full);
    app.areas = crate::app::Areas { files, diff };
    app.layout(diff.width, diff.height);
    app.clamp_cursor(diff.height);
    app.prepare_emphasis(diff.height);
}

pub(crate) fn draw(frame: &mut Frame<'_>, app: &mut App) {
    let full = frame.area();
    lay_out(app, full);
    let (files, diff, status) = areas(app, full);
    let buf = frame.buffer_mut();
    let p = app.palette.clone();
    fill(buf, full, Style::new().bg(p.bg).fg(p.fg));
    if files.width > 0 {
        draw_sidebar(buf, app, files);
    }
    draw_diff(buf, app, diff);
    draw_status(buf, app, status);
    match &app.mode {
        Mode::Switcher(sw) => draw_switcher(buf, app, sw, full),
        Mode::Editor(ed) => draw_editor(buf, app, ed, diff),
        Mode::Picker(pk) => draw_picker(buf, app, pk, full),
        Mode::Help => draw_help(buf, &p, full),
        Mode::Normal | Mode::Visual(_) | Mode::Prefix => {}
    }
}

fn fill(buf: &mut Buffer, area: Rect, style: Style) {
    for y in area.top()..area.bottom() {
        for x in area.left()..area.right() {
            buf[(x, y)].reset();
            buf[(x, y)].set_style(style);
        }
    }
}

/// Writes `text` at (x, y) within `max` cells; returns the cells used.
fn put(buf: &mut Buffer, x: u16, y: u16, text: &str, max: u16, style: Style) -> u16 {
    if max == 0 {
        return 0;
    }
    let (end, _) = buf.set_stringn(x, y, text, usize::from(max), style);
    end - x
}

fn width(text: &str) -> usize {
    unicode_width::UnicodeWidthStr::width(text)
}

fn cells(text: &str) -> u16 {
    u16::try_from(width(text)).unwrap_or(u16::MAX)
}

/// Keeps the end of `text` within `max` cells, marking the cut with `…`.
fn truncate_left(text: &str, max: usize) -> String {
    if width(text) <= max {
        return text.to_owned();
    }
    let mut out: String = text
        .chars()
        .rev()
        .scan(0, |w, c| {
            *w += usize::from(wrap::char_width(c));
            (*w < max).then_some(c)
        })
        .collect();
    out.push('…');
    out.chars().rev().collect()
}

/// Fits a folder chain like `docs/runbooks/billing/` into `max` cells by
/// dropping whole middle folders (`docs/…/billing/`), keeping the first
/// and as many trailing folders as fit; only then cutting from the left.
fn truncate_chain(label: &str, max: usize) -> String {
    if width(label) <= max {
        return label.to_owned();
    }
    let parts: Vec<&str> = label.trim_end_matches('/').split('/').collect();
    if let [first, .., last] = parts.as_slice() {
        let head = format!("{first}/…/");
        let mut best = None;
        for k in 1..parts.len() - 1 {
            let tail = parts[parts.len() - k..].join("/");
            let candidate = format!("{head}{tail}/");
            if width(&candidate) > max {
                break;
            }
            best = Some(candidate);
        }
        if let Some(best) = best {
            return best;
        }
        let short = format!("…/{last}/");
        if width(&short) <= max {
            return short;
        }
    }
    truncate_left(label, max)
}

/// Keeps both ends of `text` within `max` cells, cutting the middle, so a
/// file name keeps its start and its extension.
pub(crate) fn truncate_middle(text: &str, max: usize) -> String {
    if width(text) <= max {
        return text.to_owned();
    }
    if max == 0 {
        return String::new();
    }
    let keep = max - 1;
    let tail_w = keep / 2;
    let head_w = keep - tail_w;
    let take = |chars: &mut dyn Iterator<Item = char>, room: usize| -> String {
        chars
            .scan(0, |w, c| {
                *w += usize::from(wrap::char_width(c));
                (*w <= room).then_some(c)
            })
            .collect()
    };
    let head = take(&mut text.chars(), head_w);
    let tail: String = take(&mut text.chars().rev(), tail_w)
        .chars()
        .rev()
        .collect();
    format!("{head}…{tail}")
}

/// Fits a path into `max` cells: the directory is cut first, from the
/// left, and the name only once no directory is left, in its middle.
pub(crate) fn fit_path(path: &str, max: usize) -> (String, String) {
    let (dir, name) = path.rsplit_once('/').map_or(("", path), |(d, n)| (d, n));
    let name_w = width(name);
    if name_w >= max || dir.is_empty() {
        return (String::new(), truncate_middle(name, max));
    }
    let room = max - name_w;
    let dir = format!("{dir}/");
    let dir = if width(&dir) <= room {
        dir
    } else if room >= 3 {
        truncate_left(&dir, room)
    } else {
        String::new()
    };
    (dir, name.to_owned())
}

pub(crate) fn status_color(p: &Palette, status: Status) -> Color {
    match status {
        Status::Added => p.green,
        Status::Deleted => p.red,
        Status::Modified => p.amber,
    }
}

/// Counts as `+a` and ` −r`, leaving out a zero side when the other is not.
fn count_text(a: u32, r: u32) -> (String, String) {
    match (a, r) {
        (_, 0) if a > 0 => (format!("+{a}"), String::new()),
        (0, _) if r > 0 => (String::new(), format!("−{r}")),
        _ => (format!("+{a}"), format!(" −{r}")),
    }
}

/// Added and removed counts drawn right-aligned to `end`; returns their
/// starting column.
#[expect(clippy::many_single_char_names, reason = "cell coordinates")]
fn put_counts(
    buf: &mut Buffer,
    end: u16,
    y: u16,
    (a, r): (u32, u32),
    bg: Color,
    p: &Palette,
) -> u16 {
    let (added, removed) = count_text(a, r);
    let x = end.saturating_sub(cells(&added) + cells(&removed));
    let aw = put(
        buf,
        x,
        y,
        &added,
        end - x,
        Style::new().bg(bg).fg(p.green_dim),
    );
    put(
        buf,
        x + aw,
        y,
        &removed,
        end - x - aw,
        Style::new().bg(bg).fg(p.red_dim),
    );
    x
}

fn draw_diff(buf: &mut Buffer, app: &App, area: Rect) {
    let p = &app.palette;
    if area.width == 0 || area.height == 0 {
        return;
    }
    let Some(stream) = &app.stream else {
        put(
            buf,
            area.x + 2,
            area.y + 1,
            "Loading changes…",
            area.width,
            Style::new().fg(p.muted),
        );
        return;
    };
    if stream.files.is_empty() {
        let text = format!("No changes against {}", stream.base);
        put(
            buf,
            area.x + 2,
            area.y + 1,
            &text,
            area.width.saturating_sub(2),
            Style::new().fg(p.muted),
        );
        return;
    }
    let top = app.global_top();
    let selected = selection(app);
    for y in 0..area.height {
        let line = app.line_at(top + u32::from(y));
        let row = Rect::new(area.x, area.y + y, area.width, 1);
        draw_line(buf, app, row, line);
        if let (
            Some((file, side, lines)),
            Line::Body {
                file: f,
                row: r,
                sub,
            },
        ) = (&selected, line)
            && *file == f
            && let Some(n) = side_line(stream, f, r, sub, *side)
            && lines.contains(&n)
        {
            mark_numbers(buf, app, row, f, *side, p.selection);
            if app.visual().is_some() {
                buf[(area.x, row.y)].set_symbol("▎").set_fg(p.accent);
            }
        }
    }
    if let Some(file) = app.pinned_header()
        && area.height > 0
    {
        let row = Rect::new(area.x, area.y, area.width, 1);
        draw_line(buf, app, row, Line::Header { file });
    }
    if app.focus == Focus::Diff && app.editor().is_none() && app.cursor_row < area.height {
        let y = area.y + app.cursor_row;
        let row = Rect::new(area.x, y, area.width, 1);
        if let Line::Body { file, row: r, sub } = app.line_at(app.cursor_line())
            && let Some(pos) = app.cursor_pos()
            && side_line(stream, file, r, sub, pos.side).is_some()
        {
            mark_numbers(buf, app, row, file, pos.side, p.selection);
        }
        buf[(area.x, y)].set_symbol("▌").set_fg(p.accent);
    }
}

/// The lines to show selected: the visual selection, or those the editor
/// comments on.
fn selection(app: &App) -> Option<(usize, review::Side, RangeInclusive<u32>)> {
    if app.visual().is_some() {
        let (start, end) = app.selection()?;
        return Some((start.file, start.side, start.line..=end));
    }
    let review::Place::Lines { path, lines } = &app.editor()?.place else {
        return None;
    };
    let file = app
        .stream
        .as_ref()?
        .files
        .iter()
        .position(|f| f.change.path == *path)?;
    Some((file, lines.side, lines.start..=lines.end))
}

/// The line of `side` that code line `sub` of a row shows, unless it is a
/// card's line.
fn side_line(
    stream: &crate::app::Stream,
    file: usize,
    row: usize,
    sub: u32,
    side: review::Side,
) -> Option<u32> {
    let f = &stream.files[file];
    let r = u32::try_from(row).ok()?;
    if f.cards.iter().any(|p| p.row == r && sub >= p.start) {
        return None;
    }
    crate::app::row_line(f.rows().get(row)?, side)
}

/// Tints the line numbers of `side` on a row.
fn mark_numbers(
    buf: &mut Buffer,
    app: &App,
    row: Rect,
    file: usize,
    side: review::Side,
    bg: Color,
) {
    let Some(f) = app.stream.as_ref().and_then(|s| s.files.get(file)) else {
        return;
    };
    let nw = f.number_width();
    let (x, w) = match (f.layout(), side) {
        (crate::plan::Layout::Unified, _) => (row.x, nw * 2 + 2),
        (crate::plan::Layout::Split, review::Side::Old) => (row.x, nw + 1),
        (crate::plan::Layout::Split, review::Side::New) => {
            (row.x + row.width.saturating_sub(1) / 2 + 1, nw + 1)
        }
    };
    for cx in x..(x + w).min(row.right()) {
        buf[(cx, row.y)].set_bg(bg).set_fg(app.palette.fg);
    }
}

/// Draws line `line` of a card in columns `span` of the row `area`.
fn draw_card(
    buf: &mut Buffer,
    app: &App,
    area: Rect,
    placed: &Placed,
    line: u32,
    span: (u16, u16),
    in_stack: bool,
) {
    let p = &app.palette;
    let Some(card) = cards::get(&app.session, placed.card) else {
        return;
    };
    let lines = cards::lines(&card, cards::text_width(span.1), in_stack);
    let Some(this) = lines.get(line as usize) else {
        return;
    };
    let (bar, dim, bg) = match &card {
        cards::Card::Comment(c) => {
            let bar = match (c.sent, c.outdated, c.kind) {
                (true, ..) => p.faint,
                (_, true, _) => p.amber,
                (_, _, review::Kind::Issue) => p.red,
                (_, _, review::Kind::Nit) => p.muted,
                _ => p.accent,
            };
            (bar, c.sent, if c.sent { p.bg } else { p.panel })
        }
        cards::Card::Note(n) => (p.amber, n.seen, if n.seen { p.bg } else { p.header_bg }),
    };
    let x = area.x + span.0;
    let r = Rect::new(x, area.y, span.1.min(area.right().saturating_sub(x)), 1);
    let (text_fg, title_fg) = if dim { (p.faint, p.faint) } else { (p.fg, bar) };
    let mut style = Style::new().bg(bg).fg(text_fg);
    if this.look == cards::Look::Code {
        style = style.bg(if dim { p.bg } else { p.add_bg });
    }
    fill(buf, r, Style::new().bg(bg));
    put(buf, r.x, r.y, "▌", 1, Style::new().bg(bg).fg(bar));
    let inner = r.width.saturating_sub(3);
    match this.look {
        cards::Look::Title => {
            let used = put(
                buf,
                r.x + 2,
                r.y,
                &this.text,
                inner,
                Style::new()
                    .bg(bg)
                    .fg(title_fg)
                    .add_modifier(Modifier::BOLD),
            );
            if app.focus == Focus::Diff && app.cursor_card() == Some(placed.card) {
                let hint = match &card {
                    cards::Card::Comment(c) if !c.sent => "e edit  d delete",
                    cards::Card::Comment(_) => "d delete",
                    cards::Card::Note(_) => "d dismiss",
                };
                let hx = r.right().saturating_sub(cells(hint) + 1);
                if hx > r.x + 2 + used + 1 {
                    put(
                        buf,
                        hx,
                        r.y,
                        hint,
                        cells(hint),
                        Style::new().bg(bg).fg(p.muted),
                    );
                }
            }
        }
        cards::Look::Text | cards::Look::Code => {
            if this.look == cards::Look::Code {
                fill(
                    buf,
                    Rect::new(r.x + 1, r.y, r.width.saturating_sub(1), 1),
                    style,
                );
            }
            put(
                buf,
                r.x + 2,
                r.y,
                &this.text.replace('\t', "    "),
                inner,
                style,
            );
        }
    }
}

fn draw_line(buf: &mut Buffer, app: &App, area: Rect, line: Line) {
    let p = &app.palette;
    let Some(stream) = &app.stream else { return };
    match line {
        Line::Spacer => fill(buf, area, Style::new().bg(p.bg)),
        Line::Header { file } => draw_header(buf, app, area, file),
        Line::Cards { file, review, line } => {
            fill(buf, area, Style::new().bg(p.bg));
            let placed = if review {
                &app.review_cards
            } else {
                &stream.files[file].file_cards
            };
            if let Some(at) = placed
                .iter()
                .find(|c| (c.start..c.start + c.height).contains(&line))
            {
                let span = cards::span(area.width, crate::plan::Layout::Unified, 0, None);
                draw_card(buf, app, area, at, line - at.start, span, true);
            }
        }
        Line::Body { file, row, sub }
            if let Some(at) = stream.files[file].cards.iter().find(|c| {
                c.row as usize == row && (c.start..c.start + c.height).contains(&sub)
            }) =>
        {
            let f = &stream.files[file];
            fill(buf, area, Style::new().bg(p.bg));
            let layout = f.layout();
            if layout == crate::plan::Layout::Split {
                let half = area.width.saturating_sub(1) / 2;
                buf[(area.x + half, area.y)].set_symbol("│").set_fg(p.rule);
            }
            let side =
                cards::get(&app.session, at.card).and_then(|c| c.place().lines().map(|l| l.side));
            let span = cards::span(area.width, layout, f.number_width(), side);
            draw_card(buf, app, area, at, sub - at.start, span, false);
        }
        Line::Note { file } => {
            fill(buf, area, Style::new().bg(p.bg));
            let change = &stream.files[file].change;
            let text = match &stream.files[file].body {
                Body::Pending => "computing diff…",
                Body::Binary(_) => "binary file not shown",
                Body::Unreadable(e) => e.as_str(),
                Body::Text(_) if change.from.is_some() => "renamed without content changes",
                Body::Text(_) if change.modes.is_some() => "mode changed; content unchanged",
                Body::Text(_) => "no line changes (whitespace or line endings)",
            };
            put(
                buf,
                area.x + 2,
                area.y,
                text,
                area.width.saturating_sub(2),
                Style::new().fg(p.muted).add_modifier(Modifier::ITALIC),
            );
        }
        Line::Body { file, row, sub } => {
            let f = &stream.files[file];
            // Rows and texts are kept near the view; until then, blank.
            let (Body::Text(diff), Some(texts), Some(&row)) =
                (&f.body, app.texts(f), f.rows().get(row))
            else {
                fill(buf, area, Style::new().bg(p.bg));
                return;
            };
            let ctx = RowCtx {
                app,
                f,
                diff,
                old_hl: highlights(app, f, texts.old.0),
                new_hl: highlights(app, f, texts.new.0),
                texts,
                number_width: f.number_width(),
                wrap: app.settings.wrap,
                palette: p,
                flash: f.flashed(&row, std::time::Instant::now()),
            };
            draw_row(buf, &ctx, area, row, sub);
        }
    }
}

fn highlights<'a>(app: &'a App, f: &FileView, id: Option<gix::ObjectId>) -> Option<&'a Lines> {
    let lang = crate::highlight::language(&f.change.path)?;
    app.cache.highlight(&(id?, lang))
}

fn draw_header(buf: &mut Buffer, app: &App, area: Rect, file: usize) {
    let p = &app.palette;
    let Some(stream) = &app.stream else { return };
    let f = &stream.files[file];
    if let Some(&id) = app.header_images.get(&file) {
        let style = Style::new().bg(p.header_bg).fg(graphics::image_color(id));
        for (col, x) in (area.left()..area.right()).enumerate() {
            let cell = &mut buf[(x, area.y)];
            cell.reset();
            let symbol = u16::try_from(col).ok().and_then(graphics::placeholder);
            match symbol {
                Some(symbol) => cell.set_symbol(&symbol).set_style(style),
                // Past the image's last column.
                None => cell.set_symbol(" ").set_style(Style::new().bg(p.header_bg)),
            };
        }
        return;
    }
    fill(buf, area, Style::new().bg(p.header_bg).fg(p.fg));
    let bg = p.header_bg;
    let st = f.change.status;
    let mut x = area.x;
    let end = area.right();
    x += put(
        buf,
        x,
        area.y,
        if f.folded { "▸ " } else { "▾ " },
        end - x,
        Style::new().bg(bg).fg(p.muted),
    );
    x += put(
        buf,
        x,
        area.y,
        &format!("{STATUS_MARK} "),
        end - x,
        Style::new().bg(bg).fg(status_color(p, st)),
    );
    let muted = Style::new().bg(bg).fg(p.muted);
    let note = app.header_note(&f.change).map(|m| format!("  {m}"));
    let counts = f.counts().map(|(a, r)| (format!("+{a}"), format!(" −{r}")));
    let counts_w = counts
        .as_ref()
        .map_or(0, |(a, r)| a.chars().count() + r.chars().count());
    let mark = if f.reviewed { "  ✓" } else { "" };
    let reserve = u16::try_from(counts_w + mark.chars().count() + 2).unwrap_or(0);
    let mut avail = usize::from(end.saturating_sub(x).saturating_sub(reserve));
    // A rename shows where the file came from, given at most a third.
    if let Some(from) = &f.change.from {
        let from = truncate_middle(from, avail / 3);
        x += put(buf, x, area.y, &from, end - x, muted);
        x += put(buf, x, area.y, " → ", end - x, muted);
        avail = usize::from(end.saturating_sub(x).saturating_sub(reserve));
    }
    let note_w = note.as_deref().map_or(0, width);
    let (dir, name) = fit_path(&f.change.path, avail.saturating_sub(note_w));
    if !dir.is_empty() {
        x += put(buf, x, area.y, &dir, end - x, muted);
    }
    x += put(
        buf,
        x,
        area.y,
        &name,
        end - x,
        Style::new().bg(bg).fg(p.fg).add_modifier(Modifier::BOLD),
    );
    if let Some(note) = &note {
        put(
            buf,
            x,
            area.y,
            note,
            end.saturating_sub(reserve).saturating_sub(x),
            muted,
        );
    }
    let mut cx = end.saturating_sub(reserve) + 1;
    if let Some((a, r)) = counts {
        cx += put(
            buf,
            cx,
            area.y,
            &a,
            end - cx,
            Style::new().bg(bg).fg(p.green),
        );
        cx += put(buf, cx, area.y, &r, end - cx, Style::new().bg(bg).fg(p.red));
    }
    put(
        buf,
        cx,
        area.y,
        mark,
        end - cx,
        Style::new().bg(bg).fg(p.green),
    );
}

struct RowCtx<'a> {
    app: &'a App,
    f: &'a FileView,
    diff: &'a FileDiff,
    texts: FileTexts,
    old_hl: Option<&'a Lines>,
    new_hl: Option<&'a Lines>,
    number_width: u16,
    wrap: bool,
    palette: &'a Palette,
    /// The row's hunk changed in the last refresh.
    flash: bool,
}

impl RowCtx<'_> {
    /// Gutter colour: the kind's, or the change mark after a refresh.
    fn gutter(&self, kind_gutter: Color) -> Color {
        if self.flash {
            self.palette.selection
        } else {
            kind_gutter
        }
    }

    /// The changed side's line, if it is moved code being marked.
    fn moved(&self, side: Side) -> Option<&Moved> {
        if side.kind == Kind::Context {
            return None;
        }
        self.app.moved(self.f, !side.old, side.line?)
    }

    /// Row tint, gutter tint, sign and sign colour of one side.
    fn style(&self, side: Side) -> (Color, Color, &'static str, Color) {
        let p = self.palette;
        let (bg, gutter, sign, sign_fg) = kind_style(p, side.kind);
        if self.moved(side).is_some() {
            (p.move_bg, self.gutter(p.move_gutter), sign, p.move_fg)
        } else {
            (bg, self.gutter(gutter), sign, sign_fg)
        }
    }

    /// Emphasised byte ranges of line `n` of one side, relative to the line.
    fn emphasis(&self, new_side: bool, n: u32) -> Vec<Range<u32>> {
        let Some(hunk) = self.diff.hunk_of(new_side, n) else {
            return Vec::new();
        };
        let Some(e) = self.app.emphasis(self.f, hunk) else {
            return Vec::new();
        };
        let text = if new_side {
            &self.texts.new.1
        } else {
            &self.texts.old.1
        };
        e.within(new_side, text.start(n)..text.start(n + 1))
    }
}

/// One side of a row: which line, how it changed.
#[derive(Clone, Copy)]
struct Side {
    kind: Kind,
    old: bool,
    line: Option<u32>,
}

#[expect(clippy::too_many_lines, reason = "one region drawn top to bottom")]
fn draw_row(buf: &mut Buffer, ctx: &RowCtx<'_>, area: Rect, row: Row, sub: u32) {
    let p = ctx.palette;
    match row {
        Row::Gap { hidden, .. } => {
            // A separator: the marker under the line numbers, the count
            // where code starts, and a quiet hint at the right.
            let style = Style::new().bg(p.gap_bg).fg(p.muted);
            fill(buf, area, style);
            let gutter = (ctx.number_width * 2 + 2).min(area.width);
            put(
                buf,
                area.x + gutter.saturating_sub(4) / 2,
                area.y,
                "⋯",
                gutter,
                style,
            );
            let text = format!(
                "{hidden} unchanged line{}",
                if hidden == 1 { "" } else { "s" }
            );
            let x = area.x + gutter + 1;
            let used = put(buf, x, area.y, &text, area.right().saturating_sub(x), style);
            let hint = "enter to expand ";
            let hint_w = u16::try_from(width(hint)).unwrap_or(u16::MAX);
            if gutter + 1 + used + hint_w + 2 <= area.width {
                put(
                    buf,
                    area.right() - hint_w,
                    area.y,
                    hint,
                    hint_w,
                    Style::new().bg(p.gap_bg).fg(p.faint),
                );
            }
        }
        Row::Truncated { hidden } => {
            fill(buf, area, Style::new().bg(p.gap_bg));
            let text = format!("  …  {hidden} more rows not shown");
            put(
                buf,
                area.x,
                area.y,
                &text,
                area.width,
                Style::new().bg(p.gap_bg).fg(p.muted),
            );
        }
        Row::Line { kind, old, new } => {
            let nw = ctx.number_width;
            let side = match kind {
                Kind::Delete => Side {
                    kind,
                    old: true,
                    line: old,
                },
                _ => Side {
                    kind,
                    old: false,
                    line: new,
                },
            };
            let (bg, gutter_bg, sign, sign_fg) = ctx.style(side);
            fill(buf, area, Style::new().bg(bg));
            let gutter = Rect::new(area.x, area.y, (nw * 2 + 2).min(area.width), 1);
            fill(buf, gutter, Style::new().bg(gutter_bg));
            if sub == 0 {
                let num = |n: Option<u32>| {
                    n.map_or_else(
                        || " ".repeat(usize::from(nw)),
                        |n| format!("{:>w$}", n + 1, w = usize::from(nw)),
                    )
                };
                let nums = format!("{} {} ", num(old), num(new));
                put(
                    buf,
                    area.x,
                    area.y,
                    &nums,
                    area.width,
                    Style::new().bg(gutter_bg).fg(p.faint),
                );
                put(
                    buf,
                    area.x + nw * 2 + 2,
                    area.y,
                    sign,
                    1,
                    Style::new().bg(bg).fg(sign_fg).add_modifier(Modifier::BOLD),
                );
            }
            let code = Rect::new(
                area.x + nw * 2 + 4,
                area.y,
                area.width.saturating_sub(nw * 2 + 4),
                1,
            );
            draw_code(buf, ctx, code, side, sub);
        }
        Row::Pair { old, new, changed } => {
            let half = area.width.saturating_sub(1) / 2;
            let left = Rect::new(area.x, area.y, half, 1);
            let right = Rect::new(area.x + half + 1, area.y, half, 1);
            let (lk, rk) = if changed {
                (Kind::Delete, Kind::Insert)
            } else {
                (Kind::Context, Kind::Context)
            };
            draw_half(
                buf,
                ctx,
                left,
                Side {
                    kind: lk,
                    old: true,
                    line: old,
                },
                sub,
            );
            draw_half(
                buf,
                ctx,
                right,
                Side {
                    kind: rk,
                    old: false,
                    line: new,
                },
                sub,
            );
            buf[(area.x + half, area.y)]
                .set_symbol("│")
                .set_fg(p.rule)
                .set_bg(p.bg);
        }
    }
}

fn kind_style(p: &Palette, kind: Kind) -> (Color, Color, &'static str, Color) {
    match kind {
        Kind::Context => (p.bg, p.bg, " ", p.faint),
        Kind::Delete => (p.del_bg, p.del_gutter, "-", p.red),
        Kind::Insert => (p.add_bg, p.add_gutter, "+", p.green),
    }
}

fn draw_half(buf: &mut Buffer, ctx: &RowCtx<'_>, area: Rect, side: Side, sub: u32) {
    let p = ctx.palette;
    let nw = ctx.number_width;
    let Some(n) = side.line else {
        fill(buf, area, Style::new().bg(p.filler));
        return;
    };
    let (bg, gutter_bg, sign, sign_fg) = ctx.style(side);
    fill(buf, area, Style::new().bg(bg));
    fill(
        buf,
        Rect::new(area.x, area.y, (nw + 1).min(area.width), 1),
        Style::new().bg(gutter_bg),
    );
    if sub == 0 {
        put(
            buf,
            area.x,
            area.y,
            &format!("{:>w$} ", n + 1, w = usize::from(nw)),
            area.width,
            Style::new().bg(gutter_bg).fg(p.faint),
        );
        put(
            buf,
            area.x + nw + 1,
            area.y,
            sign,
            1,
            Style::new().bg(bg).fg(sign_fg).add_modifier(Modifier::BOLD),
        );
    }
    let code = Rect::new(
        area.x + nw + 3,
        area.y,
        area.width.saturating_sub(nw + 3),
        1,
    );
    draw_code(buf, ctx, code, side, sub);
}

/// Paints display line `sub` of a source line: syntax colour for text,
/// the row tint behind it, and a stronger tint behind changed words.
fn draw_code(buf: &mut Buffer, ctx: &RowCtx<'_>, area: Rect, side: Side, sub: u32) {
    let p = ctx.palette;
    let Some(n) = side.line else { return };
    if area.width == 0 {
        return;
    }
    let (text, hl) = if side.old {
        (&ctx.texts.old.1, ctx.old_hl)
    } else {
        (&ctx.texts.new.1, ctx.new_hl)
    };
    let moved = ctx.moved(side);
    let emph = match moved {
        Some(m) => m.emphasis.clone(),
        None if side.kind == Kind::Context => Vec::new(),
        None => ctx.emphasis(!side.old, n),
    };
    let line = text.line(n);
    let segment = if ctx.wrap {
        let Some(s) = wrap::wrap(line, area.width).into_iter().nth(sub as usize) else {
            return;
        };
        s
    } else {
        Segment {
            bytes: 0..line.len(),
            indent: 0,
        }
    };
    let (bg, emph_bg) = match side.kind {
        _ if moved.is_some() => (p.move_bg, p.move_emph),
        Kind::Context => (p.bg, p.bg),
        Kind::Delete => (p.del_bg, p.del_emph),
        Kind::Insert => (p.add_bg, p.add_emph),
    };
    let runs = hl.map_or(&[][..], |h| h.line(n));
    let truncated = !ctx.wrap && wrap::wrapped_height(line, area.width) > 1;
    let limit = if truncated {
        area.width - 1
    } else {
        area.width
    };
    paint(
        buf,
        area.x + segment.indent,
        area.y,
        limit.saturating_sub(segment.indent),
        line,
        segment.bytes.clone(),
        runs,
        &emph,
        bg,
        emph_bg,
        p,
    );
    if truncated {
        buf[(area.right() - 1, area.y)]
            .set_symbol("›")
            .set_fg(p.muted);
    }
    // A block's first line says where the code went or came from, at the
    // right when it fits beside the code.
    if sub == 0
        && !truncated
        && let Some(Moved {
            other: Some((path, at)),
            ..
        }) = moved
    {
        let room = usize::from(area.width).saturating_sub(width(&line[segment.bytes]) + 2);
        if let Some(note) = move_note(side.old, path, *at, &ctx.f.change.path, room) {
            let note_w = u16::try_from(width(&note)).unwrap_or(u16::MAX);
            let style = Style::new()
                .bg(bg)
                .fg(p.move_fg)
                .add_modifier(Modifier::ITALIC);
            put(buf, area.right() - note_w, area.y, &note, note_w, style);
        }
    }
}

/// Where a moved block's code went (from its old side) or came from, in
/// the longest form that fits in `room` columns.
fn move_note(old: bool, path: &str, at: u32, here: &str, room: usize) -> Option<String> {
    let name = path.rsplit('/').next().unwrap_or(path);
    let (verb, arrow) = if old {
        ("moved to", "→")
    } else {
        ("moved from", "←")
    };
    let line = at + 1;
    let notes = if path == here {
        vec![
            format!("{verb} line {line} "),
            format!("{arrow} line {line} "),
        ]
    } else {
        vec![
            format!("{verb} {path}:{line} "),
            format!("{verb} {name}:{line} "),
            format!("{arrow} {name}:{line} "),
        ]
    };
    notes.into_iter().find(|note| width(note) <= room)
}

#[expect(
    clippy::too_many_arguments,
    reason = "a span of one row and its styles; a struct would only rename them"
)]
fn paint(
    buf: &mut Buffer,
    x: u16,
    y: u16,
    max: u16,
    line: &str,
    bytes: Range<usize>,
    runs: &[Run],
    emph: &[Range<u32>],
    bg: Color,
    emph_bg: Color,
    p: &Palette,
) {
    let mut run_i = 0;
    let mut emph_i = 0;
    let mut chunk = String::new();
    let mut chunk_style: Option<Style> = None;
    let mut cx = x;
    let end = x + max;
    let flush = |buf: &mut Buffer, chunk: &mut String, style: Option<Style>, cx: &mut u16| {
        if let Some(style) = style
            && !chunk.is_empty()
        {
            *cx += put(buf, *cx, y, chunk, end.saturating_sub(*cx), style);
        }
        chunk.clear();
    };
    for (i, c) in line[bytes.clone()].char_indices() {
        let at = u32::try_from(bytes.start + i).unwrap_or(u32::MAX);
        while run_i < runs.len() && runs[run_i].end <= at {
            run_i += 1;
        }
        while emph_i < emph.len() && emph[emph_i].end <= at {
            emph_i += 1;
        }
        let role = runs
            .get(run_i)
            .filter(|r| r.start <= at)
            .map_or(Role::Plain, |r| r.role);
        let emphasised = emph.get(emph_i).is_some_and(|r| r.start <= at);
        let mut style = Style::new()
            .fg(p.syntax(role))
            .bg(if emphasised { emph_bg } else { bg });
        if role == Role::Comment {
            style = style.add_modifier(Modifier::ITALIC);
        }
        if chunk_style != Some(style) {
            flush(buf, &mut chunk, chunk_style, &mut cx);
            chunk_style = Some(style);
            if cx >= end {
                return;
            }
        }
        match c {
            '\t' => chunk.push_str("    "),
            c if c.is_control() => chunk.push('·'),
            c => chunk.push(c),
        }
    }
    flush(buf, &mut chunk, chunk_style, &mut cx);
}

/// The status line's note while lines are selected.
fn visual_hint(app: &App) -> Option<String> {
    app.visual()?;
    let (first, last) = app.selection()?;
    let n = last - first.line + 1;
    let s = if n == 1 { "" } else { "s" };
    Some(format!(
        "VISUAL {n} line{s}  c comment  s suggest  esc cancel  "
    ))
}

/// The status line's note on moved code, while it is marked.
fn moves_status(app: &App) -> String {
    match (app.show_moves, &app.moves) {
        (false, _) => String::new(),
        (true, None) => " · finding moves".to_owned(),
        (true, Some(m)) if m.skipped => " · moves: too many lines".to_owned(),
        (true, Some(m)) => format!(" · {} moved", m.blocks),
    }
}

/// The colour a worker's status is shown in.
fn worker_color(status: &crate::agents::Status, p: &Palette) -> Color {
    use crate::agents::Status;
    match status {
        Status::Running | Status::Paused => p.accent,
        Status::Asking => p.amber,
        Status::Done => p.green,
        Status::Failed | Status::Lost => p.red,
        Status::Stopped | Status::Other(_) => p.faint,
    }
}

/// Who works in the shown worktree and how their lead worker stands, on
/// the status line from `x`. Returns the width drawn.
fn draw_owners(buf: &mut Buffer, app: &App, x: u16, y: u16, end: u16) -> u16 {
    let p = &app.palette;
    let muted = Style::new().bg(p.header_bg).fg(p.muted);
    let Some(owners) = app.owners(app.current) else {
        return 0;
    };
    let mut at = x;
    let names = owners.names();
    if !names.is_empty() {
        at += put(buf, at, y, "  ", end - at, muted);
        at += put(buf, at, y, &names.join(", "), end - at, muted.fg(p.fg));
    }
    if let Some(lead) = owners.lead() {
        at += put(
            buf,
            at,
            y,
            &format!(" {}", lead.status.label()),
            end - at,
            muted.fg(worker_color(&lead.status, p)),
        );
    }
    at - x
}

#[expect(
    clippy::too_many_lines,
    reason = "the status line's parts, drawn left to right"
)]
fn draw_status(buf: &mut Buffer, app: &App, area: Rect) {
    let p = &app.palette;
    let bg = p.header_bg;
    fill(buf, area, Style::new().bg(bg).fg(p.muted));
    let mut x = area.x + 1;
    let end = area.right();
    let muted = Style::new().bg(bg).fg(p.muted);
    // The sidebar names the branch; without it the status line does.
    if app.areas.files.width == 0 {
        let branch = &app.repo.worktrees[app.current].branch;
        x += put(
            buf,
            x,
            area.y,
            branch,
            end - x,
            Style::new().bg(bg).fg(p.fg).add_modifier(Modifier::BOLD),
        );
        x += put(buf, x, area.y, "  ", end - x, muted);
    }
    if let Some(text) = visual_hint(app) {
        x += put(
            buf,
            x,
            area.y,
            &text,
            end - x,
            muted.fg(p.accent).add_modifier(Modifier::BOLD),
        );
    }
    if app.settings.since {
        x += put(
            buf,
            x,
            area.y,
            "since last send  ",
            end - x,
            muted.fg(p.amber).add_modifier(Modifier::BOLD),
        );
    }
    if let Some(filter) = &app.author_filter {
        x += put(
            buf,
            x,
            area.y,
            &format!("only {}  ", filter.label()),
            end - x,
            muted.fg(p.amber).add_modifier(Modifier::BOLD),
        );
    }
    if let Some(stream) = &app.stream {
        let reviewed = stream.files.iter().filter(|f| f.reviewed).count();
        let (a, r, done) = stream.counts();
        if !app.settings.since {
            x += put(
                buf,
                x,
                area.y,
                &format!("vs {}  ", stream.base),
                end - x,
                muted,
            );
        }
        x += put(
            buf,
            x,
            area.y,
            &format!("{}/{} files  ", reviewed, stream.files.len()),
            end - x,
            muted,
        );
        x += put(buf, x, area.y, &format!("+{a}"), end - x, muted.fg(p.green));
        x += put(buf, x, area.y, &format!(" −{r}"), end - x, muted.fg(p.red));
        if !done {
            x += put(buf, x, area.y, " …", end - x, muted.fg(p.faint));
        }
        let drafts = app.session.state().drafts().count();
        if drafts > 0 {
            x += put(
                buf,
                x,
                area.y,
                &format!("  {drafts} draft{}", if drafts == 1 { "" } else { "s" }),
                end - x,
                muted.fg(p.accent),
            );
        }
    }
    x += draw_owners(buf, app, x, area.y, end);
    if !app.target.is_empty() {
        x += put(buf, x, area.y, "  ", end - x, muted);
        x += put(buf, x, area.y, &app.target, end - x, muted.fg(p.fg));
    }
    let layout = match app.effective_layout(app.areas.diff.width) {
        crate::plan::Layout::Unified => "unified",
        crate::plan::Layout::Split => "split",
    };
    let mode = match app.settings.mode {
        crate::repo::Mode::Base => "branch",
        crate::repo::Mode::Head => "uncommitted",
    };
    let moves = moves_status(app);
    let right = match &app.message {
        Some(m) => format!("{m} "),
        None => format!(
            "{mode} · {layout}{}{moves} · ? ",
            if app.settings.wrap { "" } else { " · nowrap" }
        ),
    };
    let rw = cells(&right);
    if x + rw + 2 < end {
        put(buf, end - rw, area.y, &right, rw, muted.fg(p.faint));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn move_notes_shorten_to_fit() {
        let note = |old, room| move_note(old, "src/util.rs", 4, "src/server.rs", room);
        assert_eq!(note(true, 80).as_deref(), Some("moved to src/util.rs:5 "));
        assert_eq!(note(true, 20).as_deref(), Some("moved to util.rs:5 "));
        assert_eq!(note(false, 12).as_deref(), Some("← util.rs:5 "));
        assert_eq!(note(true, 5), None);
        assert_eq!(
            move_note(false, "a.rs", 9, "a.rs", 80).as_deref(),
            Some("moved from line 10 ")
        );
    }

    #[test]
    fn folder_chains_drop_middle_folders_first() {
        assert_eq!(
            truncate_chain("docs/runbooks/billing/", 30),
            "docs/runbooks/billing/"
        );
        assert_eq!(
            truncate_chain("docs/runbooks/billing/", 15),
            "docs/…/billing/"
        );
        assert_eq!(truncate_chain("a/b/c/d/e/", 9), "a/…/d/e/");
        assert_eq!(truncate_chain("docs/runbooks/billing/", 10), "…/billing/");
        assert_eq!(truncate_chain("verylongname/", 8), "…ngname/");
    }

    #[test]
    fn names_are_cut_in_the_middle() {
        assert_eq!(truncate_middle("highlight.rs", 12), "highlight.rs");
        assert_eq!(truncate_middle("highlight.rs", 9), "high…t.rs");
        assert_eq!(width(&truncate_middle("highlight.rs", 9)), 9);
    }

    #[test]
    fn paths_lose_their_directory_before_their_name() {
        let fit = |max| fit_path("docs/containers/guide.md", max);
        assert_eq!(fit(30), ("docs/containers/".into(), "guide.md".into()));
        assert_eq!(fit(14), ("…ners/".into(), "guide.md".into()));
        assert_eq!(fit(9), (String::new(), "guide.md".into()));
        assert_eq!(fit(6), (String::new(), "gui…md".into()));
    }
}
