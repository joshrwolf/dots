//! Viewer state: the worktrees, the change stream of the selected one, its
//! layout at the current width, and the scroll position. Pure state; the
//! interactive runtime and `render` both drive it.

use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use ratatui::layout::Rect;

use crate::diff::{FileDiff, Text};
use crate::highlight::{self, Key, Lines};
use crate::plan::{self, Kind, Layout, Row};
use crate::repo::{FileChange, Mode, Repo};
use crate::theme::Palette;
use crate::wrap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LayoutChoice {
    /// Split when the diff pane is at least [`SPLIT_MIN_WIDTH`] wide.
    Auto,
    Unified,
    Split,
}

pub(crate) const SPLIT_MIN_WIDTH: u16 = 160;
/// Unchanged lines shown around each change.
pub(crate) const CONTEXT: u32 = 3;
/// The most rows one file plans before the rest is cut.
pub(crate) const ROW_CAP: usize = 40_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Focus {
    Files,
    Diff,
}

#[derive(Debug, Clone)]
pub(crate) struct Settings {
    pub(crate) layout: LayoutChoice,
    pub(crate) wrap: bool,
    pub(crate) mode: Mode,
    pub(crate) sidebar: bool,
    /// Sidebar columns chosen with the resize keys; `None` sizes it to the screen.
    pub(crate) sidebar_width: Option<u16>,
    /// Header height in rows; images get two.
    pub(crate) header_rows: u16,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            layout: LayoutChoice::Auto,
            wrap: true,
            mode: Mode::Base,
            sidebar: true,
            sidebar_width: None,
            header_rows: 1,
        }
    }
}

/// A file's diff as far as it has been computed.
#[derive(Debug, Clone)]
pub(crate) enum Body {
    Pending,
    Binary,
    Unreadable(String),
    Text(Arc<FileDiff>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct LayoutKey {
    width: u16,
    layout: Layout,
    wrap: bool,
    /// False while heights are estimated at one line per row.
    exact: bool,
}

#[derive(Debug)]
pub(crate) struct FileView {
    pub(crate) change: FileChange,
    pub(crate) body: Body,
    pub(crate) expanded: HashSet<u32>,
    pub(crate) reviewed: bool,
    rows: Vec<Row>,
    /// Display offsets of `rows`; one longer than `rows`.
    offsets: Vec<u32>,
    key: Option<LayoutKey>,
    planned: Option<(Layout, usize)>,
}

impl FileView {
    pub(crate) fn new(change: FileChange) -> Self {
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

    pub(crate) fn counts(&self) -> Option<(u32, u32)> {
        match &self.body {
            Body::Text(d) => Some((d.added, d.removed)),
            _ => None,
        }
    }

    pub(crate) fn rows(&self) -> &[Row] {
        &self.rows
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
    pub(crate) fn number_width(&self) -> u16 {
        let lines = match &self.body {
            Body::Text(d) => d.old.len().max(d.new.len()),
            _ => 0,
        };
        u16::try_from(lines.max(1).ilog10() + 1).unwrap_or(9).max(3)
    }
}

/// Code columns for a file at a pane width: (unified code width, split side code width).
pub(crate) fn code_widths(pane: u16, number_width: u16) -> (u16, u16) {
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
                (Kind::Insert | Kind::Context, _, Some(n)) => diff.new.line(n),
                (_, Some(o), _) => diff.old.line(o),
                _ => "",
            };
            wrap::wrapped_height(text, widths.0)
        }
        Row::Pair { old, new, .. } => {
            let h = |text: &Text, i: Option<u32>| {
                i.map_or(1, |i| wrap::wrapped_height(text.line(i), widths.1))
            };
            h(&diff.old, old).max(h(&diff.new, new))
        }
        Row::Gap { .. } | Row::Truncated { .. } => 1,
    }
}

/// The scroll position: a display line within one file's block, so the
/// view stays put while other files load or change size.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct Anchor {
    pub(crate) file: usize,
    pub(crate) line: u32,
}

