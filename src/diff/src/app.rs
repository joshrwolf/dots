//! Viewer state: the worktrees, the change stream of the selected one, its
//! layout at the current width, and the scroll position. Pure state; the
//! interactive runtime and `render` both drive it.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use ratatui::layout::Rect;

use crate::diff::{FileDiff, Text};
use crate::highlight::{self, Key, Lines};
use crate::plan::{self, Kind, Layout, Row};
use crate::repo::{Changes, FileChange, Mode, Repo};
use crate::theme::Palette;
use crate::wrap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayoutChoice {
    /// Split when the diff pane is at least [`SPLIT_MIN_WIDTH`] wide.
    Auto,
    Unified,
    Split,
}

pub const SPLIT_MIN_WIDTH: u16 = 160;
/// Unchanged lines shown around each change.
pub const CONTEXT: u32 = 3;
/// The most rows one file plans before the rest is cut.
pub const ROW_CAP: usize = 40_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Worktrees,
    Files,
    Diff,
}

#[derive(Debug, Clone)]
pub struct Settings {
    pub layout: LayoutChoice,
    pub wrap: bool,
    pub mode: Mode,
    pub show_rail: bool,
    pub show_files: bool,
    /// Header height in rows; images get two.
    pub header_rows: u16,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            layout: LayoutChoice::Auto,
            wrap: true,
            mode: Mode::Base,
            show_rail: true,
            show_files: true,
            header_rows: 1,
        }
    }
}

/// A file's diff as far as it has been computed.
#[derive(Debug, Clone)]
pub enum Body {
    Pending,
    Binary,
    Text(Arc<FileDiff>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct LayoutKey {
    width: u16,
    layout: Layout,
    wrap: bool,
}

#[derive(Debug)]
pub struct FileView {
    pub change: FileChange,
    pub body: Body,
    pub expanded: HashSet<u32>,
    pub reviewed: bool,
    rows: Vec<Row>,
    /// Display offsets of `rows`; one longer than `rows`.
    offsets: Vec<u32>,
    key: Option<LayoutKey>,
    planned: Option<(Layout, usize)>,
}

impl FileView {
    pub fn new(change: FileChange) -> Self {
        Self {
            change,
            body: Body::Pending,
            expanded: HashSet::new(),
            reviewed: false,
            rows: Vec::new(),
            offsets: vec![0],
            key: None,
            planned: None,
        }
    }

    pub fn counts(&self) -> Option<(u32, u32)> {
        match &self.body {
            Body::Text(d) => Some((d.added, d.removed)),
            _ => None,
        }
    }

    pub fn rows(&self) -> &[Row] {
        &self.rows
    }

    pub fn row_offsets(&self) -> &[u32] {
        &self.offsets
    }

    /// Lines the body takes: rows, or one line for a note.
    fn body_lines(&self) -> u32 {
        match &self.body {
            Body::Text(_) if !self.rows.is_empty() => self.offsets.last().copied().unwrap_or(0),
            _ => 1,
        }
    }

    fn invalidate(&mut self) {
        self.key = None;
        self.planned = None;
    }

    /// Line-number column width for this file.
    pub fn number_width(&self) -> u16 {
        let lines = match &self.body {
            Body::Text(d) => d.old.len().max(d.new.len()),
            _ => 0,
        };
        u16::try_from(lines.max(1).ilog10() + 1).unwrap_or(9).max(3)
    }
}

/// Code columns for a file at a pane width: (unified code width, split side code width).
pub fn code_widths(pane: u16, number_width: u16) -> (u16, u16) {
    let unified = pane.saturating_sub(number_width * 2 + 4);
    let half = pane.saturating_sub(1) / 2;
    let split = half.saturating_sub(number_width + 3);
    (unified.max(4), split.max(4))
}

fn row_height(row: &Row, diff: &FileDiff, widths: (u16, u16), wrap: bool) -> u32 {
    if !wrap {
        return 1;
    }
    match *row {
        Row::Line { kind, old, new } => {
            let text = match (kind, old, new) {
                (Kind::Insert, _, Some(n)) | (Kind::Context, _, Some(n)) => diff.new.line(n),
                (_, Some(o), _) => diff.old.line(o),
                _ => "",
            };
            wrap::wrapped_height(text, widths.0)
        }
        Row::Pair { old, new, .. } => {
            let h = |text: &Text, i: Option<u32>| i.map_or(1, |i| wrap::wrapped_height(text.line(i), widths.1));
            h(&diff.old, old).max(h(&diff.new, new))
        }
        Row::Gap { .. } | Row::Truncated { .. } => 1,
    }
}

/// The scroll position: a display line within one file's block, so the
/// view stays put while other files load or change size.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Anchor {
    pub file: usize,
    pub line: u32,
}

/// The selected worktree's files as one scrolling stream.
#[derive(Debug)]
pub struct Stream {
    pub worktree: PathBuf,
    pub base: String,
    pub mode: Mode,
    pub files: Vec<FileView>,
    /// Start line of each file block; one longer than `files`.
    pub offsets: Vec<u32>,
}

impl Stream {
    pub fn total(&self) -> u32 {
        self.offsets.last().copied().unwrap_or(0)
    }

