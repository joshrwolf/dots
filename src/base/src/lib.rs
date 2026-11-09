//! The base a worktree's changes compare against, shared by `dots review`
//! and `dots-diff` so both show a branch against the same commit.
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
//!
//! The rule reads refs as [`Record`]s and asks a repository about commits
//! through [`History`], so each caller keeps its own way of reading Git.
//! Commits are full hexadecimal object ids.

use std::collections::{BTreeMap, BTreeSet};

/// A branch or remote default, as `git for-each-ref` reports it for
/// `refs/heads/` and `refs/remotes/*/HEAD`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Record {
    /// Full ref name, such as `refs/heads/main` or `refs/remotes/origin/HEAD`.
    pub name: String,
    /// What a symbolic ref points at, such as `refs/remotes/origin/main`.
    pub symref: Option<String>,
    /// The commit it names; `None` for a symbolic ref whose target is gone.
    pub commit: Option<String>,
    /// A branch's configured upstream, such as `refs/remotes/origin/main`.
    pub upstream: Option<String>,
    /// The branch the upstream names on its remote, such as
    /// `refs/heads/main`.
    pub remote_ref: Option<String>,
}

/// What the rule asks a repository.
pub trait History {
    type Error;
    /// Whether commit `ancestor` is `descendant` or one of its ancestors.
    fn is_ancestor(&self, ancestor: &str, descendant: &str) -> Result<bool, Self::Error>;
    /// How many commits `to` reaches that `from` does not.
    fn commits_between(&self, from: &str, to: &str) -> Result<u64, Self::Error>;
    /// The best common ancestor of two commits.
    fn merge_base(&self, left: &str, right: &str) -> Result<String, Self::Error>;
    /// The commit `reference` names now, or `None` when it names none.
    fn commit(&self, reference: &str) -> Result<Option<String>, Self::Error>;
}

/// Why the base is what it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    /// The branch's upstream, another branch than its own name on a remote.
    Upstream,
    /// HEAD is on the default branch: compare with HEAD itself.
    DefaultBranch,
    /// No default-branch candidate exists: compare with HEAD itself.
    NoDefault,
    /// The default-branch candidate nearest HEAD.
    Default,
}

/// One default-branch candidate and how far HEAD is from it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    /// Such as `upstream/main`, `origin/main` or `main`.
    pub reference: String,
    /// Commits reachable from HEAD but not from the candidate.
    pub commits: u64,
}

/// A chosen base: the commit and how it was chosen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chosen {
    pub base: String,
    /// A ref such as `origin/main`, or `HEAD`.
    pub reference: String,
    pub reason: Reason,
    /// Every default-branch candidate considered, nearest first; empty
    /// unless the default rule ran.
    pub candidates: Vec<Candidate>,
}

