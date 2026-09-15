//! Local issue handoffs into native Herdr worktree workspaces.
//!
//! Linear supplies context through its custom-script integration; agents supply
//! the same input after reading an issue through their own MCP connection.
mod agent;
mod cli;
mod store;

use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result, ensure};
use herdrkit::{Client, PaneId, TabId, WorkspaceId};
use serde::Serialize;

pub use cli::run;

/// Validated, provider-independent input to the local workspace operation.
#[derive(Debug, Clone, Serialize)]
pub struct Issue {
    identifier: String,
    branch: String,
    uuid: Option<String>,
    prompt: Option<String>,
}

impl Issue {
    /// Branch validation is shared with the worktree boundary.
    pub fn new(
        identifier: String,
        branch: String,
        uuid: Option<String>,
        prompt: Option<String>,
    ) -> Result<Self> {
        ensure!(
            !identifier.is_empty()
                && identifier.len() <= 128
                && identifier
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-'),
            "issue identifier must contain only letters, digits and hyphens (up to 128 bytes)"
        );
        if let Some(uuid) = &uuid {
            ensure!(
                uuid.len() == 36
                    && uuid
                        .bytes()
                        .enumerate()
                        .all(|(i, b)| if [8, 13, 18, 23].contains(&i) {
                            b == b'-'
                        } else {
                            b.is_ascii_hexdigit()
                        }),
                "invalid issue UUID"
            );
        }
        if let Some(prompt) = &prompt {
            ensure!(
                prompt.len() <= 1024 * 1024 && !prompt.contains('\0'),
                "issue prompt must be at most 1 MiB and contain no NUL"
            );
        }
        // Validate the branch before any state or workspace mutations.
        herdrkit::worktree::Request::new(
            PathBuf::from("/"),
            branch.clone(),
            identifier.clone(),
            false,
        )?;
        Ok(Self {
            identifier,
            branch,
            uuid: uuid.map(|value| value.to_ascii_lowercase()),
            prompt,
        })
    }
}

/// Stable response shape for agent callers; IDs come from Herdr's response.
#[derive(Debug, Serialize)]
pub struct Opened {
    pub workspace_id: WorkspaceId,
    pub tab_id: TabId,
    pub pane_id: PaneId,
    pub checkout: PathBuf,
    pub state_database: PathBuf,
}

/// Open an issue in the supplied session. State is private and outside the repo.
///
/// A durable pending binding precedes workspace mutation. A SQLite writer lock
/// serializes launchers sharing this state directory, and the shared worktree
/// operation rechecks live state while holding that lock. A crash leaves a
/// recoverable binding; retrying never selects a replacement branch name.
pub fn open(
    client: &Client,
    state_dir: &Path,
    repo: &Path,
    issue: &Issue,
    focus: bool,
) -> Result<Opened> {
    // Normalize subdirectories and linked checkouts using Herdr's repository
    // resolver, so all callers share one repository identity.
    let listing = client
        .worktree_list(repo)
        .context("resolving the selected repository")?;
    let root = std::fs::canonicalize(&listing.source.repository.root)
        .context("resolving repository root")?;
    let request = herdrkit::worktree::Request::new(
        root.clone(),
        issue.branch.clone(),
        issue.identifier.clone(),
        focus,
    )?;
    let mut store = store::Store::open(state_dir)?;
    store.reserve(&root, issue)?;
    let transaction = store.lock()?;
    store::verify(&transaction, &root, issue)?;
    let opened = request.open(client).context("opening issue workspace")?;
    store::complete(&transaction, &root, issue, &opened.worktree.checkout_path)?;
    transaction
        .commit()
        .context("saving issue workspace binding")?;
    Ok(Opened {
        workspace_id: opened.workspace.workspace_id,
        tab_id: opened.tab.tab_id,
        pane_id: opened.root_pane.pane_id,
        checkout: opened.worktree.checkout_path,
        state_database: store.path().to_path_buf(),
    })
}

/// Read the last supplied issue context by repository and branch.
pub fn context(client: &Client, state_dir: &Path, repo: &Path, branch: &str) -> Result<Issue> {
    let listing = client
        .worktree_list(repo)
        .context("resolving the selected repository")?;
    let root = std::fs::canonicalize(&listing.source.repository.root)
        .context("resolving repository root")?;
    store::Store::open(state_dir)?.context(&root, branch)
}