    pub fn counts(&self) -> (u32, u32, bool) {
        let mut done = true;
        let (mut a, mut r) = (0, 0);
        for f in &self.files {
            match f.counts() {
                Some((fa, fr)) => {
                    a += fa;
                    r += fr;
                }
                None => done &= matches!(f.body, Body::Binary),
            }
        }
        (a, r, done)
    }
}

/// Lines added and removed and files changed in a worktree, for the rail.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Summary {
    pub added: u32,
    pub removed: u32,
    pub files: usize,
}

/// What a display line of the diff pane shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Line {
    Header { file: usize, part: u16 },
    Body { file: usize, row: usize, sub: u32 },
    Note { file: usize },
    Spacer,
}

/// Hit areas from the last draw, for the mouse.
#[derive(Debug, Clone, Copy, Default)]
pub struct Areas {
    pub rail: Rect,
    pub files: Rect,
    pub diff: Rect,
}

/// One entry of the file list: a directory heading or a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ListItem {
    Dir(String),
    File(usize),
}

#[derive(Debug)]
pub struct App {
    pub repo: Repo,
    pub palette: Palette,
    pub settings: Settings,
    /// The worktree the stream shows (or is loading).
    pub current: usize,
    pub stream: Option<Stream>,
    pub top: Anchor,
    pub focus: Focus,
    pub rail_cursor: usize,
    pub files_cursor: usize,
    pub summaries: HashMap<PathBuf, Summary>,
    pub highlights: HashMap<Key, Option<Arc<Lines>>>,
    pub areas: Areas,
    pub help: bool,
    pub message: Option<String>,
    pub list: Vec<ListItem>,
    /// Width the stream was laid out for.
    laid_out: Option<(u16, Layout, bool)>,
}

impl App {
    pub fn new(repo: Repo, current: usize, palette: Palette, settings: Settings) -> Self {
        Self {
            repo,
            palette,
            settings,
            current,
            stream: None,
            top: Anchor::default(),
            focus: Focus::Diff,
            rail_cursor: current,
            files_cursor: 0,
            summaries: HashMap::new(),
            highlights: HashMap::new(),
            areas: Areas::default(),
            help: false,
            message: None,
            list: Vec::new(),
            laid_out: None,
        }
    }

    pub fn worktree_path(&self) -> &PathBuf {
        &self.repo.worktrees[self.current].path
    }

