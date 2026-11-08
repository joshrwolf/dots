//! The review's keys and I/O: the editor, the agent picker, saving the
//! review, resolving and delivering to the Pi session it is for, and agent
//! notes.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::{Msg, Runtime};
use crate::app::Line;
use crate::ctl::NoteSpec;
use crate::deliver::{self, Ack, Resolution, Why};
use crate::picker::{Batch, Picker};
use crate::plan::{Kind, Row};
use crate::repo::Reader;
use crate::review::{self, Comment, Place, Side};

/// A delivered review: how the session took it, the snapshot of what was
/// shown, and who received it.
#[derive(Debug)]
pub(crate) struct Delivered {
    ack: Ack,
    seen: BTreeMap<String, String>,
    to: String,
}

impl Runtime {
    /// Puts the cursor on the first changed line on screen.
    pub(super) fn cursor_to_change(&mut self, rows: u16) {
        let top = self.app.global_top();
        for r in self.app.settings.header_rows..rows {
            let Line::Body { file, row, sub: 0 } = self.app.line_at(top + u32::from(r)) else {
                continue;
            };
            let changed = self
                .app
                .stream
                .as_ref()
                .and_then(|s| s.files[file].rows().get(row).copied())
                .is_some_and(|row| {
                    matches!(
                        row,
                        Row::Line {
                            kind: Kind::Delete | Kind::Insert,
                            ..
                        } | Row::Pair { changed: true, .. }
                    )
                });
            if changed {
                self.app.cursor_row = r;
                return;
            }
        }
    }

    pub(super) fn editor_key(&mut self, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        let Some(editor) = &mut self.app.editor else {
            return;
        };
        let b = &mut editor.buffer;
        match (key.code, ctrl) {
            (KeyCode::Esc, _) | (KeyCode::Char('c'), true) => {
                self.app.editor = None;
            }
            (KeyCode::Enter | KeyCode::Char('s'), true) => {
                self.app.save_editor();
            }
            (KeyCode::Char('g'), true) => {
                if let Some(id) = self.app.save_editor() {
                    self.send(Batch::One(id));
                }
            }
            (KeyCode::Tab, _) => editor.kind = editor.kind.next(),
            (KeyCode::Enter, false) => b.newline(),
            (KeyCode::Backspace, _) if alt => b.delete_word(),
            (KeyCode::Char('w'), true) => b.delete_word(),
            (KeyCode::Char('h'), true) | (KeyCode::Backspace, _) => b.backspace(),
            (KeyCode::Char('u'), true) => b.delete_to_start(),
            (KeyCode::Char('d'), true) | (KeyCode::Delete, _) => b.delete(),
            (KeyCode::Left, _) | (KeyCode::Char('b'), true) => b.left(),
            (KeyCode::Right, _) | (KeyCode::Char('f'), true) => b.right(),
            (KeyCode::Up, _) | (KeyCode::Char('p'), true) => b.up(),
            (KeyCode::Down, _) | (KeyCode::Char('n'), true) => b.down(),
            (KeyCode::Home, _) | (KeyCode::Char('a'), true) => b.home(),
            (KeyCode::End, _) | (KeyCode::Char('e'), true) => b.end(),
            (KeyCode::Char(c), false) => b.insert(c.encode_utf8(&mut [0; 4])),
            _ => {}
        }
    }

    pub(super) fn open_picker(&mut self, then_send: Option<Batch>) {
        let found = deliver::agents(&deliver::inbox_dir());
        if found.agents.is_empty() {
            let mut message = "no interactive Pi session is running; drafts are kept".to_owned();
            if let Some(problem) = found.problems.first() {
                message = format!("{message} (unreadable registration {problem})");
            }
            self.app.message = Some(message);
            return;
        }
        self.app.picker = Some(Picker::new(found.agents, then_send));
    }