/// A default-branch candidate.
#[derive(Debug)]
struct DefaultRef {
    /// Such as `upstream/main` or `main`.
    reference: String,
    /// Its name as a branch, such as `main`.
    branch: String,
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

/// The refs every worktree of a repository chooses its base from.
#[derive(Debug)]
pub struct Refs {
    /// The candidates whose distance from HEAD is measured.
    defaults: Vec<DefaultRef>,
    /// The default branch names: those remotes name as theirs, or the local
    /// one when no remote names one. Pruning candidates leaves them whole.
    default_branches: BTreeSet<String>,
    upstreams: BTreeMap<String, Upstream>,
}

impl Refs {
    pub fn new(records: impl IntoIterator<Item = Record>) -> Self {
        let mut defaults = Vec::new();
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
                defaults.push(DefaultRef {
                    reference: format!("{remote}/{branch}"),
                    branch: branch.to_owned(),
                    commit,
                    priority: (rank, remote.to_owned()),
                });
            }
        }
        if let Some((branch, commit)) = local
            .get_key_value("main")
            .or_else(|| local.get_key_value("master"))
        {
            defaults.push(DefaultRef {
                reference: branch.clone(),
                branch: branch.clone(),
                commit: commit.clone(),
                priority: (3, String::new()),
            });
        }
        let remote = defaults.iter().any(|default| default.priority.0 < 3);
        let default_branches = defaults
            .iter()
            .filter(|default| !remote || default.priority.0 < 3)
            .map(|default| default.branch.clone())
            .collect();
        Self {
            defaults,
            default_branches,
            upstreams,
        }
    }

    /// Drop the candidates that can never be nearest: those whose commit an
    /// earlier-preferred candidate's commit contains, so HEAD is at least as
    /// far from them. The base chosen stays the same, and so do the default
    /// branches; only the runners-up that could not win go unmeasured.
    pub fn prune<H: History>(&mut self, history: &H) -> Result<(), H::Error> {
        let mut kept: Vec<DefaultRef> = Vec::with_capacity(self.defaults.len());
        let mut defaults = std::mem::take(&mut self.defaults);
        defaults.sort_by(|left, right| left.priority.cmp(&right.priority));
        for default in defaults {
            let mut dominated = false;
            for preferred in &kept {
                if preferred.commit == default.commit
                    || history.is_ancestor(&default.commit, &preferred.commit)?
                {
                    dominated = true;
                    break;
                }
            }
            if !dominated {
                kept.push(default);
            }
        }
        self.defaults = kept;
        Ok(())
    }

    /// The base for HEAD commit `head` on `branch` (`None` when detached).
    pub fn choose<H: History>(
        &self,
        history: &H,
        head: &str,
        branch: Option<&str>,
    ) -> Result<Chosen, H::Error> {
        let at_head = |reason| Chosen {
            base: head.to_owned(),
            reference: "HEAD".into(),
            reason,
            candidates: Vec::new(),
        };
        if let Some(branch) = branch
            && let Some(upstream) = self.upstreams.get(branch)
            && upstream.remote_ref != format!("refs/heads/{branch}")
            // An upstream whose ref is gone falls through to the defaults.
            && let Some(commit) = history.commit(&upstream.reference)?
        {
            let short = upstream
                .reference
                .strip_prefix("refs/remotes/")
                .or_else(|| upstream.reference.strip_prefix("refs/heads/"))
                .unwrap_or(&upstream.reference);
            return Ok(Chosen {
                base: history.merge_base(&commit, head)?,
                reference: short.to_owned(),
                reason: Reason::Upstream,
                candidates: Vec::new(),
            });
        }
        if let Some(branch) = branch
            && self.default_branches.contains(branch)
        {
            return Ok(at_head(Reason::DefaultBranch));
        }
        if self.defaults.is_empty() {
            return Ok(at_head(Reason::NoDefault));
        }
        let mut counted: BTreeMap<&str, u64> = BTreeMap::new();
        let mut ranked = Vec::with_capacity(self.defaults.len());
        for default in &self.defaults {
            let commits = if let Some(commits) = counted.get(default.commit.as_str()) {
                *commits
            } else {
                let commits = history.commits_between(&default.commit, head)?;
                counted.insert(&default.commit, commits);
                commits
            };
            ranked.push((commits, &default.priority, default));
        }
        ranked.sort_by(|left, right| (left.0, left.1).cmp(&(right.0, right.1)));
        let nearest = ranked[0].2;
        Ok(Chosen {
            base: history.merge_base(&nearest.commit, head)?,
            reference: nearest.reference.clone(),
            reason: Reason::Default,
            candidates: ranked
                .iter()
                .map(|(commits, _, default)| Candidate {
                    reference: default.reference.clone(),
                    commits: *commits,
                })
                .collect(),
        })
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};
    use std::convert::Infallible;

    use super::*;

    /// A commit graph: each commit and its parents.
    struct Graph(
        BTreeMap<&'static str, Vec<&'static str>>,
        BTreeMap<&'static str, &'static str>,
    );

    impl Graph {
        fn reach(&self, commit: &str) -> BTreeSet<String> {
            let mut seen = BTreeSet::new();
            let mut stack = vec![commit.to_owned()];
            while let Some(c) = stack.pop() {
                if seen.insert(c.clone()) {
                    stack.extend(self.0[c.as_str()].iter().map(|p| (*p).to_owned()));
                }
            }
            seen
        }
    }

    impl History for Graph {
        type Error = Infallible;
        fn is_ancestor(&self, ancestor: &str, descendant: &str) -> Result<bool, Infallible> {
            Ok(self.reach(descendant).contains(ancestor))
        }
        fn commits_between(&self, from: &str, to: &str) -> Result<u64, Infallible> {
            let hidden = self.reach(from);
            Ok(self.reach(to).difference(&hidden).count() as u64)
        }
        fn merge_base(&self, left: &str, right: &str) -> Result<String, Infallible> {
            let right = self.reach(right);
            let common: Vec<String> = self.reach(left).intersection(&right).cloned().collect();
            Ok(common
                .iter()
                .max_by_key(|c| self.reach(c).len())
                .cloned()
                .unwrap_or_default())
        }
        fn commit(&self, reference: &str) -> Result<Option<String>, Infallible> {
            Ok(self.1.get(reference).map(|c| (*c).to_owned()))
        }
    }

    /// m1 <- m2 <- m3 (origin/main) and m2 <- l1 (local main, unpushed);
    /// feature f1 <- f2 on m3; stacked s1 on f2.
    fn graph() -> Graph {
        Graph(
            BTreeMap::from([
                ("m1", vec![]),
                ("m2", vec!["m1"]),
                ("m3", vec!["m2"]),
                ("l1", vec!["m2"]),
                ("f1", vec!["m3"]),
                ("f2", vec!["f1"]),
                ("s1", vec!["f2"]),
            ]),
            BTreeMap::from([("refs/heads/feature", "f2")]),
        )
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

    #[test]
    fn the_nearest_default_wins_and_lists_every_candidate() {
        let refs = Refs::new([branch("main", "l1", None), remote_head("origin", "m3")]);
        let chosen = refs
            .choose(&graph(), "f2", Some("feature"))
            .expect("chosen");
        assert_eq!(
            chosen.reference, "origin/main",
            "local main is behind f2's fork"
        );
        assert_eq!(chosen.base, "m3");
        assert_eq!(chosen.reason, Reason::Default);
        assert_eq!(
            chosen.candidates,
            [
                Candidate {
                    reference: "origin/main".into(),
                    commits: 2
                },
                Candidate {
                    reference: "main".into(),
                    commits: 3
                },
            ]
        );
    }

    #[test]
    fn ties_prefer_upstream_then_origin_then_local() {
        let refs = Refs::new([
            branch("main", "m3", None),
            remote_head("origin", "m3"),
            remote_head("upstream", "m3"),
        ]);
        let chosen = refs
            .choose(&graph(), "f2", Some("feature"))
            .expect("chosen");
        assert_eq!(chosen.reference, "upstream/main");
    }

    #[test]
    fn the_default_branch_compares_with_head() {
        let refs = Refs::new([branch("main", "l1", None), remote_head("origin", "m3")]);
        let chosen = refs.choose(&graph(), "l1", Some("main")).expect("chosen");
        assert_eq!(
            (chosen.base.as_str(), chosen.reason),
            ("l1", Reason::DefaultBranch)
        );
        let none = Refs::new([branch("feature", "f2", None)]);
        let chosen = none
            .choose(&graph(), "f2", Some("feature"))
            .expect("chosen");
        assert_eq!(chosen.reason, Reason::NoDefault);
    }

    #[test]
    fn a_stacked_branch_compares_with_its_upstream_branch() {
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
        let g = graph();
        let chosen = refs.choose(&g, "s1", Some("stacked")).expect("chosen");
        assert_eq!(
            (
                chosen.reference.as_str(),
                chosen.base.as_str(),
                chosen.reason
            ),
            ("feature", "f2", Reason::Upstream)
        );
        // Its own name on a remote is not a base: only unpushed commits.
        let chosen = refs.choose(&g, "f2", Some("feature")).expect("chosen");
        assert_eq!(chosen.reason, Reason::Default);
        // An upstream that is gone falls through to the defaults.
        let gone = Refs::new([
            remote_head("origin", "m3"),
            branch(
                "stacked",
                "s1",
                Some(("refs/heads/deleted", "refs/heads/deleted")),
            ),
        ]);
        let chosen = gone.choose(&g, "s1", Some("stacked")).expect("chosen");
        assert_eq!(chosen.reason, Reason::Default);
    }

    #[test]
    fn pruning_drops_only_candidates_that_cannot_win() {
        let g = graph();
        let mut refs = Refs::new([
            branch("main", "m2", None),
            remote_head("origin", "m3"),
            remote_head("fork", "l1"),
        ]);
        let before = refs.choose(&g, "f2", Some("feature")).expect("chosen");
        refs.prune(&g).expect("pruned");
        let after = refs.choose(&g, "f2", Some("feature")).expect("chosen");
        assert_eq!(before.base, after.base);
        let kept: Vec<&str> = after
            .candidates
            .iter()
            .map(|c| c.reference.as_str())
            .collect();
        assert_eq!(
            kept,
            ["origin/main", "fork/main"],
            "main is inside origin/main"
        );
        assert_eq!(
            refs.choose(&g, "m2", Some("main")).expect("chosen").reason,
            Reason::DefaultBranch,
            "pruning keeps the default branches"
        );
    }
}
