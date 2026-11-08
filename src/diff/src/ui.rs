//! Drawing. Only visible rows are touched: each display line of the diff
//! pane is located in the stream and painted straight into the buffer.

use std::ops::Range;

use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};

use crate::app::{App, Body, Focus, Line, ListItem};
use crate::diff::{FileDiff, Text};
use crate::highlight::{Lines, Role, Run};
use crate::plan::{Kind, Row};
use crate::repo::Status;
use crate::theme::Palette;
use crate::wrap::{self, Segment};

pub(crate) const RAIL_WIDTH: u16 = 30;
pub(crate) const FILES_WIDTH: u16 = 36;

/// Splits the screen into rail, file list, diff pane and status line.
pub(crate) fn areas(app: &App, area: Rect) -> (Rect, Rect, Rect, Rect) {
    let status = Rect::new(
        area.x,
        area.bottom().saturating_sub(1),
        area.width,
        1.min(area.height),
    );
    let body = Rect::new(area.x, area.y, area.width, area.height.saturating_sub(1));
    let show_rail = app.settings.show_rail && body.width >= 130;
    let show_files = app.settings.show_files && body.width >= 100;
    let mut x = body.x;
    let mut take = |show: bool, w: u16| {
        if show {
            let r = Rect::new(x, body.y, w, body.height);
            x += w;
            r
        } else {
            Rect::new(x, body.y, 0, body.height)
        }
    };
    let rail = take(show_rail, RAIL_WIDTH);
    let files = take(show_files, FILES_WIDTH.min(body.width / 4).max(24));
    let diff = Rect::new(x, body.y, body.right().saturating_sub(x), body.height);
    (rail, files, diff, status)
}

pub(crate) fn draw(frame: &mut Frame<'_>, app: &mut App) {
    let full = frame.area();
    let (rail, files, diff, status) = areas(app, full);
    app.areas = crate::app::Areas { rail, files, diff };
    app.layout(diff.width, diff.height);
    let buf = frame.buffer_mut();
    let p = app.palette.clone();
    fill(
        buf,
        frame_area(rail, files, diff, status),
        Style::new().bg(p.bg).fg(p.fg),
    );
    if rail.width > 0 {
        draw_rail(buf, app, rail);
    }
    if files.width > 0 {
        draw_files(buf, app, files);
    }
    draw_diff(buf, app, diff);
    draw_status(buf, app, status);
    if app.help {
        draw_help(buf, &p, full);
    }
}

