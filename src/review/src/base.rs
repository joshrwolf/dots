//! The base a worktree's review compares against: a base the user or agent
//! set, else the rule `dots-base` shares with `dots-diff`.

use dots_base::{History, Reason, Record};

use crate::git::Git;
use crate::{BaseReason, Basis, Candidate, Error, Result};

/// The refs every worktree of a repository chooses its base from.
#[derive(Debug)]
pub(crate) struct Refs(dots_base::Refs);

/// A chosen base: the commit and how it was chosen.
#[derive(Debug)]
pub(crate) struct Chosen {
    pub(crate) base: String,
    pub(crate) basis: Basis,
}

impl History for Git {
    type Error = Error;

    fn is_ancestor(&self, ancestor: &str, descendant: &str) -> Result<bool> {
        Git::is_ancestor(self, ancestor, descendant)
    }

    fn commits_between(&self, from: &str, to: &str) -> Result<u64> {
        Git::commits_between(self, from, to)
    }

    fn merge_base(&self, left: &str, right: &str) -> Result<Option<String>> {
        Git::shared_base(self, left, right)
    }

    fn commit(&self, reference: &str) -> Result<Option<String>> {
        // Any failure to name a commit counts as gone.
        Ok(self.object(reference, "commit").ok())
    }
}

impl Refs {
    pub(crate) fn read(git: &Git) -> Result<Self> {
        let listed = git.base_refs()?;
        let records = listed.lines().filter_map(|line| {
            let mut fields = line.split('\0');
            let mut field = || -> Option<Option<String>> {
                let value = fields.next()?;
                Some((!value.is_empty()).then(|| value.to_owned()))
            };
            Some(Record {
                name: field()??,
                symref: field()?,
                commit: field()?,
                upstream: field()?,
                remote_ref: field()?,
            })
        });
        Ok(Self(dots_base::Refs::new(records)))
    }

    /// Drops the candidates that can never be nearest; see
    /// [`dots_base::Refs::prune`].
    pub(crate) fn prune(&mut self, git: &Git) -> Result<()> {
        self.0.prune(git)
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
        if let Some(base) = base {
            let commit = git.object(base, "commit")?;
            return Ok(Chosen {
                base: git.merge_base(&commit, head)?,
                basis: Basis {
                    reference: base.to_owned(),
                    reason: BaseReason::Override,
                    candidates: Vec::new(),
                },
            });
        }
        let chosen = self.0.choose(git, head, branch)?;
        Ok(Chosen {
            base: chosen.base,
            basis: Basis {
                reference: chosen.reference,
                reason: match chosen.reason {
                    Reason::Upstream => BaseReason::Upstream,
                    Reason::DefaultBranch => BaseReason::DefaultBranch,
                    Reason::NoDefault => BaseReason::NoDefault,
                    Reason::Default => BaseReason::Default,
                    Reason::NoSharedHistory => BaseReason::NoSharedHistory,
                },
                candidates: chosen
                    .candidates
                    .into_iter()
                    .map(|c| Candidate {
                        reference: c.reference,
                        commits: c.commits,
                    })
                    .collect(),
            },
        })
    }
}
