//! The worktree switcher: a filter line over a table of worktrees, each
//! with its branch, how its lead worker stands, who works there, its counts
//! and when it last changed. Counts fill in as they are computed.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};

use super::{cells, fill, put, worker_color};
use crate::agents::Status;
use crate::app::{App, Standing};
use crate::switcher::Switcher;
use crate::theme::Palette;

/// Cells between columns.
const GAP: u16 = 2;
/// The widest the branch column grows.
const BRANCH_MAX: u16 = 40;
/// The narrowest the branch column shrinks to make room.
const BRANCH_MIN: u16 = 12;
/// The narrowest the popup is, for the filter line.
const MIN_WIDTH: u16 = 60;

/// One worktree's cells. Empty strings leave a cell blank.
struct Row {
    state: Option<(String, Color)>,
    /// The lead worker's label and the interactive sessions there.
    who: String,
    model: String,
    workers: String,
    added: String,
    removed: String,
    files: (String, Color),
    age: String,
}

impl Row {
    fn new(app: &App, worktree: usize, now: std::time::SystemTime) -> Self {
        let p = &app.palette;
        let owners = app.owners(worktree);
        let lead = owners.and_then(crate::agents::Owners::lead);
        let mut who: Vec<String> = lead.map(|w| w.label.clone()).into_iter().collect();
        if let Some(o) = owners {
            who.extend(o.sessions.iter().map(|a| format!("pi {}", a.label())));
        }
        let others = owners.map_or(0, |o| o.workers.len().saturating_sub(1));
        let workers = match others {
            0 => String::new(),
            1 => "+1 worker".to_owned(),
            n => format!("+{n} workers"),
        };
        let summary = app.summaries.get(&app.repo.worktrees[worktree].path);
        let count = |sign: char, n: u32| {
            if n == 0 {
                String::new()
            } else {
                format!("{sign}{}", compact(n))
            }
        };
        let (added, removed, files) = match summary {
            None => (String::new(), String::new(), (String::new(), p.faint)),
            Some(s) if s.files == 0 => (String::new(), String::new(), ("clean".into(), p.faint)),
            Some(s) => (
                count('+', s.added),
                count('−', s.removed),
                (
                    if s.files == 1 {
                        "1 file".into()
                    } else {
                        format!("{} files", s.files)
                    },
                    p.muted,
                ),
            ),
        };
        Self {
            state: lead.map(|w| (w.status.label().to_owned(), worker_color(&w.status, p))),
            who: who.join(", "),
            model: lead
                .and_then(crate::agents::Worker::model_name)
                .unwrap_or_default()
                .to_owned(),
            workers,
            added,
            removed,
            files,
            age: app
                .changed_at(worktree)
                .map(|t| crate::switcher::age(t, now))
                .unwrap_or_default(),
        }
    }
}

/// A count in at most six cells: `9999`, `146.9k`, `12.3M`.
fn compact(n: u32) -> String {
    let n64 = f64::from(n);
    match n {
        0..10_000 => n.to_string(),
        10_000..999_950 => format!("{:.1}k", n64 / 1e3),
        _ => format!("{:.1}M", n64 / 1e6),
    }
}

/// Column widths, each the widest of its cells; zero drops a column.
struct Columns {
    branch: u16,
    state: u16,
    who: u16,
    model: u16,
    workers: u16,
    added: u16,
    removed: u16,
    files: u16,
    age: u16,
}

