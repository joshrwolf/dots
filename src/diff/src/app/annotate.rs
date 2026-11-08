//! The review in the viewer: the diff cursor and selection, the comment
//! editor, cards, reviewed marks and agent notes.

use std::collections::BTreeMap;

use super::{App, CardRef, Line, Note, Pos};
use crate::cards;
use crate::diff::Text;
use crate::editor::{Buffer, Editor};
use crate::plan::{Kind, Row};
use crate::review::{self, Place, Side, content_key};

/// What a new comment is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Scope {
    /// The cursor line or the selection.
    Lines,
    /// Replacement code for the cursor line or the selection.
    Suggestion,
    File,
    Review,
}

/// Lines kept between the cursor and the edge of the view.
const CURSOR_MARGIN: u16 = 3;

fn text_lines(text: &Text) -> Vec<&str> {
    (0..text.len()).map(|i| text.line(i)).collect()
}

impl App {
    /// The stream line under the diff cursor.
    pub(crate) fn cursor_line(&self) -> u32 {
        self.global_top() + u32::from(self.cursor_row)
    }

    /// Keeps the cursor on screen, below the pinned header and above the
    /// end of the stream.
    pub(crate) fn clamp_cursor(&mut self, rows: u16) {
        let total = self.stream.as_ref().map_or(0, super::Stream::total);
        let min = self.settings.header_rows.min(rows.saturating_sub(1));
        let last = total.saturating_sub(1).saturating_sub(self.global_top());
        let max = u16::try_from(last)
            .unwrap_or(u16::MAX)
            .min(rows.saturating_sub(1))
            .max(min);
        self.cursor_row = self.cursor_row.clamp(min, max);
    }

    /// Moves the cursor `delta` lines, scrolling to keep it a few lines from
    /// the edges.
    pub(crate) fn move_cursor(&mut self, delta: i64, rows: u16) {
        let Some(stream) = &self.stream else { return };
        let last = i64::from(stream.total().saturating_sub(1));
        let target = (i64::from(self.cursor_line()) + delta).clamp(0, last);
        let target = u32::try_from(target).unwrap_or(0);
        let header = u32::from(self.settings.header_rows);
        let margin = u32::from(CURSOR_MARGIN.min(rows / 4));
        let top = self.global_top();
        if target < top + header + margin {
            self.scroll_to(target.saturating_sub(header + margin), rows);
        } else if target + margin >= top + u32::from(rows) {
            self.scroll_to(target + margin + 1 - u32::from(rows), rows);
        }
        let row = target.saturating_sub(self.global_top());
        self.cursor_row = u16::try_from(row).unwrap_or(u16::MAX);
        self.clamp_cursor(rows);
        self.sync_files_cursor_to(self.cursor_file());
    }

    fn sync_files_cursor_to(&mut self, file: Option<usize>) {
        if let Some(row) = file.and_then(|f| self.row_of_file(f)) {
            self.cursor = row;
        }
    }

    /// The file the cursor is in.
    pub(crate) fn cursor_file(&self) -> Option<usize> {
        let stream = self.stream.as_ref()?;
        if stream.files.is_empty() {
            return None;
        }
        let line = self.cursor_line();
        Some(
            stream
                .offsets
                .partition_point(|&o| o <= line)
                .saturating_sub(1)
                .min(stream.files.len() - 1),
        )
    }

    /// The source line under the cursor: on a split row, the new side
    /// unless the old one is chosen or the new one is blank.
    pub(crate) fn cursor_pos(&self) -> Option<Pos> {
        let Line::Body { file, row, .. } = self.line_at(self.cursor_line()) else {
            return None;
        };
        let f = &self.stream.as_ref()?.files[file];
        let (side, line) = match *f.rows().get(row)? {
            Row::Line {
                kind: Kind::Delete,
                old: Some(o),
                ..
            } => (Side::Old, o),
            Row::Line { new: Some(n), .. } => (Side::New, n),
            Row::Pair { old, new, .. } => match (old, new) {
                (Some(o), _) if self.cursor_old || new.is_none() => (Side::Old, o),
                (_, Some(n)) => (Side::New, n),
                _ => return None,
            },
            _ => return None,
        };
        Some(Pos { file, side, line })
    }