    pub(super) fn picker_key(&mut self, key: KeyEvent, ctrl: bool) {
        let Some(picker) = self.app.picker.as_mut() else {
            return;
        };
        match (key.code, ctrl) {
            (KeyCode::Esc, _) | (KeyCode::Char('g'), true) => self.app.picker = None,
            (KeyCode::Enter, _) => {
                let chosen = picker.selected().cloned();
                let then_send = picker.then_send;
                self.app.picker = None;
                let Some(agent) = chosen else { return };
                self.app.review.agent = Some(agent.session_id.clone());
                self.app.review_dirty = true;
                self.set_resolution(Resolution::Agent {
                    agent,
                    worker: None,
                    why: Why::Picked,
                });
                if let Some(batch) = then_send {
                    self.send(batch);
                }
            }
            (KeyCode::Down, _) | (KeyCode::Char('n' | 'j'), true) => picker.move_by(1),
            (KeyCode::Up, _) | (KeyCode::Char('p' | 'k'), true) => picker.move_by(-1),
            (KeyCode::Backspace, _) => picker.pop(),
            (KeyCode::Char('u'), true) => {
                while !picker.query.is_empty() {
                    picker.pop();
                }
            }
            (KeyCode::Char(c), false) => picker.push(c),
            _ => {}
        }
    }

    /// Sends every draft, or one comment, to the resolved session. With no
    /// single session, asks which.
    pub(super) fn send(&mut self, batch: Batch) {
        if self.sending {
            self.app.message = Some("still sending the last review".to_owned());
            return;
        }
        let comments: Vec<&Comment> = self
            .app
            .review
            .drafts()
            .filter(|c| batch.includes(c.id))
            .collect();
        if comments.is_empty() {
            self.app.message = Some("no drafts to send; c comments on a line".to_owned());
            return;
        }
        let (agent, worker) = match &self.resolution {
            Some(Resolution::Agent { agent, worker, .. }) => (agent.clone(), worker.clone()),
            Some(_) => {
                self.open_picker(Some(batch));
                return;
            }
            None => {
                self.app.message = Some("still looking for the agent; try again".to_owned());
                return;
            }
        };
        let (Some(stream), Some((repo, root))) = (&self.app.stream, self.source.clone()) else {
            return;
        };
        let wt = &self.app.repo.worktrees[self.app.current];
        let header = review::Header {
            branch: &wt.branch,
            worktree: &wt.path,
            base: &stream.base,
            worker: worker.as_ref().map(|w| (w.label.as_str(), w.id.as_str())),
        };
        let text = review::message(&header, &comments);
        let ids: Vec<u64> = comments.iter().map(|c| c.id).collect();
        let changes: Vec<_> = stream.files.iter().map(|f| f.change.clone()).collect();
        let seen = self.app.review.seen.clone();
        let worktree = stream.worktree.clone();
        let tx = self.tx.clone();
        self.sending = true;
        self.app.message = Some(format!("sending to {}…", agent.label()));
        std::thread::spawn(move || {
            let mut reader = Reader::new(repo.to_thread_local(), root);
            let mut seen = seen;
            for change in &changes {
                if let Ok(id) = reader.keep(change) {
                    seen.insert(change.path.clone(), review::content_key(id));
                }
            }
            let result = deliver::send(&agent, &text)
                .map(|ack| {
                    reader.prune_kept(&seen);
                    Delivered {
                        ack,
                        seen,
                        to: agent.label(),
                    }
                })
                .map_err(|e| format!("{e:#}"));
            let _ = tx.send(Msg::Sent {
                worktree,
                ids,
                result,
            });
        });
    }

    pub(super) fn on_sent(
        &mut self,
        worktree: &Path,
        ids: &[u64],
        result: Result<Delivered, String>,
    ) {
        self.sending = false;
        match result {
            Ok(d) => {
                if self.app.review_worktree.as_deref() == Some(worktree) {
                    self.app.review.sent(ids, d.seen);
                    self.app.review_dirty = true;
                    self.app.cards_changed();
                }
                let n = ids.len();
                let how = match d.ack {
                    Ack::Started => "it started on them",
                    Ack::Queued => "queued after its current turn",
                };
                self.app.message = Some(format!(
                    "sent {n} comment{} to {}: {how}",
                    if n == 1 { "" } else { "s" },
                    d.to
                ));
                if self.app.since {
                    self.start_load();
                }
            }
            Err(e) => {
                self.app.message = Some(format!("not sent, drafts kept: {e}"));
                self.resolution = None;
                self.spawn_resolve();
            }
        }
    }

