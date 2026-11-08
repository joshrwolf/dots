//! Viewer state: the worktrees, the change stream of the selected one, its
//! layout at the current width, and the scroll position. Pure state; the
//! interactive runtime and `render` both drive it.

use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::path::PathBuf;
use std::sync::Arc;

use ratatui::layout::Rect;

use crate::cards::{self, CardRef, Placed};
use crate::diff::{self, FileDiff, HunkEmphasis, Text};
use crate::editor::Editor;
use crate::highlight;
use crate::plan::{self, Heights, Kind, Layout, Row};
use crate::repo::{self, Changes, FileChange, Reader, Repo, Side, Target};
use crate::review;
use crate::session::Session;
use crate::theme::Palette;
use crate::wrap;

mod annotate;
mod cache;
mod sidebar;
mod standing;
pub(crate) use annotate::{Scope, row_line};
pub(crate) use cache::Cache;
use cache::Loaded;
pub(crate) use sidebar::{ListItem, Sidebar};
pub(crate) use standing::{Marker, Standing};

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
/// Both sides of a file's text, by content id: what a diff job hands over
/// besides the diff, and what drawing reads.
#[derive(Debug, Clone)]
pub(crate) struct FileTexts {
    pub(crate) old: (Option<gix::ObjectId>, Arc<Text>),
    pub(crate) new: (Option<gix::ObjectId>, Arc<Text>),
}

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

/// What keys and the mouse act on: the views, or one overlay over them.
#[derive(Debug, Default)]
pub(crate) enum Mode {
    #[default]
    Normal,
    /// Selecting lines from the anchor to the cursor.
    Visual(Pos),
    /// `;` was pressed; the next key completes the command.
    Prefix,
    Help,
    Switcher(crate::switcher::Switcher),
    Picker(crate::picker::Picker),
    Editor(Editor),
}

impl Mode {
    /// Whether an overlay or a pending key takes the input, so the views
    /// ignore the mouse.
    pub(crate) fn modal(&self) -> bool {
        !matches!(self, Mode::Normal | Mode::Visual(_))
    }
}

/// How file headers are drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Headers {
    /// Text, with a blank line after each file.
    Text,
    /// Kitty images, each drawing a separator along its top edge in place
    /// of the blank line.
    Images,
}

#[derive(Debug, Clone)]
pub(crate) struct Settings {
    pub(crate) layout: LayoutChoice,
    pub(crate) wrap: bool,
    pub(crate) mode: repo::Mode,
    /// Show only what changed since the last send.
    pub(crate) since: bool,
    pub(crate) sidebar: bool,
    /// Sidebar columns chosen with the resize keys; `None` sizes it to the screen.
    pub(crate) sidebar_width: Option<u16>,
    pub(crate) headers: Headers,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            layout: LayoutChoice::Auto,
            wrap: true,
            mode: repo::Mode::Base,
            since: false,
            sidebar: true,
            sidebar_width: None,
            headers: Headers::Text,
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

/// What a stream is laid out for.
#[derive(Debug, Clone, Copy)]
struct Measure {
    width: u16,
    layout: Layout,
    wrap: bool,
    /// Width of stacked cards.
    stack_width: u16,
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
    /// Shown as its header alone.
    pub(crate) folded: bool,
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
            folded: false,
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
    pub(crate) fn body_start(&self) -> u32 {
        self.above + 1 + self.card_lines()
    }

    /// Display lines of the cards under the header, none while folded.
    fn card_lines(&self) -> u32 {
        if self.folded { 0 } else { self.file_card_lines }
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
            _ if self.folded => 0,
            Body::Text(_) if self.heights.rows() > 0 => self.heights.total(),
            _ => 1,
        }
    }

    fn invalidate(&mut self) {
        self.key = None;
        self.planned = None;
    }

