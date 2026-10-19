//! Create or reopen a branch's workspace without replacing its panes or agents.
//!
//! Callers must resolve a canonical repository root and hold their durable
//! reservation and process lock across [`Request::open`]. This operation always
//! re-lists immediately before creation, and recovers a failed create response by
//! repository and branch. The server does not expose an atomic create-or-open
//! operation: unrelated clients can still race, and an in-flight creation may
//! become visible only on a later retry. Never retry with a different branch.

use std::path::{Component, Path, PathBuf};

use crate::Client;
use crate::api::{Pane, Tab, Workspace, Worktree, WorktreeCreated, WorktreeOpened};

/// Validated inputs for one repository-and-branch workspace operation.
#[derive(Debug, Clone)]
pub struct Request {
    root: PathBuf,
    branch: String,
    label: String,
    focus: bool,
}

/// What happened to the requested checkout's workspace.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Disposition {
    Created,
    Opened,
    AlreadyOpen,
}

/// The actual Herdr topology returned by creation or reopening.
#[derive(Debug, Clone)]
pub struct Opened {
    pub workspace: Workspace,
    pub tab: Tab,
    pub root_pane: Pane,
    pub worktree: Worktree,
    pub disposition: Disposition,
}

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    #[error("worktree repository root must be an absolute, normalized Unicode path: {0:?}")]
    InvalidRoot(PathBuf),
    #[error("invalid worktree branch name: {0:?}")]
    InvalidBranch(String),
    #[error("worktree workspace label must be nonempty and contain no control characters")]
    InvalidLabel,
    #[error("worktree listing returned repository {actual:?}, expected {expected:?}")]
    RepositoryMismatch { expected: PathBuf, actual: PathBuf },
    #[error("multiple checkouts use branch {0}; select an existing checkout explicitly")]
    AmbiguousBranch(String),
    #[error("checkout for branch {0} is unavailable; repair it before opening")]
    UnavailableCheckout(String),
    #[error("could not list worktrees for {root:?}")]
    List {
        root: PathBuf,
        #[source]
        source: crate::Error,
    },
    #[error("could not open the checkout for branch {branch}")]
    Open {
        branch: String,
        #[source]
        source: crate::Error,
    },
    #[error("could not create the checkout for branch {branch}")]
    Create {
        branch: String,
        #[source]
        source: crate::Error,
    },
    #[error("creation failed and recovery could not determine whether branch {branch} was created")]
    Recovery {
        branch: String,
        create: Box<crate::Error>,
        #[source]
        recovery: Box<Error>,
    },
}

impl Request {
    /// Validate inputs before any Herdr request. `root` must already be the
    /// canonical repository root, not a subdirectory or linked checkout.
    pub fn new(root: PathBuf, branch: String, label: String, focus: bool) -> Result<Self, Error> {
        if !root.is_absolute()
            || root.to_str().is_none_or(|value| value.contains('\0'))
            || root
                .components()
                .any(|component| matches!(component, Component::ParentDir | Component::CurDir))
        {
            return Err(Error::InvalidRoot(root));
        }
        if !valid_branch(&branch) {
            return Err(Error::InvalidBranch(branch));
        }
        if label.trim().is_empty() || label.chars().any(char::is_control) {
            return Err(Error::InvalidLabel);
        }
        Ok(Self {
            root,
            branch,
            label,
            focus,
        })
    }

    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    #[must_use]
    pub fn branch(&self) -> &str {
        &self.branch
    }

    /// Open an existing checkout or create one, then return its actual topology.
    /// Existing workspaces retain their panes and running agents.
    pub fn open(&self, client: &Client) -> Result<Opened, Error> {
        if let Some(opened) = self.reopen(client)? {
            return Ok(opened);
        }
        match client.worktree_create(
            None,
            Some(&self.root),
            Some(&self.branch),
            None,
            None,
            Some(&self.label),
            self.focus,
        ) {
            Ok(created) => Ok(created.into()),
            Err(create) => match self.reopen(client) {
                Ok(Some(opened)) => Ok(opened),
                Ok(None) => Err(Error::Create {
                    branch: self.branch.clone(),
                    source: create,
                }),
                Err(recovery) => Err(Error::Recovery {
                    branch: self.branch.clone(),
                    create: Box::new(create),
                    recovery: Box::new(recovery),
                }),
            },
        }
    }

