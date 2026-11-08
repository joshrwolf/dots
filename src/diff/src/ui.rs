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

/// Sidebar width when the user has not chosen one: a fifth of the screen.
const SIDEBAR_MIN: u16 = 26;
const SIDEBAR_MAX: u16 = 32;
/// Below this width the diff gets the whole screen.
const SIDEBAR_HIDE_BELOW: u16 = 80;
/// The status mark of a file, coloured by its status.
pub(crate) const STATUS_MARK: &str = "●";

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

pub(crate) fn draw(frame: &mut Frame<'_>, app: &mut App) {
    let full = frame.area();
    let (files, diff, status) = areas(app, full);
    app.areas = crate::app::Areas { files, diff };
    app.layout(diff.width, diff.height);
    let buf = frame.buffer_mut();
    let p = app.palette.clone();
    fill(buf, full, Style::new().bg(p.bg).fg(p.fg));
    if files.width > 0 {
        draw_sidebar(buf, app, files);
    }
    draw_diff(buf, app, diff);
    draw_status(buf, app, status);
    if app.switcher.is_some() {
        draw_switcher(buf, app, full);
    }
    if app.help {
        draw_help(buf, &p, full);
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
#[allow(clippy::many_single_char_names)]
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

/// The sidebar: the current worktree's branch, then its files as a tree.
#[allow(clippy::many_single_char_names, clippy::too_many_lines)]
fn draw_sidebar(buf: &mut Buffer, app: &App, area: Rect) {
    let p = &app.palette;
    let inner = Rect::new(area.x, area.y, area.width.saturating_sub(1), area.height);
    fill(buf, inner, Style::new().bg(p.panel).fg(p.fg));
    for y in area.top()..area.bottom() {
        let cell = &mut buf[(area.right() - 1, y)];
        cell.set_symbol("│").set_fg(p.rule).set_bg(p.bg);
    }
    // The branch line, which `w` replaces with the switcher.
    let branch = &app.repo.worktrees[app.current].branch;
    let head = Rect::new(inner.x, inner.y, inner.width, 1);
    fill(buf, head, Style::new().bg(p.header_bg));
    let room = inner.width.saturating_sub(2);
    let branch = truncate_middle(branch, usize::from(room));
    put(
        buf,
        inner.x + 1,
        inner.y,
        &branch,
        room,
        Style::new()
            .bg(p.header_bg)
            .fg(p.fg)
            .add_modifier(Modifier::BOLD),
    );
    let list = Rect::new(
        inner.x,
        inner.y + 1,
        inner.width,
        inner.height.saturating_sub(1),
    );
    let Some(stream) = &app.stream else {
        put(
            buf,
            list.x + 1,
            list.y,
            "loading…",
            list.width,
            Style::new().bg(p.panel).fg(p.faint),
        );
        return;
    };
    let focused = app.focus == Focus::Files;
    let rows = usize::from(list.height);
    let start = app.list_start(rows);
    let visible = &app.list[start..(start + rows).min(app.list.len())];
    let end = list.right().saturating_sub(1);
    // Names matter more than counts: the counts column shows only when
    // every visible name fits beside it, so rows stay aligned.
    let show_counts = visible.iter().all(|item| match item {
        ListItem::Dir { .. } => true,
        ListItem::File { index, depth } => {
            let f = &stream.files[*index];
            let name = f.change.path.rsplit('/').next().unwrap_or(&f.change.path);
            let counts = f.counts().map_or(3, |(a, r)| {
                let (added, removed) = count_text(a, r);
                cells(&added) + cells(&removed)
            });
            list.x + 3 + depth + cells(name) + 1 + counts <= end
        }
    });
    for (slot, item) in visible.iter().enumerate() {
        let y = list.y + u16::try_from(slot).unwrap_or(0);
        match item {
            ListItem::Dir { label, depth } => {
                let x = list.x + 1 + depth;
                let label = truncate_left(label, usize::from(list.right().saturating_sub(x + 1)));
                put(
                    buf,
                    x,
                    y,
                    &label,
                    list.right().saturating_sub(x),
                    Style::new().bg(p.panel).fg(p.muted),
                );
            }
            ListItem::File { index, depth } => {
                let f = &stream.files[*index];
                let bg = if *index == app.files_cursor && focused {
                    p.selection
                } else if *index == app.top.file {
                    p.header_bg
                } else {
                    p.panel
                };
                fill(
                    buf,
                    Rect::new(list.x, y, list.width, 1),
                    Style::new().bg(bg),
                );
                let x = list.x + 1 + depth;
                let st = f.change.status;
                put(
                    buf,
                    x,
                    y,
                    STATUS_MARK,
                    1,
                    Style::new().bg(bg).fg(status_color(p, st)),
                );
                let name = f.change.path.rsplit('/').next().unwrap_or(&f.change.path);
                let name_x = x + 2;
                let counts = f.counts().filter(|_| show_counts);
                let counts_x = match (counts, &f.body) {
                    (Some(c), _) => put_counts(buf, end, y, c, bg, p),
                    (None, Body::Binary) if show_counts => {
                        let x = end.saturating_sub(3);
                        put(buf, x, y, "bin", 3, Style::new().bg(bg).fg(p.faint));
                        x
                    }
                    _ => end,
                };
                let room = usize::from(counts_x.saturating_sub(name_x + 1));
                let name = truncate_middle(name, room);
                let style = if f.reviewed {
                    Style::new().bg(bg).fg(p.faint)
                } else {
                    Style::new().bg(bg).fg(p.fg)
                };
                put(
                    buf,
                    name_x,
                    y,
                    &name,
                    counts_x.saturating_sub(name_x),
                    style,
                );
            }
        }
    }
}

/// A thin rounded frame just inside `r`.
fn border(buf: &mut Buffer, r: Rect, style: Style) {
    if r.width < 2 || r.height < 2 {
        return;
    }
    let (right, bottom) = (r.right() - 1, r.bottom() - 1);
    for x in r.left()..=right {
        buf[(x, r.y)].set_symbol("─").set_style(style);
        buf[(x, bottom)].set_symbol("─").set_style(style);
    }
    for y in r.top()..=bottom {
        buf[(r.x, y)].set_symbol("│").set_style(style);
        buf[(right, y)].set_symbol("│").set_style(style);
    }
    buf[(r.x, r.y)].set_symbol("╭");
    buf[(right, r.y)].set_symbol("╮");
    buf[(r.x, bottom)].set_symbol("╰");
    buf[(right, bottom)].set_symbol("╯");
}

/// The worktree switcher: a filter line over branch, counts, file count
/// and how recently each was used. Counts fill in as they are computed.
#[allow(clippy::many_single_char_names, clippy::too_many_lines)]
fn draw_switcher(buf: &mut Buffer, app: &App, area: Rect) {
    let Some(sw) = &app.switcher else { return };
    let p = &app.palette;
    let w = 76.min(area.width.saturating_sub(4));
    let n = u16::try_from(app.repo.worktrees.len()).unwrap_or(u16::MAX);
    let h = (n + 2).min(area.height.saturating_sub(4)).max(3);
    let outer = Rect::new(
        area.x + (area.width - w) / 2,
        area.y + area.height.saturating_sub(h + 2) / 3,
        w,
        h + 2,
    );
    let bg = p.panel;
    fill(buf, outer, Style::new().bg(bg).fg(p.fg));
    border(buf, outer, Style::new().bg(bg).fg(p.rule));
    let r = Rect::new(outer.x + 1, outer.y + 1, w - 2, h);
    let w = r.width;
    let x = r.x + 1;
    let iw = w.saturating_sub(2);
    let px = x + put(
        buf,
        x,
        r.y,
        "> ",
        iw,
        Style::new()
            .bg(bg)
            .fg(p.accent)
            .add_modifier(Modifier::BOLD),
    );
    let qx = px
        + put(
            buf,
            px,
            r.y,
            &sw.query,
            iw.saturating_sub(px - x),
            Style::new().bg(bg).fg(p.fg),
        );
    put(buf, qx, r.y, "▏", 1, Style::new().bg(bg).fg(p.accent));
    let count = format!("{}/{}", sw.matches.len(), n);
    put(
        buf,
        r.right().saturating_sub(cells(&count) + 1),
        r.y,
        &count,
        cells(&count),
        Style::new().bg(bg).fg(p.faint),
    );
    for col in r.left()..r.right() {
        buf[(col, r.y + 1)]
            .set_symbol("─")
            .set_fg(p.rule)
            .set_bg(bg);
    }
    let rows = usize::from(h.saturating_sub(2));
    let start = sw.cursor.saturating_sub(rows.saturating_sub(1));
    let now = std::time::SystemTime::now();
    for (slot, (i, m)) in sw
        .matches
        .iter()
        .enumerate()
        .skip(start)
        .take(rows)
        .enumerate()
    {
        let y = r.y + 2 + u16::try_from(slot).unwrap_or(0);
        let wt = &app.repo.worktrees[m.worktree];
        let row_bg = if i == sw.cursor { p.selection } else { bg };
        fill(buf, Rect::new(r.x, y, w, 1), Style::new().bg(row_bg));
        if m.worktree == app.current {
            put(buf, r.x, y, "▌", 1, Style::new().bg(row_bg).fg(p.accent));
        }
        // Right columns: counts, file count, age; blank until known.
        let end = r.right().saturating_sub(1);
        let age = wt
            .touched
            .map(|t| crate::switcher::age(t, now))
            .unwrap_or_default();
        put(
            buf,
            end.saturating_sub(4),
            y,
            &format!("{age:>4}"),
            4,
            Style::new().bg(row_bg).fg(p.faint),
        );
        let files_end = end.saturating_sub(5);
        let mut left_end = files_end.saturating_sub(24);
        if let Some(s) = app.summaries.get(&wt.path) {
            let (files, fg) = match s.files {
                0 => ("clean".to_owned(), p.faint),
                1 => ("1 file".to_owned(), p.muted),
                n => (format!("{n} files"), p.muted),
            };
            put(
                buf,
                files_end.saturating_sub(cells(&files)),
                y,
                &files,
                cells(&files),
                Style::new().bg(row_bg).fg(fg),
            );
            if s.files > 0 {
                let cx = put_counts(
                    buf,
                    files_end.saturating_sub(11),
                    y,
                    (s.added, s.removed),
                    row_bg,
                    p,
                );
                left_end = left_end.min(cx);
            }
        }
        let room = usize::from(left_end.saturating_sub(x + 2));
        let branch = truncate_middle(&wt.branch, room);
        let plain = Style::new().bg(row_bg).fg(p.fg);
        let hit = plain.fg(p.accent).add_modifier(Modifier::BOLD);
        let mut cx = x + 1;
        for (ci, ch) in branch.chars().enumerate() {
            let hit_here = branch.chars().count() == wt.branch.chars().count()
                && m.positions.contains(&u32::try_from(ci).unwrap_or(u32::MAX));
            cx += put(
                buf,
                cx,
                y,
                ch.encode_utf8(&mut [0; 4]),
                left_end.saturating_sub(cx),
                if hit_here { hit } else { plain },
            );
        }
    }
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
        &format!("{STATUS_MARK} "),
        end - x,
        Style::new().bg(bg).fg(status_color(p, st)),
    );
    let counts = f.counts().map(|(a, r)| (format!("+{a}"), format!(" −{r}")));
    let counts_w = counts
        .as_ref()
        .map_or(0, |(a, r)| a.chars().count() + r.chars().count());
    let mark = if f.reviewed { "  ✓" } else { "" };
    let reserve = u16::try_from(counts_w + mark.chars().count() + 2).unwrap_or(0);
    let avail = usize::from(end.saturating_sub(x).saturating_sub(reserve));
    let (dir, name) = fit_path(&f.change.path, avail);
    if !dir.is_empty() {
        x += put(
            buf,
            x,
            area.y,
            &dir,
            end - x,
            Style::new().bg(bg).fg(p.muted),
        );
    }
    put(
        buf,
        x,
        area.y,
        &name,
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
    if let Some(stream) = &app.stream {
        let reviewed = stream.files.iter().filter(|f| f.reviewed).count();
        let (a, r, done) = stream.counts();
        x += put(
            buf,
            x,
            area.y,
            &format!("vs {}  ", stream.base),
            end - x,
            muted,
        );
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
        Some(m) => format!("{m} "),
        None => format!(
            "{mode} · {layout}{} · ? ",
            if app.settings.wrap { "" } else { " · nowrap" }
        ),
    };
    let rw = cells(&right);
    if x + rw + 2 < end {
        put(buf, end - rw, area.y, &right, rw, muted.fg(p.faint));
    }
}

const HELP: &[(&str, &str)] = &[
    ("j k  ↓ ↑", "scroll, or move in the file list"),
    ("^d ^u  space", "half page down/up, page down"),
    ("g  G", "top, bottom"),
    ("J K  ] [", "next/previous file"),
    ("n  N", "next/previous change"),
    ("w", "switch worktree (type to filter)"),
    ("{  }", "previous/next worktree"),
    ("tab  h l", "focus files or diff"),
    ("enter", "open file; expand a fold"),
    ("e  E", "expand the first fold on screen; all folds in file"),
    ("v", "mark file reviewed"),
    ("s", "layout: auto, unified, split"),
    ("z", "wrap long lines"),
    ("m", "compare: branch (merge base) or uncommitted"),
    ("t  < >", "show sidebar; narrower, wider"),
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

#[cfg(test)]
mod tests {
    use super::*;

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