    /// Finds who a review of the shown worktree goes to, off the event loop.
    pub(super) fn spawn_resolve(&self) {
        let worktree = self.app.worktree_path().clone();
        let canonical = |p: &Path| p.canonicalize().unwrap_or_else(|_| p.to_owned());
        let worktrees: Vec<PathBuf> = self
            .app
            .repo
            .worktrees
            .iter()
            .map(|w| canonical(&w.path))
            .collect();
        let bound = self
            .bound
            .as_ref()
            .filter(|(_, i)| *i == self.app.current)
            .map(|(b, _)| b.clone());
        let picked = self.app.review.agent.clone();
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let found = deliver::agents(&deliver::inbox_dir());
            let workers = deliver::workers().map_err(|e| format!("{e:#}"));
            let here = canonical(&worktree);
            let resolution = deliver::resolve(
                &found.agents,
                workers.as_deref().map_err(String::as_str),
                deliver::Ask {
                    worktree: &here,
                    worktrees: &worktrees,
                    bound: bound.as_deref(),
                    picked: picked.as_deref(),
                },
            );
            let _ = tx.send(Msg::Resolved {
                worktree,
                resolution,
                agents: found.agents,
                problems: found.problems,
            });
        });
    }

    pub(super) fn set_resolution(&mut self, resolution: Resolution) {
        self.app.target = match &resolution {
            Resolution::Agent { agent, worker, why } => {
                let mut t = format!("→ {} ({})", agent.label(), agent.pane());
                if let (Why::Parent, Some(w)) = (why, worker) {
                    t.push_str(" for ");
                    t.push_str(&w.label);
                }
                t
            }
            Resolution::Ambiguous(found) => format!("→ {} agents here: A to choose", found.len()),
            Resolution::None => "→ no agent: A to choose".to_owned(),
            Resolution::Unavailable(e) => format!("→ workers unreadable ({e}): A to choose"),
        };
        self.resolution = Some(resolution);
    }

    pub(super) fn save_review(&mut self) {
        self.app.review_dirty = false;
        let Some(dir) = &self.app.review_dir else {
            return;
        };
        if let Err(e) = self.app.review.save(dir) {
            self.app.message = Some(format!("review not saved: {e:#}"));
        }
    }

    /// Takes the saved review of a newly shown worktree; the one in memory
    /// stays while the same worktree reloads.
    pub(super) fn install_review(
        &mut self,
        worktree: &Path,
        loaded: Result<(PathBuf, review::State), String>,
    ) {
        if self.app.review_worktree.as_deref() == Some(worktree) {
            return;
        }
        if self.app.review_dirty {
            self.save_review();
        }
        self.app.review_worktree = Some(worktree.to_owned());
        self.resolution = None;
        "→ …".clone_into(&mut self.app.target);
        self.app.visual = None;
        match loaded {
            Ok((dir, state)) => {
                self.app.review = state;
                self.app.review_dir = Some(dir);
            }
            Err(e) => {
                self.app.review = review::State::default();
                self.app.review_dir = None;
                self.app.message = Some(format!("review not loaded, so not saved: {e}"));
            }
        }
    }

    /// A path from `ctl`: absolute, or within the shown worktree. Returns
    /// its worktree and the path within it.
    pub(super) fn relative(&self, path: &str) -> (PathBuf, String) {
        let current = self.app.worktree_path().clone();
        let p = Path::new(path);
        if !p.is_absolute() {
            return (current, path.to_owned());
        }
        let index = self.app.repo.index_of(p).unwrap_or(self.app.current);
        let root = self.app.repo.worktrees[index].path.clone();
        let canonical = |p: &Path| p.canonicalize().unwrap_or_else(|_| p.to_owned());
        let rel = canonical(p)
            .strip_prefix(canonical(&root))
            .map_or_else(|_| path.to_owned(), |r| r.to_string_lossy().into_owned());
        (root, rel)
    }

    /// Places an agent's note.
    pub(super) fn note(&mut self, spec: NoteSpec) {
        let (worktree, place) = match spec.file {
            None => (self.app.worktree_path().clone(), Place::Review),
            Some(file) => {
                let (worktree, path) = self.relative(&file);
                let place = match spec.line {
                    None => Place::File { path },
                    Some(line) => {
                        let start = line.saturating_sub(1);
                        Place::Lines {
                            path,
                            lines: review::Lines {
                                side: if spec.old { Side::Old } else { Side::New },
                                start,
                                end: spec.end.map_or(start, |e| e.saturating_sub(1).max(start)),
                                text: Vec::new(),
                            },
                        }
                    }
                };
                (worktree, place)
            }
        };
        self.app.add_note(worktree, place, spec.text);
    }
}
