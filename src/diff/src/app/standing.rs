//! Which worktrees wait for review: who works in each, whether its changes
//! are new since the user last looked, and the order the switcher and the
//! next-to-review keys follow.

use std::time::SystemTime;

use super::App;
use crate::agents::{Owners, Snapshot};

/// Where a worktree stands, in the order the switcher lists them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Standing {
    /// Changes the user has not looked at, and no worker still at them.
    Waiting,
    /// A worker, or an interactive session, works there.
    Active,
    Idle,
}

impl App {
    /// Takes newly read sessions and workers. Returns a problem reading
    /// them that the user has not been told about.
    pub(crate) fn set_agents(&mut self, snapshot: Snapshot) -> Option<String> {
        let problem = (snapshot.problems != self.agents.problems)
            .then(|| snapshot.problems.first().map(|p| format!("Pi agents: {p}")))
            .flatten();
        self.agents = snapshot;
        self.match_owners();
        problem
    }

    /// Matches the sessions and workers to the worktrees again, after
    /// either changed.
    pub(crate) fn match_owners(&mut self) {
        let paths: Vec<_> = self.repo.worktrees.iter().map(|w| w.path.clone()).collect();
        self.owners = self.agents.owners(&paths);
    }

    /// Who works in worktree `i`.
    pub(crate) fn owners(&self, i: usize) -> Option<&Owners> {
        self.owners.get(i)
    }

    /// The content name the user last looked at in worktree `i`: the shown
    /// review's for the shown worktree, else its summary's.
    fn looked(&self, i: usize) -> Option<&str> {
        let path = &self.repo.worktrees.get(i)?.path;
        if self.session.shows(path) {
            return self.session.state().looked.as_deref();
        }
        self.summaries.get(path)?.looked.as_deref()
    }

    /// Whether worktree `i` has changes the user has not looked at.
    pub(crate) fn unseen(&self, i: usize) -> bool {
        let Some(s) = self
            .repo
            .worktrees
            .get(i)
            .and_then(|w| self.summaries.get(&w.path))
        else {
            return false;
        };
        s.files > 0 && s.content.is_some() && s.content.as_deref() != self.looked(i)
    }

    pub(crate) fn standing(&self, i: usize) -> Standing {
        let owners = self.owners(i);
        if owners.is_some_and(Owners::busy) {
            Standing::Active
        } else if self.unseen(i) {
            Standing::Waiting
        } else if owners.is_some_and(|o| !o.sessions.is_empty()) {
            Standing::Active
        } else {
            Standing::Idle
        }
    }

    /// When worktree `i` last changed: its HEAD or index, or an
    /// uncommitted file.
    pub(crate) fn changed_at(&self, i: usize) -> Option<SystemTime> {
        let w = self.repo.worktrees.get(i)?;
        let file = self.summaries.get(&w.path).and_then(|s| s.changed);
        w.touched.max(file)
    }

    /// The worktrees as the switcher lists them: waiting for review, then
    /// active, then the rest, each most recently changed first.
    pub(crate) fn review_order(&self) -> Vec<usize> {
        let mut order: Vec<usize> = (0..self.repo.worktrees.len()).collect();
        order.sort_by_cached_key(|&i| {
            (
                self.standing(i),
                std::cmp::Reverse(self.changed_at(i)),
                self.repo.worktrees[i].branch.clone(),
            )
        });
        order
    }

    /// The next worktree waiting for review after the shown one in review
    /// order, or with `forward` false the one before it, cycling.
    pub(crate) fn next_to_review(&self, forward: bool) -> Option<usize> {
        let order = self.review_order();
        let n = order.len();
        let at = order.iter().position(|&i| i == self.current).unwrap_or(0);
        (1..=n)
            .map(|d| {
                if forward {
                    order[(at + d) % n]
                } else {
                    order[(at + n - d % n) % n]
                }
            })
            .find(|&i| i != self.current && self.standing(i) == Standing::Waiting)
    }

