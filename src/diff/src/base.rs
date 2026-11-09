//! The base a worktree's changes compare against.
//!
//! In order: the branch's upstream when it is another branch (a stacked
//! branch tracking the one it builds on), never the branch's own name on a
//! remote, which would show only unpushed commits; HEAD itself on the
//! default branch, so only uncommitted changes; otherwise the nearest
//! default-branch candidate. Candidates are each remote's default branch
//! (`refs/remotes/REMOTE/HEAD`) and the local `main`, else `master`; the
//! nearest is the one HEAD has the fewest commits beyond, which skips a stale
//! fork's `origin` and a local `main` that is behind, and keeps unpushed
//! commits of a local `main` out of the branches built on it. Ties prefer
//! `upstream`, then `origin`, then other remotes, then the local branch.
//! A base shares history with HEAD: an upstream or candidate that shares
//! none is passed over, and when none does, as on an orphan branch, HEAD is
//! compared with itself.
//!
//! The rule reads refs as [`Record`]s and asks the repository about commits
//! through [`History`]. Commits are full hexadecimal object ids.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::Result;

/// A branch or remote default, as `git for-each-ref` reports it for
/// `refs/heads/` and `refs/remotes/*/HEAD`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Record {
    /// Full ref name, such as `refs/heads/main` or `refs/remotes/origin/HEAD`.
    pub(crate) name: String,
    /// What a symbolic ref points at, such as `refs/remotes/origin/main`.
    pub(crate) symref: Option<String>,
    /// The commit it names; `None` for a symbolic ref whose target is gone.
    pub(crate) commit: Option<String>,
    /// A branch's configured upstream, such as `refs/remotes/origin/main`.
    pub(crate) upstream: Option<String>,
    /// The branch the upstream names on its remote, such as
    /// `refs/heads/main`.
    pub(crate) remote_ref: Option<String>,
}

/// What the rule asks the repository.
pub(crate) trait History {
    /// How many commits `to` reaches that `from` does not, or `None` when
    /// that is more than `at_most`.
    fn commits_between(&self, from: &str, to: &str, at_most: u64) -> Result<Option<u64>>;
    /// The best common ancestor of two commits, or `None` when they share
    /// no history.
    fn merge_base(&self, left: &str, right: &str) -> Result<Option<String>>;
    /// The commit `reference` names now, or `None` when it names none.
    fn commit(&self, reference: &str) -> Result<Option<String>>;
}

/// A chosen base.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Chosen {
    pub(crate) base: String,
    /// A ref such as `origin/main`, or `HEAD`.
    pub(crate) reference: String,
    /// The nearest candidate when no candidate shares history with HEAD,
    /// so HEAD is compared with itself.
    pub(crate) unshared: Option<String>,
}

/// The first limit candidates are counted to; each round raises it
/// eightfold until some candidate comes within it, so a long history
/// behind a stale candidate is not walked.
const FIRST_LIMIT: u64 = 256;

/// A default-branch candidate.
#[derive(Debug)]
struct DefaultRef {
    /// Such as `upstream/main` or `main`.
    reference: String,
    commit: String,
    /// Lower wins ties.
    priority: (u8, String),
}

/// A local branch's configured upstream.
#[derive(Debug)]
struct Upstream {
    /// Full ref name, such as `refs/remotes/origin/main`.
    reference: String,
    /// The branch it names on its remote, such as `refs/heads/main`.
    remote_ref: String,
}

/// The refs a worktree chooses its base from.
#[derive(Debug)]
pub(crate) struct Refs {
    /// The candidates whose distance from HEAD is measured.
    defaults: Vec<DefaultRef>,
    /// The default branch names: those remotes name as theirs, or the local
    /// one when no remote names one.
    default_branches: BTreeSet<String>,
    upstreams: BTreeMap<String, Upstream>,
}

