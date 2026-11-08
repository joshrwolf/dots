//! Viewer state: the worktrees, the change stream of the selected one, its
//! layout at the current width, and the scroll position. Pure state; the
//! interactive runtime and `render` both drive it.

use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::path::PathBuf;
use std::sync::Arc;

use ratatui::layout::Rect;

use crate::cards::{self, Placed};
use crate::diff::{self, FileDiff, HunkEmphasis, Text};
use crate::editor::Editor;
use crate::highlight::{self, Key, Lines};
use crate::lru::Lru;
use crate::plan::{self, Heights, Kind, Layout, Row};
use crate::repo::{FileChange, Mode, Reader, Repo, Side, Target};
use crate::review;
use crate::theme::Palette;
use crate::wrap;

mod annotate;
pub(crate) use annotate::{Scope, row_line};

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
/// Bytes of file text kept for drawing; the rest is reread on demand.
const TEXT_BUDGET: usize = 64 * 1024 * 1024;
/// Bytes of syntax colour kept.
const HIGHLIGHT_BUDGET: usize = 32 * 1024 * 1024;
const EMPHASIS_BUDGET: usize = 4 * 1024 * 1024;

/// Both sides of a file's text, by content id: what a diff job hands over
/// besides the diff, and what drawing reads.
#[derive(Debug, Clone)]
pub(crate) struct FileTexts {
    pub(crate) old: (Option<gix::ObjectId>, Arc<Text>),
    pub(crate) new: (Option<gix::ObjectId>, Arc<Text>),
}

/// A hunk's word emphasis is keyed by the contents it was computed from.
type EmphasisKey = (Option<gix::ObjectId>, Option<gix::ObjectId>, u32);

/// How long changed hunks stay marked after a refresh.
pub(crate) const FLASH: std::time::Duration = std::time::Duration::from_millis(1200);

/// Where the view was in a file, to find again after the file changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SourceLine {
    /// A new-side line number and the display line within it.
    New(u32, u32),
    /// A display line within the file block.
    Display(u32),
}

/// Whether `kept`'s diff still describes `change`: the same old blob and
/// the same new content, even when it moved from the working tree into the
/// index.
fn same_contents(kept: &FileView, change: &FileChange) -> bool {
    if kept.change.old != change.old {
        return false;
    }
    if kept.change.new == change.new {
        return true;
    }
    match (&kept.body, change.new) {
        (Body::Text(d), Side::Blob(id)) => d.new_id == Some(id),
        _ => false,
    }
}

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
    /// A binary file, with its new side's content id.
    Binary(Option<gix::ObjectId>),
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
    /// Marked reviewed at its current content.
    pub(crate) reviewed: bool,
    /// Cards under rows, by row, for files laid out near the view.
    pub(crate) cards: Vec<Placed>,
    /// Cards under the header: file-level ones and those whose lines are
    /// not shown.
    pub(crate) file_cards: Vec<Placed>,
    /// Display lines of `file_cards`.
    pub(crate) file_card_lines: u32,
    /// Display lines above the header: the review-level cards, on the first
    /// file.
    pub(crate) above: u32,
    /// Planned rows, kept only for files near the view.
    rows: Vec<Row>,
    /// Display heights of the rows, measured near the view and estimated
    /// at one line per row elsewhere.
    heights: Heights,
    key: Option<LayoutKey>,
    planned: Option<(Layout, usize)>,
    /// The diff this file had before its contents changed, until the new
    /// one arrives.
    previous: Option<Arc<FileDiff>>,
    /// Hunks that changed in the last refresh, and when.
    pub(crate) flash: Vec<usize>,
    pub(crate) flash_at: Option<std::time::Instant>,
}