    /// Records that the user looked at the shown worktree's changes, named
    /// by `content`.
    pub(crate) fn look(&mut self, content: &str) {
        if self.session.state().looked.as_deref() != Some(content) {
            self.session.edit().looked = Some(content.to_owned());
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::time::Duration;

    use super::*;
    use crate::agents::{Status, Worker};
    use crate::app::{Settings, Summary};
    use crate::repo::{Repo, Worktree};
    use crate::theme::{Detected, Palette};

    fn at(secs: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(secs)
    }

    fn worker(cwd: &str, status: Status) -> Worker {
        Worker {
            id: format!("w-{cwd}"),
            label: "w".into(),
            status,
            cwd: cwd.into(),
            model: None,
            owner_session: None,
            session_file: None,
            updated_at: 1,
        }
    }

    fn summary(content: &str, looked: Option<&str>, changed: u64) -> Summary {
        Summary {
            added: 1,
            removed: 0,
            files: 1,
            content: Some(content.into()),
            changed: Some(at(changed)),
            looked: looked.map(Into::into),
        }
    }

    /// Worktrees `/w/a` … `/w/e`.
    fn app() -> App {
        let wt = |b: &str| Worktree {
            path: PathBuf::from(format!("/w/{b}")),
            branch: b.into(),
            main: false,
            touched: Some(at(1)),
        };
        let repo = Repo {
            worktrees: ["a", "b", "c", "d", "e"].map(wt).to_vec(),
        };
        let mut app = App::new(
            repo,
            0,
            Palette::new(Detected::light()),
            Settings::default(),
        );
        let s = &mut app.summaries;
        // a: looked at; b: new, older; c: new, but a worker is at it;
        // d: a finished worker's changes, newest; e: nothing.
        s.insert("/w/a".into(), summary("A", Some("A"), 50));
        s.insert("/w/b".into(), summary("B2", Some("B1"), 10));
        s.insert("/w/c".into(), summary("C", None, 60));
        s.insert("/w/d".into(), summary("D", None, 40));
        app.set_agents(Snapshot {
            agents: Vec::new(),
            workers: vec![
                worker("/w/c", Status::Running),
                worker("/w/d", Status::Done),
            ],
            problems: Vec::new(),
        });
        app
    }

    #[test]
    fn waiting_worktrees_come_first_then_active_then_the_rest() {
        let app = app();
        let standing: Vec<Standing> = (0..5).map(|i| app.standing(i)).collect();
        assert_eq!(
            standing,
            [
                Standing::Idle,
                Standing::Waiting,
                Standing::Active,
                Standing::Waiting,
                Standing::Idle
            ]
        );
        // Within a group, the most recent change first.
        assert_eq!(app.review_order(), [3, 1, 2, 0, 4]);
    }

    #[test]
    fn looking_at_the_shown_worktree_records_its_content() {
        let mut app = app();
        app.current = 1;
        app.session.install(
            std::path::Path::new("/w/b"),
            Err("no review in a fixture".into()),
        );
        assert!(app.unseen(1), "the shown review never looked");
        app.look("B2");
        assert!(!app.unseen(1));
        assert_eq!(app.session.state().looked.as_deref(), Some("B2"));
        assert_eq!(app.review_order()[0], 3);
    }

    #[test]
    fn next_to_review_cycles_through_waiting_worktrees_in_order() {
        let mut app = app();
        // Order: d, b (waiting), c (active), a, e.
        app.current = 0;
        assert_eq!(app.next_to_review(true), Some(3));
        assert_eq!(app.next_to_review(false), Some(1));
        app.current = 3;
        assert_eq!(app.next_to_review(true), Some(1));
        assert_eq!(app.next_to_review(false), Some(1), "only one other");
        app.current = 1;
        assert_eq!(app.next_to_review(true), Some(3), "wraps");
        app.summaries.get_mut(Path::new("/w/d")).expect("d").looked = Some("D".into());
        assert_eq!(app.next_to_review(true), None, "the shown one is not next");
    }
}
