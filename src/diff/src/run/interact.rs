//! What the user's keys and mouse do.

use ratatui::layout::Position;
use ratatui::termina::Event;
use ratatui::termina::escape::csi::{self, Csi};
use ratatui::termina::event::{
    KeyCode, KeyEvent, KeyEventKind, Modifiers, MouseButton, MouseEvent, MouseEventKind,
};

use crate::app::{Focus, LayoutChoice, Line, ListItem, Mode, Scope};
use crate::keys::{self, Action};
use crate::picker::Batch;
use crate::repo::{self};
use crate::switcher::Switcher;

use super::Runtime;

pub(super) fn step(at: usize, delta: i64, len: usize) -> usize {
    if len == 0 {
        return 0;
    }
    let max = i64::try_from(len - 1).unwrap_or(i64::MAX);
    let next = (i64::try_from(at).unwrap_or(0) + delta).clamp(0, max);
    usize::try_from(next).unwrap_or(0)
}

impl Runtime {
    pub(super) fn input(&mut self, event: &Event) -> bool {
        match event {
            Event::Key(key) if key.kind != KeyEventKind::Release => {
                self.key(*key);
                true
            }
            Event::Mouse(mouse) => self.mouse(*mouse),
            Event::Paste(text) => {
                if let Mode::Editor(editor) = &mut self.app.mode {
                    editor.buffer.insert(text);
                }
                true
            }
            Event::Csi(Csi::Mode(csi::Mode::ReportTheme(_))) => {
                // Light and dark switched; ask for the new colours.
                if self.theme.next_poll.is_some() {
                    self.timings.note("the terminal reported a theme change");
                    self.theme.ask = true;
                }
                false
            }
            Event::WindowResized(_) | Event::FocusIn => true,
            _ => false,
        }
    }

    pub(super) fn key(&mut self, key: KeyEvent) {
        // Terminals reporting keys unambiguously send Shift+c rather than C.
        let key = match key.code {
            KeyCode::Char(c)
                if key.modifiers.contains(Modifiers::SHIFT) && c.is_ascii_lowercase() =>
            {
                KeyEvent {
                    code: KeyCode::Char(c.to_ascii_uppercase()),
                    ..key
                }
            }
            _ => key,
        };
        let ctrl = key.modifiers.contains(Modifiers::CONTROL);
        self.app.message = None;
        match std::mem::take(&mut self.app.mode) {
            Mode::Editor(editor) => self.editor_key(editor, key),
            // Ctrl-C quits from anywhere else, overlays included.
            mode if ctrl && key.code == KeyCode::Char('c') => {
                self.app.mode = mode;
                self.quit = true;
            }
            // Any key closes help.
            Mode::Help => {}
            Mode::Switcher(switcher) => self.switcher_key(switcher, key, ctrl),
            Mode::Picker(picker) => self.picker_key(picker, key, ctrl),
            Mode::Prefix => match key.code {
                KeyCode::Char('c') => self.app.begin_comment(Scope::Review),
                _ => self.app.message = Some("; then c comments on the whole review".to_owned()),
            },
            mode @ (Mode::Normal | Mode::Visual(_)) => {
                self.app.mode = mode;
                let Some(action) = keys::action(key.code, ctrl) else {
                    return;
                };
                if action.needs_diff() && self.app.focus != Focus::Diff {
                    self.app.message = Some("press tab to focus the diff first".to_owned());
                    return;
                }
                self.act(action);
            }
        }
    }