/// Forget only the local binding and saved context; keep the checkout/workspace.
/// Use explicitly when reconciling a renamed issue or branch.
pub fn unbind(client: &Client, state_dir: &Path, repo: &Path, branch: &str) -> Result<()> {
    let listing = client
        .worktree_list(repo)
        .context("resolving the selected repository")?;
    let root = std::fs::canonicalize(&listing.source.repository.root)
        .context("resolving repository root")?;
    store::Store::open(state_dir)?.unbind(&root, branch)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_invalid_boundary_data() {
        assert!(Issue::new("ENG-12".into(), "--force".into(), None, None).is_err());
        assert!(Issue::new("ENG-12\n".into(), "eng-12".into(), None, None).is_err());
        assert!(
            Issue::new(
                "ENG-12".into(),
                "eng-12".into(),
                Some("not-a-uuid".into()),
                None
            )
            .is_err()
        );
        assert!(Issue::new("ENG-12".into(), "eng-12".into(), None, Some("\0".into())).is_err());
    }

    #[test]
    fn prompt_is_data_and_preserves_unicode() {
        let prompt = "Fix cafe\u{301} 👨‍👩‍👧 and show `$(whoami)` literally.";
        let issue =
            Issue::new("ENG-12".into(), "eng-12".into(), None, Some(prompt.into())).unwrap();
        let json = serde_json::to_string(&issue).unwrap();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&json)
                .unwrap()
                .get("prompt")
                .unwrap(),
            prompt
        );
    }
}

#[cfg(test)]
mod integration_tests {
    use super::*;
    use herdrkit::testing::{Server, Step};
    use serde_json::json;
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);

    fn reply(result: &serde_json::Value) -> String {
        format!(r#"{{"id":"{{id}}","result":{result}}}"#)
    }

    #[test]
    fn lost_response_then_repeated_launch_preserve_workspace_and_context() {
        let directory = std::env::temp_dir().join(format!(
            "herdr-issue-workflow-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(directory.join("repository")).unwrap();
        let root = std::fs::canonicalize(directory.join("repository")).unwrap();
        let checkout = root.join("task");
        std::fs::create_dir_all(&checkout).unwrap();
        let worktree = json!({"path":checkout,"label":"ENG-12","branch":"eng-12","is_bare":false,"is_detached":false,"is_prunable":false,"is_linked_worktree":true});
        let source = json!({"repo_key":"example","repo_name":"example","repo_root":root,"source_checkout_path":root});
        let empty = reply(&json!({"type":"worktree_list","source":source,"worktrees":[]}));
        let existing =
            reply(&json!({"type":"worktree_list","source":source,"worktrees":[worktree]}));
        let opened = reply(
            &json!({"type":"worktree_opened","workspace":{"workspace_id":"w3","label":"ENG-12","focused":false,"agent_status":"idle"},"tab":{"tab_id":"w3:t1","workspace_id":"w3"},"root_pane":{"pane_id":"w3:p1","tab_id":"w3:t1","workspace_id":"w3","cwd":checkout},"worktree":worktree,"already_open":true}),
        );
        let server = Server::start(vec![
            Step::reply(empty.clone()),
            Step::reply(empty),
            Step::reply(
                r#"{"id":"{id}","error":{"code":"timeout","message":"creation response lost"}}"#,
            ),
            Step::reply(existing.clone()),
            Step::reply(opened.clone()),
            Step::reply(existing.clone()),
            Step::reply(existing.clone()),
            Step::reply(opened),
            Step::reply(existing.clone()),
            Step::reply(existing.clone()),
            Step::reply(existing),
        ]);
        let state = directory.join("state");
        let first = open(
            &server.client(),
            &state,
            &root,
            &Issue::new(
                "ENG-12".into(),
                "eng-12".into(),
                None,
                Some("Fix cafe\u{301} 👨‍👩‍👧".into()),
            )
            .unwrap(),
            false,
        )
        .unwrap();
        let second = open(
            &server.client(),
            &state,
            &root,
            &Issue::new("ENG-12".into(), "eng-12".into(), None, None).unwrap(),
            false,
        )
        .unwrap();
        assert_eq!(first.workspace_id, second.workspace_id);
        assert_eq!(first.pane_id, second.pane_id);
        assert_eq!(
            context(&server.client(), &state, &root, "eng-12")
                .unwrap()
                .prompt
                .as_deref(),
            Some("Fix cafe\u{301} 👨‍👩‍👧")
        );
        for kind in ["codex", "claude"] {
            let command = crate::agent::command(&server.client(), &state, &checkout, kind).unwrap();
            assert_eq!(command.get_program(), kind);
            assert_eq!(command.get_current_dir(), Some(checkout.as_path()));
            let args = command.get_args().collect::<Vec<_>>();
            assert_eq!(*args.first().unwrap(), "--");
            assert!(
                args.get(1)
                    .unwrap()
                    .to_str()
                    .unwrap()
                    .contains("Fix cafe\u{301} 👨‍👩‍👧")
            );
            assert_eq!(args.len(), 2);
        }
        let requests = server.requests();
        assert_eq!(
            requests
                .iter()
                .filter(|request| request.method == "worktree.create")
                .count(),
            1
        );
        assert!(
            requests
                .iter()
                .filter(|request| request.method == "worktree.open")
                .all(|request| request.params.get("focus") == Some(&json!(false)))
        );
        assert!(
            requests
                .iter()
                .all(|request| !request.method.starts_with("agent."))
        );
        drop(server);
        std::fs::remove_dir_all(directory).unwrap();
    }
}
