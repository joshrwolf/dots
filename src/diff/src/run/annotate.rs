//! The review's keys and I/O: the editor, the agent picker, saving the
//! review, resolving and delivering to the Pi session it is for, and agent
//! notes.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use ratatui::termina::event::{KeyCode, KeyEvent, Modifiers};

use super::{Msg, Runtime};
use crate::app::{Line, Mode};
use crate::ctl::NoteSpec;
use crate::deliver::{self, Ack, Resolution, Why};
use crate::editor::Editor;
use crate::picker::{Batch, Picker};
use crate::plan::{Kind, Row};
use crate::repo::{self, Reader};
use crate::review::{self, Comment, Place, Side};

/// A delivered review: how the session took it, the snapshot of what was
/// shown, and who received it.
#[derive(Debug)]
pub(crate) struct Delivered {
    ack: Ack,
    seen: BTreeMap<String, String>,
    to: String,
}

#[cfg(test)]
impl Delivered {
    pub(crate) fn test(seen: BTreeMap<String, String>) -> Self {
        Self {
            ack: Ack::Started,
            seen,
            to: "agent".to_owned(),
        }
    }
}

/// Who reviews go to, and the send under way.
#[derive(Debug, Default)]
pub(super) struct Delivery {
    /// The Rex pane reviews go to while its worktree is shown.
    pub(super) bound: Option<(String, usize)>,
    /// Who a review goes to, once resolved.
    pub(super) resolution: Option<Resolution>,
    /// Numbers resolutions and picks, so an older result never replaces a
    /// newer one.
    pub(super) resolve_seq: u64,
    /// The live sessions.
    pub(super) agents: Vec<crate::agents::Agent>,
    /// A review is being delivered.
    pub(super) sending: bool,
}