/// The selected worktree's files as one scrolling stream.
#[derive(Debug)]
pub(crate) struct Stream {
    pub(crate) worktree: PathBuf,
    pub(crate) base: String,
    pub(crate) mode: Mode,
    pub(crate) files: Vec<FileView>,
    /// Start line of each file block; one longer than `files`.
    pub(crate) offsets: Vec<u32>,
}

impl Stream {
    pub(crate) fn total(&self) -> u32 {
        self.offsets.last().copied().unwrap_or(0)
    }

    pub(crate) fn counts(&self) -> (u32, u32, bool) {
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

/// Lines added and removed and files changed in a worktree, for the switcher.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Summary {
    pub(crate) added: u32,
    pub(crate) removed: u32,
    pub(crate) files: usize,
}

/// What a display line of the diff pane shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Line {
    Header { file: usize, part: u16 },
    Body { file: usize, row: usize, sub: u32 },
    Note { file: usize },
    Spacer,
}

/// Hit areas from the last draw, for the mouse.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Areas {
    pub(crate) files: Rect,
    pub(crate) diff: Rect,
}

/// How a tree row's guides are drawn: for each ancestor column, whether a
/// line continues past this row, and whether the row is its parent's last.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct Guides {
    /// Bit `k` set: the guide in column `k` continues below this row.
    pub(crate) rails: u64,
    pub(crate) last: bool,
}

impl Guides {
    /// Guides for the children of a row at `depth` drawn with `self`.
    fn child(self, depth: u16, last: bool) -> Self {
        let rails = if depth == 0 || last || depth > 64 {
            self.rails
        } else {
            self.rails | 1 << (depth - 1)
        };
        Self { rails, last: false }
    }

    /// Whether column `k` of a row continues a guide from above.
    pub(crate) fn rail(self, k: u16) -> bool {
        k < 64 && self.rails & 1 << k != 0
    }
}

/// One row of the file tree, `depth` levels in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ListItem {
    Dir {
        /// The row's text: one directory, or a collapsed chain of them.
        label: String,
        /// Full path with a trailing slash; the collapse key.
        path: String,
        depth: u16,
        /// Indexes of the files inside, which are contiguous.
        files: Range<usize>,
        guides: Guides,
    },
    File {
        index: usize,
        depth: u16,
        guides: Guides,
    },
}

impl ListItem {
    pub(crate) fn depth(&self) -> u16 {
        match self {
            ListItem::Dir { depth, .. } | ListItem::File { depth, .. } => *depth,
        }
    }

    pub(crate) fn guides(&self) -> Guides {
        match self {
            ListItem::Dir { guides, .. } | ListItem::File { guides, .. } => *guides,
        }
    }

    /// The first file the row stands for.
    fn first_file(&self) -> usize {
        match self {
            ListItem::Dir { files, .. } => files.start,
            ListItem::File { index, .. } => *index,
        }
    }
}

#[derive(Debug)]
pub(crate) struct App {
    pub(crate) repo: Repo,
    pub(crate) palette: Palette,
    pub(crate) settings: Settings,
    /// The worktree the stream shows (or is loading).
    pub(crate) current: usize,
    pub(crate) stream: Option<Stream>,
    pub(crate) top: Anchor,
    pub(crate) focus: Focus,
    pub(crate) switcher: Option<crate::switcher::Switcher>,
    /// The selected sidebar row, an index into `list`.
    pub(crate) cursor: usize,
    pub(crate) summaries: HashMap<PathBuf, Summary>,
    pub(crate) highlights: HashMap<Key, Option<Arc<Lines>>>,
    pub(crate) areas: Areas,
    pub(crate) help: bool,
    pub(crate) message: Option<String>,
    /// The whole file tree.
    tree: Vec<ListItem>,
    /// Its rows outside collapsed directories, as the sidebar shows them.
    pub(crate) list: Vec<ListItem>,
    /// Paths of collapsed directories.
    pub(crate) collapsed: HashSet<String>,
    /// Width the stream was laid out for.
    laid_out: Option<(u16, Layout, bool, (usize, usize))>,
}