impl FileView {
    pub(crate) fn new(change: FileChange) -> Self {
        Self {
            change,
            body: Body::Pending,
            expanded: HashSet::new(),
            reviewed: false,
            cards: Vec::new(),
            file_cards: Vec::new(),
            file_card_lines: 0,
            above: 0,
            rows: Vec::new(),
            heights: Heights::default(),
            key: None,
            planned: None,
            previous: None,
            flash: Vec::new(),
            flash_at: None,
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

    /// The layout its rows were planned in.
    pub(crate) fn layout(&self) -> Layout {
        self.planned.map_or(Layout::Unified, |(l, _)| l)
    }

    /// The first body line of the file's block, after the cards above, the
    /// header and the file's cards.
    pub(crate) fn body_start(&self, header: u32) -> u32 {
        self.above + header + self.file_card_lines
    }

    /// The new side's content key, once known.
    pub(crate) fn content_key(&self) -> Option<String> {
        let id = match (&self.body, &self.change.new) {
            (Body::Text(d), _) => d.new_id,
            (Body::Binary(id), _) => *id,
            (_, Side::Blob(id)) => Some(*id),
            (_, Side::Missing) => None,
            (_, Side::Worktree(_)) => return None,
        };
        Some(review::content_key(id))
    }

    /// The planned rows: kept ones near the view, planned afresh elsewhere.
    fn planned_rows(&self) -> std::borrow::Cow<'_, [Row]> {
        let (Body::Text(diff), Some((layout, _))) = (&self.body, self.planned) else {
            return std::borrow::Cow::Borrowed(&[]);
        };
        if self.rows.len() == self.heights.rows() as usize {
            return std::borrow::Cow::Borrowed(&self.rows);
        }
        std::borrow::Cow::Owned(plan::plan(
            &diff.hunks,
            diff.old_len,
            diff.new_len,
            &self.expanded,
            plan::Options {
                layout,
                context: CONTEXT,
                cap: ROW_CAP,
            },
        ))
    }

    /// Lines the body takes: rows, or one line for a note.
    fn body_lines(&self) -> u32 {
        match &self.body {
            Body::Text(_) if self.heights.rows() > 0 => self.heights.total(),
            _ => 1,
        }
    }

    fn invalidate(&mut self) {
        self.key = None;
        self.planned = None;
    }

    /// Whether `row` lies in a hunk changed by the last
    /// refresh, still marked at `now`.
    pub(crate) fn flashed(&self, row: &Row, now: std::time::Instant) -> bool {
        let (Some(at), Body::Text(diff)) = (self.flash_at, &self.body) else {
            return false;
        };
        if now >= at + FLASH {
            return false;
        }
        let hunk = match *row {
            Row::Line {
                kind: Kind::Delete,
                old: Some(o),
                ..
            } => diff.hunk_of(false, o),
            Row::Line {
                kind: Kind::Insert,
                new: Some(n),
                ..
            } => diff.hunk_of(true, n),
            Row::Pair {
                changed: true,
                old,
                new,
            } => new
                .and_then(|n| diff.hunk_of(true, n))
                .or_else(|| old.and_then(|o| diff.hunk_of(false, o))),
            _ => None,
        };
        hunk.is_some_and(|h| self.flash.contains(&h))
    }

    /// Line-number column width for this file.
    pub(crate) fn number_width(&self) -> u16 {
        let lines = match &self.body {
            Body::Text(d) => d.old_len.max(d.new_len),
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

fn row_height(row: &Row, texts: &FileTexts, widths: (u16, u16)) -> u32 {
    let (old, new) = (&texts.old.1, &texts.new.1);
    match *row {
        Row::Line {
            kind,
            old: o,
            new: n,
        } => {
            let text = match (kind, o, n) {
                (Kind::Insert | Kind::Context, _, Some(n)) => new.line(n),
                (_, Some(o), _) => old.line(o),
                _ => "",
            };
            wrap::wrapped_height(text, widths.0)
        }
        Row::Pair { old: o, new: n, .. } => {
            let h = |text: &Text, i: Option<u32>| {
                i.map_or(1, |i| wrap::wrapped_height(text.line(i), widths.1))
            };
            h(old, o).max(h(new, n))
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
    pub(crate) target: Target,
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
                None => done &= matches!(f.body, Body::Binary(_)),
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
    Header {
        file: usize,
        part: u16,
    },
    /// Line `line` of a stack of cards: the review's above the first file,
    /// or a file's under its header.
    Cards {
        file: usize,
        review: bool,
        line: u32,
    },
    /// Line `sub` of a row; lines past its code are its cards.
    Body {
        file: usize,
        row: usize,
        sub: u32,
    },
    Note {
        file: usize,
    },
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
#[allow(clippy::struct_excessive_bools)]
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
    pub(crate) highlights: Lru<Key, Option<Arc<Lines>>>,
    /// File texts by content id, for files near the view.
    texts: Lru<gix::ObjectId, Arc<Text>>,
    emphasis: Lru<EmphasisKey, Option<Arc<HunkEmphasis>>>,
    /// Rereads texts the cache dropped.
    reader: Option<Reader>,
    empty: Arc<Text>,
    /// Set when a file's contents changed under its diff, so it needs a new one.
    pub(crate) needs_diffs: bool,
    /// Where to put the view once the top file's new diff is laid out.
    pending_anchor: Option<(String, SourceLine)>,
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
    /// The worktree's review, and the Git directory it is saved in; `None`
    /// when it could not be read, so it is not overwritten.
    pub(crate) review: review::State,
    pub(crate) review_dir: Option<PathBuf>,
    /// The review changed since it was last saved.
    pub(crate) review_dirty: bool,
    /// The worktree `review` belongs to.
    pub(crate) review_worktree: Option<PathBuf>,
    /// Agent notes for this viewer's lifetime.
    pub(crate) notes: Vec<Note>,
    pub(crate) next_note: u64,
    /// The diff cursor, as a screen row of the diff pane.
    pub(crate) cursor_row: u16,
    /// On a split row, comment on the old side.
    pub(crate) cursor_old: bool,
    /// Where a visual selection started.
    pub(crate) visual: Option<Pos>,
    pub(crate) editor: Option<Editor>,
    pub(crate) picker: Option<crate::picker::Picker>,
    /// Show only what changed since the last send.
    pub(crate) since: bool,
    /// Who a send goes to, as the status line shows it.
    pub(crate) target: String,
    /// The review-level cards, above the first file.
    pub(crate) review_cards: Vec<Placed>,
    /// A card to bring into view once its file is laid out.
    pending_card: Option<CardRef>,
}

/// A card in the stream: a comment, or an agent's note.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum CardRef {
    Comment(u64),
    Note(u64),
}

/// A note an agent placed for the user, gone when its file changes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Note {
    pub(crate) id: u64,
    pub(crate) worktree: PathBuf,
    pub(crate) place: review::Place,
    pub(crate) text: String,
    /// The content of its file when placed, once known.
    pub(crate) content: Option<String>,
    /// Shown on screen, and since scrolled out of view.
    pub(crate) shown: bool,
    pub(crate) seen: bool,
}

/// A source line under the cursor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Pos {
    pub(crate) file: usize,
    pub(crate) side: review::Side,
    pub(crate) line: u32,
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
            highlights: Lru::new(HIGHLIGHT_BUDGET),
            texts: Lru::new(TEXT_BUDGET),
            emphasis: Lru::new(EMPHASIS_BUDGET),
            reader: None,
            empty: Arc::new(Text::new(Arc::from(""))),
            needs_diffs: false,
            pending_anchor: None,
            areas: Areas::default(),
            help: false,
            message: None,
            tree: Vec::new(),
            list: Vec::new(),
            collapsed: HashSet::new(),
            laid_out: None,
            review: review::State::default(),
            review_dir: None,
            review_dirty: false,
            review_worktree: None,
            notes: Vec::new(),
            next_note: 1,
            cursor_row: 0,
            cursor_old: false,
            visual: None,
            editor: None,
            picker: None,
            since: false,
            target: String::new(),
            review_cards: Vec::new(),
            pending_card: None,
        }
    }

    pub(crate) fn worktree_path(&self) -> &PathBuf {
        &self.repo.worktrees[self.current].path
    }

    /// Shows a freshly loaded change set. When it is the same worktree,
    /// unchanged files keep their diffs and the view stays on the same
    /// source line.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn load(
        &mut self,
        worktree: PathBuf,
        mode: Mode,
        base: String,
        target: Target,
        files: Vec<FileChange>,
        reader: Option<Reader>,
    ) {
        self.reader = reader;
        let same = self
            .stream
            .as_ref()
            .is_some_and(|s| s.worktree == worktree && s.mode == mode);
        if !same {
            self.stream = None;
            self.top = Anchor::default();
            self.cursor = 0;
            self.collapsed.clear();
        }
        self.replace_files(files);
        if let Some(stream) = &mut self.stream {
            stream.worktree = worktree;
            stream.base = base;
            stream.mode = mode;
            stream.target = target;
        }
    }