    /// The card under the cursor.
    pub(crate) fn cursor_card(&self) -> Option<CardRef> {
        let stream = self.stream.as_ref()?;
        let hit = |placed: &[cards::Placed], row: u32, line: u32| {
            placed
                .iter()
                .find(|p| p.row == row && (p.start..p.start + p.height).contains(&line))
                .map(|p| p.card)
        };
        match self.line_at(self.cursor_line()) {
            Line::Cards {
                review: true, line, ..
            } => hit(&self.review_cards, 0, line),
            Line::Cards { file, line, .. } => hit(&stream.files[file].file_cards, 0, line),
            Line::Body { file, row, sub } => {
                hit(&stream.files[file].cards, u32::try_from(row).ok()?, sub)
            }
            _ => None,
        }
    }

    /// The lines a comment would cover: the selection, or the cursor line.
    /// A selection stays in its anchor's file and side: its far end is the
    /// line of that side nearest the cursor, between it and the anchor.
    pub(crate) fn selection(&self) -> Option<(Pos, u32)> {
        let Some(v) = &self.visual else {
            return self.cursor_pos().map(|at| {
                let line = at.line;
                (at, line)
            });
        };
        let end = self.selection_end(v).unwrap_or(v.line);
        let start = Pos {
            line: v.line.min(end),
            ..v.clone()
        };
        Some((start, v.line.max(end)))
    }

    /// The far end of the selection anchored at `v`.
    fn selection_end(&self, v: &Pos) -> Option<u32> {
        let rows = self.stream.as_ref()?.files.get(v.file)?.rows();
        let on_side = |r: &Row| row_line(r, v.side);
        let first = || rows.iter().find_map(on_side);
        let last = || rows.iter().rev().find_map(on_side);
        let file = self.cursor_file()?;
        if file != v.file {
            return if file < v.file { first() } else { last() };
        }
        match self.line_at(self.cursor_line()) {
            Line::Body { row, .. } => {
                let before = rows.get(..=row)?.iter().rev().find_map(on_side);
                let after = rows.get(row..)?.iter().find_map(on_side);
                match (before, after) {
                    (Some(b), _) if b >= v.line => Some(b),
                    (_, Some(a)) if a <= v.line => Some(a),
                    (b, a) => b.or(a),
                }
            }
            Line::Header { .. } | Line::Cards { .. } => first(),
            _ => last(),
        }
    }

    /// Starts or ends a visual line selection at the cursor.
    pub(crate) fn toggle_visual(&mut self) {
        self.visual = match self.visual {
            Some(_) => None,
            None => self.cursor_pos(),
        };
        if self.visual.is_none() && self.cursor_pos().is_none() {
            self.message = Some("move to a code line to select".to_owned());
        }
    }

    /// Whether this viewer may change the review. It may once it holds the
    /// review, taking it with the saved review from a viewer that is gone.
    pub(crate) fn review_writable(&mut self) -> bool {
        let Some(dir) = &self.review_dir else {
            // Not loaded, so never saved.
            return true;
        };
        if self.review_lock.is_some() {
            return true;
        }
        match review::Lock::take_state(dir) {
            Ok(Some((lock, state))) => {
                self.review = state;
                self.review_lock = Some(lock);
                self.refresh_marks();
                self.cards_changed();
                true
            }
            Ok(None) => {
                self.message = Some(review::READ_ONLY.to_owned());
                false
            }
            Err(e) => {
                self.message = Some(format!("review is read-only: {e:#}"));
                false
            }
        }
    }

    /// Opens the editor for a new comment.
    pub(crate) fn begin_comment(&mut self, scope: Scope) {
        if !self.review_writable() {
            return;
        }
        match self.new_editor(scope) {
            Ok(editor) => self.editor = Some(editor),
            Err(e) => self.message = Some(e),
        }
    }