    /// Shows a freshly loaded change set, keeping diffs of unchanged files
    /// and the scroll position when it is the same worktree.
    pub fn load(&mut self, worktree: PathBuf, mode: Mode, changes: Changes) {
        let mut previous: HashMap<String, FileView> = HashMap::new();
        let mut anchor_path = None;
        if let Some(old) = self.stream.take()
            && old.worktree == worktree
            && old.mode == mode
        {
            anchor_path = old.files.get(self.top.file).map(|f| f.change.path.clone());
            previous = old.files.into_iter().map(|f| (f.change.path.clone(), f)).collect();
        } else {
            self.top = Anchor::default();
            self.files_cursor = 0;
        }
        let files: Vec<FileView> = changes
            .files
            .into_iter()
            .map(|change| match previous.remove(&change.path) {
                Some(mut kept) if kept.change.old == change.old && kept.change.new == change.new => {
                    kept.change = change;
                    kept
                }
                Some(stale) => {
                    let mut f = FileView::new(change);
                    f.reviewed = false;
                    f.expanded = stale.expanded;
                    f
                }
                None => FileView::new(change),
            })
            .collect();
        if let Some(path) = anchor_path {
            match files.iter().position(|f| f.change.path == path) {
                Some(i) => self.top.file = i,
                None => self.top = Anchor { file: self.top.file.min(files.len().saturating_sub(1)), line: 0 },
            }
        }
        self.list = list_items(&files);
        self.stream = Some(Stream {
            worktree,
            base: changes.base,
            mode,
            offsets: vec![0; files.len() + 1],
            files,
        });
        self.laid_out = None;
    }

    /// Indexes of files that still need a diff.
    pub fn pending(&self) -> Vec<usize> {
        self.stream.as_ref().map_or_else(Vec::new, |s| {
            s.files
                .iter()
                .enumerate()
                .filter(|(_, f)| matches!(f.body, Body::Pending))
                .map(|(i, _)| i)
                .collect()
        })
    }

    pub fn set_body(&mut self, file: usize, body: Body) {
        let Some(stream) = &mut self.stream else { return };
        let Some(f) = stream.files.get_mut(file) else { return };
        f.body = body;
        f.invalidate();
        self.laid_out = None;
    }

    /// Plans rows and measures them for a diff pane `width` wide. Cheap when
    /// nothing changed.
    pub fn layout(&mut self, width: u16) {
        let layout = self.effective_layout(width);
        let wrap = self.settings.wrap;
        if self.laid_out == Some((width, layout, wrap)) {
            return;
        }
        let header = u32::from(self.settings.header_rows);
        let Some(stream) = &mut self.stream else { return };
        // Keep the row under the top of the view across the relayout.
        let anchor_row = stream.files.get(self.top.file).and_then(|f| {
            let line = self.top.line.checked_sub(header)?;
            (!f.rows.is_empty()).then(|| plan::locate(&f.offsets, line).0)
        });
        let key = LayoutKey { width, layout, wrap };
        for f in &mut stream.files {
            if f.key == Some(key) {
                continue;
            }
            let Body::Text(diff) = &f.body else {
                f.rows.clear();
                f.offsets = vec![0];
                f.key = Some(key);
                continue;
            };
            let expanded_size = f.expanded.len();
            if f.planned != Some((layout, expanded_size)) {
                f.rows = plan::plan(
                    &diff.hunks,
                    diff.old.len(),
                    diff.new.len(),
                    &f.expanded,
                    plan::Options {
                        layout,
                        context: CONTEXT,
                        cap: ROW_CAP,
                    },
                );
                f.planned = Some((layout, expanded_size));
            }
            let widths = code_widths(width, f.number_width());
            f.offsets = plan::offsets(f.rows.iter().map(|r| row_height(r, diff, widths, wrap)));
            f.key = Some(key);
        }
        let mut at = 0;
        for (i, f) in stream.files.iter().enumerate() {
            stream.offsets[i] = at;
            at += header + f.body_lines() + 1;
        }
        stream.offsets[stream.files.len()] = at;
        if let (Some(row), Some(f)) = (anchor_row, stream.files.get(self.top.file)) {
            self.top.line = header + f.offsets.get(row).copied().unwrap_or(0);
        }
        self.laid_out = Some((width, layout, wrap));
    }

