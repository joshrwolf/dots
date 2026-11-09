//! The worktree switcher: the worktrees in review order, filtered fuzzily
//! by branch name and the agents working in them.

use std::time::{Duration, SystemTime};

use nucleo::pattern::{CaseMatching, Normalization, Pattern};
use nucleo::{Config, Matcher, Utf32Str};

use crate::app::App;

#[derive(Debug, Clone, Default)]
pub(crate) struct Switcher {
    pub(crate) query: String,
    /// Position in `matches`.
    pub(crate) cursor: usize,
    pub(crate) matches: Vec<Match>,
}

/// A worktree that passes the filter, with the matched character positions
/// of its branch name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Match {
    pub(crate) worktree: usize,
    pub(crate) positions: Vec<u32>,
}

impl Switcher {
    /// Opens on the first worktree in review order other than the shown one.
    pub(crate) fn new(app: &App) -> Self {
        let mut s = Self::default();
        s.refilter(app);
        s.cursor = s
            .matches
            .iter()
            .position(|m| m.worktree != app.current)
            .unwrap_or(0);
        s
    }

    pub(crate) fn push(&mut self, app: &App, c: char) {
        self.query.push(c);
        self.refilter(app);
    }

    pub(crate) fn pop(&mut self, app: &App) {
        self.query.pop();
        self.refilter(app);
    }

    pub(crate) fn move_by(&mut self, delta: i64) {
        let n = i64::try_from(self.matches.len()).unwrap_or(i64::MAX);
        if n > 0 {
            let at = i64::try_from(self.cursor).unwrap_or(0) + delta;
            self.cursor = usize::try_from(at.clamp(0, n - 1)).unwrap_or(0);
        }
    }

    pub(crate) fn selected(&self) -> Option<usize> {
        self.matches.get(self.cursor).map(|m| m.worktree)
    }

    /// Best matches first, in review order among equals. A worktree also
    /// matches by the names of the agents working in it.
    fn refilter(&mut self, app: &App) {
        let mut matcher = Matcher::new(Config::DEFAULT);
        let pattern = Pattern::parse(&self.query, CaseMatching::Smart, Normalization::Smart);
        let mut buf = Vec::new();
        let mut scored: Vec<(u32, Match)> = app
            .review_order()
            .into_iter()
            .filter_map(|i| {
                let branch = &app.repo.worktrees[i].branch;
                let mut haystack = branch.clone();
                for name in app
                    .owners(i)
                    .map(crate::agents::Owners::names)
                    .unwrap_or_default()
                {
                    haystack.push_str("  ");
                    haystack.push_str(&name);
                }
                let mut positions = Vec::new();
                let score = pattern.indices(
                    Utf32Str::new(&haystack, &mut buf),
                    &mut matcher,
                    &mut positions,
                )?;
                let in_branch = u32::try_from(branch.chars().count()).unwrap_or(u32::MAX);
                positions.retain(|&p| p < in_branch);
                positions.sort_unstable();
                positions.dedup();
                Some((
                    score,
                    Match {
                        worktree: i,
                        positions,
                    },
                ))
            })
            .collect();
        // Stable, so review order holds among equal scores.
        scored.sort_by_key(|(score, _)| std::cmp::Reverse(*score));
        self.matches = scored.into_iter().map(|(_, m)| m).collect();
        self.cursor = self.cursor.min(self.matches.len().saturating_sub(1));
    }
}

/// How long ago `then` was, in one short unit: `40s`, `5m`, `3h`, `2d`, `6w`.
pub(crate) fn age(then: SystemTime, now: SystemTime) -> String {
    let secs = now.duration_since(then).unwrap_or(Duration::ZERO).as_secs();
    match secs {
        0..60 => format!("{secs}s"),
        60..3600 => format!("{}m", secs / 60),
        3600..86_400 => format!("{}h", secs / 3600),
        86_400..1_209_600 => format!("{}d", secs / 86_400),
        _ => format!("{}w", secs / 604_800),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::{Snapshot, Status, Worker};
    use crate::app::Settings;
    use crate::repo::{Repo, Worktree};
    use crate::theme::{Detected, Palette};

    fn app(current: usize) -> App {
        let wt = |branch: &str, mins_ago: u64| Worktree {
            path: format!("/w/{branch}").into(),
            branch: branch.into(),
            main: false,
            touched: Some(SystemTime::UNIX_EPOCH + Duration::from_secs(10_000 - mins_ago * 60)),
        };
        let repo = Repo {
            worktrees: vec![wt("main", 50), wt("fix-parser", 5), wt("feature-wrap", 1)],
        };
        let mut app = App::new(
            repo,
            current,
            Palette::new(Detected::light()),
            Settings::default(),
        );
        app.set_agents(Snapshot {
            agents: Vec::new(),
            workers: vec![Worker {
                id: "w-1".into(),
                label: "lexer".into(),
                status: Status::Running,
                cwd: "/w/main".into(),
                model: None,
                owner_session: None,
                session_file: None,
                updated_at: 1,
            }],
            problems: Vec::new(),
            registry: None,
        });
        app
    }

    #[test]
    fn opens_on_the_first_other_worktree_in_review_order() {
        let s = Switcher::new(&app(1));
        let order: Vec<usize> = s.matches.iter().map(|m| m.worktree).collect();
        assert_eq!(
            order,
            vec![0, 2, 1],
            "a worker's worktree first, then recency"
        );
        assert_eq!(s.selected(), Some(0));
        assert_eq!(Switcher::new(&app(0)).selected(), Some(2));
    }

    #[test]
    fn filters_fuzzily_and_marks_matches() {
        let app = app(0);
        let mut s = Switcher::new(&app);
        for c in "fpar".chars() {
            s.push(&app, c);
        }
        assert_eq!(s.selected(), Some(1));
        assert_eq!(s.matches[0].positions, vec![0, 4, 5, 6]);
        s.pop(&app);
        s.pop(&app);
        s.pop(&app);
        assert_eq!(s.matches.len(), 2, "both f… branches");
    }

    #[test]
    fn filters_by_the_agents_working_there() {
        let app = app(1);
        let mut s = Switcher::new(&app);
        for c in "lexer".chars() {
            s.push(&app, c);
        }
        assert_eq!(s.matches.len(), 1);
        assert_eq!(s.selected(), Some(0));
        assert!(s.matches[0].positions.is_empty(), "nothing in the branch");
    }

    #[test]
    fn ages_use_one_short_unit() {
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(10_000_000);
        let ago = |s| age(now - Duration::from_secs(s), now);
        assert_eq!(ago(30), "30s");
        assert_eq!(ago(300), "5m");
        assert_eq!(ago(7200), "2h");
        assert_eq!(ago(3 * 86_400), "3d");
        assert_eq!(ago(30 * 86_400), "4w");
    }
}
