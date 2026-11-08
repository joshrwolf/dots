//! The review this viewer writes: the saved review of the shown worktree,
//! this viewer's hold on it, and the notes agents placed. Every change and
//! save of the review goes through here.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::app::FileTexts;
use crate::diff::Text;
use crate::review::{self, Lock, Place, Side, State};

/// A note an agent placed for the user, gone when its file changes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Note {
    pub(crate) id: u64,
    pub(crate) worktree: PathBuf,
    pub(crate) place: Place,
    pub(crate) text: String,
    /// The content of its file when placed, once known.
    pub(crate) content: Option<String>,
    /// Shown on screen, and since scrolled out of view.
    pub(crate) shown: bool,
    pub(crate) seen: bool,
}

fn text_lines(text: &Text) -> Vec<&str> {
    (0..text.len()).map(|i| text.line(i)).collect()
}

#[derive(Debug)]
pub(crate) struct Session {
    state: State,
    /// The Git directory the review is saved in; `None` when it could not
    /// be read, so it is not overwritten.
    dir: Option<PathBuf>,
    /// This viewer's hold on the review; without it the review is read-only.
    lock: Option<Lock>,
    /// The review changed since it was last saved.
    dirty: bool,
    /// The worktree the review belongs to.
    worktree: Option<PathBuf>,
    /// Agent notes for this viewer's lifetime, on any worktree.
    notes: Vec<Note>,
    next_note: u64,
}

impl Default for Session {
    fn default() -> Self {
        Self {
            state: State::default(),
            dir: None,
            lock: None,
            dirty: false,
            worktree: None,
            notes: Vec::new(),
            next_note: 1,
        }
    }
}

impl Session {
    pub(crate) fn state(&self) -> &State {
        &self.state
    }

    /// The review, to change; it is saved at the next turn of the loop.
    /// Callers check [`Session::hold`] first.
    pub(crate) fn edit(&mut self) -> &mut State {
        self.dirty = true;
        &mut self.state
    }

    pub(crate) fn dirty(&self) -> bool {
        self.dirty
    }

    /// The Git directory the review is saved in.
    pub(crate) fn dir(&self) -> Option<&Path> {
        self.dir.as_deref()
    }

    /// Whether the review is `worktree`'s.
    pub(crate) fn shows(&self, worktree: &Path) -> bool {
        self.worktree.as_deref() == Some(worktree)
    }

    /// Makes sure this viewer may change the review, taking it with the
    /// saved review from a viewer that is gone. Returns whether the saved
    /// review replaced the one shown, or why the review is read-only.
    pub(crate) fn hold(&mut self) -> Result<bool, String> {
        let Some(dir) = &self.dir else {
            // Not loaded, so never saved.
            return Ok(false);
        };
        if self.lock.is_some() {
            return Ok(false);
        }
        match Lock::take_state(dir) {
            Ok(Some((lock, state))) => {
                self.state = state;
                self.lock = Some(lock);
                Ok(true)
            }
            Ok(None) => Err(review::READ_ONLY.to_owned()),
            Err(e) => Err(format!("review is read-only: {e:#}")),
        }
    }

    /// Saves the review if this viewer holds it; without the hold, changes
    /// such as re-anchoring stay in this viewer.
    pub(crate) fn save(&mut self) -> Result<(), String> {
        self.dirty = false;
        let (Some(dir), Some(_)) = (&self.dir, &self.lock) else {
            return Ok(());
        };
        self.state
            .save(dir)
            .map_err(|e| format!("review not saved: {e:#}"))
    }

    /// Takes the saved review of a newly shown worktree, after saving the
    /// one it replaces. Returns what the user should know.
    pub(crate) fn install(
        &mut self,
        worktree: &Path,
        loaded: Result<(PathBuf, State), String>,
    ) -> Option<String> {
        let mut message = None;
        if self.dirty {
            message = self.save().err();
        }
        self.worktree = Some(worktree.to_owned());
        self.lock = None;
        match loaded {
            Ok((dir, state)) => {
                // Saved since it was read off the event loop, or another
                // viewer's: read it again under the lock.
                self.state = match Lock::take_state(&dir) {
                    Ok(Some((lock, state))) => {
                        self.lock = Some(lock);
                        state
                    }
                    Ok(None) => {
                        message = Some(review::READ_ONLY.to_owned());
                        state
                    }
                    Err(e) => {
                        message = Some(format!("review is read-only: {e:#}"));
                        state
                    }
                };
                self.dir = Some(dir);
            }
            Err(e) => {
                self.state = State::default();
                self.dir = None;
                message = Some(format!("review not loaded, so not saved: {e}"));
            }
        }
        message
    }

    /// Records comments `ids` of `worktree`'s review, saved in `dir`, as
    /// sent. Returns whether that is the review shown.
    pub(crate) fn sent(
        &mut self,
        worktree: &Path,
        dir: Option<&Path>,
        ids: &[u64],
        seen: BTreeMap<String, String>,
    ) -> Result<bool, String> {
        if self.shows(worktree) {
            self.edit().sent(ids, seen);
            return Ok(true);
        }
        if let Some(dir) = dir {
            review::record_sent(dir, ids, seen).map_err(|e| format!("{e:#}"))?;
        }
        Ok(false)
    }