    fn new_editor(&self, scope: Scope) -> Result<Editor, String> {
        let stream = self.stream.as_ref().ok_or("nothing loaded yet")?;
        let editor = |place, text: &str, suggestion| Editor {
            buffer: Buffer::new(text),
            kind: review::Kind::Comment,
            place,
            suggestion,
            editing: None,
        };
        match scope {
            Scope::Review => Ok(editor(Place::Review, "", false)),
            Scope::File => {
                let file = match self.focus {
                    super::Focus::Files => {
                        self.list.get(self.cursor).map(super::ListItem::first_file)
                    }
                    super::Focus::Diff => self.cursor_file(),
                }
                .ok_or("no file here")?;
                let path = stream
                    .files
                    .get(file)
                    .ok_or("no file here")?
                    .change
                    .path
                    .clone();
                Ok(editor(Place::File { path }, "", false))
            }
            Scope::Lines | Scope::Suggestion => {
                let (start, end) = self
                    .selection()
                    .ok_or("move to a code line, or press C for the whole file")?;
                if self.since && start.side == Side::Old {
                    return Err("since the last send, comment on the new side".to_owned());
                }
                let f = stream
                    .files
                    .get(start.file)
                    .ok_or("the selection is gone")?;
                let texts = self.texts(f).ok_or("the file is still loading")?;
                let text = if start.side == Side::Old {
                    texts.old.1
                } else {
                    texts.new.1
                };
                let anchored: Vec<String> = (start.line..=end)
                    .map(|i| text.line(i).to_owned())
                    .collect();
                let prefill = if scope == Scope::Suggestion {
                    anchored.join("\n")
                } else {
                    String::new()
                };
                let place = Place::Lines {
                    path: f.change.path.clone(),
                    lines: review::Lines {
                        side: start.side,
                        start: start.line,
                        end,
                        text: anchored,
                    },
                };
                Ok(editor(place, &prefill, scope == Scope::Suggestion))
            }
        }
    }

    /// Opens the editor on a draft.
    pub(crate) fn edit_card(&mut self, card: CardRef) {
        let CardRef::Comment(id) = card else {
            self.message = Some("notes are the agent's; d dismisses one".to_owned());
            return;
        };
        if !self.review_writable() {
            return;
        }
        let Some(c) = self.review.comments.iter().find(|c| c.id == id) else {
            return;
        };
        if c.sent {
            self.message = Some("sent comments are not edited; add another".to_owned());
            return;
        }
        let (text, suggestion) = match &c.suggestion {
            Some(code) => (code.as_str(), true),
            None => (c.text.as_str(), false),
        };
        self.editor = Some(Editor {
            buffer: Buffer::new(text),
            kind: c.kind,
            place: c.place.clone(),
            suggestion,
            editing: Some(id),
        });
    }

    pub(crate) fn delete_card(&mut self, card: CardRef) {
        match card {
            CardRef::Comment(id) => {
                if self.review_writable() {
                    self.review.comments.retain(|c| c.id != id);
                    self.review_dirty = true;
                }
            }
            CardRef::Note(id) => self.notes.retain(|n| n.id != id),
        }
        self.cards_changed();
    }

    /// Saves the editor's comment, or drops a new one left empty. Returns
    /// the comment saved.
    pub(crate) fn save_editor(&mut self) -> Option<u64> {
        let editor = self.editor.take()?;
        self.visual = None;
        let text = editor.buffer.text();
        let empty = text.trim().is_empty() && !editor.suggestion;
        let id = match editor.editing {
            Some(id) if empty => {
                self.review.comments.retain(|c| c.id != id);
                None
            }
            Some(id) => {
                let c = self.review.comments.iter_mut().find(|c| c.id == id)?;
                if editor.suggestion {
                    c.suggestion = Some(text);
                } else {
                    text.trim_end().clone_into(&mut c.text);
                }
                c.kind = editor.kind;
                Some(id)
            }
            None if empty => None,
            None => {
                let content = self.anchor_content(&editor.place);
                let (text, suggestion) = if editor.suggestion {
                    (String::new(), Some(text))
                } else {
                    (text.trim_end().to_owned(), None)
                };
                let id = self.review.add(editor.place, editor.kind, text, suggestion);
                if let Some(c) = self.review.comments.iter_mut().find(|c| c.id == id) {
                    c.content = content;
                }
                Some(id)
            }
        };
        self.review_dirty = true;
        self.cards_changed();
        id
    }

    /// The content id of the side a place's lines are on.
    fn anchor_content(&self, place: &Place) -> Option<String> {
        let Place::Lines { path, lines } = place else {
            return None;
        };
        let f = self
            .stream
            .as_ref()?
            .files
            .iter()
            .find(|f| f.change.path == *path)?;
        match lines.side {
            Side::Old => Some(content_key(f.change.old)),
            Side::New => f.content_key(),
        }
    }

    /// Lays cards out again after they changed.
    pub(crate) fn cards_changed(&mut self) {
        self.laid_out = None;
        if let Some(stream) = &mut self.stream {
            for f in &mut stream.files {
                f.key = None;
            }
        }
    }