impl App {
    pub(crate) fn new(repo: Repo, current: usize, palette: Palette, settings: Settings) -> Self {
        Self {
            repo,
            palette,
            settings,
            current,
            stream: None,
            top: Anchor::default(),
            focus: Focus::Diff,
            switcher: None,
            cursor: 0,
            summaries: HashMap::new(),
            highlights: HashMap::new(),
            areas: Areas::default(),
            help: false,
            message: None,
            tree: Vec::new(),
            list: Vec::new(),
            collapsed: HashSet::new(),
            laid_out: None,
        }
    }

    pub(crate) fn worktree_path(&self) -> &PathBuf {
        &self.repo.worktrees[self.current].path
    }

    /// Shows a freshly loaded change set, keeping diffs of unchanged files
    /// and the scroll position when it is the same worktree.
    pub(crate) fn load(
        &mut self,
        worktree: PathBuf,
        mode: Mode,
        base: String,
        files: Vec<FileChange>,
    ) {
        let mut previous: HashMap<String, FileView> = HashMap::new();
        let mut anchor_path = None;
        if let Some(old) = self.stream.take()
            && old.worktree == worktree
            && old.mode == mode
        {
            anchor_path = old.files.get(self.top.file).map(|f| f.change.path.clone());
            previous = old
                .files
                .into_iter()
                .map(|f| (f.change.path.clone(), f))
                .collect();
        } else {
            self.top = Anchor::default();
            self.cursor = 0;
            self.collapsed.clear();
        }
        let files: Vec<FileView> = files
            .into_iter()
            .map(|change| match previous.remove(&change.path) {
                Some(mut kept)
                    if kept.change.old == change.old && kept.change.new == change.new =>
                {
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
                None => {
                    self.top = Anchor {
                        file: self.top.file.min(files.len().saturating_sub(1)),
                        line: 0,
                    }
                }
            }
        }
        self.tree = list_items(&files);
        self.list = visible(&self.tree, &self.collapsed);
        self.stream = Some(Stream {
            worktree,
            base,
            mode,
            offsets: vec![0; files.len() + 1],
            files,
        });
        self.laid_out = None;
        self.sync_files_cursor();
    }

    /// Indexes of files that still need a diff.
    pub(crate) fn pending(&self) -> Vec<usize> {
        self.stream.as_ref().map_or_else(Vec::new, |s| {
            s.files
                .iter()
                .enumerate()
                .filter(|(_, f)| matches!(f.body, Body::Pending))
                .map(|(i, _)| i)
                .collect()
        })
    }

    pub(crate) fn set_body(&mut self, file: usize, body: Body) {
        let Some(stream) = &mut self.stream else {
            return;
        };
        let Some(f) = stream.files.get_mut(file) else {
            return;
        };
        f.body = body;
        f.invalidate();
        self.laid_out = None;
    }

    /// Plans rows and measures them for a diff pane `width` wide and `rows`
    /// tall. Wrapped heights are measured only for files near the view;
    /// the rest count one line per row until they come close, so huge
    /// streams lay out in time proportional to what is on screen.
    pub(crate) fn layout(&mut self, width: u16, rows: u16) {
        let layout = self.effective_layout(width);
        let wrap = self.settings.wrap;
        let header = u32::from(self.settings.header_rows);
        let top = self.global_top();
        let Some(stream) = &mut self.stream else {
            return;
        };
        let near = |line: u32| {
            stream
                .offsets
                .partition_point(|&o| o <= line)
                .saturating_sub(1)
        };
        let window = (
            near(top.saturating_sub(u32::from(rows))),
            near(top + 3 * u32::from(rows)),
        );
        if self.laid_out == Some((width, layout, wrap, window)) {
            return;
        }
        // Keep the row under the top of the view across the relayout.
        let anchor_row = stream.files.get(self.top.file).and_then(|f| {
            let line = self.top.line.checked_sub(header)?;
            (!f.rows.is_empty()).then(|| plan::locate(&f.offsets, line).0)
        });
        for (i, f) in stream.files.iter_mut().enumerate() {
            let exact = !wrap || (window.0..=window.1).contains(&i);
            let key = LayoutKey {
                width,
                layout,
                wrap,
                exact,
            };
            // An exact layout serves an estimate's place too.
            if f.key == Some(key) || f.key == Some(LayoutKey { exact: true, ..key }) {
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
            f.offsets = if exact {
                plan::offsets(f.rows.iter().map(|r| row_height(r, diff, widths, wrap)))
            } else {
                (0..=u32::try_from(f.rows.len()).unwrap_or(u32::MAX)).collect()
            };
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
        self.laid_out = Some((width, layout, wrap, window));
    }

    pub(crate) fn effective_layout(&self, width: u16) -> Layout {
        match self.settings.layout {
            LayoutChoice::Auto if width >= SPLIT_MIN_WIDTH => Layout::Split,
            LayoutChoice::Split => Layout::Split,
            LayoutChoice::Unified | LayoutChoice::Auto => Layout::Unified,
        }
    }

    pub(crate) fn global_top(&self) -> u32 {
        self.stream
            .as_ref()
            .and_then(|s| s.offsets.get(self.top.file))
            .map_or(0, |o| o + self.top.line)
    }

    /// Scrolls so `line` of the stream is at the top, clamped so the view
    /// stays filled.
    pub(crate) fn scroll_to(&mut self, line: u32, view_rows: u16) {
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

    pub(crate) fn scroll_by(&mut self, delta: i64, view_rows: u16) {
        let target = i64::from(self.global_top()) + delta;
        self.scroll_to(u32::try_from(target.max(0)).unwrap_or(u32::MAX), view_rows);
    }

    /// What display line `line` of the stream shows.
    pub(crate) fn line_at(&self, line: u32) -> Line {
        let Some(stream) = &self.stream else {
            return Line::Spacer;
        };
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
    pub(crate) fn jump_to_file(&mut self, i: usize, view_rows: u16) {
        let Some(stream) = &self.stream else { return };
        if let Some(&start) = stream.offsets.get(i) {
            self.scroll_to(start, view_rows);
            self.top = Anchor { file: i, line: 0 };
        }
        self.sync_files_cursor();
    }

    pub(crate) fn next_file(&mut self, forward: bool, view_rows: u16) {
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
    pub(crate) fn next_hunk(&mut self, forward: bool, view_rows: u16) {
        let Some(stream) = &self.stream else { return };
        let header = u32::from(self.settings.header_rows);
        let top = self.global_top();
        let mut starts = Vec::new();
        for (fi, f) in stream.files.iter().enumerate() {
            let mut in_change = false;
            for (ri, row) in f.rows.iter().enumerate() {
                let change = matches!(
                    row,
                    Row::Line {
                        kind: Kind::Delete | Kind::Insert,
                        ..
                    } | Row::Pair { changed: true, .. }
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
    pub(crate) fn expand_visible_gap(&mut self, view_rows: u16) -> bool {
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
    pub(crate) fn expand_at(&mut self, file: usize, row: usize) -> bool {
        let Some(stream) = &mut self.stream else {
            return false;
        };
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
    pub(crate) fn toggle_expand_all(&mut self) {
        let Some(stream) = &mut self.stream else {
            return;
        };
        let Some(f) = stream.files.get_mut(self.top.file) else {
            return;
        };
        let Body::Text(diff) = &f.body else { return };
        if f.expanded.is_empty() {
            f.expanded = (0..=u32::try_from(diff.hunks.len()).unwrap_or(0)).collect();
        } else {
            f.expanded.clear();
        }
        f.invalidate();
        self.laid_out = None;
    }

    pub(crate) fn toggle_reviewed(&mut self) {
        if let Some(f) = self
            .stream
            .as_mut()
            .and_then(|s| s.files.get_mut(self.top.file))
        {
            f.reviewed = !f.reviewed;
        }
    }

    pub(crate) fn relayout(&mut self) {
        self.laid_out = None;
        if let Some(stream) = &mut self.stream {
            for f in &mut stream.files {
                f.invalidate();
            }
        }
    }

    /// Moves the sidebar cursor to the file at the top of the view, or to
    /// the collapsed directory hiding it.
    pub(crate) fn sync_files_cursor(&mut self) {
        if let Some(row) = self.row_of_file(self.top.file) {
            self.cursor = row;
        }
    }

    /// The sidebar row showing file `index`: its own row or the collapsed
    /// directory that holds it.
    pub(crate) fn row_of_file(&self, index: usize) -> Option<usize> {
        self.list.iter().position(|it| match it {
            ListItem::File { index: i, .. } => *i == index,
            ListItem::Dir { path, files, .. } => {
                files.contains(&index) && self.collapsed.contains(path)
            }
        })
    }

    /// Selects sidebar row `row` and brings the file it stands for (a
    /// directory's first file) to the top of the stream.
    pub(crate) fn select_row(&mut self, row: usize, view_rows: u16) {
        let Some(item) = self.list.get(row) else {
            return;
        };
        let file = item.first_file();
        self.jump_to_file(file, view_rows);
        self.cursor = row;
    }

    /// Collapses or expands the directory at sidebar row `row`; false when
    /// the row is a file.
    pub(crate) fn toggle_dir(&mut self, row: usize) -> bool {
        let Some(ListItem::Dir { path, .. }) = self.list.get(row) else {
            return false;
        };
        let path = path.clone();
        self.set_collapsed(&path, !self.collapsed.contains(&path));
        true
    }

    /// Collapses or expands the directory at `path`, keeping its row selected.
    pub(crate) fn set_collapsed(&mut self, path: &str, collapse: bool) {
        if collapse {
            self.collapsed.insert(path.to_owned());
        } else {
            self.collapsed.remove(path);
        }
        self.list = visible(&self.tree, &self.collapsed);
        if let Some(row) = self
            .list
            .iter()
            .position(|it| matches!(it, ListItem::Dir { path: p, .. } if p == path))
        {
            self.cursor = row;
        }
    }

    /// The sidebar row of the directory holding row `row`, if any.
    pub(crate) fn parent_row(&self, row: usize) -> Option<usize> {
        let depth = self.list.get(row)?.depth();
        (0..row).rev().find(|&r| self.list[r].depth() < depth)
    }

    /// Lines added and removed in files `range`, and whether all of them
    /// are reviewed.
    pub(crate) fn dir_summary(&self, range: Range<usize>) -> (u32, u32, bool) {
        let Some(stream) = &self.stream else {
            return (0, 0, false);
        };
        let files = stream.files.get(range).unwrap_or_default();
        let (mut a, mut r) = (0, 0);
        for (fa, fr) in files.iter().filter_map(FileView::counts) {
            a += fa;
            r += fr;
        }
        let reviewed = !files.is_empty() && files.iter().all(|f| f.reviewed);
        (a, r, reviewed)
    }

    /// The first file-tree row shown in a sidebar list `rows` tall: the
    /// cursor stays near the middle.
    pub(crate) fn list_start(&self, rows: usize) -> usize {
        self.cursor
            .saturating_sub(rows / 2)
            .min(self.list.len().saturating_sub(rows))
    }

    /// Highlight jobs, most wanted first: files on screen, then neighbours.
    pub(crate) fn wanted_highlights(&self, view_rows: u16, prefetch: usize) -> Vec<highlight::Job> {
        let Some(stream) = &self.stream else {
            return Vec::new();
        };
        let top = self.global_top();
        let first = self.top.file;
        let bottom = top + u32::from(view_rows);
        let last = stream
            .offsets
            .partition_point(|&o| o < bottom)
            .saturating_sub(1);
        let mut order: Vec<usize> =
            (first..=last.min(stream.files.len().saturating_sub(1))).collect();
        for d in 1..=prefetch {
            order.push(last + d);
            if let Some(i) = first.checked_sub(d) {
                order.push(i);
            }
        }
        let mut jobs = Vec::new();
        for i in order {
            let Some(f) = stream.files.get(i) else {
                continue;
            };
            let Body::Text(diff) = &f.body else { continue };
            let Some(lang) = highlight::language(&f.change.path) else {
                continue;
            };
            for text in [&diff.new, &diff.old] {
                if text.len() == 0
                    || text.src.len() > highlight::MAX_BYTES
                    || text.len() > highlight::MAX_LINES
                {
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

/// Reads both sides of a file and diffs them, or marks it binary.
pub(crate) fn compute_body(repo: &gix::Repository, root: &Path, change: &FileChange) -> Body {
    let (old, new) = match crate::repo::contents(repo, root, change) {
        Ok(sides) => sides,
        Err(e) => return Body::Unreadable(format!("{e:#}")),
    };
    let (old, new) = (old.as_slice(), new.as_slice());
    let binary = |b: &[u8]| b[..b.len().min(8000)].contains(&0);
    if binary(old) || binary(new) {
        return Body::Binary;
    }
    let text = |b: &[u8]| Text::new(Arc::from(String::from_utf8_lossy(b).as_ref()));
    Body::Text(Arc::new(FileDiff::compute(text(old), text(new))))
}

/// The file list as a nested tree in path order. A directory chain with a
/// single child directory collapses into one row, like `src/diff/src/`.
pub(crate) fn list_items(files: &[FileView]) -> Vec<ListItem> {
    let paths: Vec<&str> = files.iter().map(|f| f.change.path.as_str()).collect();
    let mut items = Vec::new();
    tree(&paths, 0..paths.len(), 0, 0, Guides::default(), &mut items);
    items
}

/// Lists `paths[range]`, which share their first `prefix` bytes, as
/// children at `depth` below ancestors drawn with `guides`.
fn tree(
    paths: &[&str],
    range: Range<usize>,
    prefix: usize,
    depth: u16,
    guides: Guides,
    out: &mut Vec<ListItem>,
) {
    let mut i = range.start;
    while i < range.end {
        let rest = &paths[i][prefix..];
        let Some(slash) = rest.find('/') else {
            let last = i + 1 == range.end;
            out.push(ListItem::File {
                index: i,
                depth,
                guides: Guides { last, ..guides },
            });
            i += 1;
            continue;
        };
        // Paths in sorted order keep each directory's contents together.
        let head = &rest[..=slash];
        let end = (i..range.end)
            .find(|&j| !paths[j][prefix..].starts_with(head))
            .unwrap_or(range.end);
        let mut label_end = prefix + head.len();
        while let Some(slash) = paths[i][label_end..].find('/') {
            let next = &paths[i][label_end..=label_end + slash];
            if !(i..end).all(|j| paths[j][label_end..].starts_with(next)) {
                break;
            }
            label_end += next.len();
        }
        let last = end == range.end;
        out.push(ListItem::Dir {
            label: paths[i][prefix..label_end].to_owned(),
            path: paths[i][..label_end].to_owned(),
            depth,
            files: i..end,
            guides: Guides { last, ..guides },
        });
        tree(
            paths,
            i..end,
            label_end,
            depth + 1,
            guides.child(depth, last),
            out,
        );
        i = end;
    }
}

/// The rows of `tree` left showing when the directories in `collapsed`
/// hide their contents.
pub(crate) fn visible(tree: &[ListItem], collapsed: &HashSet<String>) -> Vec<ListItem> {
    let mut out = Vec::with_capacity(tree.len());
    let mut hide_below: Option<u16> = None;
    for item in tree {
        if let Some(d) = hide_below {
            if item.depth() > d {
                continue;
            }
            hide_below = None;
        }
        if let ListItem::Dir { path, depth, .. } = item
            && collapsed.contains(path)
        {
            hide_below = Some(*depth);
        }
        out.push(item.clone());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repo::Status;
    use crate::theme::Detected;

    fn change(path: &str, _old: &str, new: &str) -> FileChange {
        FileChange {
            path: path.to_owned(),
            status: Status::Modified,
            old: None,
            new: Some(crate::repo::Stamp {
                modified: None,
                len: new.len() as u64,
            }),
        }
    }

    fn app_with(files: &[(&str, &str, &str)]) -> App {
        let repo = Repo {
            worktrees: vec![crate::repo::Worktree {
                path: "/r".into(),
                branch: "main".into(),
                main: true,
                touched: None,
            }],
        };
        let mut app = App::new(
            repo,
            0,
            Palette::new(Detected::light()),
            Settings::default(),
        );
        let changes = files.iter().map(|(p, o, n)| change(p, o, n)).collect();
        app.load("/r".into(), Mode::Base, "main".into(), changes);
        for (i, &(_, o, n)) in files.iter().enumerate() {
            let diff = FileDiff::compute(Text::new(Arc::from(o)), Text::new(Arc::from(n)));
            app.set_body(i, Body::Text(Arc::new(diff)));
        }
        app
    }

    #[test]
    fn stream_stacks_header_rows_and_spacer_per_file() {
        let mut app = app_with(&[("a.rs", "x\n", "y\n"), ("b/c.rs", "", "1\n2\n")]);
        app.layout(80, 40);
        let s = app.stream.as_ref().expect("loaded");
        // a: header + 2 rows + spacer; b: header + 2 rows + spacer.
        assert_eq!(s.offsets, vec![0, 4, 8]);
        assert_eq!(app.line_at(0), Line::Header { file: 0, part: 0 });
        assert_eq!(
            app.line_at(2),
            Line::Body {
                file: 0,
                row: 1,
                sub: 0
            }
        );
        assert_eq!(app.line_at(3), Line::Spacer);
        assert_eq!(app.line_at(4), Line::Header { file: 1, part: 0 });
        let labels: Vec<_> = app
            .list
            .iter()
            .map(|it| match it {
                ListItem::Dir { label, .. } => label.clone(),
                ListItem::File { index, .. } => index.to_string(),
            })
            .collect();
        assert_eq!(labels, ["0", "b/", "1"]);
    }

    /// A tree row as text: guides, then `name/` or a file index.
    fn row(item: &ListItem) -> String {
        let g = item.guides();
        let mut s: String = (0..item.depth())
            .map(|k| match () {
                () if k + 1 == item.depth() => {
                    if g.last {
                        "╰ "
                    } else {
                        "├ "
                    }
                }
                () if g.rail(k) => "│ ",
                () => "  ",
            })
            .collect();
        match item {
            ListItem::Dir { label, .. } => s.push_str(label),
            ListItem::File { index, .. } => s.push_str(&index.to_string()),
        }
        s
    }

    const PATHS: [&str; 6] = [
        "docs/containers/a.md",
        "docs/containers/b.md",
        "src/x/deep/y.rs",
        "src/x/z.rs",
        "src/z.rs",
        "top.rs",
    ];

    #[test]
    fn file_tree_nests_with_guides_and_compacts_chains() {
        let mut items = Vec::new();
        tree(&PATHS, 0..PATHS.len(), 0, 0, Guides::default(), &mut items);
        let rows: Vec<String> = items.iter().map(row).collect();
        assert_eq!(
            rows,
            [
                "docs/containers/",
                "├ 0",
                "╰ 1",
                "src/",
                "├ x/",
                "│ ├ deep/",
                "│ │ ╰ 2",
                "│ ╰ 3",
                "╰ 4",
                "5",
            ]
        );
        let ListItem::Dir { path, files, .. } = &items[4] else {
            panic!("x/ is a directory");
        };
        assert_eq!((path.as_str(), files.clone()), ("src/x/", 2..4));
    }

    #[test]
    fn collapsed_directories_hide_their_contents() {
        let mut items = Vec::new();
        tree(&PATHS, 0..PATHS.len(), 0, 0, Guides::default(), &mut items);
        let collapsed: HashSet<String> = ["src/x/".to_owned()].into();
        let rows: Vec<String> = visible(&items, &collapsed).iter().map(row).collect();
        assert_eq!(
            rows,
            ["docs/containers/", "├ 0", "╰ 1", "src/", "├ x/", "╰ 4", "5"]
        );
    }

    #[test]
    fn sidebar_cursor_follows_files_into_collapsed_folders() {
        let files: Vec<_> = PATHS
            .iter()
            .map(|p| {
                (
                    *p, "a
", "b
",
                )
            })
            .collect();
        let mut app = app_with(&files);
        app.layout(80, 40);
        app.set_collapsed("src/x/", true);
        assert_eq!(app.cursor, 4, "the collapsed folder stays selected");
        app.jump_to_file(3, 40);
        assert_eq!(app.cursor, 4, "src/x/z.rs shows as its folder");
        app.select_row(5, 40);
        assert_eq!(app.top.file, 4);
        assert_eq!(app.parent_row(5), Some(3));
        assert!(app.toggle_dir(4));
        assert_eq!(app.list.len(), 10);
        let stream = app.stream.as_mut().expect("loaded");
        stream.files[2].reviewed = true;
        assert!(!app.dir_summary(2..4).2);
        app.stream.as_mut().expect("loaded").files[3].reviewed = true;
        assert!(app.dir_summary(2..4).2);
    }

    #[test]
    fn wrapping_grows_rows_at_narrow_widths() {
        let long = format!("{}\n", "word ".repeat(30));
        let mut app = app_with(&[("a.txt", "", &long)]);
        app.layout(200, 40);
        let wide = app.stream.as_ref().expect("loaded").total();
        app.layout(40, 40);
        let narrow = app.stream.as_ref().expect("loaded").total();
        assert!(narrow > wide, "{narrow} > {wide}");
        app.settings.wrap = false;
        app.layout(40, 40);
        assert_eq!(app.stream.as_ref().expect("loaded").total(), wide);
    }

    #[test]
    fn scroll_position_survives_relayout_and_reload() {
        let body: String = (0..50).fold(String::new(), |s, i| s + "line " + &i.to_string() + "\n");
        let mut app = app_with(&[("a.rs", "", &body), ("b.rs", "", &body)]);
        app.layout(80, 40);
        app.jump_to_file(1, 10);
        app.scroll_by(5, 10);
        let before = app.top;
        app.layout(30, 40);
        assert_eq!(app.top.file, before.file);
        let path = PathBuf::from("/r");
        let changes = vec![
            change("0.rs", "", "new\n"),
            change("a.rs", "", &body),
            change("b.rs", "", &body),
        ];
        app.load(path, Mode::Base, "main".into(), changes);
        assert_eq!(app.top.file, 2, "anchored to b.rs by path");
    }

    #[test]
    fn auto_layout_splits_wide_panes() {
        let app = app_with(&[]);
        assert_eq!(app.effective_layout(120), Layout::Unified);
        assert_eq!(app.effective_layout(SPLIT_MIN_WIDTH), Layout::Split);
    }
}