    fn reopen(&self, client: &Client) -> Result<Option<Opened>, Error> {
        let listing = client
            .worktree_list(&self.root)
            .map_err(|source| Error::List {
                root: self.root.clone(),
                source,
            })?;
        if listing.source.repository.root != self.root {
            return Err(Error::RepositoryMismatch {
                expected: self.root.clone(),
                actual: listing.source.repository.root,
            });
        }
        let mut matches = listing
            .worktrees
            .into_iter()
            .filter(|worktree| worktree.branch.as_deref() == Some(&self.branch));
        let Some(worktree) = matches.next() else {
            return Ok(None);
        };
        if matches.next().is_some() {
            return Err(Error::AmbiguousBranch(self.branch.clone()));
        }
        if worktree.is_bare
            || worktree.is_prunable
            || worktree.is_detached
            || !worktree.checkout_path.is_absolute()
        {
            return Err(Error::UnavailableCheckout(self.branch.clone()));
        }
        client
            .worktree_open(
                &self.root,
                &worktree.checkout_path,
                Some(&self.label),
                self.focus,
            )
            .map(|opened| Some(opened.into()))
            .map_err(|source| Error::Open {
                branch: self.branch.clone(),
                source,
            })
    }
}

// Git check-ref-format --branch rules, including the branch-only '-' and HEAD
// exclusions. Reflog shorthands are deliberately rejected rather than expanded.
#[expect(
    clippy::case_sensitive_file_extension_comparisons,
    reason = "Git ref components reject the literal case-sensitive .lock suffix"
)]
fn valid_branch(branch: &str) -> bool {
    !branch.is_empty()
        && branch != "HEAD"
        && branch != "@"
        && !branch.starts_with('-')
        && !branch.ends_with('.')
        && !branch.contains("..")
        && !branch.contains("@{")
        && !branch
            .chars()
            .any(|ch| ch.is_control() || " ~^:?*[\\".contains(ch))
        && branch
            .split('/')
            .all(|part| !part.is_empty() && !part.starts_with('.') && !part.ends_with(".lock"))
}

impl From<WorktreeCreated> for Opened {
    fn from(value: WorktreeCreated) -> Self {
        Self {
            workspace: value.workspace,
            tab: value.tab,
            root_pane: value.root_pane,
            worktree: value.worktree,
            disposition: Disposition::Created,
        }
    }
}