    /// Drafts and sent comments per file path, for the sidebar.
    pub(crate) fn draft_counts(&self) -> BTreeMap<&str, usize> {
        let mut out = BTreeMap::new();
        for c in self.review.drafts() {
            if let Some(path) = c.place.path() {
                *out.entry(path).or_default() += 1;
            }
        }
        out
    }

    /// Marks the cursor's file reviewed, or unmarks it.
    pub(crate) fn toggle_reviewed(&mut self) {
        if !self.review_writable() {
            return;
        }
        let file = match self.focus {
            super::Focus::Files => self.list.get(self.cursor).map(super::ListItem::first_file),
            super::Focus::Diff => self.cursor_file(),
        };
        let Some(f) = file.and_then(|i| self.stream.as_mut()?.files.get_mut(i)) else {
            return;
        };
        let Some(content) = f.content_key() else {
            self.message = Some("wait for the file's diff".to_owned());
            return;
        };
        f.reviewed = self.review.toggle_reviewed(&f.change.path, &content);
        self.review_dirty = true;
    }

    /// Sets every file's reviewed flag from the marks.
    pub(crate) fn refresh_marks(&mut self) {
        let Some(stream) = &mut self.stream else {
            return;
        };
        for f in &mut stream.files {
            f.reviewed = f
                .content_key()
                .is_some_and(|c| self.review.reviewed(&f.change.path, &c));
        }
    }

    /// After file `i`'s diff arrived: its reviewed flag, comments that
    /// follow their lines, and notes that go when the content changed.
    pub(super) fn after_body(&mut self, i: usize, texts: Option<&super::FileTexts>) {
        let since = self.since;
        let Some(stream) = &mut self.stream else {
            return;
        };
        let worktree = &stream.worktree;
        let Some(f) = stream.files.get_mut(i) else {
            return;
        };
        let content = f.content_key();
        f.reviewed = content
            .as_deref()
            .is_some_and(|c| self.review.reviewed(&f.change.path, c));
        let path = f.change.path.as_str();
        let mut changed = false;
        if let Some(content) = content {
            let before = self.notes.len();
            self.notes.retain_mut(|n| {
                if n.worktree != *worktree || n.place.path() != Some(path) {
                    return true;
                }
                match &n.content {
                    None => {
                        n.content = Some(content.clone());
                        true
                    }
                    Some(c) => *c == content,
                }
            });
            changed |= self.notes.len() != before;
        }
        let anchored = self
            .review
            .comments
            .iter()
            .any(|c| matches!(&c.place, Place::Lines { path: p, .. } if p == path));
        if let (true, Some(texts)) = (anchored, texts) {
            let before = self.review.comments.clone();
            let new_key = content_key(texts.new.0);
            self.review
                .file_changed(path, Side::New, &new_key, &text_lines(&texts.new.1));
            if !since {
                let old_key = content_key(texts.old.0);
                self.review
                    .file_changed(path, Side::Old, &old_key, &text_lines(&texts.old.1));
            }
            if self.review.comments != before {
                self.review_dirty = true;
                changed = true;
            }
        }
        if changed {
            self.cards_changed();
        }
    }

    /// Adds an agent's note on `worktree`; its path is within it.
    pub(crate) fn add_note(&mut self, worktree: std::path::PathBuf, place: Place, text: String) {
        let content = place.path().and_then(|p| {
            self.stream
                .as_ref()
                .filter(|s| s.worktree == worktree)?
                .files
                .iter()
                .find(|f| f.change.path == p)?
                .content_key()
        });
        let id = self.next_note;
        self.next_note += 1;
        self.notes.push(Note {
            id,
            worktree,
            place,
            text,
            content,
            shown: false,
            seen: false,
        });
        self.cards_changed();
    }

    /// Clears the notes of one file, or all of them.
    pub(crate) fn clear_notes(&mut self, path: Option<&str>) {
        self.notes
            .retain(|n| path.is_some_and(|p| n.place.path() != Some(p)));
        self.cards_changed();
    }

