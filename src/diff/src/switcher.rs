//! The worktree switcher: a fuzzy filter over branch names.

use std::time::{Duration, SystemTime};

use nucleo::pattern::{CaseMatching, Normalization, Pattern};
use nucleo::{Config, Matcher, Utf32Str};

use crate::repo::Repo;

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
    /// Opens on the most recently used worktree other than `current`.
    pub(crate) fn new(repo: &Repo, current: usize) -> Self {
        let mut s = Self::default();
        s.refilter(repo);
        s.cursor = s
            .matches
            .iter()
            .position(|m| m.worktree != current)
            .unwrap_or(0);
        s
    }

    pub(crate) fn push(&mut self, repo: &Repo, c: char) {
        self.query.push(c);
        self.refilter(repo);
    }

    pub(crate) fn pop(&mut self, repo: &Repo) {
        self.query.pop();
        self.refilter(repo);
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

    /// Best matches first; with no query, most recently used first.
    fn refilter(&mut self, repo: &Repo) {
        let mut matcher = Matcher::new(Config::DEFAULT);
        let pattern = Pattern::parse(&self.query, CaseMatching::Smart, Normalization::Smart);
        let mut buf = Vec::new();
        let mut scored: Vec<(u32, Match)> = repo
            .worktrees
            .iter()
            .enumerate()
            .filter_map(|(i, wt)| {
                let haystack = Utf32Str::new(&wt.branch, &mut buf);
                let mut positions = Vec::new();
                let score = pattern.indices(haystack, &mut matcher, &mut positions)?;
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
        scored.sort_by(|(sa, a), (sb, b)| {
            sb.cmp(sa).then_with(|| {
                let touched = |m: &Match| repo.worktrees[m.worktree].touched;
                touched(b).cmp(&touched(a))
            })
        });
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
    use crate::repo::Worktree;

    fn repo() -> Repo {
        let wt = |branch: &str, mins_ago: u64| Worktree {
            path: format!("/w/{branch}").into(),
            branch: branch.into(),
            main: false,
            touched: Some(SystemTime::UNIX_EPOCH + Duration::from_secs(10_000 - mins_ago * 60)),
        };
        Repo {
            worktrees: vec![wt("main", 50), wt("fix-parser", 5), wt("feature-wrap", 1)],
        }
    }

    #[test]
    fn opens_on_the_most_recent_other_worktree() {
        let s = Switcher::new(&repo(), 2);
        let order: Vec<usize> = s.matches.iter().map(|m| m.worktree).collect();
        assert_eq!(order, vec![2, 1, 0]);
        assert_eq!(s.selected(), Some(1));
    }

    #[test]
    fn filters_fuzzily_and_marks_matches() {
        let repo = repo();
        let mut s = Switcher::new(&repo, 0);
        for c in "fpar".chars() {
            s.push(&repo, c);
        }
        assert_eq!(s.selected(), Some(1));
        assert_eq!(s.matches[0].positions, vec![0, 4, 5, 6]);
        s.pop(&repo);
        s.pop(&repo);
        s.pop(&repo);
        assert_eq!(s.matches.len(), 2, "both f… branches");
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