impl Refs {
    pub(crate) fn new(records: impl IntoIterator<Item = Record>) -> Self {
        let mut defaults = Vec::new();
        let mut default_branches = BTreeSet::new();
        let mut upstreams = BTreeMap::new();
        let mut local: BTreeMap<String, String> = BTreeMap::new();
        for record in records {
            if let Some(branch) = record.name.strip_prefix("refs/heads/") {
                if let Some(upstream) = record.upstream.filter(|u| !u.is_empty()) {
                    upstreams.insert(
                        branch.to_owned(),
                        Upstream {
                            reference: upstream,
                            remote_ref: record.remote_ref.unwrap_or_default(),
                        },
                    );
                }
                if (branch == "main" || branch == "master")
                    && let Some(commit) = record.commit
                {
                    local.insert(branch.to_owned(), commit);
                }
            } else if let Some(remote) = record
                .name
                .strip_prefix("refs/remotes/")
                .and_then(|rest| rest.strip_suffix("/HEAD"))
            {
                // A default naming a ref that is gone has no commit.
                let Some(branch) = record
                    .symref
                    .as_deref()
                    .and_then(|s| s.strip_prefix(&format!("refs/remotes/{remote}/")))
                else {
                    continue;
                };
                let Some(commit) = record.commit.filter(|c| !c.is_empty()) else {
                    continue;
                };
                let rank = match remote {
                    "upstream" => 0,
                    "origin" => 1,
                    _ => 2,
                };
                default_branches.insert(branch.to_owned());
                defaults.push(DefaultRef {
                    reference: format!("{remote}/{branch}"),
                    commit,
                    priority: (rank, remote.to_owned()),
                });
            }
        }
        if let Some((branch, commit)) = local
            .get_key_value("main")
            .or_else(|| local.get_key_value("master"))
        {
            if default_branches.is_empty() {
                default_branches.insert(branch.clone());
            }
            defaults.push(DefaultRef {
                reference: branch.clone(),
                commit: commit.clone(),
                priority: (3, String::new()),
            });
        }
        Self {
            defaults,
            default_branches,
            upstreams,
        }
    }

    /// The base for HEAD commit `head` on `branch` (`None` when detached).
    pub(crate) fn choose(
        &self,
        history: &impl History,
        head: &str,
        branch: Option<&str>,
    ) -> Result<Chosen> {
        let at_head = |unshared| Chosen {
            base: head.to_owned(),
            reference: "HEAD".into(),
            unshared,
        };
        if let Some(branch) = branch
            && let Some(upstream) = self.upstreams.get(branch)
            && upstream.remote_ref != format!("refs/heads/{branch}")
            // An upstream whose ref is gone, or that shares no history,
            // falls through to the defaults.
            && let Some(commit) = history.commit(&upstream.reference)?
            && let Some(base) = history.merge_base(&commit, head)?
        {
            let short = upstream
                .reference
                .strip_prefix("refs/remotes/")
                .or_else(|| upstream.reference.strip_prefix("refs/heads/"))
                .unwrap_or(&upstream.reference);
            return Ok(Chosen {
                base,
                reference: short.to_owned(),
                unshared: None,
            });
        }
        if branch.is_some_and(|branch| self.default_branches.contains(branch))
            || self.defaults.is_empty()
        {
            return Ok(at_head(None));
        }
        let counted = self.count(history, head)?;
        let mut ranked: Vec<_> = self
            .defaults
            .iter()
            .filter_map(|d| Some((*counted.get(d.commit.as_str())?, &d.priority, d)))
            .collect();
        ranked.sort_by(|left, right| (left.0, left.1).cmp(&(right.0, right.1)));
        for (_, _, default) in &ranked {
            if let Some(base) = history.merge_base(&default.commit, head)? {
                return Ok(Chosen {
                    base,
                    reference: default.reference.clone(),
                    unshared: None,
                });
            }
        }
        Ok(at_head(
            ranked
                .first()
                .map(|(_, _, default)| default.reference.clone()),
        ))
    }