    /// Does what a key is bound to.
    #[expect(clippy::too_many_lines, reason = "one match arm per message or action")]
    pub(super) fn act(&mut self, action: Action) {
        let rows = self.view_rows();
        let half = i64::from(rows / 2);
        let page = i64::from(rows.saturating_sub(2));
        let diff = self.app.focus == Focus::Diff;
        match action {
            Action::Quit => self.quit = true,
            Action::Help => self.app.mode = Mode::Help,
            Action::Down => self.move_by(1),
            Action::Up => self.move_by(-1),
            Action::HalfDown => self.scroll(half),
            Action::HalfUp => self.scroll(-half),
            Action::Space if !diff && self.app.sidebar.toggle_dir(self.app.sidebar.cursor()) => {}
            Action::Space | Action::PageDown => self.scroll(page),
            Action::PageUp => self.scroll(-page),
            Action::Top if diff => self.app.move_cursor(i64::MIN / 2, rows),
            Action::Bottom if diff => self.app.move_cursor(i64::MAX / 2, rows),
            Action::Top => {
                self.app.scroll_to(0, rows);
                self.app.sync_files_cursor();
            }
            Action::Bottom => {
                self.app.scroll_to(u32::MAX, rows);
                self.app.sync_files_cursor();
            }
            Action::NextFile | Action::PrevFile => {
                self.app.next_file(action == Action::NextFile, rows);
                if diff {
                    self.app.cursor_row = 0;
                    self.app.clamp_cursor(rows);
                }
            }
            Action::NextChange | Action::PrevChange => {
                self.app.next_hunk(action == Action::NextChange, rows);
                if diff {
                    self.cursor_to_change(rows);
                }
            }
            Action::Comment => self.app.begin_comment(Scope::Lines),
            Action::Suggest => self.app.begin_comment(Scope::Suggestion),
            Action::Visual => self.app.toggle_visual(),
            Action::Cancel => self.app.end_visual(),
            Action::FileComment => self.app.begin_comment(Scope::File),
            Action::Prefix => self.app.mode = Mode::Prefix,
            Action::Enter if !diff => {
                let row = self.app.sidebar.cursor();
                if !self.app.sidebar.toggle_dir(row) {
                    self.app.select_row(row, rows);
                    self.app.focus = Focus::Diff;
                }
            }
            Action::Enter | Action::Expand => {
                if !diff {
                    self.app.expand_visible_gap(rows);
                } else if let Some(card) = self.app.cursor_card() {
                    self.app.edit_card(card);
                } else if let Line::Header { file, .. } = self.app.line_at(self.app.cursor_line()) {
                    self.app.toggle_fold(file);
                } else if let Line::Body { file, row, .. } =
                    self.app.line_at(self.app.cursor_line())
                    && self.app.expand_at(file, row)
                {
                } else {
                    self.app.expand_visible_gap(rows);
                }
            }
            Action::ExpandAll => self.app.toggle_expand_all(),
            Action::Fold => {
                let file = if diff { self.app.cursor_file() } else { None };
                self.app.toggle_fold(file.unwrap_or(self.app.top.file));
            }
            Action::FoldAll => self.app.toggle_fold_all(),
            Action::Delete => match self.app.cursor_card() {
                Some(card) => self.app.delete_card(card),
                None => self.app.message = Some("move onto a card to delete it".to_owned()),
            },
            Action::NextCard => self.app.next_card(true, rows),
            Action::PrevCard => self.app.next_card(false, rows),
            Action::OldSide => self.app.cursor_old = !self.app.cursor_old,
            Action::Send => self.send(Batch::All),
            Action::PickAgent => self.open_picker(None),
            Action::Moves => {
                self.app.show_moves = !self.app.show_moves;
                self.app.message = Some(
                    if self.app.show_moves {
                        "showing moved code"
                    } else {
                        "moved code hidden"
                    }
                    .to_owned(),
                );
                self.refresh_moves();
            }
            Action::Since => {
                self.app.settings.since = !self.app.settings.since;
                self.reload();
            }
            Action::Reviewed => self.app.toggle_reviewed(),
            Action::Switcher => {
                self.app.mode = Mode::Switcher(Switcher::new(&self.app));
            }
            Action::NextWorktree => {
                self.switch((self.app.current + 1) % self.app.repo.worktrees.len());
            }
            Action::PrevWorktree => {
                let n = self.app.repo.worktrees.len();
                self.switch((self.app.current + n - 1) % n);
            }
            Action::NextReview | Action::PrevReview => {
                match self.app.next_to_review(action == Action::NextReview) {
                    Some(i) => self.switch(i),
                    None => self.app.message = Some("nothing waits for review".to_owned()),
                }
            }
            Action::Focus => {
                self.app.focus = if diff && self.app.areas.files.width > 0 {
                    Focus::Files
                } else {
                    Focus::Diff
                };
            }
            Action::Left if !diff => self.tree_left(),
            Action::Left => {
                if self.app.areas.files.width > 0 {
                    self.app.focus = Focus::Files;
                }
            }
            Action::Right if !diff => self.tree_right(),
            Action::Right => {}
            Action::Layout => {
                self.app.settings.layout = match self.app.settings.layout {
                    LayoutChoice::Auto => LayoutChoice::Unified,
                    LayoutChoice::Unified => LayoutChoice::Split,
                    LayoutChoice::Split => LayoutChoice::Auto,
                };
            }
            Action::Wrap => {
                self.app.settings.wrap = !self.app.settings.wrap;
                self.app.relayout();
            }
            Action::Compare => {
                self.app.settings.mode = match self.app.settings.mode {
                    repo::Mode::Base => repo::Mode::Head,
                    repo::Mode::Head => repo::Mode::Base,
                };
                self.reload();
            }
            Action::Sidebar => {
                self.app.settings.sidebar = !self.app.settings.sidebar;
                if !self.app.settings.sidebar {
                    self.app.focus = Focus::Diff;
                }
            }
            Action::Narrower | Action::Wider => {
                let now = self.app.areas.files.width;
                let width = if action == Action::Narrower {
                    now.saturating_sub(4)
                } else {
                    now + 4
                };
                self.app.settings.sidebar = true;
                self.app.settings.sidebar_width = Some(width.clamp(16, 64));
            }
            Action::Refresh => self.start_load(),
        }
    }