impl Runtime {
    /// Puts the cursor on the first changed line on screen.
    pub(super) fn cursor_to_change(&mut self, rows: u16) {
        let top = self.app.global_top();
        for r in 1..rows {
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

    /// Keys while the comment editor is open; it is closed while they
    /// are handled.
    pub(super) fn editor_key(&mut self, mut editor: Editor, key: KeyEvent) {
        let ctrl = key.modifiers.contains(Modifiers::CONTROL);
        let alt = key.modifiers.contains(Modifiers::ALT);
        let b = &mut editor.buffer;
        match (key.code, ctrl) {
            (KeyCode::Escape, _) | (KeyCode::Char('c'), true) => return,
            (KeyCode::Enter | KeyCode::Char('s'), true) => {
                self.app.save_editor(editor);
                return;
            }
            (KeyCode::Char('g'), true) => {
                if let Some(id) = self.app.save_editor(editor) {
                    self.send(Batch::One(id));
                }
                return;
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
        self.app.mode = Mode::Editor(editor);
    }

    pub(super) fn open_picker(&mut self, then_send: Option<Batch>) {
        if !self.app.review_writable() {
            return;
        }
        let found = crate::agents::agents(&crate::agents::inbox_dir());
        if found.agents.is_empty() {
            let mut message = "no interactive Pi session is running; drafts are kept".to_owned();
            if let Some(problem) = found.problems.first() {
                message = format!("{message} (unreadable registration {problem})");
            }
            self.app.message = Some(message);
            return;
        }
        self.app.mode = Mode::Picker(Picker::new(found.agents, then_send));
    }

    /// Keys while the agent picker is open; it is closed while they are
    /// handled.
    pub(super) fn picker_key(&mut self, mut picker: Picker, key: KeyEvent, ctrl: bool) {
        match (key.code, ctrl) {
            (KeyCode::Escape, _) | (KeyCode::Char('g'), true) => return,
            (KeyCode::Enter, _) => {
                let Some(agent) = picker.selected().cloned() else {
                    return;
                };
                // Resolutions started before the pick do not replace it.
                self.delivery.resolve_seq += 1;
                self.app.session.edit().agent = Some(agent.session_id.clone());
                self.set_resolution(Resolution::Agent {
                    agent,
                    worker: None,
                    why: Why::Picked,
                });
                if let Some(batch) = picker.then_send {
                    self.send(batch);
                }
                return;
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
        self.app.mode = Mode::Picker(picker);
    }

    /// Sends every draft, or one comment, to the resolved session. With no
    /// single session, asks which.
    pub(super) fn send(&mut self, batch: Batch) {
        if self.delivery.sending {
            self.app.message = Some("still sending the last review".to_owned());
            return;
        }
        if !self.app.review_writable() {
            return;
        }
        let comments: Vec<&Comment> = self
            .app
            .session
            .state()
            .drafts()
            .filter(|c| batch.includes(c.id))
            .collect();
        if comments.is_empty() {
            self.app.message = Some("no drafts to send; c comments on a line".to_owned());
            return;
        }
        let (agent, worker) = match &self.delivery.resolution {
            Some(Resolution::Agent { agent, worker, .. }) => (agent.clone(), worker.clone()),
            Some(other) => {
                let gone = *other == Resolution::PickGone;
                self.open_picker(Some(batch));
                if gone && matches!(self.app.mode, Mode::Picker(_)) {
                    self.app.message =
                        Some("the session you picked has ended; choose another".to_owned());
                }
                return;
            }
            None => {
                self.app.message = Some("still looking for the agent; try again".to_owned());
                return;
            }
        };
        let (Some(stream), Some((repo, root))) = (&self.app.stream, self.pipeline.source.clone())
        else {
            return;
        };
        let wt = &self.app.repo.worktrees[self.app.current];
        let header = review::Header {
            branch: &wt.branch,
            worktree: &wt.path,
            base: &stream.base,
            worker: worker.as_ref().map(|w| review::Reviewed {
                label: &w.label,
                id: &w.id,
                status: w.status.label(),
                via: self
                    .app
                    .owning_worker(w)
                    .map(|o| (o.label.as_str(), o.id.as_str())),
            }),
        };
        let text = review::message(&header, &comments);
        let ids: Vec<u64> = comments.iter().map(|c| c.id).collect();
        let changes: Vec<_> = stream.files.iter().map(|f| f.change.clone()).collect();
        let target = stream.target;
        let seen = self.app.session.state().seen.clone();
        let worktree = stream.worktree.clone();
        let dir = self.app.session.dir().map(Path::to_owned);
        let tx = self.tx.clone();
        self.delivery.sending = true;
        self.app.message = Some(format!("sending to {}…", agent.label()));
        std::thread::spawn(move || {
            let mut reader = Reader::new(repo.to_thread_local(), root);
            let seen = review::seen_at_send(seen, &changes, target, &mut reader);
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
                dir,
                ids,
                result,
            });
        });
    }

    /// A send finished. `dir` holds the review it was sent from, which the
    /// user may have switched away from since.
    pub(super) fn on_sent(
        &mut self,
        worktree: &Path,
        dir: Option<&Path>,
        ids: &[u64],
        result: Result<Delivered, String>,
    ) {
        self.delivery.sending = false;
        match result {
            Ok(d) => {
                let n = ids.len();
                let how = match d.ack {
                    Ack::Started => "it started on them",
                    Ack::Queued => "queued after its current turn",
                };
                let mut message = format!(
                    "sent {n} comment{} to {}: {how}",
                    if n == 1 { "" } else { "s" },
                    d.to
                );
                match self.app.session.sent(worktree, dir, ids, d.seen) {
                    Ok(true) => self.app.cards_changed(),
                    Ok(false) => {}
                    Err(e) => {
                        let _ = write!(
                            message,
                            ", but not recorded there, so they stay drafts: {e}"
                        );
                    }
                }
                self.app.message = Some(message);
                if self.app.settings.since {
                    self.start_load();
                }
            }
            Err(e) => {
                self.app.message = Some(format!("not sent, drafts kept: {e}"));
                self.delivery.resolution = None;
                self.spawn_resolve();
            }
        }
    }

    /// Finds who a review of the shown worktree goes to, off the event loop.
    pub(super) fn spawn_resolve(&mut self) {
        self.delivery.resolve_seq += 1;
        let seq = self.delivery.resolve_seq;
        let worktree = self.app.worktree_path().clone();
        let worktrees: Vec<PathBuf> = self
            .app
            .repo
            .worktrees
            .iter()
            .map(|w| repo::canonical(&w.path))
            .collect();
        let bound = self
            .delivery
            .bound
            .as_ref()
            .filter(|(_, i)| *i == self.app.current)
            .map(|(b, _)| b.clone());
        let picked = self.app.session.state().agent.clone();
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let found = crate::agents::agents(&crate::agents::inbox_dir());
            let workers = crate::agents::workers().map_err(|e| format!("{e:#}"));
            let here = repo::canonical(&worktree);
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
                seq,
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
            Resolution::PickGone => "→ the picked session ended: A to choose".to_owned(),
        };
        self.delivery.resolution = Some(resolution);
    }

    pub(super) fn save_review(&mut self) {
        if let Err(e) = self.app.session.save() {
            self.app.message = Some(e);
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
        let rel = repo::canonical(p)
            .strip_prefix(repo::canonical(&root))
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