    /// How many commits `head` has beyond the nearest candidate commits:
    /// every candidate counts to a limit that rises until some come within
    /// it. A candidate sharing history with `head` is always nearer than
    /// one that shares none.
    fn count(&self, history: &impl History, head: &str) -> Result<BTreeMap<&str, u64>> {
        let commits: BTreeSet<&str> = self.defaults.iter().map(|d| d.commit.as_str()).collect();
        let mut limit = FIRST_LIMIT;
        loop {
            let mut counted = BTreeMap::new();
            for &commit in &commits {
                if let Some(n) = history.commits_between(commit, head, limit)? {
                    counted.insert(commit, n);
                }
            }
            if !counted.is_empty() || commits.is_empty() {
                return Ok(counted);
            }
            limit = limit.saturating_mul(8);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::collections::{BTreeMap, BTreeSet};

    use super::*;

    /// A commit graph: each commit and its parents, the refs naming
    /// commits, and how many commits counting has walked.
    struct Graph {
        parents: BTreeMap<&'static str, Vec<&'static str>>,
        refs: BTreeMap<&'static str, &'static str>,
        walked: Cell<u64>,
    }

    impl Graph {
        fn reach(&self, commit: &str) -> BTreeSet<String> {
            let mut seen = BTreeSet::new();
            let mut stack = vec![commit.to_owned()];
            while let Some(c) = stack.pop() {
                if seen.insert(c.clone()) {
                    stack.extend(self.parents[c.as_str()].iter().map(|p| (*p).to_owned()));
                }
            }
            seen
        }

        /// Adds `count` commits named `{prefix}{i}` in a line from `parent`;
        /// the last one's name.
        fn chain(&mut self, prefix: &str, parent: &'static str, count: usize) -> &'static str {
            let mut parent = parent;
            for i in 0..count {
                let name: &'static str = Box::leak(format!("{prefix}{i}").into_boxed_str());
                self.parents.insert(name, vec![parent]);
                parent = name;
            }
            parent
        }
    }

    impl History for Graph {
        fn commits_between(&self, from: &str, to: &str, at_most: u64) -> Result<Option<u64>> {
            let hidden = self.reach(from);
            let n = self.reach(to).difference(&hidden).count() as u64;
            self.walked.set(self.walked.get() + n.min(at_most + 1));
            Ok((n <= at_most).then_some(n))
        }
        fn merge_base(&self, left: &str, right: &str) -> Result<Option<String>> {
            let right = self.reach(right);
            let common: Vec<String> = self.reach(left).intersection(&right).cloned().collect();
            Ok(common.iter().max_by_key(|c| self.reach(c).len()).cloned())
        }
        fn commit(&self, reference: &str) -> Result<Option<String>> {
            Ok(self.refs.get(reference).map(|c| (*c).to_owned()))
        }
    }

    /// m1 <- m2 <- m3 (origin/main) and m2 <- l1 (local main, unpushed);
    /// feature f1 <- f2 on m3; stacked s1 on f2.
    fn graph() -> Graph {
        Graph {
            parents: BTreeMap::from([
                ("m1", vec![]),
                ("m2", vec!["m1"]),
                ("m3", vec!["m2"]),
                ("l1", vec!["m2"]),
                ("f1", vec!["m3"]),
                ("f2", vec!["f1"]),
                ("s1", vec!["f2"]),
            ]),
            refs: BTreeMap::from([("refs/heads/feature", "f2")]),
            walked: Cell::new(0),
        }
    }

    fn branch(name: &str, commit: &str, upstream: Option<(&str, &str)>) -> Record {
        Record {
            name: format!("refs/heads/{name}"),
            commit: Some(commit.into()),
            upstream: upstream.map(|u| u.0.into()),
            remote_ref: upstream.map(|u| u.1.into()),
            ..Record::default()
        }
    }

    fn remote_head(remote: &str, commit: &str) -> Record {
        Record {
            name: format!("refs/remotes/{remote}/HEAD"),
            symref: Some(format!("refs/remotes/{remote}/main")),
            commit: Some(commit.into()),
            ..Record::default()
        }
    }

    /// The chosen reference and base.
    fn chosen(refs: &Refs, g: &Graph, head: &str, branch: Option<&str>) -> (String, String) {
        let chosen = refs.choose(g, head, branch).expect("chosen");
        (chosen.reference, chosen.base)
    }

    fn pair(reference: &str, base: &str) -> (String, String) {
        (reference.to_owned(), base.to_owned())
    }

    #[test]
    fn the_nearest_default_wins() {
        let g = graph();
        let refs = Refs::new([
            branch("main", "l1", None),
            remote_head("origin", "m3"),
            remote_head("upstream", "m2"),
        ]);
        for (head, want) in [
            ("f2", pair("origin/main", "m3")),
            ("s1", pair("origin/main", "m3")),
            ("m3", pair("origin/main", "m3")),
            ("l1", pair("main", "l1")),
        ] {
            assert_eq!(chosen(&refs, &g, head, None), want, "head {head}");
        }
    }

    #[test]
    fn ties_prefer_upstream_then_origin_then_local() {
        let g = graph();
        let all = Refs::new([
            branch("main", "m3", None),
            remote_head("origin", "m3"),
            remote_head("upstream", "m3"),
        ]);
        assert_eq!(
            chosen(&all, &g, "f2", Some("feature")),
            pair("upstream/main", "m3")
        );
        let local = Refs::new([branch("main", "m3", None), remote_head("origin", "m3")]);
        assert_eq!(
            chosen(&local, &g, "f2", Some("feature")),
            pair("origin/main", "m3")
        );
    }

    #[test]
    fn the_default_branch_compares_with_head() {
        let g = graph();
        let refs = Refs::new([branch("main", "l1", None), remote_head("origin", "m3")]);
        assert_eq!(chosen(&refs, &g, "l1", Some("main")), pair("HEAD", "l1"));
        // Without a remote default, the local main is the default branch.
        let local = Refs::new([branch("main", "l1", None)]);
        assert_eq!(chosen(&local, &g, "l1", Some("main")), pair("HEAD", "l1"));
        // With no candidate at all, HEAD compares with itself.
        let none = Refs::new([branch("feature", "f2", None)]);
        assert_eq!(chosen(&none, &g, "f2", Some("feature")), pair("HEAD", "f2"));
    }

    #[test]
    fn a_stacked_branch_compares_with_its_upstream_branch() {
        let g = graph();
        let refs = Refs::new([
            remote_head("origin", "m3"),
            branch(
                "stacked",
                "s1",
                Some(("refs/heads/feature", "refs/heads/feature")),
            ),
            branch(
                "feature",
                "f2",
                Some(("refs/remotes/origin/feature", "refs/heads/feature")),
            ),
        ]);
        assert_eq!(
            chosen(&refs, &g, "s1", Some("stacked")),
            pair("feature", "f2")
        );
        // Its own name on a remote is not a base: only unpushed commits.
        assert_eq!(
            chosen(&refs, &g, "f2", Some("feature")),
            pair("origin/main", "m3")
        );
        // An upstream that is gone falls through to the defaults.
        let gone = Refs::new([
            remote_head("origin", "m3"),
            branch(
                "stacked",
                "s1",
                Some(("refs/heads/deleted", "refs/heads/deleted")),
            ),
        ]);
        assert_eq!(
            chosen(&gone, &g, "s1", Some("stacked")),
            pair("origin/main", "m3")
        );
    }

    #[test]
    fn a_default_named_branch_compares_with_its_differently_named_upstream() {
        // Local main tracks origin/trunk: its upstream wins over its role as
        // a default-branch candidate, whether one record carries the branch
        // or one gives its commit and another its upstream.
        let g = Graph {
            refs: BTreeMap::from([("refs/remotes/origin/trunk", "m3")]),
            ..graph()
        };
        let trunk = Some(("refs/remotes/origin/trunk", "refs/heads/trunk"));
        let whole = Refs::new([remote_head("origin", "m3"), branch("main", "f2", trunk)]);
        let split = Refs::new([
            remote_head("origin", "m3"),
            branch("main", "f2", None),
            Record {
                commit: None,
                ..branch("main", "", trunk)
            },
        ]);
        for refs in [whole, split] {
            assert_eq!(
                chosen(&refs, &g, "f2", Some("main")),
                pair("origin/trunk", "m3")
            );
        }
    }

    #[test]
    fn bases_share_history_with_head() {
        // o1 <- o2 is an orphan branch; o2 also tracks feature.
        let mut g = graph();
        g.parents.extend([("o1", vec![]), ("o2", vec!["o1"])]);
        let refs = Refs::new([
            branch("main", "l1", None),
            remote_head("origin", "m3"),
            branch(
                "pages",
                "o2",
                Some(("refs/heads/feature", "refs/heads/feature")),
            ),
        ]);
        let orphan = refs.choose(&g, "o2", Some("pages")).expect("chosen");
        assert_eq!(
            orphan,
            Chosen {
                base: "o2".into(),
                reference: "HEAD".into(),
                unshared: Some("origin/main".into()),
            }
        );
        // Only a candidate that shares history gives the base.
        g.parents.insert("x1", vec!["o2"]);
        g.parents.insert("m3", vec!["m2", "o1"]);
        assert_eq!(chosen(&refs, &g, "x1", None), pair("origin/main", "o1"));
    }

    #[test]
    fn counting_stops_at_the_nearest_candidate() {
        // A stale origin far behind main, and a feature on main.
        let mut g = graph();
        let tip = g.chain("x", "m1", 1200);
        g.parents.insert("n1", vec![tip]);
        g.parents.insert("h1", vec!["n1"]);
        let refs = Refs::new([branch("main", "n1", None), remote_head("origin", "m2")]);
        assert_eq!(chosen(&refs, &g, "h1", None), pair("main", "n1"));
        let walked = g.walked.get();
        assert!(
            walked <= 2 * (FIRST_LIMIT + 1),
            "walked {walked} commits of a 1200-commit history"
        );
    }

    #[test]
    fn the_limit_rises_until_a_candidate_comes_within_it() {
        let mut g = graph();
        let tip = g.chain("y", "m3", 3000);
        let refs = Refs::new([remote_head("origin", "m3"), remote_head("fork", "m1")]);
        assert_eq!(chosen(&refs, &g, tip, None), pair("origin/main", "m3"));
    }
}