    pub fn effective_layout(&self, width: u16) -> Layout {
        match self.settings.layout {
            LayoutChoice::Unified => Layout::Unified,
            LayoutChoice::Split => Layout::Split,
            LayoutChoice::Auto if width >= SPLIT_MIN_WIDTH => Layout::Split,
            LayoutChoice::Auto => Layout::Unified,
        }
    }

    pub fn global_top(&self) -> u32 {
        self.stream
            .as_ref()
            .and_then(|s| s.offsets.get(self.top.file))
            .map_or(0, |o| o + self.top.line)
    }

    /// Scrolls so `line` of the stream is at the top, clamped so the view
    /// stays filled.
    pub fn scroll_to(&mut self, line: u32, view_rows: u16) {
        let Some(stream) = &self.stream else { return };
        let max = stream.total().saturating_sub(u32::from(view_rows));
        let line = line.min(max);
        let file = stream
            .offsets
            .partition_point(|&o| o <= line)
            .saturating_sub(1)
            .min(stream.files.len().saturating_sub(1));
        self.top = Anchor {
            file,
            line: line - stream.offsets[file],
        };
    }

    pub fn scroll_by(&mut self, delta: i64, view_rows: u16) {
        let target = i64::from(self.global_top()) + delta;
        self.scroll_to(u32::try_from(target.max(0)).unwrap_or(u32::MAX), view_rows);
    }

    /// What display line `line` of the stream shows.
    pub fn line_at(&self, line: u32) -> Line {
        let Some(stream) = &self.stream else { return Line::Spacer };
        if line >= stream.total() {
            return Line::Spacer;
        }
        let file = stream.offsets.partition_point(|&o| o <= line) - 1;
        let f = &stream.files[file];
        let local = line - stream.offsets[file];
        let header = u32::from(self.settings.header_rows);
        if local < header {
            return Line::Header {
                file,
                part: u16::try_from(local).unwrap_or(0),
            };
        }
        let body = local - header;
        if body >= f.body_lines() {
            return Line::Spacer;
        }
        if f.rows.is_empty() {
            return Line::Note { file };
        }
        let (row, sub) = plan::locate(&f.offsets, body);
        Line::Body { file, row, sub }
    }

    /// Jumps to the start of file `i`.
    pub fn jump_to_file(&mut self, i: usize, view_rows: u16) {
        let Some(stream) = &self.stream else { return };
        if let Some(&start) = stream.offsets.get(i) {
            self.scroll_to(start, view_rows);
            self.top = Anchor { file: i, line: 0 };
        }
        self.sync_files_cursor();
    }

    pub fn next_file(&mut self, forward: bool, view_rows: u16) {
        let Some(stream) = &self.stream else { return };
        let n = stream.files.len();
        if n == 0 {
            return;
        }
        let target = if forward {
            (self.top.file + 1).min(n - 1)
        } else if self.top.line > 0 {
            self.top.file
        } else {
            self.top.file.saturating_sub(1)
        };
        self.jump_to_file(target, view_rows);
    }

    /// Moves to the next or previous start of a change block.
    pub fn next_hunk(&mut self, forward: bool, view_rows: u16) {
        let Some(stream) = &self.stream else { return };
        let header = u32::from(self.settings.header_rows);
        let top = self.global_top();
        let mut starts = Vec::new();
        for (fi, f) in stream.files.iter().enumerate() {
            let mut in_change = false;
            for (ri, row) in f.rows.iter().enumerate() {
                let change = matches!(
                    row,
                    Row::Line { kind: Kind::Delete | Kind::Insert, .. } | Row::Pair { changed: true, .. }
                );
                if change && !in_change {
                    // Leave a little context above the change.
                    let line = stream.offsets[fi] + header + f.offsets[ri];
                    starts.push(line.saturating_sub(2).max(stream.offsets[fi]));
                }
                in_change = change;
            }
        }
        let target = if forward {
            starts.into_iter().find(|&s| s > top)
        } else {
            starts.into_iter().rev().find(|&s| s < top)
        };
        if let Some(line) = target {
            self.scroll_to(line, view_rows);
            self.sync_files_cursor();
        }
    }

