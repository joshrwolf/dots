//! The base a worktree's review compares against.
//!
//! In order: a base the user or agent set; the branch's upstream when it is
//! another branch (a stacked branch tracking the one it builds on), never the
//! branch's own name on a remote, which would show only unpushed commits;
//! HEAD itself on the default branch, so only uncommitted changes; otherwise
//! the nearest default-branch candidate. Candidates are each remote's default
//! branch (`refs/remotes/REMOTE/HEAD`) and the local `main`, else `master`;
//! the nearest is the one HEAD has the fewest commits beyond, which skips a
//! stale fork's `origin` and a local `main` that is behind, and keeps
//! unpushed commits of a local `main` out of the branches built on it. Ties
//! prefer `upstream`, then `origin`, then other remotes, then the local
//! branch.
use std::collections::BTreeMap;

use crate::git::Git;
use crate::{BaseReason, Basis, Candidate, Result};

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
pub(crate) struct Refs {
    defaults: Vec<DefaultRef>,
    upstreams: BTreeMap<String, Upstream>,
}

/// A chosen base: the commit and how it was chosen.
#[derive(Debug)]
pub(crate) struct Chosen {
    pub(crate) base: String,
    pub(crate) basis: Basis,
}

impl Refs {
    pub(crate) fn read(git: &Git) -> Result<Self> {
        let mut defaults = Vec::new();
        let mut upstreams = BTreeMap::new();
        let mut local: BTreeMap<String, String> = BTreeMap::new();
        for line in git.base_refs()?.lines() {
            let mut fields = line.split('\0');
            let (Some(name), Some(symref), Some(commit), Some(upstream), Some(remote_ref)) = (
                fields.next(),
                fields.next(),
                fields.next(),
                fields.next(),
                fields.next(),
            ) else {
                continue;
            };
            if let Some(branch) = name.strip_prefix("refs/heads/") {
                if !upstream.is_empty() {
                    upstreams.insert(
                        branch.to_owned(),
                        Upstream {
                            reference: upstream.to_owned(),
                            remote_ref: remote_ref.to_owned(),
                        },
                    );
                }
                if branch == "main" || branch == "master" {
                    local.insert(branch.to_owned(), commit.to_owned());
                }
            } else if let Some(remote) = name
                .strip_prefix("refs/remotes/")
                .and_then(|rest| rest.strip_suffix("/HEAD"))
            {
                // A default naming a ref that is gone has no commit.
                let Some(branch) = symref.strip_prefix(&format!("refs/remotes/{remote}/")) else {
                    continue;
                };
                if commit.is_empty() {
                    continue;
                }
                let rank = match remote {
                    "upstream" => 0,
                    "origin" => 1,
                    _ => 2,
                };
                defaults.push(DefaultRef {
                    reference: format!("{remote}/{branch}"),
                    branch: branch.to_owned(),
                    commit: commit.to_owned(),
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
        Ok(Self {
            defaults,
            upstreams,
        })
    }

    /// The base for HEAD commit `head` on `branch` (`None` when detached),
    /// with `base` set explicitly or not.
    pub(crate) fn choose(
        &self,
        git: &Git,
        head: &str,
        branch: Option<&str>,
        base: Option<&str>,
    ) -> Result<Chosen> {
        let merged = |reference: String, commit: &str, reason| -> Result<Chosen> {
            Ok(Chosen {
                base: git.merge_base(commit, head)?,
                basis: Basis {
                    reference,
                    reason,
                    candidates: Vec::new(),
                },
            })
        };
        let at_head = |reason| Chosen {
            base: head.to_owned(),
            basis: Basis {
                reference: "HEAD".into(),
                reason,
                candidates: Vec::new(),
            },
        };
        if let Some(base) = base {
            let commit = git.object(base, "commit")?;
            return merged(base.to_owned(), &commit, BaseReason::Override);
        }
        if let Some(branch) = branch
            && let Some(upstream) = self.upstreams.get(branch)
            && upstream.remote_ref != format!("refs/heads/{branch}")
            // An upstream whose ref is gone falls through to the defaults.
            && let Ok(commit) = git.object(&upstream.reference, "commit")
        {
            let short = upstream
                .reference
                .strip_prefix("refs/remotes/")
                .or_else(|| upstream.reference.strip_prefix("refs/heads/"))
                .unwrap_or(&upstream.reference);
            return merged(short.to_owned(), &commit, BaseReason::Upstream);
        }
        // The default branch is what remotes name as theirs, or the local
        // one when no remote names one.
        let remote = self.defaults.iter().any(|default| default.priority.0 < 3);
        if let Some(branch) = branch
            && self
                .defaults
                .iter()
                .filter(|default| !remote || default.priority.0 < 3)
                .any(|default| default.branch == branch)
        {
            return Ok(at_head(BaseReason::DefaultBranch));
        }
        if self.defaults.is_empty() {
            return Ok(at_head(BaseReason::NoDefault));
        }
        let mut counted: BTreeMap<&str, u64> = BTreeMap::new();
        let mut ranked = Vec::with_capacity(self.defaults.len());
        for default in &self.defaults {
            let commits = if let Some(commits) = counted.get(default.commit.as_str()) {
                *commits
            } else {
                let commits = git.commits_between(&default.commit, head)?;
                counted.insert(&default.commit, commits);
                commits
            };
            ranked.push((commits, &default.priority, default));
        }
        ranked.sort_by(|left, right| (left.0, left.1).cmp(&(right.0, right.1)));
        let nearest = ranked[0].2;
        let mut chosen = merged(
            nearest.reference.clone(),
            &nearest.commit,
            BaseReason::Default,
        )?;
        chosen.basis.candidates = ranked
            .iter()
            .map(|(commits, _, default)| Candidate {
                reference: default.reference.clone(),
                commits: *commits,
            })
            .collect();
        Ok(chosen)
    }
}