    /// Moves the comments on `path` along with its new texts; with `both`,
    /// on the old side too. Returns whether any moved or went outdated.
    pub(crate) fn file_changed(&mut self, path: &str, texts: &FileTexts, both: bool) -> bool {
        let anchored = self
            .state
            .comments
            .iter()
            .any(|c| matches!(&c.place, Place::Lines { path: p, .. } if p == path));
        if !anchored {
            return false;
        }
        let before = self.state.comments.clone();
        let new_key = review::content_key(texts.new.0);
        self.state
            .file_changed(path, Side::New, &new_key, &text_lines(&texts.new.1));
        if both {
            let old_key = review::content_key(texts.old.0);
            self.state
                .file_changed(path, Side::Old, &old_key, &text_lines(&texts.old.1));
        }
        let changed = self.state.comments != before;
        self.dirty |= changed;
        changed
    }

    pub(crate) fn notes(&self) -> &[Note] {
        &self.notes
    }

    /// Adds an agent's note on `worktree`, about its file's `content`.
    pub(crate) fn add_note(
        &mut self,
        worktree: PathBuf,
        place: Place,
        text: String,
        content: Option<String>,
    ) {
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
    }

    pub(crate) fn dismiss_note(&mut self, id: u64) {
        self.notes.retain(|n| n.id != id);
    }

    /// Clears the notes of one file, or all of them.
    pub(crate) fn clear_notes(&mut self, path: Option<&str>) {
        self.notes
            .retain(|n| path.is_some_and(|p| n.place.path() != Some(p)));
    }

    /// `worktree`'s file `path` has `content`: notes placed before it was
    /// known take it, and notes on other content go. Returns whether any
    /// went.
    pub(crate) fn note_content(&mut self, worktree: &Path, path: &str, content: &str) -> bool {
        let before = self.notes.len();
        self.notes.retain_mut(|n| {
            if n.worktree != worktree || n.place.path() != Some(path) {
                return true;
            }
            match &n.content {
                None => {
                    n.content = Some(content.to_owned());
                    true
                }
                Some(c) => c == content,
            }
        });
        self.notes.len() != before
    }

    /// Notes `visible` become shown; shown ones not visible become seen.
    pub(crate) fn notes_seen(&mut self, visible: &[u64]) {
        for n in &mut self.notes {
            if visible.contains(&n.id) {
                n.shown = true;
            } else if n.shown {
                n.seen = true;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::review::Kind;

    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("dots-diff-session-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch dir");
        dir
    }

    fn draft(session: &mut Session, text: &str) -> u64 {
        session
            .edit()
            .add(Place::Review, Kind::Comment, text.into(), None)
    }

    #[test]
    fn edits_are_saved_by_the_holder_and_survive_a_switch() {
        let dir = scratch("save");
        let mut session = Session::default();
        assert_eq!(
            session.install(Path::new("/wt/a"), Ok((dir.clone(), State::default()))),
            None
        );
        assert_eq!(session.hold(), Ok(false), "already held");
        draft(&mut session, "one");
        assert!(session.dirty());
        // Switching saves the review it replaces.
        let message = session.install(Path::new("/wt/b"), Err("unreadable".into()));
        assert!(message.is_some_and(|m| m.contains("not saved")));
        assert_eq!(State::load(&dir).expect("load").drafts().count(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_second_viewer_is_read_only_until_the_first_is_gone() {
        let dir = scratch("second");
        let mut first = Session::default();
        first.install(Path::new("/wt"), Ok((dir.clone(), State::default())));
        draft(&mut first, "first");
        first.save().expect("save");
        let mut second = Session::default();
        let message = second.install(Path::new("/wt"), Ok((dir.clone(), State::default())));
        assert_eq!(message.as_deref(), Some(review::READ_ONLY));
        assert_eq!(second.hold(), Err(review::READ_ONLY.to_owned()));
        // A read-only viewer's changes are never written.
        draft(&mut second, "second");
        second.save().expect("save");
        assert_eq!(State::load(&dir).expect("load").drafts().count(), 1);
        drop(first);
        assert_eq!(
            second.hold(),
            Ok(true),
            "the saved review replaces the one shown"
        );
        assert_eq!(second.state().drafts().count(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_review_that_could_not_be_read_is_never_saved() {
        let mut session = Session::default();
        let message = session.install(Path::new("/wt"), Err("bad json".into()));
        assert!(message.is_some_and(|m| m.contains("bad json")));
        assert_eq!(session.dir(), None);
        draft(&mut session, "x");
        assert_eq!(session.save(), Ok(()));
        assert!(!session.dirty());
    }

    #[test]
    fn notes_are_dismissed_by_id_and_cleared_by_path() {
        let mut session = Session::default();
        let file = |path: &str| Place::File { path: path.into() };
        for (path, text) in [("a.rs", "a"), ("b.rs", "b"), ("b.rs", "b2"), ("c.rs", "c")] {
            session.add_note(PathBuf::from("/wt"), file(path), text.into(), None);
        }
        let a = session.notes()[0].id;
        session.dismiss_note(a);
        session.clear_notes(Some("b.rs"));
        let left: Vec<_> = session.notes().iter().map(|n| n.text.as_str()).collect();
        assert_eq!(left, ["c"]);
        session.clear_notes(None);
        assert_eq!(session.notes().len(), 0);
    }
}
