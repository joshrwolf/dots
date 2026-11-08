//! Overlays drawn over the panes: the comment editor, the agent picker,
//! the worktree switcher and help.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};

use super::{cells, fill, put, put_counts, truncate_left, truncate_middle};
use crate::app::App;
use crate::editor::Editor;
use crate::keys::BINDINGS;
use crate::picker::Picker;
use crate::review;
use crate::switcher::Switcher;
use crate::theme::Palette;
use crate::wrap::{self};

/// The comment editor, over the bottom of the diff pane.
#[expect(clippy::too_many_lines, reason = "one region drawn top to bottom")]
pub(super) fn draw_editor(buf: &mut Buffer, app: &App, ed: &Editor, area: Rect) {
    let p = &app.palette;
    let lines = ed.buffer.lines();
    let w = area.width.saturating_sub(4).clamp(20.min(area.width), 110);
    let text_rows = u16::try_from(lines.len())
        .unwrap_or(u16::MAX)
        .clamp(3, (area.height / 2).max(3));
    let h = (text_rows + 4).min(area.height);
    let outer = Rect::new(
        area.x + (area.width - w) / 2,
        area.bottom().saturating_sub(h + 1).max(area.y),
        w,
        h,
    );
    let bg = p.panel;
    fill(buf, outer, Style::new().bg(bg).fg(p.fg));
    border(buf, outer, Style::new().bg(bg).fg(p.accent));
    let inner = Rect::new(
        outer.x + 2,
        outer.y + 1,
        w.saturating_sub(4),
        h.saturating_sub(2),
    );
    let what = match &ed.place {
        review::Place::Review => "the review".to_owned(),
        review::Place::File { path } => path.clone(),
        review::Place::Lines { path, lines } => {
            if lines.end > lines.start {
                format!("{path}:{}-{}", lines.start + 1, lines.end + 1)
            } else {
                format!("{path}:{}", lines.start + 1)
            }
        }
    };
    let verb = match (ed.editing, ed.suggestion) {
        (_, true) => "suggest replacement code for",
        (Some(_), false) => "edit comment on",
        (None, false) => "comment on",
    };
    let title = format!("{verb} {what}");
    let kind = format!("[{}]", ed.kind.label());
    let used = put(
        buf,
        inner.x,
        inner.y,
        &title,
        inner.width.saturating_sub(cells(&kind) + 2),
        Style::new().bg(bg).fg(p.fg).add_modifier(Modifier::BOLD),
    );
    let kind_color = match ed.kind {
        review::Kind::Issue => p.red,
        review::Kind::Nit => p.muted,
        _ => p.accent,
    };
    put(
        buf,
        (inner.x + used + 2).min(inner.right().saturating_sub(cells(&kind))),
        inner.y,
        &kind,
        cells(&kind),
        Style::new().bg(bg).fg(kind_color),
    );
    let (row, col) = ed.buffer.cursor();
    let rows = usize::from(text_rows);
    let first = row.saturating_sub(rows.saturating_sub(1));
    let line = |i: usize| -> &str { lines.get(i).map_or("", String::as_str) };
    // Long lines scroll sideways with the cursor.
    let before: String = line(row).chars().take(col).collect();
    let shift = usize::from(cells(&before).saturating_sub(inner.width.saturating_sub(2)));
    let text_style = if ed.suggestion {
        Style::new().bg(p.add_bg).fg(p.fg)
    } else {
        Style::new().bg(bg).fg(p.fg)
    };
    for slot in 0..rows {
        let y = inner.y + 1 + u16::try_from(slot).unwrap_or(0);
        if ed.suggestion {
            fill(buf, Rect::new(inner.x, y, inner.width, 1), text_style);
        }
        let text = skip_cells(&line(first + slot).replace('\t', "    "), shift);
        put(buf, inner.x, y, &text, inner.width, text_style);
    }
    if lines.len() == 1 && lines[0].is_empty() && !ed.suggestion {
        put(
            buf,
            inner.x + 1,
            inner.y + 1,
            "type the comment; Enter adds a line",
            inner.width.saturating_sub(1),
            Style::new().bg(bg).fg(p.faint),
        );
    }
    let cx = inner.x + cells(&before).saturating_sub(u16::try_from(shift).unwrap_or(0));
    let cy = inner.y + 1 + u16::try_from(row - first).unwrap_or(0);
    if cx < inner.right() && cy < inner.bottom() {
        buf[(cx, cy)].set_bg(p.accent).set_fg(p.bg);
    }
    let hints = "Tab type · Ctrl+Enter or Ctrl+S save · Ctrl+G send now · Esc cancel";
    put(
        buf,
        inner.x,
        inner.bottom().saturating_sub(1),
        hints,
        inner.width,
        Style::new().bg(bg).fg(p.muted),
    );
}