impl Columns {
    /// Widths for `rows`, fitting `room` cells by narrowing who works
    /// there first, then the branch.
    fn fit(app: &App, rows: &[(usize, Row)], room: u16) -> Self {
        let widest =
            |f: &dyn Fn(&Row) -> &str| rows.iter().map(|(_, r)| cells(f(r))).max().unwrap_or(0);
        let mut c = Self {
            branch: rows
                .iter()
                .map(|(i, _)| cells(&app.repo.worktrees[*i].branch))
                .max()
                .unwrap_or(0)
                .min(BRANCH_MAX),
            state: widest(&|r| r.state.as_ref().map_or("", |s| s.0.as_str())),
            who: widest(&|r| &r.who),
            model: widest(&|r| &r.model),
            workers: widest(&|r| &r.workers),
            added: widest(&|r| &r.added),
            removed: widest(&|r| &r.removed),
            files: widest(&|r| &r.files.0),
            age: widest(&|r| &r.age),
        };
        let over = c.width().saturating_sub(room);
        let who = c.who.saturating_sub(over);
        let over = over - (c.who - who);
        c.who = who;
        c.branch = c.branch.saturating_sub(over).max(BRANCH_MIN.min(c.branch));
        c
    }

    /// The table's width, from the badge to the age.
    fn width(&self) -> u16 {
        let cols = [
            self.state,
            self.who,
            self.model,
            self.workers,
            self.added,
            self.removed,
            self.files,
            self.age,
        ];
        2 + self.branch
            + cols
                .iter()
                .filter(|&&w| w > 0)
                .map(|w| w + GAP)
                .sum::<u16>()
    }
}

#[expect(
    clippy::many_single_char_names,
    clippy::too_many_lines,
    reason = "one region drawn top to bottom, in cell coordinates"
)]
pub(super) fn draw_switcher(buf: &mut Buffer, app: &App, sw: &Switcher, area: Rect) {
    let p = &app.palette;
    let now = std::time::SystemTime::now();
    // Widths come from every worktree, so filtering keeps the table still.
    let all: Vec<(usize, Row)> = (0..app.repo.worktrees.len())
        .map(|i| (i, Row::new(app, i, now)))
        .collect();
    let max_w = area.width.saturating_sub(4);
    let cols = Columns::fit(app, &all, max_w.saturating_sub(4));
    // The popup fits the table; the flexible column takes any extra.
    let w = (cols.width() + 4).max(MIN_WIDTH).min(max_w);
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
    super::overlay::border(buf, outer, Style::new().bg(bg).fg(p.rule));
    let r = Rect::new(outer.x + 1, outer.y + 1, w - 2, h);
    let x = r.x + 1;
    let iw = r.width.saturating_sub(2);
    let cols = Columns {
        who: cols.who + iw.saturating_sub(cols.width()),
        ..cols
    };
    draw_filter(buf, app, sw, r, n);
    let rows = usize::from(h.saturating_sub(2));
    let start = sw.cursor.saturating_sub(rows.saturating_sub(1));
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
        let row = &all[m.worktree].1;
        let row_bg = if i == sw.cursor { p.selection } else { bg };
        let style = Style::new().bg(row_bg);
        fill(buf, Rect::new(r.x, y, r.width, 1), style);
        if m.worktree == app.current {
            put(buf, r.x, y, "▌", 1, style.fg(p.accent));
        }
        if app.standing(m.worktree) == Standing::Asking {
            let fg = worker_color(&Status::Asking, p);
            put(buf, x, y, "?", 1, style.fg(fg).add_modifier(Modifier::BOLD));
        } else if let Some(marker) = app.marker(m.worktree) {
            let badge = match marker.status {
                Status::Asking => "?",
                Status::Done => "✓",
                Status::Stopped => "■",
                _ => "✗",
            };
            let fg = worker_color(&marker.status, p);
            put(
                buf,
                x,
                y,
                badge,
                1,
                style.fg(fg).add_modifier(Modifier::BOLD),
            );
        } else if app.unseen(m.worktree) {
            put(buf, x, y, "●", 1, style.fg(p.accent));
        }
        // The branch, with the filter's matches marked while it fits.
        let branch = super::truncate_middle(&wt.branch, usize::from(cols.branch));
        let whole = branch.chars().count() == wt.branch.chars().count();
        let plain = style.fg(p.fg);
        let hit = plain.fg(p.accent).add_modifier(Modifier::BOLD);
        let mut cx = x + 2;
        let branch_end = cx + cols.branch;
        for (ci, ch) in branch.chars().enumerate() {
            let hit_here = whole && m.positions.contains(&u32::try_from(ci).unwrap_or(u32::MAX));
            cx += put(
                buf,
                cx,
                y,
                ch.encode_utf8(&mut [0; 4]),
                branch_end.saturating_sub(cx),
                if hit_here { hit } else { plain },
            );
        }
        // Left-aligned text columns, then right-aligned numbers.
        let muted = style.fg(p.muted);
        let (state, state_fg) = row
            .state
            .as_ref()
            .map_or(("", p.faint), |(s, fg)| (s.as_str(), *fg));
        let who = truncate_end(&row.who, usize::from(cols.who));
        let cells_left = [
            (state, cols.state, style.fg(state_fg), false),
            (who.as_str(), cols.who, muted, false),
            (row.model.as_str(), cols.model, style.fg(p.faint), false),
            (row.workers.as_str(), cols.workers, style.fg(p.faint), false),
            (row.added.as_str(), cols.added, style.fg(p.green_dim), true),
            (
                row.removed.as_str(),
                cols.removed,
                style.fg(p.red_dim),
                true,
            ),
            (
                row.files.0.as_str(),
                cols.files,
                style.fg(row.files.1),
                true,
            ),
            (row.age.as_str(), cols.age, style.fg(p.faint), true),
        ];
        let mut cx = branch_end;
        for (text, width, style, right) in cells_left {
            if width == 0 {
                continue;
            }
            cx += GAP;
            let tx = if right {
                cx + width.saturating_sub(cells(text))
            } else {
                cx
            };
            put(buf, tx, y, text, (cx + width).saturating_sub(tx), style);
            cx += width;
        }
    }
}