fn frame_area(a: Rect, b: Rect, c: Rect, d: Rect) -> Rect {
    a.union(b).union(c).union(d)
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

fn truncate_left(text: &str, max: usize) -> String {
    let width = unicode_width::UnicodeWidthStr::width(text);
    if width <= max {
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

fn status_color(p: &Palette, status: Status) -> Color {
    match status {
        Status::Added => p.green,
        Status::Deleted => p.red,
        Status::Modified => p.yellow,
    }
}

#[allow(clippy::many_single_char_names, clippy::too_many_lines)]
fn draw_rail(buf: &mut Buffer, app: &App, area: Rect) {
    let p = &app.palette;
    fill(buf, area, Style::new().bg(p.panel).fg(p.fg));
    let focused = app.focus == Focus::Worktrees;
    let title = format!(" {} worktrees", app.repo.worktrees.len());
    put(
        buf,
        area.x,
        area.y,
        &title,
        area.width,
        Style::new()
            .fg(if focused { p.accent } else { p.muted })
            .add_modifier(Modifier::BOLD),
    );
    let per = 2u16;
    let visible = usize::from(area.height.saturating_sub(2) / per);
    let start = app.rail_cursor.saturating_sub(visible.saturating_sub(1));
    for (slot, (i, wt)) in app
        .repo
        .worktrees
        .iter()
        .enumerate()
        .skip(start)
        .take(visible)
        .enumerate()
    {
        let y = area.y + 2 + u16::try_from(slot).unwrap_or(0) * per;
        let selected = i == app.rail_cursor;
        let current = i == app.current;
        let bg = if selected && focused {
            p.selection
        } else {
            p.panel
        };
        fill(
            buf,
            Rect::new(area.x, y, area.width, per),
            Style::new().bg(bg),
        );
        let marker = if current { "▌" } else { " " };
        put(buf, area.x, y, marker, 1, Style::new().fg(p.accent).bg(bg));
        // The repository prefix goes first when space runs out; branch names
        // differ at their start, so they are cut at the end.
        let room = usize::from(area.width.saturating_sub(3));
        let prefix = format!("{}@", app.repo.name);
        let branch_w = unicode_width::UnicodeWidthStr::width(wt.branch.as_str());
        let style = Style::new().bg(bg).fg(p.fg).add_modifier(if current {
            Modifier::BOLD
        } else {
            Modifier::empty()
        });
        let mut nx = area.x + 2;
        if prefix.chars().count() + branch_w <= room {
            nx += put(
                buf,
                nx,
                y,
                &prefix,
                area.width - 3,
                Style::new().bg(bg).fg(p.muted),
            );
        }
        let left = usize::from(area.right().saturating_sub(nx + 1));
        let branch = if branch_w > left {
            let cut: String = wt.branch.chars().take(left.saturating_sub(1)).collect();
            format!("{cut}…")
        } else {
            wt.branch.clone()
        };
        put(
            buf,
            nx,
            y,
            &branch,
            area.right().saturating_sub(nx + 1),
            style,
        );
        let x = area.x + 2;
        let w = area.width.saturating_sub(3);
        match app.summaries.get(&wt.path) {
            Some(s) if s.files == 0 => {
                put(
                    buf,
                    x,
                    y + 1,
                    "no changes",
                    w,
                    Style::new().bg(bg).fg(p.faint),
                );
            }
            Some(s) => {
                let mut cx = x;
                cx += put(
                    buf,
                    cx,
                    y + 1,
                    &format!("+{}", s.added),
                    w,
                    Style::new().bg(bg).fg(p.green),
                );
                cx += put(
                    buf,
                    cx,
                    y + 1,
                    &format!(" −{}", s.removed),
                    w.saturating_sub(cx - x),
                    Style::new().bg(bg).fg(p.red),
                );
                let files = format!(" · {} file{}", s.files, if s.files == 1 { "" } else { "s" });
                put(
                    buf,
                    cx,
                    y + 1,
                    &files,
                    w.saturating_sub(cx - x),
                    Style::new().bg(bg).fg(p.muted),
                );
            }
            None => {
                put(buf, x, y + 1, "…", w, Style::new().bg(bg).fg(p.faint));
            }
        }
    }
    draw_vrule(buf, p, area);
}

fn draw_vrule(buf: &mut Buffer, p: &Palette, area: Rect) {
    let x = area.right().saturating_sub(1);
    for y in area.top()..area.bottom() {
        buf[(x, y)].set_symbol("│").set_fg(p.rule);
    }
}

#[allow(clippy::many_single_char_names, clippy::too_many_lines)]
fn draw_files(buf: &mut Buffer, app: &App, area: Rect) {
    let p = &app.palette;
    fill(buf, area, Style::new().bg(p.panel).fg(p.fg));
    let focused = app.focus == Focus::Files;
    let Some(stream) = &app.stream else {
        put(
            buf,
            area.x + 1,
            area.y,
            "loading…",
            area.width,
            Style::new().fg(p.muted),
        );
        draw_vrule(buf, p, area);
        return;
    };
    let reviewed = stream.files.iter().filter(|f| f.reviewed).count();
    let title = format!(
        " {} files  {}/{} reviewed",
        stream.files.len(),
        reviewed,
        stream.files.len()
    );
    put(
        buf,
        area.x,
        area.y,
        &title,
        area.width - 1,
        Style::new()
            .fg(if focused { p.accent } else { p.muted })
            .add_modifier(Modifier::BOLD),
    );
    let rows = usize::from(area.height.saturating_sub(2));
    let cursor_item = app
        .list
        .iter()
        .position(|it| *it == ListItem::File(app.files_cursor))
        .unwrap_or(0);
    let start = cursor_item
        .saturating_sub(rows / 2)
        .min(app.list.len().saturating_sub(rows));
    let w = area.width.saturating_sub(1);
    for (slot, item) in app.list.iter().skip(start).take(rows).enumerate() {
        let y = area.y + 2 + u16::try_from(slot).unwrap_or(0);
        match item {
            ListItem::Dir(dir) => {
                let text = truncate_left(&format!("{dir}/"), usize::from(w.saturating_sub(1)));
                put(buf, area.x + 1, y, &text, w - 1, Style::new().fg(p.muted));
            }
            ListItem::File(i) => {
                let f = &stream.files[*i];
                let selected = *i == app.files_cursor;
                let current = *i == app.top.file;
                let bg = if selected && focused {
                    p.selection
                } else if current {
                    p.header_bg
                } else {
                    p.panel
                };
                fill(buf, Rect::new(area.x, y, w, 1), Style::new().bg(bg));
                let st = f.change.status;
                put(
                    buf,
                    area.x + 1,
                    y,
                    &st.letter().to_string(),
                    1,
                    Style::new()
                        .bg(bg)
                        .fg(status_color(p, st))
                        .add_modifier(Modifier::BOLD),
                );
                let counts = match (&f.body, f.counts()) {
                    (_, Some((a, r))) => format!("+{a} −{r}"),
                    (Body::Binary, _) => "bin".to_owned(),
                    _ => "…".to_owned(),
                };
                let cw = u16::try_from(unicode_width::UnicodeWidthStr::width(counts.as_str()))
                    .unwrap_or(0);
                let name = f.change.path.rsplit('/').next().unwrap_or(&f.change.path);
                let indent = if f.change.path.contains('/') { 2 } else { 0 };
                let name_w = w.saturating_sub(4 + indent + cw + 1);
                let name = truncate_left(name, usize::from(name_w));
                let name_style = Style::new()
                    .bg(bg)
                    .fg(if f.reviewed { p.muted } else { p.fg });
                let name_style = if f.reviewed {
                    name_style.add_modifier(Modifier::CROSSED_OUT)
                } else {
                    name_style
                };
                put(buf, area.x + 3 + indent, y, &name, name_w, name_style);
                let cx = area.x + w.saturating_sub(cw + 1);
                if let Some((a, r)) = f.counts() {
                    let aw = put(
                        buf,
                        cx,
                        y,
                        &format!("+{a}"),
                        cw,
                        Style::new().bg(bg).fg(p.green),
                    );
                    put(
                        buf,
                        cx + aw,
                        y,
                        &format!(" −{r}"),
                        cw - aw,
                        Style::new().bg(bg).fg(p.red),
                    );
                } else {
                    put(buf, cx, y, &counts, cw, Style::new().bg(bg).fg(p.faint));
                }
            }
        }
    }
    draw_vrule(buf, p, area);
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
    for y in 0..area.height {
        let line = app.line_at(top + u32::from(y));
        let row = Rect::new(area.x, area.y + y, area.width, 1);
        draw_line(buf, app, row, line);
    }
    // The current file's header stays pinned at the top.
    let file = app.top.file;
    for part in 0..app.settings.header_rows.min(area.height) {
        let row = Rect::new(area.x, area.y + part, area.width, 1);
        draw_line(buf, app, row, Line::Header { file, part });
    }
}

fn draw_line(buf: &mut Buffer, app: &App, area: Rect, line: Line) {
    let p = &app.palette;
    let Some(stream) = &app.stream else { return };
    match line {
        Line::Spacer => fill(buf, area, Style::new().bg(p.bg)),
        Line::Header { file, part } => draw_header(buf, app, area, file, part),
        Line::Note { file } => {
            fill(buf, area, Style::new().bg(p.bg));
            let text = match &stream.files[file].body {
                Body::Pending => "computing diff…",
                Body::Binary => "binary file not shown",
                Body::Unreadable(e) => e.as_str(),
                Body::Text(_) => "no line changes (whitespace, mode or line endings)",
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
            let Body::Text(diff) = &f.body else { return };
            let ctx = RowCtx {
                diff,
                old_hl: highlights(app, f, &diff.old),
                new_hl: highlights(app, f, &diff.new),
                number_width: f.number_width(),
                wrap: app.settings.wrap,
                palette: p,
            };
            draw_row(buf, &ctx, area, f.rows()[row], sub);
        }
    }
}

fn highlights<'a>(app: &'a App, f: &crate::app::FileView, text: &Text) -> Option<&'a Lines> {
    let lang = crate::highlight::language(&f.change.path)?;
    app.highlights.get(&(text.hash, lang))?.as_deref()
}

pub(crate) fn draw_header(buf: &mut Buffer, app: &App, area: Rect, file: usize, part: u16) {
    let p = &app.palette;
    let Some(stream) = &app.stream else { return };
    let f = &stream.files[file];
    fill(buf, area, Style::new().bg(p.header_bg).fg(p.fg));
    if part + 1 < app.settings.header_rows {
        // Upper rows of a tall header are left for its image.
        return;
    }
    let bg = p.header_bg;
    let st = f.change.status;
    let mut x = area.x + 1;
    let end = area.right();
    x += put(
        buf,
        x,
        area.y,
        &format!("{} ", st.letter()),
        end - x,
        Style::new()
            .bg(bg)
            .fg(status_color(p, st))
            .add_modifier(Modifier::BOLD),
    );
    let counts = f.counts().map(|(a, r)| (format!("+{a}"), format!(" −{r}")));
    let counts_w = counts
        .as_ref()
        .map_or(0, |(a, r)| a.chars().count() + r.chars().count());
    let mark = if f.reviewed { "  ✓ reviewed" } else { "" };
    let reserve = u16::try_from(counts_w + mark.chars().count() + 2).unwrap_or(0);
    let avail = usize::from(end.saturating_sub(x).saturating_sub(reserve));
    let path = truncate_left(&f.change.path, avail);
    let (dir, name) = path
        .rsplit_once('/')
        .map_or(("", path.as_str()), |(d, n)| (d, n));
    if !dir.is_empty() {
        x += put(
            buf,
            x,
            area.y,
            &format!("{dir}/"),
            end - x,
            Style::new().bg(bg).fg(p.muted),
        );
    }
    put(
        buf,
        x,
        area.y,
        name,
        end - x,
        Style::new().bg(bg).fg(p.fg).add_modifier(Modifier::BOLD),
    );
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
    diff: &'a FileDiff,
    old_hl: Option<&'a Lines>,
    new_hl: Option<&'a Lines>,
    number_width: u16,
    wrap: bool,
    palette: &'a Palette,
}

/// One side of a row: which line, how it changed.
#[derive(Clone, Copy)]
struct Side {
    kind: Kind,
    old: bool,
    line: Option<u32>,
}

#[allow(clippy::too_many_lines)]
fn draw_row(buf: &mut Buffer, ctx: &RowCtx<'_>, area: Rect, row: Row, sub: u32) {
    let p = ctx.palette;
    match row {
        Row::Gap { hidden, .. } => {
            fill(buf, area, Style::new().bg(p.gap_bg).fg(p.muted));
            let text = format!(
                "  ⋯  {hidden} unchanged line{}  ·  e to expand",
                if hidden == 1 { "" } else { "s" }
            );
            put(
                buf,
                area.x,
                area.y,
                &text,
                area.width,
                Style::new().bg(p.gap_bg).fg(p.muted),
            );
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
            let (bg, gutter_bg, sign, sign_fg) = kind_style(p, kind);
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
    let (bg, gutter_bg, sign, sign_fg) = kind_style(p, side.kind);
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
    let (text, hl, emph) = if side.old {
        (&ctx.diff.old, ctx.old_hl, ctx.diff.emph_old.get(&n))
    } else {
        (&ctx.diff.new, ctx.new_hl, ctx.diff.emph_new.get(&n))
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
        Kind::Context => (p.bg, p.bg),
        Kind::Delete => (p.del_bg, p.del_emph),
        Kind::Insert => (p.add_bg, p.add_emph),
    };
    let runs = hl.map_or(&[][..], |h| h.line(n));
    let emph = emph.map_or(&[][..], Vec::as_slice);
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
        segment.bytes,
        runs,
        emph,
        bg,
        emph_bg,
        p,
    );
    if truncated {
        buf[(area.right() - 1, area.y)]
            .set_symbol("›")
            .set_fg(p.muted);
    }
}

#[allow(clippy::too_many_arguments)]
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

fn draw_status(buf: &mut Buffer, app: &App, area: Rect) {
    let p = &app.palette;
    fill(buf, area, Style::new().bg(p.header_bg).fg(p.muted));
    let wt = &app.repo.worktrees[app.current];
    let mut x = area.x + 1;
    let end = area.right();
    x += put(
        buf,
        x,
        area.y,
        &format!("{}@{}", app.repo.name, wt.branch),
        end - x,
        Style::new()
            .bg(p.header_bg)
            .fg(p.fg)
            .add_modifier(Modifier::BOLD),
    );
    if let Some(stream) = &app.stream {
        x += put(
            buf,
            x,
            area.y,
            &format!("  vs {}", stream.base),
            end - x,
            Style::new().bg(p.header_bg).fg(p.muted),
        );
        let (a, r, done) = stream.counts();
        x += put(
            buf,
            x,
            area.y,
            &format!("  {} files ", stream.files.len()),
            end - x,
            Style::new().bg(p.header_bg).fg(p.muted),
        );
        x += put(
            buf,
            x,
            area.y,
            &format!("+{a}"),
            end - x,
            Style::new().bg(p.header_bg).fg(p.green),
        );
        x += put(
            buf,
            x,
            area.y,
            &format!(" −{r}"),
            end - x,
            Style::new().bg(p.header_bg).fg(p.red),
        );
        if !done {
            x += put(
                buf,
                x,
                area.y,
                " …",
                end - x,
                Style::new().bg(p.header_bg).fg(p.faint),
            );
        }
    }
    let layout = match app.effective_layout(app.areas.diff.width) {
        crate::plan::Layout::Unified => "unified",
        crate::plan::Layout::Split => "split",
    };
    let mode = match app.settings.mode {
        crate::repo::Mode::Base => "branch",
        crate::repo::Mode::Head => "uncommitted",
    };
    let right = match &app.message {
        Some(m) => format!("{m}  "),
        None => format!(
            "{mode} · {layout} · {}  ? help ",
            if app.settings.wrap { "wrap" } else { "nowrap" }
        ),
    };
    let rw = u16::try_from(unicode_width::UnicodeWidthStr::width(right.as_str())).unwrap_or(0);
    if x + rw + 2 < end {
        put(
            buf,
            end - rw,
            area.y,
            &right,
            rw,
            Style::new().bg(p.header_bg).fg(p.muted),
        );
    }
}

const HELP: &[(&str, &str)] = &[
    ("j k  ↓ ↑", "scroll, or move in a list"),
    ("^d ^u  space", "half page down/up, page down"),
    ("g  G", "top, bottom"),
    ("J K  ] [", "next/previous file"),
    ("n  N", "next/previous change"),
    ("{  }", "previous/next worktree"),
    ("tab  h l", "focus worktrees, files, diff"),
    ("enter", "open worktree or file; expand a fold"),
    ("e  E", "expand the first fold on screen; all folds in file"),
    ("v", "mark file reviewed"),
    ("s", "layout: auto, unified, split"),
    ("w", "wrap long lines"),
    ("m", "compare: branch (merge base) or uncommitted"),
    ("1  2", "show worktrees, files"),
    ("r", "refresh now"),
    ("q", "quit"),
];

#[allow(clippy::many_single_char_names)]
fn draw_help(buf: &mut Buffer, p: &Palette, area: Rect) {
    let w = 62.min(area.width);
    let h = (u16::try_from(HELP.len()).unwrap_or(0) + 4).min(area.height);
    let r = Rect::new(
        area.x + (area.width - w) / 2,
        area.y + (area.height - h) / 2,
        w,
        h,
    );
    fill(buf, r, Style::new().bg(p.header_bg).fg(p.fg));
    put(
        buf,
        r.x + 2,
        r.y + 1,
        "dots-diff keys",
        w - 2,
        Style::new()
            .bg(p.header_bg)
            .fg(p.accent)
            .add_modifier(Modifier::BOLD),
    );
    for (i, (keys, what)) in HELP.iter().enumerate() {
        let y = r.y + 3 + u16::try_from(i).unwrap_or(0);
        if y >= r.bottom() {
            break;
        }
        put(
            buf,
            r.x + 2,
            y,
            keys,
            14,
            Style::new()
                .bg(p.header_bg)
                .fg(p.fg)
                .add_modifier(Modifier::BOLD),
        );
        put(
            buf,
            r.x + 17,
            y,
            what,
            w.saturating_sub(18),
            Style::new().bg(p.header_bg).fg(p.muted),
        );
    }
}