    /// Expands the first folded region at or below the top of the view.
    pub fn expand_visible_gap(&mut self, view_rows: u16) -> bool {
        let top = self.global_top();
        for line in top..top + u32::from(view_rows) {
            if let Line::Body { file, row, .. } = self.line_at(line)
                && self.expand_at(file, row)
            {
                return true;
            }
        }
        false
    }

    /// Expands the gap at `row` of `file`, if it is one.
    pub fn expand_at(&mut self, file: usize, row: usize) -> bool {
        let Some(stream) = &mut self.stream else { return false };
        let f = &mut stream.files[file];
        let Some(Row::Gap { id, .. }) = f.rows.get(row).copied() else {
            return false;
        };
        f.expanded.insert(id);
        f.invalidate();
        self.laid_out = None;
        true
    }

    /// Expands or folds every region of the file at the top.
    pub fn toggle_expand_all(&mut self) {
        let Some(stream) = &mut self.stream else { return };
        let Some(f) = stream.files.get_mut(self.top.file) else { return };
        let Body::Text(diff) = &f.body else { return };
        if f.expanded.is_empty() {
            f.expanded = (0..=u32::try_from(diff.hunks.len()).unwrap_or(0)).collect();
        } else {
            f.expanded.clear();
        }
        f.invalidate();
        self.laid_out = None;
    }

    pub fn toggle_reviewed(&mut self) {
        if let Some(f) = self.stream.as_mut().and_then(|s| s.files.get_mut(self.top.file)) {
            f.reviewed = !f.reviewed;
        }
    }

    pub fn relayout(&mut self) {
        self.laid_out = None;
        if let Some(stream) = &mut self.stream {
            for f in &mut stream.files {
                f.invalidate();
            }
        }
    }

    pub fn sync_files_cursor(&mut self) {
        self.files_cursor = self.top.file;
    }

    /// Highlight jobs, most wanted first: files on screen, then neighbours.
    pub fn wanted_highlights(&self, view_rows: u16, prefetch: usize) -> Vec<highlight::Job> {
        let Some(stream) = &self.stream else { return Vec::new() };
        let top = self.global_top();
        let first = self.top.file;
        let bottom = top + u32::from(view_rows);
        let last = stream.offsets.partition_point(|&o| o < bottom).saturating_sub(1);
        let mut order: Vec<usize> = (first..=last.min(stream.files.len().saturating_sub(1))).collect();
        for d in 1..=prefetch {
            order.push(last + d);
            if let Some(i) = first.checked_sub(d) {
                order.push(i);
            }
        }
        let mut jobs = Vec::new();
        for i in order {
            let Some(f) = stream.files.get(i) else { continue };
            let Body::Text(diff) = &f.body else { continue };
            let Some(lang) = highlight::language(&f.change.path) else { continue };
            for text in [&diff.new, &diff.old] {
                if text.len() == 0 || text.src.len() > highlight::MAX_BYTES || text.len() > highlight::MAX_LINES {
                    continue;
                }
                let key = (text.hash, lang);
                if !self.highlights.contains_key(&key) {
                    jobs.push(highlight::Job {
                        key,
                        text: Arc::clone(text),
                    });
                }
            }
        }
        jobs
    }
}

/// Computes a file's diff, or marks it binary.
pub fn compute_body(change: &FileChange) -> Body {
    let old = change.old.as_deref().unwrap_or(&[]);
    let new = change.new.as_deref().unwrap_or(&[]);
    let binary = |b: &[u8]| b[..b.len().min(8000)].contains(&0);
    if binary(old) || binary(new) {
        return Body::Binary;
    }
    let text = |b: &[u8]| Text::new(Arc::from(String::from_utf8_lossy(b).as_ref()));
    Body::Text(Arc::new(FileDiff::compute(text(old), text(new))))
}