/// The filter line and the count of matches, over a rule.
fn draw_filter(buf: &mut Buffer, app: &App, sw: &Switcher, r: Rect, n: u16) {
    let p: &Palette = &app.palette;
    let bg = p.panel;
    let x = r.x + 1;
    let iw = r.width.saturating_sub(2);
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
    let count_of = |want: Standing| {
        (0..app.repo.worktrees.len())
            .filter(|&i| app.standing(i) == want)
            .count()
    };
    let mut parts: Vec<String> = [
        (count_of(Standing::Asking), "asking"),
        (count_of(Standing::Waiting), "to review"),
    ]
    .into_iter()
    .filter(|&(k, _)| k > 0)
    .map(|(k, what)| format!("{k} {what}"))
    .collect();
    parts.push(format!("{}/{}", sw.matches.len(), n));
    let count = parts.join(" · ");
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
}

/// `text` in at most `max` cells, ending in `…` when cut.
fn truncate_end(text: &str, max: usize) -> String {
    if super::width(text) <= max {
        return text.to_owned();
    }
    let mut out = String::new();
    let mut used = 0;
    for ch in text.chars() {
        let w = super::width(ch.encode_utf8(&mut [0; 4]));
        if used + w + 1 > max {
            break;
        }
        used += w;
        out.push(ch);
    }
    if max > 0 {
        out.push('…');
    }
    out
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::{Duration, SystemTime};

    use super::*;
    use crate::agents::{Snapshot, Worker};
    use crate::app::{Settings, Summary};
    use crate::repo::{Repo, Worktree};
    use crate::theme::{Detected, Palette};

    #[test]
    fn counts_stay_narrow() {
        assert_eq!(compact(9_999), "9999");
        assert_eq!(compact(146_936), "146.9k");
        assert_eq!(compact(999_949), "999.9k");
        assert_eq!(compact(999_950), "1.0M");
        assert_eq!(compact(89_051_093), "89.1M");
    }

    #[test]
    fn labels_are_cut_at_the_end() {
        assert_eq!(truncate_end("pi-attention-integration", 12), "pi-attentio…");
        assert_eq!(truncate_end("short", 12), "short");
        assert_eq!(truncate_end("abc", 0), "");
    }

    /// The text of row `y` of `buf`.
    fn line(buf: &Buffer, y: u16) -> String {
        (0..buf.area.width).map(|x| buf[(x, y)].symbol()).collect()
    }

    /// The display column of the end of the first `needle` on `line`.
    fn end_of(line: &str, needle: &str) -> usize {
        let at = line
            .find(needle)
            .unwrap_or_else(|| panic!("{needle:?} in {line:?}"));
        line[..at + needle.len()].chars().count()
    }

    /// The display column of the start of the first `needle` on `line`.
    fn start_of(line: &str, needle: &str) -> usize {
        let at = line
            .find(needle)
            .unwrap_or_else(|| panic!("{needle:?} in {line:?}"));
        line[..at].chars().count()
    }

    #[test]
    fn columns_line_up_across_rows() {
        let then = SystemTime::now() - Duration::from_hours(72);
        let wt = |b: &str| Worktree {
            path: PathBuf::from(format!("/w/{b}")),
            branch: b.into(),
            main: false,
            touched: Some(then),
        };
        let repo = Repo {
            worktrees: ["big-import", "fix", "quiet-docs"].map(wt).to_vec(),
        };
        let mut app = App::new(
            repo,
            1,
            Palette::new(Detected::light()),
            Settings::default(),
        );
        let summary = |added, removed, files| Summary {
            added,
            removed,
            files,
            content: None,
            changed: Some(then),
            looked: None,
        };
        app.summaries
            .insert("/w/big-import".into(), summary(146_936, 89_051, 1093));
        app.summaries.insert("/w/fix".into(), summary(12, 3, 2));
        app.summaries
            .insert("/w/quiet-docs".into(), summary(7, 0, 1));
        let worker = |id: &str, label: &str, cwd: &str, updated_at| Worker {
            id: id.into(),
            label: label.into(),
            status: Status::Done,
            cwd: cwd.into(),
            model: Some("anthropic/claude-opus-5-5".into()),
            owner_session: None,
            session_file: None,
            updated_at,
        };
        app.set_agents(Snapshot {
            agents: Vec::new(),
            workers: vec![
                worker("w-1", "importer-with-a-long-label", "/w/big-import", 3),
                worker("w-2", "helper", "/w/big-import", 2),
                worker("w-3", "fixer", "/w/fix", 1),
            ],
            problems: Vec::new(),
        });
        let sw = Switcher::new(&app);
        let area = Rect::new(0, 0, 120, 12);
        let mut buf = Buffer::empty(area);
        draw_switcher(&mut buf, &app, &sw, area);
        let rows: Vec<String> = (0..area.height)
            .map(|y| line(&buf, y))
            .filter(|l| l.contains("file"))
            .collect();
        let row = |b: &str| {
            rows.iter()
                .find(|l| l.contains(b))
                .unwrap_or_else(|| panic!("row {b} in {rows:#?}"))
        };
        let (big, fix, docs) = (row("big-import"), row(" fix "), row("quiet-docs"));
        // Numbers are right-aligned in their own columns.
        assert_eq!(end_of(big, "+146.9k"), end_of(fix, "+12"));
        assert_eq!(end_of(big, "+146.9k"), end_of(docs, "+7"));
        assert_eq!(end_of(big, "−89.1k"), end_of(fix, "−3"));
        assert_eq!(end_of(big, "1093 files"), end_of(fix, "2 files"));
        assert_eq!(end_of(big, "1093 files"), end_of(docs, "1 file"));
        assert_eq!(end_of(big, "3d"), end_of(docs, "3d"));
        assert!(
            start_of(big, "1093") > end_of(big, "−89.1k"),
            "removed and files apart: {big}"
        );
        // Text columns start together; a row without an owner leaves them blank.
        assert_eq!(start_of(big, "done"), start_of(fix, "done"));
        assert_eq!(
            start_of(big, "importer-with-a-long-label"),
            start_of(fix, "fixer")
        );
        assert_eq!(start_of(big, "opus-5-5"), start_of(fix, "opus-5-5"));
        assert!(big.contains("+1 worker"), "{big}");
        assert!(!docs.contains("done") && !docs.contains("opus"), "{docs}");
    }
}