    /// Notes on screen become shown; shown ones off screen become seen, and
    /// draw dimmed.
    pub(crate) fn mark_notes_seen(&mut self, rows: u16) {
        if self.notes.is_empty() {
            return;
        }
        let top = self.global_top();
        let mut visible = Vec::new();
        for line in top..top + u32::from(rows) {
            let saved = self.cursor_row;
            self.cursor_row = u16::try_from(line - top).unwrap_or(0);
            if let Some(CardRef::Note(id)) = self.cursor_card() {
                visible.push(id);
            }
            self.cursor_row = saved;
        }
        for n in &mut self.notes {
            if visible.contains(&n.id) {
                n.shown = true;
            } else if n.shown {
                n.seen = true;
            }
        }
    }

    /// Cards of the shown worktree in stream order: review-level, then by
    /// file and line.
    fn ordered_cards(&self) -> Vec<(usize, u32, CardRef)> {
        let Some(stream) = &self.stream else {
            return Vec::new();
        };
        let index = cards::index(&self.review, &self.notes, &stream.worktree);
        let mut out: Vec<(usize, u32, CardRef)> = index.review.iter().map(|&c| (0, 0, c)).collect();
        for (i, f) in stream.files.iter().enumerate() {
            for &card in index
                .by_path
                .get(f.change.path.as_str())
                .into_iter()
                .flatten()
            {
                let line = cards::get(&self.review, &self.notes, card)
                    .and_then(|c| c.place().lines().map(|l| l.end + 1))
                    .unwrap_or(0);
                out.push((i, line, card));
            }
        }
        out
    }

    /// Moves the cursor to the next or previous card.
    pub(crate) fn next_card(&mut self, forward: bool, rows: u16) {
        let order = self.ordered_cards();
        let here = self.cursor_card();
        let at = match here.and_then(|c| order.iter().position(|o| o.2 == c)) {
            Some(i) if forward => order.get(i + 1),
            Some(i) => i.checked_sub(1).and_then(|i| order.get(i)),
            None => {
                let file = self.cursor_file().unwrap_or(0);
                let line = self.cursor_pos().map_or(0, |p| p.line + 1);
                if forward {
                    order.iter().find(|o| (o.0, o.1) > (file, line))
                } else {
                    order.iter().rev().find(|o| (o.0, o.1) < (file, line))
                }
            }
        };
        let Some(&(file, _, card)) = at else {
            self.message = Some(
                if forward {
                    "no more cards below"
                } else {
                    "no more cards above"
                }
                .to_owned(),
            );
            return;
        };
        self.pending_card = Some(card);
        if !self.settle_card(rows) {
            self.jump_to_file(file, rows);
        }
    }

    /// Where a laid-out card starts in the stream.
    fn locate_card(&self, card: CardRef) -> Option<u32> {
        let stream = self.stream.as_ref()?;
        let header = u32::from(self.settings.header_rows);
        if let Some(p) = self.review_cards.iter().find(|p| p.card == card) {
            return Some(stream.offsets[0] + p.start);
        }
        for (i, f) in stream.files.iter().enumerate() {
            if let Some(p) = f.file_cards.iter().find(|p| p.card == card) {
                return Some(stream.offsets[i] + f.above + header + p.start);
            }
            if let Some(p) = f.cards.iter().find(|p| p.card == card) {
                return Some(
                    stream.offsets[i] + f.body_start(header) + f.heights.start(p.row) + p.start,
                );
            }
        }
        None
    }

    /// Brings the pending card into view with the cursor on it, once it is
    /// laid out. Returns whether it was.
    pub(super) fn settle_card(&mut self, rows: u16) -> bool {
        let Some(card) = self.pending_card else {
            return false;
        };
        let Some(line) = self.locate_card(card) else {
            return false;
        };
        self.pending_card = None;
        let top = self.global_top();
        if line < top + u32::from(self.settings.header_rows) || line >= top + u32::from(rows) {
            self.scroll_to(line.saturating_sub(u32::from(rows / 3)), rows);
        }
        self.cursor_row = u16::try_from(line.saturating_sub(self.global_top())).unwrap_or(0);
        self.clamp_cursor(rows);
        true
    }
}

/// The line of `side` that a row shows, if any.
pub(crate) fn row_line(row: &Row, side: Side) -> Option<u32> {
    match (*row, side) {
        (Row::Line { kind, old, .. }, Side::Old) if kind != Kind::Insert => old,
        (Row::Line { kind, new, .. }, Side::New) if kind != Kind::Delete => new,
        (Row::Pair { old, .. }, Side::Old) => old,
        (Row::Pair { new, .. }, Side::New) => new,
        _ => None,
    }
}