/// The file list grouped under directory headings, in path order.
pub fn list_items(files: &[FileView]) -> Vec<ListItem> {
    let mut items = Vec::new();
    let mut dir: Option<&str> = None;
    for (i, f) in files.iter().enumerate() {
        let parent = f.change.path.rsplit_once('/').map_or("", |(d, _)| d);
        if dir != Some(parent) {
            if !parent.is_empty() {
                items.push(ListItem::Dir(parent.to_owned()));
            }
            dir = Some(parent);
        }
        items.push(ListItem::File(i));
    }
    items
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repo::Status;
    use crate::theme::Detected;

    fn change(path: &str, old: &str, new: &str) -> FileChange {
        FileChange {
            path: path.to_owned(),
            status: Status::Modified,
            old: Some(Arc::from(old.as_bytes())),
            new: Some(Arc::from(new.as_bytes())),
        }
    }

    fn app_with(files: &[(&str, &str, &str)]) -> App {
        let repo = Repo {
            name: "r".into(),
            worktrees: vec![crate::repo::Worktree {
                path: "/r".into(),
                branch: "main".into(),
                main: true,
            }],
        };
        let mut app = App::new(repo, 0, Palette::new(Detected::light()), Settings::default());
        let changes = Changes {
            base: "main".into(),
            files: files.iter().map(|(p, o, n)| change(p, o, n)).collect(),
        };
        app.load("/r".into(), Mode::Base, changes);
        for i in 0..files.len() {
            let (_, o, n) = files[i];
            let diff = FileDiff::compute(Text::new(Arc::from(o)), Text::new(Arc::from(n)));
            app.set_body(i, Body::Text(Arc::new(diff)));
        }
        app
    }

    #[test]
    fn stream_stacks_header_rows_and_spacer_per_file() {
        let mut app = app_with(&[("a.rs", "x\n", "y\n"), ("b/c.rs", "", "1\n2\n")]);
        app.layout(80);
        let s = app.stream.as_ref().expect("loaded");
        // a: header + 2 rows + spacer; b: header + 2 rows + spacer.
        assert_eq!(s.offsets, vec![0, 4, 8]);
        assert_eq!(app.line_at(0), Line::Header { file: 0, part: 0 });
        assert_eq!(app.line_at(2), Line::Body { file: 0, row: 1, sub: 0 });
        assert_eq!(app.line_at(3), Line::Spacer);
        assert_eq!(app.line_at(4), Line::Header { file: 1, part: 0 });
        assert_eq!(app.list, vec![ListItem::File(0), ListItem::Dir("b".into()), ListItem::File(1)]);
    }

    #[test]
    fn wrapping_grows_rows_at_narrow_widths() {
        let long = format!("{}\n", "word ".repeat(30));
        let mut app = app_with(&[("a.txt", "", &long)]);
        app.layout(200);
        let wide = app.stream.as_ref().expect("loaded").total();
        app.layout(40);
        let narrow = app.stream.as_ref().expect("loaded").total();
        assert!(narrow > wide, "{narrow} > {wide}");
        app.settings.wrap = false;
        app.layout(40);
        assert_eq!(app.stream.as_ref().expect("loaded").total(), wide);
    }

    #[test]
    fn scroll_position_survives_relayout_and_reload() {
        let body: String = (0..50).map(|i| format!("line {i}\n")).collect();
        let mut app = app_with(&[("a.rs", "", &body), ("b.rs", "", &body)]);
        app.layout(80);
        app.jump_to_file(1, 10);
        app.scroll_by(5, 10);
        let before = app.top;
        app.layout(30);
        assert_eq!(app.top.file, before.file);
        let path = PathBuf::from("/r");
        let changes = Changes {
            base: "main".into(),
            files: vec![change("0.rs", "", "new\n"), change("a.rs", "", &body), change("b.rs", "", &body)],
        };
        app.load(path, Mode::Base, changes);
        assert_eq!(app.top.file, 2, "anchored to b.rs by path");
    }

    #[test]
    fn auto_layout_splits_wide_panes() {
        let app = app_with(&[]);
        assert_eq!(app.effective_layout(120), Layout::Unified);
        assert_eq!(app.effective_layout(SPLIT_MIN_WIDTH), Layout::Split);
    }
}
