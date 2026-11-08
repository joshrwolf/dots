//! Choosing the Pi session a review goes to: a fuzzy filter over live
//! sessions by name, pane and directory.

use nucleo::pattern::{CaseMatching, Normalization, Pattern};
use nucleo::{Config, Matcher, Utf32Str};

use crate::deliver::Agent;

#[derive(Debug, Clone)]
pub(crate) struct Picker {
    pub(crate) agents: Vec<Agent>,
    pub(crate) query: String,
    /// Indexes into `agents` that pass the filter, best first.
    pub(crate) matches: Vec<usize>,
    pub(crate) cursor: usize,
    /// The send waiting for the choice, if any.
    pub(crate) then_send: Option<Batch>,
}

/// What a send delivers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Batch {
    /// Every draft.
    All,
    /// One comment, by id.
    One(u64),
}

impl Batch {
    pub(crate) fn includes(self, id: u64) -> bool {
        match self {
            Self::All => true,
            Self::One(one) => one == id,
        }
    }
}

impl Picker {
    pub(crate) fn new(agents: Vec<Agent>, then_send: Option<Batch>) -> Self {
        let mut p = Self {
            agents,
            query: String::new(),
            matches: Vec::new(),
            cursor: 0,
            then_send,
        };
        p.refilter();
        p
    }

    /// What the filter matches and the list shows.
    pub(crate) fn line(agent: &Agent) -> String {
        format!(
            "{}  {}  {}",
            agent.label(),
            agent.pane(),
            agent.cwd.display()
        )
    }

    pub(crate) fn push(&mut self, c: char) {
        self.query.push(c);
        self.refilter();
    }

    pub(crate) fn pop(&mut self) {
        self.query.pop();
        self.refilter();
    }

    pub(crate) fn move_by(&mut self, delta: i64) {
        let n = i64::try_from(self.matches.len()).unwrap_or(i64::MAX);
        if n > 0 {
            let at = i64::try_from(self.cursor).unwrap_or(0) + delta;
            self.cursor = usize::try_from(at.clamp(0, n - 1)).unwrap_or(0);
        }
    }

    pub(crate) fn selected(&self) -> Option<&Agent> {
        self.matches.get(self.cursor).map(|&i| &self.agents[i])
    }

    fn refilter(&mut self) {
        let mut matcher = Matcher::new(Config::DEFAULT);
        let pattern = Pattern::parse(&self.query, CaseMatching::Smart, Normalization::Smart);
        let mut buf = Vec::new();
        let mut scored: Vec<(u32, usize)> = self
            .agents
            .iter()
            .enumerate()
            .filter_map(|(i, a)| {
                let line = Self::line(a);
                let score = pattern.score(Utf32Str::new(&line, &mut buf), &mut matcher)?;
                Some((score, i))
            })
            .collect();
        scored.sort_by_key(|&(score, i)| (std::cmp::Reverse(score), i));
        self.matches = scored.into_iter().map(|(_, i)| i).collect();
        self.cursor = self.cursor.min(self.matches.len().saturating_sub(1));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn agent(id: &str, name: &str, cwd: &str) -> Agent {
        Agent {
            pid: 1,
            session_id: id.into(),
            session_file: None,
            session_name: Some(name.into()),
            rex_block: Some(format!("block:{id}")),
            cwd: cwd.into(),
            socket: format!("/i/{id}.sock").into(),
        }
    }

    #[test]
    fn filters_by_name_pane_and_directory() {
        let mut p = Picker::new(
            vec![
                agent("a1", "auth fix", "/r/auth"),
                agent("b2", "docs", "/r/docs"),
            ],
            None,
        );
        assert_eq!(p.matches, vec![0, 1]);
        for c in "docs".chars() {
            p.push(c);
        }
        assert_eq!(p.selected().map(|a| a.session_id.as_str()), Some("b2"));
        p.pop();
        p.pop();
        p.pop();
        p.pop();
        for c in "a1".chars() {
            p.push(c);
        }
        assert_eq!(p.selected().map(|a| a.session_id.as_str()), Some("a1"));
        p.push('z');
        p.push('z');
        assert_eq!(p.selected(), None);
    }
}