    /// Replaces the changes of `paths` alone, from a path-limited scan:
    /// `files` holds those of them that still differ.
    pub(crate) fn apply_paths(&mut self, paths: &[String], files: Vec<FileChange>) {
        let Some(stream) = &self.stream else { return };
        let touched: HashSet<&str> = paths.iter().map(String::as_str).collect();
        let mut merged: Vec<FileChange> = stream
            .files
            .iter()
            .filter(|f| !touched.contains(f.change.path.as_str()))
            .map(|f| f.change.clone())
            .chain(files)
            .collect();
        merged.sort_unstable_by(|a, b| a.path.cmp(&b.path));
        self.replace_files(merged);
    }

    /// Swaps in a new file list for the current stream, keeping what still
    /// applies: diffs of files whose contents are unchanged, expanded
    /// regions, and the source line at the top of the view.
    fn replace_files(&mut self, files: Vec<FileChange>) {
        let mut previous: HashMap<String, FileView> = HashMap::new();
        let mut anchor = None;
        let old = self.stream.take();
        let (worktree, base, mode, target) = if let Some(old) = old {
            anchor = self.source_anchor(&old);
            let keys = (old.worktree, old.base, old.mode, old.target);
            previous = old
                .files
                .into_iter()
                .map(|f| (f.change.path.clone(), f))
                .collect();
            keys
        } else {
            // `load` fills these in right after.
            let null = gix::ObjectId::null(gix::hash::Kind::Sha1);
            let target = Target {
                base_tree: null,
                head_tree: null,
            };
            (PathBuf::new(), String::new(), Mode::Base, target)
        };
        let files: Vec<FileView> = files
            .into_iter()
            .map(|change| match previous.remove(&change.path) {
                Some(mut kept) if same_contents(&kept, &change) => {
                    kept.change = change;
                    kept
                }
                Some(stale) => {
                    let mut f = FileView::new(change);
                    f.expanded = stale.expanded;
                    if let Body::Text(d) = stale.body {
                        f.previous = Some(d);
                    }
                    f
                }
                None => FileView::new(change),
            })
            .collect();
        if let Some((path, _)) = &anchor {
            if let Some(i) = files.iter().position(|f| f.change.path == *path) {
                self.top.file = i;
            } else {
                self.top = Anchor {
                    file: self.top.file.min(files.len().saturating_sub(1)),
                    line: 0,
                };
                anchor = None;
            }
        }
        self.pending_anchor = anchor;
        self.tree = list_items(&files);
        self.list = visible(&self.tree, &self.collapsed);
        self.stream = Some(Stream {
            worktree,
            base,
            mode,
            target,
            offsets: vec![0; files.len() + 1],
            files,
        });
        self.laid_out = None;
        self.sync_files_cursor();
    }