    /// Keys while the worktree switcher is open: typing filters it.
    pub(super) fn switcher_key(&mut self, mut sw: Switcher, key: KeyEvent, ctrl: bool) {
        let app = &self.app;
        match (key.code, ctrl) {
            (KeyCode::Escape, _) | (KeyCode::Char('c' | 'g'), true) => return,
            (KeyCode::Enter, _) => {
                if let Some(i) = sw.selected() {
                    self.switch(i);
                }
                return;
            }
            (KeyCode::Down, _) | (KeyCode::Char('n' | 'j'), true) => sw.move_by(1),
            (KeyCode::Up, _) | (KeyCode::Char('p' | 'k'), true) => sw.move_by(-1),
            (KeyCode::Backspace, _) => sw.pop(app),
            (KeyCode::Char('u'), true) => {
                while !sw.query.is_empty() {
                    sw.pop(app);
                }
            }
            (KeyCode::Char(c), false) => sw.push(app, c),
            _ => {}
        }
        self.app.mode = Mode::Switcher(sw);
    }

    /// Scrolls the view; the cursor keeps its place on screen.
    pub(super) fn scroll(&mut self, delta: i64) {
        let rows = self.view_rows();
        self.app.scroll_by(delta, rows);
        self.app.clamp_cursor(rows);
        self.app.sync_files_cursor();
    }

    pub(super) fn move_by(&mut self, delta: i64) {
        match self.app.focus {
            Focus::Diff => self.app.move_cursor(delta, self.view_rows()),
            Focus::Files => {
                let sidebar = &self.app.sidebar;
                let row = step(sidebar.cursor(), delta, sidebar.rows().len());
                self.app.select_row(row, self.view_rows());
            }
        }
    }

    pub(super) fn mouse(&mut self, mouse: MouseEvent) -> bool {
        if self.app.mode.modal() {
            return false;
        }
        let at = Position::new(mouse.column, mouse.row);
        let areas = self.app.areas;
        let rows = self.view_rows();
        if let MouseEventKind::Drag(MouseButton::Left) = mouse.kind {
            // Dragging in the diff selects lines, from where the press was.
            if self.app.focus != Focus::Diff || !areas.diff.contains(at) {
                return false;
            }
            if let (Mode::Normal, Some(at)) = (&self.app.mode, self.app.cursor_pos()) {
                self.app.mode = Mode::Visual(at);
            }
            self.app.cursor_row = mouse.row - areas.diff.y;
            self.app.clamp_cursor(rows);
            return true;
        }
        let delta = match mouse.kind {
            MouseEventKind::ScrollDown => 3,
            MouseEventKind::ScrollUp => -3,
            MouseEventKind::Down(MouseButton::Left) => 0,
            _ => return false,
        };
        if areas.diff.contains(at) {
            if delta != 0 {
                self.scroll(delta);
            } else {
                self.app.focus = Focus::Diff;
                self.app.end_visual();
                let y = mouse.row - areas.diff.y;
                let pinned = self.app.pinned_header().filter(|_| y == 0);
                if let Some(file) = pinned {
                    self.app.toggle_fold(file);
                    return true;
                }
                if let Line::Header { file, .. } =
                    self.app.line_at(self.app.global_top() + u32::from(y))
                {
                    self.app.toggle_fold(file);
                    return true;
                }
                self.app.cursor_row = mouse.row - areas.diff.y;
                self.app.clamp_cursor(rows);
                if let Line::Body { file, row, .. } = self.app.line_at(self.app.cursor_line()) {
                    self.app.expand_at(file, row);
                }
            }
        } else if areas.files.contains(at) {
            if delta != 0 {
                self.app.focus = Focus::Files;
                self.move_by(delta.signum());
            } else if let Some(row) = self.row_at(mouse.row) {
                self.app.focus = Focus::Files;
                if !self.app.sidebar.toggle_dir(row) {
                    self.app.select_row(row, rows);
                }
            }
        }
        true
    }

    /// The sidebar row at screen row `y`, below the sidebar's branch line.
    pub(super) fn row_at(&self, y: u16) -> Option<usize> {
        let area = self.app.areas.files;
        let rows = usize::from(area.height.saturating_sub(1));
        let start = self.app.sidebar.list_start(rows);
        let slot = usize::from(y.checked_sub(area.y + 1)?);
        let row = start + slot;
        (row < self.app.sidebar.rows().len()).then_some(row)
    }

    /// Collapses the selected directory, or moves to the one holding the
    /// selected row.
    pub(super) fn tree_left(&mut self) {
        let sidebar = &mut self.app.sidebar;
        let row = sidebar.cursor();
        match sidebar.selected() {
            Some(ListItem::Dir { path, .. }) if !sidebar.is_collapsed(path) => {
                let path = path.clone();
                sidebar.set_collapsed(&path, true);
            }
            Some(_) => {
                if let Some(parent) = sidebar.parent_row(row) {
                    self.app.select_row(parent, self.view_rows());
                }
            }
            None => {}
        }
    }

    /// Expands the selected directory or steps into it; on a file, moves
    /// focus to the diff.
    pub(super) fn tree_right(&mut self) {
        let sidebar = &mut self.app.sidebar;
        let row = sidebar.cursor();
        match sidebar.selected() {
            Some(ListItem::Dir { path, .. }) if sidebar.is_collapsed(path) => {
                let path = path.clone();
                sidebar.set_collapsed(&path, false);
            }
            Some(ListItem::Dir { .. }) => self.app.select_row(row + 1, self.view_rows()),
            _ => self.app.focus = Focus::Diff,
        }
    }
}
