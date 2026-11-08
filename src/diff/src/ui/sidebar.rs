//! The sidebar: the worktree's branch, then its files as a tree.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};

use super::{
    REVIEWED_MARK, STATUS_MARK, cells, count_text, fill, put, put_counts, status_color,
    truncate_chain, truncate_middle,
};
use crate::app::{App, Body, Focus, ListItem};

/// The sidebar: the current worktree's branch, then its files as a tree.
#[expect(clippy::too_many_lines, reason = "one region drawn top to bottom")]
pub(super) fn draw_sidebar(buf: &mut Buffer, app: &App, area: Rect) {
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
    let sidebar = &app.sidebar;
    let start = sidebar.list_start(rows);
    let all = sidebar.rows();
    let visible = &all[start..(start + rows).min(all.len())];
    let end = list.right().saturating_sub(1);
    let current_row = sidebar.row_of_file(app.top.file);
    let drafts = app.draft_counts();
    // What each row shows: marker, name, counts and whether it is reviewed.
    let rows: Vec<SidebarRow<'_>> = visible
        .iter()
        .map(|item| match item {
            ListItem::Dir {
                label, path, files, ..
            } => {
                let (a, r, reviewed) = app.dir_summary(files.clone());
                let open = !sidebar.is_collapsed(path);
                SidebarRow {
                    mark: (if open { "▾" } else { "▸" }, p.muted),
                    name: label.as_str(),
                    counts: ((a, r) != (0, 0)).then_some((a, r)),
                    binary: false,
                    reviewed,
                    dir: true,
                    drafts: 0,
                }
            }
            ListItem::File { index, .. } => {
                let f = &stream.files[*index];
                let mark = if f.reviewed {
                    (REVIEWED_MARK, p.faint)
                } else {
                    (STATUS_MARK, status_color(p, f.change.status))
                };
                SidebarRow {
                    mark,
                    name: f.change.path.rsplit('/').next().unwrap_or(&f.change.path),
                    counts: f.counts(),
                    binary: matches!(f.body, Body::Binary(_)),
                    reviewed: f.reviewed,
                    dir: false,
                    drafts: drafts.get(f.change.path.as_str()).copied().unwrap_or(0),
                }
            }
        })
        .collect();
    // Names matter more than counts: a row shows its right-aligned counts
    // only when its whole name fits beside them.
    let fits = |item: &ListItem, row: &SidebarRow<'_>| {
        let counts = match row.counts {
            Some((a, r)) => {
                let (added, removed) = count_text(a, r);
                cells(&added) + cells(&removed)
            }
            None if row.binary => 3,
            None => 0,
        };
        let name = cells(row.name) + if row.dir && row.reviewed { 2 } else { 0 };
        list.x + 3 + 2 * item.depth() + name + 1 + counts <= end
    };
    for (slot, (item, row)) in visible.iter().zip(&rows).enumerate() {
        let y = list.y + u16::try_from(slot).unwrap_or(0);
        let index = start + slot;
        let bg = if index == sidebar.cursor() && focused {
            p.selection
        } else if Some(index) == current_row {
            p.header_bg
        } else {
            p.panel
        };
        fill(
            buf,
            Rect::new(list.x, y, list.width, 1),
            Style::new().bg(bg),
        );
        let depth = item.depth();
        let guides = item.guides();
        let guide = Style::new().bg(bg).fg(p.faint);
        for k in 0..depth {
            let symbol = if k + 1 == depth {
                if guides.last { "╰" } else { "├" }
            } else if guides.rail(k) {
                "│"
            } else {
                continue;
            };
            put(buf, list.x + 1 + 2 * k, y, symbol, 1, guide);
        }
        let show_counts = fits(item, row);
        let x = list.x + 1 + 2 * depth;
        put(buf, x, y, row.mark.0, 1, Style::new().bg(bg).fg(row.mark.1));
        let name_x = x + 2;
        let counts_x = match row.counts.filter(|_| show_counts) {
            Some(c) => put_counts(buf, end, y, c, bg, p),
            None if row.binary && show_counts => {
                let x = end.saturating_sub(3);
                put(buf, x, y, "bin", 3, Style::new().bg(bg).fg(p.faint));
                x
            }
            None => end,
        };
        let room = usize::from(counts_x.saturating_sub(name_x + 1));
        let fg = match (row.reviewed, row.dir) {
            (true, _) => p.faint,
            (false, true) => p.muted,
            (false, false) => p.fg,
        };
        let badge = (row.drafts > 0).then(|| format!(" {} ", row.drafts));
        let badge_w = badge.as_deref().map_or(0, |b| usize::from(cells(b)) + 1);
        let badge = badge.filter(|_| room >= badge_w + 4);
        let room = if badge.is_some() {
            room - badge_w
        } else {
            room
        };
        let check = row.dir && row.reviewed && room >= 4;
        let room = if check { room - 2 } else { room };
        let name = if row.dir {
            truncate_chain(row.name, room)
        } else {
            truncate_middle(row.name, room)
        };
        let w = put(
            buf,
            name_x,
            y,
            &name,
            counts_x.saturating_sub(name_x),
            Style::new().bg(bg).fg(fg),
        );
        if check {
            put(
                buf,
                name_x + w + 1,
                y,
                REVIEWED_MARK,
                1,
                Style::new().bg(bg).fg(p.green_dim),
            );
        }
        if let Some(badge) = badge {
            put(
                buf,
                name_x + w + 1,
                y,
                &badge,
                cells(&badge),
                Style::new()
                    .bg(p.accent)
                    .fg(p.bg)
                    .add_modifier(Modifier::BOLD),
            );
        }
    }
}

/// One sidebar row as drawn.
struct SidebarRow<'a> {
    mark: (&'static str, Color),
    name: &'a str,
    counts: Option<(u32, u32)>,
    binary: bool,
    reviewed: bool,
    dir: bool,
    /// Draft comments on the file.
    drafts: usize,
}