    /// The file and source position at the top of the view: a new-side line
    /// number when the top is on a line, else the display line in the file.
    fn source_anchor(&self, stream: &Stream) -> Option<(String, SourceLine)> {
        let f = stream.files.get(self.top.file)?;
        let header = u32::from(self.settings.header_rows);
        let at = match self.top.line.checked_sub(f.body_start(header)) {
            Some(body) if f.heights.rows() > 0 => {
                let (row, sub) = f.heights.locate(body);
                match f.rows.get(row as usize) {
                    Some(Row::Line { new: Some(n), .. } | Row::Pair { new: Some(n), .. }) => {
                        SourceLine::New(*n, sub)
                    }
                    _ => SourceLine::Display(self.top.line),
                }
            }
            _ => SourceLine::Display(self.top.line),
        };
        Some((f.change.path.clone(), at))
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

    /// Sets a file's diff. Its texts are kept when the file is near the
    /// view, where they will be drawn soon.
    pub(crate) fn set_body(&mut self, file: usize, body: Body, texts: Option<FileTexts>) {
        let near = self.near_view(file);
        let Some(stream) = &mut self.stream else {
            return;
        };
        let Some(f) = stream.files.get_mut(file) else {
            return;
        };
        // Mark the hunks a refresh changed.
        if let (Some(before), Body::Text(after)) = (f.previous.take(), &body) {
            f.flash = (0..after.hunks.len())
                .filter(|&i| !before.hunks.contains(&after.hunks[i]))
                .collect();
            f.flash_at = (!f.flash.is_empty()).then(std::time::Instant::now);
        }
        f.body = body;
        f.invalidate();
        self.laid_out = None;
        self.after_body(file, texts.as_ref());
        if near && let Some(texts) = texts {
            for (id, text) in [texts.old, texts.new] {
                if let Some(id) = id {
                    let size = text.bytes();
                    self.texts.insert(id, text, size);
                }
            }
        }
    }

    /// Whether file `i` lies in the window laid out exactly around the view.
    fn near_view(&self, i: usize) -> bool {
        self.laid_out
            .map_or(i <= self.top.file + 2, |(.., w)| (w.0..=w.1).contains(&i))
    }

    /// Both texts of a file whose diff is computed, if they are loaded.
    pub(crate) fn texts(&self, f: &FileView) -> Option<FileTexts> {
        file_texts(&self.texts, &self.empty, f)
    }

    /// Loads file `i`'s texts unless cached. A working-tree file that
    /// changed since its diff was computed sends the file back for a new
    /// diff instead.
    fn ensure_texts(&mut self, i: usize) {
        let Some(f) = self.stream.as_ref().and_then(|s| s.files.get(i)) else {
            return;
        };
        let Body::Text(diff) = &f.body else { return };
        let wanted = [f.change.old, diff.new_id];
        let mut missing = false;
        for id in wanted.into_iter().flatten() {
            missing |= self.texts.get(&id).is_none();
        }
        if !missing {
            return;
        }
        let Some(reader) = &mut self.reader else {
            return;
        };
        let change = f.change.clone();
        let contents = reader.contents(&change);
        let stale = match contents {
            Ok(c) if c.new_id == wanted[1] => {
                for (id, bytes) in [(change.old, c.old), (c.new_id, c.new)] {
                    if let Some(id) = id {
                        let text = Text::from_bytes(&bytes);
                        let size = text.bytes();
                        self.texts.insert(id, Arc::new(text), size);
                    }
                }
                false
            }
            Ok(_) => true,
            Err(e) => {
                self.set_body(i, Body::Unreadable(format!("{e:#}")), None);
                false
            }
        };
        if stale {
            self.set_body(i, Body::Pending, None);
            self.needs_diffs = true;
        }
    }

    /// Plans rows and measures them for a diff pane `width` wide and `rows`
    /// tall. Rows are kept and wrapped heights measured only for files near
    /// the view; the rest keep a row count at one line per row until they
    /// come close, so huge streams lay out in time and memory proportional
    /// to what is on screen.
    #[allow(clippy::too_many_lines)]
    pub(crate) fn layout(&mut self, width: u16, rows: u16) {
        let layout = self.effective_layout(width);
        let wrap = self.settings.wrap;
        let header = u32::from(self.settings.header_rows);
        let top = self.global_top();
        let Some(stream) = &self.stream else {
            return;
        };
        let last = stream.files.len().saturating_sub(1);
        let near = |line: u32| {
            stream
                .offsets
                .partition_point(|&o| o <= line)
                .saturating_sub(1)
                .min(last)
        };
        let window = (
            near(top.saturating_sub(u32::from(rows))),
            near(top + 3 * u32::from(rows)),
        );
        if self.laid_out == Some((width, layout, wrap, window)) {
            return;
        }
        for i in window.0..=window.1 {
            self.ensure_texts(i);
        }
        let Some(stream) = &mut self.stream else {
            return;
        };
        // Keep the row under the top of the view across the relayout.
        let anchor_row = stream.files.get(self.top.file).and_then(|f| {
            let line = self.top.line.checked_sub(f.body_start(header))?;
            (f.heights.rows() > 0).then(|| f.heights.locate(line).0)
        });
        let index = cards::index(&self.review, &self.notes, &stream.worktree);
        let stack_width = cards::span(width, layout, 0, None).1;
        let (review_cards, above) = cards::stack(
            &self.review,
            &self.notes,
            index.review.iter().copied(),
            stack_width,
        );
        self.review_cards = review_cards;
        for (i, f) in stream.files.iter_mut().enumerate() {
            let exact = (window.0..=window.1).contains(&i);
            f.above = if i == 0 { above } else { 0 };
            let mine = index
                .by_path
                .get(f.change.path.as_str())
                .map_or(&[][..], Vec::as_slice);
            let Body::Text(diff) = &f.body else {
                f.rows = Vec::new();
                f.heights = Heights::default();
                f.key = None;
                f.cards = Vec::new();
                (f.file_cards, f.file_card_lines) =
                    cards::stack(&self.review, &self.notes, mine.iter().copied(), stack_width);
                continue;
            };
            // One-sided files read best full width, even in split layout.
            let layout = if diff.old_len == 0 || diff.new_len == 0 {
                Layout::Unified
            } else {
                layout
            };
            let key = LayoutKey {
                width,
                layout,
                wrap,
                exact,
            };
            let planned = (layout, f.expanded.len());
            let have_rows = f.rows.len() == f.heights.rows() as usize && f.planned == Some(planned);
            if f.planned != Some(planned) || (exact && !have_rows) {
                f.rows = plan::plan(
                    &diff.hunks,
                    diff.old_len,
                    diff.new_len,
                    &f.expanded,
                    plan::Options {
                        layout,
                        context: CONTEXT,
                        cap: ROW_CAP,
                    },
                );
                f.planned = Some(planned);
                f.heights = Heights::uniform(u32::try_from(f.rows.len()).unwrap_or(u32::MAX));
                f.key = None;
            }
            if exact {
                if f.key != Some(key) {
                    // Cards whose lines are shown go under them; the rest
                    // stack under the header.
                    let mut stacked = Vec::new();
                    let mut under: Vec<(u32, CardRef, review::Side)> = Vec::new();
                    for &card in mine {
                        let c = cards::get(&self.review, &self.notes, card);
                        let outdated = matches!(c, Some(cards::Card::Comment(c)) if c.outdated);
                        let row = c
                            .as_ref()
                            .and_then(|c| c.place().lines())
                            .filter(|_| !outdated)
                            .and_then(|l| cards::attach(&f.rows, l).map(|r| (r, l.side)));
                        match row {
                            Some((r, side)) => under.push((r, card, side)),
                            None => stacked.push(card),
                        }
                    }
                    under.sort_by_key(|&(r, ..)| r);
                    (f.file_cards, f.file_card_lines) =
                        cards::stack(&self.review, &self.notes, stacked, stack_width);
                    let nw = f.number_width();
                    let widths = code_widths(width, nw);
                    let texts = file_texts(&self.texts, &self.empty, f);
                    let mut placed: Vec<Placed> = Vec::with_capacity(under.len());
                    let code = |r: u32| match &texts {
                        Some(texts) if wrap => row_height(&f.rows[r as usize], texts, widths),
                        _ => 1,
                    };
                    for (r, card, side) in under {
                        let start = match placed.last() {
                            Some(p) if p.row == r => p.start + p.height,
                            _ => code(r),
                        };
                        let w = cards::text_width(cards::span(width, layout, nw, Some(side)).1);
                        let height = cards::get(&self.review, &self.notes, card)
                            .map_or(0, |c| cards::lines(&c, w, false).len());
                        placed.push(Placed {
                            card,
                            row: r,
                            start,
                            height: u32::try_from(height).unwrap_or(1),
                        });
                    }
                    f.heights = if texts.is_some() && wrap || !placed.is_empty() {
                        let mut next = placed.iter().peekable();
                        Heights::measured((0..f.rows.len()).map(|r| {
                            let r = u32::try_from(r).unwrap_or(u32::MAX);
                            let mut h = code(r);
                            while let Some(p) = next.next_if(|p| p.row == r) {
                                h = p.start + p.height;
                            }
                            h
                        }))
                    } else {
                        Heights::uniform(f.heights.rows())
                    };
                    f.cards = placed;
                    f.key = Some(key);
                }
            } else {
                f.rows = Vec::new();
                f.cards = Vec::new();
                (f.file_cards, f.file_card_lines) = cards::stack(
                    &self.review,
                    &self.notes,
                    mine.iter().copied().filter(|&c| {
                        cards::get(&self.review, &self.notes, c)
                            .is_some_and(|c| c.place().lines().is_none())
                    }),
                    stack_width,
                );
                // A measurement at another width or layout is no estimate.
                if f.key
                    .is_none_or(|k| (k.width, k.layout, k.wrap) != (width, layout, wrap))
                {
                    f.heights = Heights::uniform(f.heights.rows());
                    f.key = Some(key);
                }
            }
        }
        let mut at = 0;
        for (i, f) in stream.files.iter().enumerate() {
            stream.offsets[i] = at;
            at += f.body_start(header) + f.body_lines() + 1;
        }
        stream.offsets[stream.files.len()] = at;
        if let (Some(row), Some(f)) = (anchor_row, stream.files.get(self.top.file)) {
            self.top.line = f.body_start(header) + f.heights.start(row);
        }
        self.laid_out = Some((width, layout, wrap, window));
        self.restore_anchor();
        self.settle_card(rows);
    }

    /// Puts the view back on the source line it showed before the top
    /// file changed, once that file's new diff is laid out.
    fn restore_anchor(&mut self) {
        let Some((path, at)) = &self.pending_anchor else {
            return;
        };
        let header = u32::from(self.settings.header_rows);
        let Some(f) = self
            .stream
            .as_ref()
            .and_then(|s| s.files.get(self.top.file))
        else {
            self.pending_anchor = None;
            return;
        };
        if f.change.path != *path {
            self.pending_anchor = None;
            return;
        }
        match (&f.body, *at) {
            (Body::Pending, _) => return,
            (Body::Text(_), SourceLine::New(n, sub)) if !f.rows.is_empty() => {
                let row = f
                    .rows
                    .iter()
                    .position(|r| match r {
                        Row::Line { new: Some(x), .. } | Row::Pair { new: Some(x), .. } => *x >= n,
                        _ => false,
                    })
                    .unwrap_or(f.rows.len() - 1);
                let row = u32::try_from(row).unwrap_or(0);
                let height = f.heights.start(row + 1) - f.heights.start(row);
                self.top.line =
                    f.body_start(header) + f.heights.start(row) + sub.min(height.saturating_sub(1));
            }
            (_, SourceLine::Display(line)) => {
                self.top.line = line.min(f.body_start(header) + f.body_lines());
            }
            _ => {}
        }
        self.pending_anchor = None;
    }

    /// When the oldest change marks fade, so the view repaints then.
    pub(crate) fn flash_deadline(&self) -> Option<std::time::Instant> {
        self.stream
            .as_ref()?
            .files
            .iter()
            .filter_map(|f| f.flash_at)
            .min()
            .map(|at| at + FLASH)
    }

    /// Clears change marks that have faded by `now`.
    pub(crate) fn expire_flashes(&mut self, now: std::time::Instant) {
        if let Some(stream) = &mut self.stream {
            for f in &mut stream.files {
                if f.flash_at.is_some_and(|at| now >= at + FLASH) {
                    f.flash_at = None;
                    f.flash.clear();
                }
            }
        }
    }

    pub(crate) fn effective_layout(&self, width: u16) -> Layout {
        match self.settings.layout {
            LayoutChoice::Auto if width >= SPLIT_MIN_WIDTH => Layout::Split,
            LayoutChoice::Split => Layout::Split,
            LayoutChoice::Unified | LayoutChoice::Auto => Layout::Unified,
        }
    }

    /// The file whose header is pinned at the top of the view: the top
    /// file's, unless the view starts on the review's cards above it.
    pub(crate) fn pinned_header(&self) -> Option<usize> {
        let file = self.stream.as_ref()?.files.get(self.top.file)?;
        (self.top.line >= file.above).then_some(self.top.file)
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
        if local < f.above {
            return Line::Cards {
                file,
                review: true,
                line: local,
            };
        }
        let local = local - f.above;
        if local < header {
            return Line::Header {
                file,
                part: u16::try_from(local).unwrap_or(0),
            };
        }
        if local < header + f.file_card_lines {
            return Line::Cards {
                file,
                review: false,
                line: local - header,
            };
        }
        let body = local - header - f.file_card_lines;
        if body >= f.body_lines() {
            return Line::Spacer;
        }
        if f.heights.rows() == 0 {
            return Line::Note { file };
        }
        let (row, sub) = f.heights.locate(body);
        Line::Body {
            file,
            row: row as usize,
            sub,
        }
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

    /// Moves to the next or previous start of a change block, searching
    /// outward from the file at the top.
    pub(crate) fn next_hunk(&mut self, forward: bool, view_rows: u16) {
        let Some(stream) = &self.stream else { return };
        let n = stream.files.len();
        if n == 0 {
            return;
        }
        let header = u32::from(self.settings.header_rows);
        let top = self.global_top();
        let order: Box<dyn Iterator<Item = usize>> = if forward {
            Box::new(self.top.file..n)
        } else {
            Box::new((0..=self.top.file.min(n - 1)).rev())
        };
        for fi in order {
            let f = &stream.files[fi];
            let rows = f.planned_rows();
            let mut starts = Vec::new();
            let mut in_change = false;
            for (ri, row) in (0u32..).zip(rows.iter()) {
                let change = matches!(
                    row,
                    Row::Line {
                        kind: Kind::Delete | Kind::Insert,
                        ..
                    } | Row::Pair { changed: true, .. }
                );
                if change && !in_change {
                    // Leave a little context above the change.
                    let line = stream.offsets[fi] + f.body_start(header) + f.heights.start(ri);
                    starts.push(line.saturating_sub(2).max(stream.offsets[fi]));
                }
                in_change = change;
            }
            let target = if forward {
                starts.into_iter().find(|&s| s > top)
            } else {
                starts.into_iter().rev().find(|&s| s < top)
            };
            if let Some(line) = target {
                self.scroll_to(line, view_rows);
                self.sync_files_cursor();
                return;
            }
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
            let Some(texts) = self.texts(f) else { continue };
            let Some(lang) = highlight::language(&f.change.path) else {
                continue;
            };
            for (id, text) in [&texts.new, &texts.old] {
                let Some(id) = *id else { continue };
                if text.len() == 0
                    || text.src.len() > highlight::MAX_BYTES
                    || text.len() > highlight::MAX_LINES
                {
                    continue;
                }
                let key = (id, lang);
                if !self.highlights.contains(&key) {
                    jobs.push(highlight::Job {
                        key,
                        text: Arc::clone(text),
                    });
                }
            }
        }
        jobs
    }

    /// Bytes held by the text, colour and emphasis caches.
    pub(crate) fn cache_bytes(&self) -> [usize; 3] {
        [
            self.texts.bytes(),
            self.highlights.bytes(),
            self.emphasis.bytes(),
        ]
    }

    /// Computes word emphasis for the changed hunks on screen.
    pub(crate) fn prepare_emphasis(&mut self, view_rows: u16) {
        let top = self.global_top();
        let mut wanted: Vec<(EmphasisKey, usize, usize)> = Vec::new();
        for line in top..top + u32::from(view_rows) {
            let Line::Body { file, row, .. } = self.line_at(line) else {
                continue;
            };
            let Some(stream) = &self.stream else { return };
            let f = &stream.files[file];
            let Body::Text(diff) = &f.body else { continue };
            let hunk = match f.rows.get(row) {
                Some(Row::Line {
                    kind: Kind::Delete,
                    old: Some(o),
                    ..
                }) => diff.hunk_of(false, *o),
                Some(Row::Line {
                    kind: Kind::Insert,
                    new: Some(n),
                    ..
                }) => diff.hunk_of(true, *n),
                Some(Row::Pair {
                    changed: true,
                    old,
                    new,
                }) => old
                    .and_then(|o| diff.hunk_of(false, o))
                    .or_else(|| new.and_then(|n| diff.hunk_of(true, n))),
                _ => None,
            };
            if let Some(h) = hunk {
                let key = (
                    f.change.old,
                    diff.new_id,
                    u32::try_from(h).unwrap_or(u32::MAX),
                );
                if !self.emphasis.contains(&key) && !wanted.iter().any(|(k, ..)| *k == key) {
                    wanted.push((key, file, h));
                }
            }
        }
        for (key, file, h) in wanted {
            let Some(stream) = &self.stream else { return };
            let f = &stream.files[file];
            let (Body::Text(diff), Some(texts)) = (&f.body, self.texts(f)) else {
                continue;
            };
            let emphasis = diff::hunk_emphasis(&texts.old.1, &texts.new.1, &diff.hunks[h]);
            let size = emphasis
                .as_ref()
                .map_or(0, |e| (e.old.len() + e.new.len()) * 8)
                + 64;
            self.emphasis.insert(key, emphasis.map(Arc::new), size);
        }
    }

    /// The word emphasis of hunk `hunk` of `f`, once computed.
    pub(crate) fn emphasis(&self, f: &FileView, hunk: usize) -> Option<&HunkEmphasis> {
        let Body::Text(diff) = &f.body else {
            return None;
        };
        let key = (f.change.old, diff.new_id, u32::try_from(hunk).ok()?);
        self.emphasis.peek(&key)?.as_deref()
    }
}

/// Reads both sides of a file and diffs them, or marks it binary. The
/// texts come back too, for the cache.
pub(crate) fn compute_body(reader: &mut Reader, change: &FileChange) -> (Body, Option<FileTexts>) {
    let contents = match reader.contents(change) {
        Ok(c) => c,
        Err(e) => return (Body::Unreadable(format!("{e:#}")), None),
    };
    if diff::is_binary(&contents.old) || diff::is_binary(&contents.new) {
        return (Body::Binary(contents.new_id), None);
    }
    let old = Arc::new(Text::from_bytes(&contents.old));
    let new = Arc::new(Text::from_bytes(&contents.new));
    let diff = FileDiff::compute(&old, &new, contents.new_id);
    let texts = FileTexts {
        old: (change.old, old),
        new: (contents.new_id, new),
    };
    (Body::Text(Arc::new(diff)), Some(texts))
}

/// Both texts of `f` from the cache; a missing side is the empty text.
fn file_texts(
    texts: &Lru<gix::ObjectId, Arc<Text>>,
    empty: &Arc<Text>,
    f: &FileView,
) -> Option<FileTexts> {
    let Body::Text(diff) = &f.body else {
        return None;
    };
    let side = |id: Option<gix::ObjectId>| match id {
        Some(id) => texts.peek(&id).cloned().map(|t| (Some(id), t)),
        None => Some((None, Arc::clone(empty))),
    };
    Some(FileTexts {
        old: side(f.change.old)?,
        new: side(diff.new_id)?,
    })
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

    fn change(path: &str, old: &str, new: &str) -> FileChange {
        FileChange {
            path: path.to_owned(),
            status: Status::Modified,
            old: (!old.is_empty()).then(|| id_of(old)),
            new: crate::repo::Side::Worktree(crate::repo::Stamp {
                modified: None,
                len: new.len() as u64,
            }),
        }
    }

    fn id_of(text: &str) -> gix::ObjectId {
        gix::objs::compute_hash(
            gix::hash::Kind::Sha1,
            gix::object::Kind::Blob,
            text.as_bytes(),
        )
        .expect("hashing")
    }

    fn target() -> Target {
        let null = gix::ObjectId::null(gix::hash::Kind::Sha1);
        Target {
            base_tree: null,
            head_tree: null,
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
        app.load(
            "/r".into(),
            Mode::Base,
            "main".into(),
            target(),
            changes,
            None,
        );
        for (i, &(_, o, n)) in files.iter().enumerate() {
            let (old, new) = (
                Arc::new(Text::new(Arc::from(o))),
                Arc::new(Text::new(Arc::from(n))),
            );
            let new_id = Some(id_of(n));
            let diff = FileDiff::compute(&old, &new, new_id);
            let texts = FileTexts {
                old: ((!o.is_empty()).then(|| id_of(o)), old),
                new: (new_id, new),
            };
            app.set_body(i, Body::Text(Arc::new(diff)), Some(texts));
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
        app.load(path, Mode::Base, "main".into(), target(), changes, None);
        assert_eq!(app.top.file, 2, "anchored to b.rs by path");
    }

    #[test]
    fn auto_layout_splits_wide_panes() {
        let app = app_with(&[]);
        assert_eq!(app.effective_layout(120), Layout::Unified);
        assert_eq!(app.effective_layout(SPLIT_MIN_WIDTH), Layout::Split);
    }
}