    /// Lays the file out with its `cards`: plans its rows, and near the
    /// view (`exact`) keeps them, places cards under the lines they are
    /// about and measures wrapped heights. Elsewhere it keeps a row count,
    /// and only cards not about lines stack under the header.
    fn lay_out(
        &mut self,
        m: Measure,
        exact: bool,
        session: &Session,
        cards: &[CardRef],
        cache: &Cache,
    ) {
        let Body::Text(diff) = &self.body else {
            self.rows = Vec::new();
            self.heights = Heights::default();
            self.key = None;
            self.cards = Vec::new();
            (self.file_cards, self.file_card_lines) =
                cards::stack(session, cards.iter().copied(), m.stack_width);
            return;
        };
        // One-sided files read best full width, even in split layout.
        let layout = if diff.old_len == 0 || diff.new_len == 0 {
            Layout::Unified
        } else {
            m.layout
        };
        let key = LayoutKey {
            width: m.width,
            layout,
            wrap: m.wrap,
            exact,
        };
        let planned = (layout, self.expanded.len());
        let have_rows =
            self.rows.len() == self.heights.rows() as usize && self.planned == Some(planned);
        if self.planned != Some(planned) || (exact && !have_rows) {
            self.rows = plan::plan(
                &diff.hunks,
                diff.old_len,
                diff.new_len,
                &self.expanded,
                plan::Options {
                    layout,
                    context: CONTEXT,
                    cap: ROW_CAP,
                },
            );
            self.planned = Some(planned);
            self.heights = Heights::uniform(u32::try_from(self.rows.len()).unwrap_or(u32::MAX));
            self.key = None;
        }
        if !exact {
            self.rows = Vec::new();
            self.cards = Vec::new();
            let unlined = cards
                .iter()
                .copied()
                .filter(|&c| cards::get(session, c).is_some_and(|c| c.place().lines().is_none()));
            (self.file_cards, self.file_card_lines) = cards::stack(session, unlined, m.stack_width);
            // A measurement at another width or layout is no estimate.
            if self
                .key
                .is_none_or(|k| (k.width, k.layout, k.wrap) != (m.width, layout, m.wrap))
            {
                self.heights = Heights::uniform(self.heights.rows());
                self.key = Some(key);
            }
            return;
        }
        if self.key == Some(key) {
            return;
        }
        let nw = self.number_width();
        let widths = code_widths(m.width, nw);
        let texts = cache.texts(self);
        let code = |r: u32| match &texts {
            Some(texts) if m.wrap => row_height(&self.rows[r as usize], texts, widths),
            _ => 1,
        };
        let (placed, stacked) = cards::place(session, cards, &self.rows, code, m.width, layout, nw);
        (self.file_cards, self.file_card_lines) = cards::stack(session, stacked, m.stack_width);
        self.heights = if texts.is_some() && m.wrap || !placed.is_empty() {
            let mut next = placed.iter().peekable();
            Heights::measured((0..self.rows.len()).map(|r| {
                let r = u32::try_from(r).unwrap_or(u32::MAX);
                let mut h = code(r);
                while let Some(p) = next.next_if(|p| p.row == r) {
                    h = p.start + p.height;
                }
                h
            }))
        } else {
            Heights::uniform(self.heights.rows())
        };
        self.cards = placed;
        self.key = Some(key);
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

/// What a stream compares: a worktree, in a mode, against a base.
#[derive(Debug)]
struct Keys {
    worktree: PathBuf,
    mode: repo::Mode,
    base: String,
    target: Target,
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
    pub(crate) mode: repo::Mode,
    pub(crate) target: Target,
    pub(crate) files: Vec<FileView>,
    /// Start line of each file block; one longer than `files`.
    offsets: Vec<u32>,
    /// What the offsets were measured for: pane width, layout, wrapping
    /// and the files laid out exactly. `None` once they are stale.
    laid_out: Option<(u16, Layout, bool, Window)>,
}

/// Files laid out exactly around the view, first to last.
type Window = (usize, usize);

impl Stream {
    fn new(keys: Keys, files: Vec<FileView>) -> Self {
        Self {
            worktree: keys.worktree,
            base: keys.base,
            mode: keys.mode,
            target: keys.target,
            offsets: vec![0; files.len() + 1],
            laid_out: None,
            files,
        }
    }

    /// Lines in the stream, as last laid out.
    pub(crate) fn total(&self) -> u32 {
        self.offsets.last().copied().unwrap_or(0)
    }

    /// The first line of file `i`'s block.
    pub(crate) fn offset(&self, i: usize) -> Option<u32> {
        self.offsets.get(i).copied()
    }

    /// The file whose block holds stream line `line`, or the last file.
    pub(crate) fn file_at(&self, line: u32) -> usize {
        self.offsets
            .partition_point(|&o| o <= line)
            .saturating_sub(1)
            .min(self.files.len().saturating_sub(1))
    }

    /// Marks the layout stale, so the next draw lays the stream out again.
    fn relayout(&mut self) {
        self.laid_out = None;
    }

    /// Sums the file blocks' lines into their start lines.
    fn measure(&mut self, spacers: bool) {
        let mut at = 0;
        for (i, f) in self.files.iter().enumerate() {
            self.offsets[i] = at;
            at += f.body_start() + f.body_lines() + u32::from(spacers);
        }
        self.offsets[self.files.len()] = at;
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
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Summary {
    pub(crate) added: u32,
    pub(crate) removed: u32,
    pub(crate) files: usize,
    /// Names the changes by their content: equal summaries of equal changes
    /// have equal names.
    pub(crate) content: Option<String>,
    /// When an uncommitted file last changed.
    pub(crate) changed: Option<std::time::SystemTime>,
    /// The content the user last looked at, from the worktree's review.
    pub(crate) looked: Option<String>,
}

/// What a display line of the diff pane shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Line {
    Header {
        file: usize,
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
    pub(crate) mode: Mode,
    pub(crate) sidebar: Sidebar,
    pub(crate) summaries: HashMap<PathBuf, Summary>,
    /// Interactive Pi sessions and workers, once read.
    agents: Option<crate::agents::Snapshot>,
    /// Who works in each worktree, in the repository's order.
    owners: Vec<crate::agents::Owners>,
    /// Workers that finished or asked while the viewer was open, by the
    /// worktree they worked in, until it is shown.
    pub(crate) markers: HashMap<PathBuf, Marker>,
    pub(crate) cache: Cache,
    /// Set when a file's contents changed under its diff, so it needs a new one.
    pub(crate) needs_diffs: bool,
    /// Where to put the view once the top file's new diff is laid out.
    pending_anchor: Option<(String, SourceLine)>,
    pub(crate) areas: Areas,
    /// With `--graphics`, the kitty image that shows each file's header
    /// in this frame, by file.
    pub(crate) header_images: HashMap<usize, u32>,
    pub(crate) message: Option<String>,
    /// The review and agents' notes.
    pub(crate) session: Session,
    /// The diff cursor, as a screen row of the diff pane.
    pub(crate) cursor_row: u16,
    /// On a split row, comment on the old side.
    pub(crate) cursor_old: bool,
    /// Mark code moved between places.
    pub(crate) show_moves: bool,
    /// The last moved-code search, once one finished.
    pub(crate) moves: Option<Arc<crate::moves::Moves>>,
    /// Who a send goes to, as the status line shows it.
    pub(crate) target: String,
    /// The review-level cards, above the first file.
    pub(crate) review_cards: Vec<Placed>,
    /// A card to bring into view once its file is laid out.
    pending_card: Option<CardRef>,
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
            mode: Mode::Normal,
            sidebar: Sidebar::default(),
            summaries: HashMap::new(),
            agents: None,
            owners: Vec::new(),
            markers: HashMap::new(),
            cache: Cache::default(),
            needs_diffs: false,
            pending_anchor: None,
            areas: Areas::default(),
            header_images: HashMap::new(),
            message: None,
            session: Session::default(),
            cursor_row: 0,
            cursor_old: false,
            show_moves: false,
            moves: None,
            target: String::new(),
            review_cards: Vec::new(),
            pending_card: None,
        }
    }

    pub(crate) fn worktree_path(&self) -> &PathBuf {
        &self.repo.worktrees[self.current].path
    }

    /// Shows a worktree's freshly loaded changes in `mode`, with its saved
    /// review when it is newly shown. Returns what the user should know
    /// about the review.
    pub(crate) fn install(
        &mut self,
        worktree: PathBuf,
        mode: repo::Mode,
        changes: Changes,
        review: Result<(PathBuf, review::State), String>,
    ) -> Option<String> {
        self.markers.remove(&worktree);
        let message = if self.session.shows(&worktree) {
            None
        } else {
            self.end_visual();
            self.session.install(&worktree, review)
        };
        let reader = Reader::new(changes.repo.to_thread_local(), changes.root);
        let keys = Keys {
            worktree,
            mode,
            base: changes.base,
            target: changes.target,
        };
        self.load(keys, changes.files, Some(reader));
        self.refresh_marks();
        self.cards_changed();
        message
    }

    /// Shows a change set. When it is the same worktree, unchanged files
    /// keep their diffs and the view stays on the same source line.
    fn load(&mut self, keys: Keys, files: Vec<FileChange>, reader: Option<Reader>) {
        self.cache.set_reader(reader);
        let same = self
            .stream
            .as_ref()
            .is_some_and(|s| s.worktree == keys.worktree && s.mode == keys.mode);
        if !same {
            self.stream = None;
            self.top = Anchor::default();
            self.sidebar.reset();
        }
        self.replace_files(keys, files);
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
        let keys = Keys {
            worktree: stream.worktree.clone(),
            mode: stream.mode,
            base: stream.base.clone(),
            target: stream.target,
        };
        self.replace_files(keys, merged);
    }

    /// Swaps in a new file list for the stream, keeping what still
    /// applies: diffs of files whose contents are unchanged, expanded
    /// regions, and the source line at the top of the view.
    fn replace_files(&mut self, keys: Keys, files: Vec<FileChange>) {
        let old = self.stream.take();
        // A selection follows its file by path, and ends when it is gone.
        let visual = match &self.mode {
            Mode::Visual(v) => old
                .as_ref()
                .and_then(|s| s.files.get(v.file))
                .map(|f| (f.change.path.clone(), v.clone())),
            _ => None,
        };
        let mut anchor = old.as_ref().and_then(|s| self.source_anchor(s));
        let mut previous: HashMap<String, FileView> = old
            .into_iter()
            .flat_map(|s| s.files)
            .map(|f| (f.change.path.clone(), f))
            .collect();
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
                    f.folded = stale.folded;
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
        if let Mode::Visual(_) = self.mode {
            self.mode = visual
                .and_then(|(path, v)| {
                    let file = files.iter().position(|f| f.change.path == path)?;
                    Some(Mode::Visual(Pos { file, ..v }))
                })
                .unwrap_or_default();
        }
        self.pending_anchor = anchor;
        self.sidebar.set_files(&files);
        self.stream = Some(Stream::new(keys, files));
        self.sync_files_cursor();
    }

    /// The file and source position at the top of the view: a new-side line
    /// number when the top is on a line, else the display line in the file.
    fn source_anchor(&self, stream: &Stream) -> Option<(String, SourceLine)> {
        let f = stream.files.get(self.top.file)?;
        let at = match self.top.line.checked_sub(f.body_start()) {
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
        stream.relayout();
        self.after_body(file, texts.as_ref());
        if near && let Some(texts) = texts {
            self.cache.keep_texts(texts);
        }
    }

    /// Whether file `i` lies in the window laid out exactly around the view.
    fn near_view(&self, i: usize) -> bool {
        self.stream
            .as_ref()
            .and_then(|s| s.laid_out)
            .map_or(i <= self.top.file + 2, |(.., w)| (w.0..=w.1).contains(&i))
    }

    /// Both texts of a file whose diff is computed, if they are loaded.
    pub(crate) fn texts(&self, f: &FileView) -> Option<FileTexts> {
        self.cache.texts(f)
    }

    /// Loads file `i`'s texts unless cached. A working-tree file that
    /// changed since its diff was computed sends the file back for a new
    /// diff instead.
    fn ensure_texts(&mut self, i: usize) {
        let Some(f) = self.stream.as_ref().and_then(|s| s.files.get(i)) else {
            return;
        };
        match self.cache.load_texts(f) {
            Loaded::Ready => {}
            Loaded::Stale => {
                self.set_body(i, Body::Pending, None);
                self.needs_diffs = true;
            }
            Loaded::Unreadable(e) => self.set_body(i, Body::Unreadable(e), None),
        }
    }

    /// Plans rows and measures them for a diff pane `width` wide and `rows`
    /// tall. Rows are kept and wrapped heights measured only for files near
    /// the view; the rest keep a row count at one line per row until they
    /// come close, so huge streams lay out in time and memory proportional
    /// to what is on screen.
    pub(crate) fn layout(&mut self, width: u16, rows: u16) {
        let layout = self.effective_layout(width);
        let wrap = self.settings.wrap;
        let top = self.global_top();
        let Some(stream) = &self.stream else {
            return;
        };
        // Before the first layout there are no offsets to look files up by.
        let window = if stream.total() == 0 {
            let last = stream.files.len().saturating_sub(1);
            (self.top.file, (self.top.file + 2).min(last))
        } else {
            (
                stream.file_at(top.saturating_sub(u32::from(rows))),
                stream.file_at(top + 3 * u32::from(rows)),
            )
        };
        if stream.laid_out == Some((width, layout, wrap, window)) {
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
            let line = self.top.line.checked_sub(f.body_start())?;
            (f.heights.rows() > 0).then(|| f.heights.locate(line).0)
        });
        let index = cards::index(&self.session, &stream.worktree);
        let measure = Measure {
            width,
            layout,
            wrap,
            stack_width: cards::span(width, layout, 0, None).1,
        };
        let (review_cards, above) = cards::stack(
            &self.session,
            index.review.iter().copied(),
            measure.stack_width,
        );
        self.review_cards = review_cards;
        for (i, f) in stream.files.iter_mut().enumerate() {
            f.above = if i == 0 { above } else { 0 };
            let mine = index
                .by_path
                .get(f.change.path.as_str())
                .map_or(&[][..], Vec::as_slice);
            let exact = (window.0..=window.1).contains(&i);
            f.lay_out(measure, exact, &self.session, mine, &self.cache);
        }
        stream.measure(self.settings.headers == Headers::Text);
        if let (Some(row), Some(f)) = (anchor_row, stream.files.get(self.top.file)) {
            self.top.line = f.body_start() + f.heights.start(row);
        }
        stream.laid_out = Some((width, layout, wrap, window));
        self.restore_anchor();
        self.settle_card(rows);
    }

    /// Puts the view back on the source line it showed before the top
    /// file changed, once that file's new diff is laid out.
    fn restore_anchor(&mut self) {
        let Some((path, at)) = &self.pending_anchor else {
            return;
        };
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
                    f.body_start() + f.heights.start(row) + sub.min(height.saturating_sub(1));
            }
            (_, SourceLine::Display(line)) => {
                self.top.line = line.min(f.body_start() + f.body_lines());
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
        let file = stream.file_at(line);
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
        if local < f.above {
            return Line::Cards {
                file,
                review: true,
                line: local,
            };
        }
        let local = local - f.above;
        if local == 0 {
            return Line::Header { file };
        }
        if local <= f.card_lines() {
            return Line::Cards {
                file,
                review: false,
                line: local - 1,
            };
        }
        let body = local - 1 - f.card_lines();
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

    /// The file at `path` within the worktree, or else the first whose
    /// path ends with it.
    pub(crate) fn find_file(&self, path: &str) -> Option<usize> {
        let files = &self.stream.as_ref()?.files;
        files
            .iter()
            .position(|f| f.change.path == path)
            .or_else(|| files.iter().position(|f| f.change.path.ends_with(path)))
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
        let top = self.global_top();
        let order: Box<dyn Iterator<Item = usize>> = if forward {
            Box::new(self.top.file..n)
        } else {
            Box::new((0..=self.top.file.min(n - 1)).rev())
        };
        for fi in order {
            let f = &stream.files[fi];
            if f.folded {
                continue;
            }
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
                    let line = stream.offsets[fi] + f.body_start() + f.heights.start(ri);
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
        stream.relayout();
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
        stream.relayout();
    }

    /// Folds file `file` to its header or unfolds it. A file folded from
    /// inside shows its header at the top.
    pub(crate) fn toggle_fold(&mut self, file: usize) {
        let Some(stream) = &mut self.stream else {
            return;
        };
        let Some(f) = stream.files.get_mut(file) else {
            return;
        };
        f.folded = !f.folded;
        if f.folded && self.top.file == file {
            self.top.line = self.top.line.min(f.above);
        }
        stream.relayout();
    }

    /// Folds every file, or unfolds them all when all are folded.
    pub(crate) fn toggle_fold_all(&mut self) {
        let Some(stream) = &mut self.stream else {
            return;
        };
        let fold = !stream.files.iter().all(|f| f.folded);
        for f in &mut stream.files {
            f.folded = fold;
        }
        if let Some(f) = stream.files.get(self.top.file) {
            self.top.line = self.top.line.min(f.above);
        }
        stream.relayout();
    }

    pub(crate) fn relayout(&mut self) {
        if let Some(stream) = &mut self.stream {
            stream.relayout();
            for f in &mut stream.files {
                f.invalidate();
            }
        }
    }

    /// Moves the sidebar cursor to the file at the top of the view, or to
    /// the collapsed directory hiding it.
    pub(crate) fn sync_files_cursor(&mut self) {
        self.sidebar.follow_file(self.top.file);
    }

    /// Selects sidebar row `row` and brings the file it stands for (a
    /// directory's first file) to the top of the stream.
    pub(crate) fn select_row(&mut self, row: usize, view_rows: u16) {
        let Some(file) = self.sidebar.rows().get(row).map(ListItem::first_file) else {
            return;
        };
        self.jump_to_file(file, view_rows);
        self.sidebar.select(row);
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
                if !self.cache.has_highlight(&key) {
                    jobs.push(highlight::Job {
                        key,
                        text: Arc::clone(text),
                    });
                }
            }
        }
        jobs
    }

    /// Computes within-line emphasis for the changed hunks on screen.
    pub(crate) fn prepare_emphasis(&mut self, view_rows: u16) {
        let top = self.global_top();
        let mut wanted: Vec<(cache::EmphasisKey, usize, usize)> = Vec::new();
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
                if !self.cache.has_emphasis(&key) && !wanted.iter().any(|(k, ..)| *k == key) {
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
            self.cache.keep_emphasis(key, emphasis);
        }
    }

    /// Line `n` of one side of `f`, if moved code is shown and it moved.
    pub(crate) fn moved(
        &self,
        f: &FileView,
        new_side: bool,
        n: u32,
    ) -> Option<&crate::moves::Moved> {
        if !self.show_moves {
            return None;
        }
        let Body::Text(diff) = &f.body else {
            return None;
        };
        let key = (f.change.old, diff.new_id);
        self.moves.as_ref()?.line(&f.change.path, key, new_side, n)
    }

    /// Identifies the diffs a moved-code search would read, once none is
    /// pending.
    pub(crate) fn move_fingerprint(&self) -> Option<u64> {
        use std::hash::{Hash as _, Hasher as _};
        let stream = self.stream.as_ref()?;
        let mut h = std::hash::DefaultHasher::new();
        stream.worktree.hash(&mut h);
        for f in &stream.files {
            match &f.body {
                Body::Pending => return None,
                Body::Text(diff) => (&f.change.path, f.change.old, diff.new_id).hash(&mut h),
                _ => {}
            }
        }
        Some(h.finish())
    }

    /// Every diffed file, once none is pending, to search for moved code.
    pub(crate) fn move_inputs(&self) -> Option<Vec<(FileChange, Arc<FileDiff>)>> {
        let stream = self.stream.as_ref()?;
        let mut out = Vec::with_capacity(stream.files.len());
        for f in &stream.files {
            match &f.body {
                Body::Pending => return None,
                Body::Text(diff) if !diff.hunks.is_empty() => {
                    out.push((f.change.clone(), Arc::clone(diff)));
                }
                _ => {}
            }
        }
        Some(out)
    }

    /// The within-line emphasis of hunk `hunk` of `f`, once computed.
    pub(crate) fn emphasis(&self, f: &FileView, hunk: usize) -> Option<&HunkEmphasis> {
        let Body::Text(diff) = &f.body else {
            return None;
        };
        let key = (f.change.old, diff.new_id, u32::try_from(hunk).ok()?);
        self.cache.emphasis(&key)
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

#[cfg(test)]
pub(crate) mod fixture {
    //! Loaded apps without Git, for tests.

    use super::*;
    use crate::repo::Status;
    use crate::theme::Detected;

    pub(crate) fn change(path: &str, old: &str, new: &str) -> FileChange {
        FileChange {
            path: path.to_owned(),
            status: Status::Modified,
            old: (!old.is_empty()).then(|| id_of(old)),
            new: crate::repo::Side::Worktree(crate::repo::Stamp {
                modified: None,
                len: new.len() as u64,
            }),
            from: None,
            modes: None,
        }
    }

    pub(crate) fn id_of(text: &str) -> gix::ObjectId {
        gix::objs::compute_hash(
            gix::hash::Kind::Sha1,
            gix::object::Kind::Blob,
            text.as_bytes(),
        )
        .expect("hashing")
    }

    pub(crate) fn target() -> Target {
        let null = gix::ObjectId::null(gix::hash::Kind::Sha1);
        Target {
            base_tree: null,
            head_tree: null,
        }
    }

    /// The fixture worktree's stream keys.
    pub(super) fn keys() -> Keys {
        Keys {
            worktree: "/r".into(),
            mode: repo::Mode::Base,
            base: "main".into(),
            target: target(),
        }
    }

    pub(crate) fn app_with(files: &[(&str, &str, &str)]) -> App {
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
        app.load(keys(), changes, None);
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
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::fixture::{app_with, change};
    use super::*;

    #[test]
    fn stream_stacks_header_rows_and_spacer_per_file() {
        let mut app = app_with(&[("a.rs", "x\n", "y\n"), ("b/c.rs", "", "1\n2\n")]);
        app.layout(80, 40);
        let s = app.stream.as_ref().expect("loaded");
        // a: header + 2 rows + spacer; b: header + 2 rows + spacer.
        assert_eq!(s.offsets, vec![0, 4, 8]);
        assert_eq!(app.line_at(0), Line::Header { file: 0 });
        assert_eq!(
            app.line_at(2),
            Line::Body {
                file: 0,
                row: 1,
                sub: 0
            }
        );
        assert_eq!(app.line_at(3), Line::Spacer);
        assert_eq!(app.line_at(4), Line::Header { file: 1 });
        let labels: Vec<_> = app
            .sidebar
            .rows()
            .iter()
            .map(|it| match it {
                ListItem::Dir { label, .. } => label.clone(),
                ListItem::File { index, .. } => index.to_string(),
            })
            .collect();
        assert_eq!(labels, ["0", "b/", "1"]);
    }

    #[test]
    fn image_headers_follow_the_last_line_before_them() {
        let mut app = app_with(&[("a.rs", "x\n", "y\n"), ("b/c.rs", "", "1\n2\n")]);
        app.settings.headers = Headers::Images;
        app.layout(80, 40);
        let s = app.stream.as_ref().expect("loaded");
        // a: header + 2 rows; b: header + 2 rows.
        assert_eq!(s.offsets, vec![0, 3, 6]);
        assert_eq!(app.line_at(3), Line::Header { file: 1 });
        assert_eq!(app.line_at(6), Line::Spacer, "past the end");
    }

    #[test]
    fn folded_files_show_only_their_header() {
        let mut app = app_with(&[("a.rs", "x\n", "y\n"), ("b.rs", "", "1\n2\n")]);
        app.layout(80, 40);
        // Folding from inside the file brings its header to the top.
        app.top.line = 2;
        app.toggle_fold(0);
        assert_eq!(app.top.line, 0);
        app.layout(80, 40);
        let offsets = |app: &App| app.stream.as_ref().expect("loaded").offsets.clone();
        // a: header + spacer; b: header + 2 rows + spacer.
        assert_eq!(offsets(&app), vec![0, 2, 6]);
        assert_eq!(app.line_at(1), Line::Spacer);
        assert_eq!(app.line_at(2), Line::Header { file: 1 });
        app.toggle_fold_all();
        app.layout(80, 40);
        assert_eq!(offsets(&app), vec![0, 2, 4]);
        app.toggle_fold_all();
        app.layout(80, 40);
        assert_eq!(offsets(&app), vec![0, 4, 8]);
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
        let changes = vec![
            change("0.rs", "", "new\n"),
            change("a.rs", "", &body),
            change("b.rs", "", &body),
        ];
        app.load(super::fixture::keys(), changes, None);
        assert_eq!(app.top.file, 2, "anchored to b.rs by path");
    }

    #[test]
    fn a_selection_follows_its_file_across_reloads_and_ends_with_it() {
        let mut app = app_with(&[("a.rs", "x\n", "y\n"), ("b.rs", "x\n", "y\nz\n")]);
        app.layout(80, 40);
        let pos = |file| Pos {
            file,
            side: review::Side::New,
            line: 0,
        };
        app.mode = Mode::Visual(pos(1));
        // A file sorting in ahead keeps the selection on b.rs.
        app.apply_paths(&["a0.rs".to_owned()], vec![change("a0.rs", "", "n\n")]);
        assert_eq!(app.visual(), Some(&pos(2)));
        // b.rs reverted: the selection ends, and commenting cannot panic.
        app.apply_paths(&["b.rs".to_owned()], Vec::new());
        app.layout(80, 40);
        assert!(matches!(app.mode, Mode::Normal), "{:?}", app.mode);
        app.begin_comment(Scope::Lines);
    }

    #[test]
    fn a_second_viewer_shows_the_review_read_only_until_the_first_is_gone() {
        let dir = std::env::temp_dir().join(format!("dots-diff-viewers-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut app = app_with(&[("a.rs", "a\n", "b\n")]);
        app.layout(80, 20);
        let first = review::Lock::take(&dir).expect("lock").expect("free");
        let message = app
            .session
            .install(Path::new("/r"), Ok((dir.clone(), review::State::default())));
        assert_eq!(message.as_deref(), Some(review::READ_ONLY));
        app.toggle_reviewed();
        assert!(app.session.state().marks.is_empty(), "read only");
        assert!(
            app.message
                .as_deref()
                .is_some_and(|m| m.contains("read-only"))
        );
        // The first viewer saved a draft and exited.
        let mut saved = review::State::default();
        saved.add(
            review::Place::Review,
            review::Kind::Comment,
            "first".into(),
            None,
        );
        saved.save(&dir).expect("save");
        drop(first);
        app.toggle_reviewed();
        let state = app.session.state();
        assert_eq!(state.marks.len(), 1, "writable now");
        assert_eq!(state.comments.len(), 1, "with the saved review");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn auto_layout_splits_wide_panes() {
        let app = app_with(&[]);
        assert_eq!(app.effective_layout(120), Layout::Unified);
        assert_eq!(app.effective_layout(SPLIT_MIN_WIDTH), Layout::Split);
    }
}