impl From<WorktreeOpened> for Opened {
    fn from(value: WorktreeOpened) -> Self {
        Self {
            workspace: value.workspace,
            tab: value.tab,
            root_pane: value.root_pane,
            worktree: value.worktree,
            disposition: if value.already_open {
                Disposition::AlreadyOpen
            } else {
                Disposition::Opened
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{Server, Step};
    use serde_json::{Value, json};

    fn request() -> Request {
        Request::new(
            "/src/example".into(),
            "eng-12-task".into(),
            "ENG-12 · Task".into(),
            true,
        )
        .unwrap()
    }

    fn checkout() -> Value {
        json!({"path":"/wt/example/task", "label":"eng-12-task","branch":"eng-12-task","is_bare":false,"is_detached":false,"is_prunable":false,"is_linked_worktree":true})
    }

    fn listing(root: &str, worktrees: &[Value]) -> String {
        let result = json!({"type":"worktree_list","source":{"repo_key":"example","repo_name":"example","repo_root":root,"source_checkout_path":root},"worktrees":worktrees});
        format!(r#"{{"id":"{{id}}","result":{result}}}"#)
    }

    fn topology(created: bool, already_open: bool) -> String {
        let result = json!({"type":if created {"worktree_created"} else {"worktree_opened"},"workspace":{"workspace_id":"w3","label":"ENG-12 · Task","focused":true,"agent_status":"idle"},"tab":{"tab_id":"w3:t1","workspace_id":"w3"},"root_pane":{"pane_id":"w3:p1","workspace_id":"w3","tab_id":"w3:t1","cwd":"/wt/example/task"},"worktree":checkout(),"already_open":already_open});
        format!(r#"{{"id":"{{id}}","result":{result}}}"#)
    }

    #[test]
    fn existing_workspace_opens_without_creating_a_checkout() {
        let server = Server::start(vec![
            Step::reply(listing("/src/example", &[checkout()])),
            Step::reply(topology(false, true)),
        ]);
        let result = request().open(&server.client()).unwrap();
        assert_eq!(result.disposition, Disposition::AlreadyOpen);
        assert_eq!(result.workspace.workspace_id, crate::WorkspaceId::new("w3"));
        assert_eq!(result.tab.tab_id, crate::TabId::new("w3:t1"));
        assert_eq!(result.root_pane.pane_id, crate::PaneId::new("w3:p1"));
        let requests = server.requests();
        assert_eq!(
            requests
                .iter()
                .map(|r| r.method.as_str())
                .collect::<Vec<_>>(),
            ["worktree.list", "worktree.open"]
        );
        assert_eq!(
            requests.first().unwrap().params,
            json!({"cwd":"/src/example"})
        );
        assert_eq!(
            requests.last().unwrap().params,
            json!({"cwd":"/src/example","path":"/wt/example/task","label":"ENG-12 · Task","focus":true})
        );
    }

    #[test]
    fn creation_follows_a_fresh_listing_and_returns_actual_topology() {
        let server = Server::start(vec![
            Step::reply(listing("/src/example", &[])),
            Step::reply(topology(true, false)),
        ]);
        let result = request().open(&server.client()).unwrap();
        assert_eq!(result.disposition, Disposition::Created);
        assert_eq!(result.worktree.checkout_path, Path::new("/wt/example/task"));
        let requests = server.requests();
        assert_eq!(
            requests
                .iter()
                .map(|r| r.method.as_str())
                .collect::<Vec<_>>(),
            ["worktree.list", "worktree.create"]
        );
        assert_eq!(
            requests.last().unwrap().params,
            json!({"cwd":"/src/example","branch":"eng-12-task","label":"ENG-12 · Task","focus":true})
        );
    }

    #[test]
    fn lost_creation_response_recovers_by_repository_and_branch() {
        let server = Server::start(vec![
            Step::reply(listing("/src/example", &[])),
            Step::reply(
                r#"{"id":"{id}","error":{"code":"timeout","message":"request timed out"}}"#,
            ),
            Step::reply(listing("/src/example", &[checkout()])),
            Step::reply(topology(false, false)),
        ]);
        let result = request().open(&server.client()).unwrap();
        assert_eq!(result.disposition, Disposition::Opened);
        assert_eq!(
            server
                .requests()
                .iter()
                .map(|r| r.method.as_str())
                .collect::<Vec<_>>(),
            [
                "worktree.list",
                "worktree.create",
                "worktree.list",
                "worktree.open"
            ]
        );
    }

    #[test]
    fn recovery_failure_preserves_both_errors() {
        let server = Server::start(vec![
            Step::reply(listing("/src/example", &[])),
            Step::reply(
                r#"{"id":"{id}","error":{"code":"timeout","message":"request timed out"}}"#,
            ),
            Step::reply(
                r#"{"id":"{id}","error":{"code":"offline","message":"repository unavailable"}}"#,
            ),
        ]);
        let error = request().open(&server.client()).unwrap_err();
        let Error::Recovery {
            create, recovery, ..
        } = error
        else {
            panic!("unexpected error: {error}");
        };
        assert!(matches!(*create, crate::Error::Rejected { ref code, .. } if code == "timeout"));
        assert!(matches!(*recovery, Error::List { .. }));
    }

    #[test]
    fn unsafe_or_ambiguous_listings_never_mutate() {
        let mut prunable = checkout();
        *prunable.get_mut("is_prunable").unwrap() = json!(true);
        let mut bare = checkout();
        *bare.get_mut("is_bare").unwrap() = json!(true);
        let mut detached = checkout();
        *detached.get_mut("is_detached").unwrap() = json!(true);
        for (root, worktrees) in [
            ("/another/repo", vec![]),
            ("/src/example", vec![checkout(), checkout()]),
            ("/src/example", vec![prunable]),
            ("/src/example", vec![bare]),
            ("/src/example", vec![detached]),
        ] {
            let server = Server::start(vec![Step::reply(listing(root, &worktrees))]);
            assert!(request().open(&server.client()).is_err());
            assert_eq!(server.requests().len(), 1);
        }
    }

    #[test]
    fn boundary_validation_rejects_bad_paths_and_git_refs() {
        for root in ["", "relative", "/src/../repo", "/src/\0repo"] {
            assert!(Request::new(root.into(), "task".into(), "Task".into(), false).is_err());
        }
        for branch in [
            "",
            "HEAD",
            "@",
            "-task",
            "task.",
            "task..a",
            "task@{1}",
            "task name",
            "task\n",
            "task\\name",
            "task?",
            "task*",
            "task[",
            "task~",
            "task^",
            "task:",
            "/task",
            "task/",
            "task//a",
            ".task",
            "task/.a",
            "task.lock",
            "task/a.lock",
        ] {
            assert!(!valid_branch(branch), "accepted {branch:?}");
        }
        for branch in ["task", "eng-12/task", "cafe\u{301}/👨‍👩‍👧", "task@name"] {
            assert!(valid_branch(branch), "rejected {branch:?}");
        }
    }
}