/// `text` less its first `n` cells.
fn skip_cells(text: &str, n: usize) -> String {
    let mut skipped = 0;
    text.chars()
        .skip_while(|c| {
            let w = usize::from(wrap::char_width(*c));
            let skip = skipped + w <= n;
            if skip {
                skipped += w;
            }
            skip
        })
        .collect()
}

/// The list of live Pi sessions to send the review to.
#[expect(clippy::many_single_char_names, reason = "cell coordinates")]
pub(super) fn draw_picker(buf: &mut Buffer, app: &App, pk: &Picker, area: Rect) {
    let p = &app.palette;
    let w = 96.min(area.width.saturating_sub(4));
    let n = u16::try_from(pk.agents.len()).unwrap_or(u16::MAX);
    let h = (n + 3).min(area.height.saturating_sub(4)).max(4);
    let outer = Rect::new(
        area.x + (area.width - w) / 2,
        area.y + area.height.saturating_sub(h + 2) / 3,
        w,
        h + 2,
    );
    let bg = p.panel;
    fill(buf, outer, Style::new().bg(bg).fg(p.fg));
    border(buf, outer, Style::new().bg(bg).fg(p.rule));
    let r = Rect::new(outer.x + 2, outer.y + 1, w.saturating_sub(4), h);
    put(
        buf,
        r.x,
        r.y,
        "Send reviews of this worktree to",
        r.width,
        Style::new().bg(bg).fg(p.muted),
    );
    let qx = r.x
        + put(
            buf,
            r.x,
            r.y + 1,
            "> ",
            r.width,
            Style::new()
                .bg(bg)
                .fg(p.accent)
                .add_modifier(Modifier::BOLD),
        );
    let end = qx
        + put(
            buf,
            qx,
            r.y + 1,
            &pk.query,
            r.width.saturating_sub(2),
            Style::new().bg(bg).fg(p.fg),
        );
    put(buf, end, r.y + 1, "▏", 1, Style::new().bg(bg).fg(p.accent));
    let rows = usize::from(h.saturating_sub(3));
    let start = pk.cursor.saturating_sub(rows.saturating_sub(1));
    for (slot, (i, &agent)) in pk
        .matches
        .iter()
        .enumerate()
        .skip(start)
        .take(rows)
        .enumerate()
    {
        let y = r.y + 3 + u16::try_from(slot).unwrap_or(0);
        let a = &pk.agents[agent];
        let row_bg = if i == pk.cursor { p.selection } else { bg };
        fill(
            buf,
            Rect::new(r.x.saturating_sub(1), y, r.width + 2, 1),
            Style::new().bg(row_bg),
        );
        let used = put(
            buf,
            r.x,
            y,
            &a.label(),
            r.width / 3,
            Style::new()
                .bg(row_bg)
                .fg(p.fg)
                .add_modifier(Modifier::BOLD),
        );
        let pane = format!("  {}  ", a.pane());
        let px = r.x + used;
        let used2 = put(
            buf,
            px,
            y,
            &pane,
            r.width.saturating_sub(used),
            Style::new().bg(row_bg).fg(p.accent),
        );
        let cwd = a.cwd.display().to_string();
        let room = usize::from(r.width.saturating_sub(used + used2));
        put(
            buf,
            px + used2,
            y,
            &truncate_left(&cwd, room),
            r.width.saturating_sub(used + used2),
            Style::new().bg(row_bg).fg(p.muted),
        );
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
#[expect(
    clippy::many_single_char_names,
    clippy::too_many_lines,
    reason = "one region drawn top to bottom, in cell coordinates"
)]
pub(super) fn draw_switcher(buf: &mut Buffer, app: &App, sw: &Switcher, area: Rect) {
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

#[expect(clippy::many_single_char_names, reason = "cell coordinates")]
pub(super) fn draw_help(buf: &mut Buffer, p: &Palette, area: Rect) {
    let w = 80.min(area.width);
    let h = (u16::try_from(BINDINGS.len()).unwrap_or(0) + 4).min(area.height);
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
    for (i, b) in BINDINGS.iter().enumerate() {
        let (keys, what) = (b.label, b.help);
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
