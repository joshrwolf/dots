use std::path::Path;

use anyhow::{Context as _, Result};
use herdrkit::Client;
use herdrkit::api::AgentRef;

use crate::Destination;

pub fn open(client: &Client, destination: &Destination) -> Result<()> {
    match destination {
        Destination::Workspace { id, .. } => client
            .workspace_focus(id)
            .with_context(|| format!("focusing {id}")),
        Destination::Agent { pane, name, .. } => client
            .agent_focus(AgentRef::Pane(pane))
            .with_context(|| format!("focusing the agent {name} in {pane}")),
        // Focus-or-create against herdr's own record of which workspace owns
        // the checkout, so no path bookkeeping is needed here.
        Destination::Worktree {
            path, repository, ..
        } => client
            .worktree_open(&repository.root, path, None, true)
            .map(|_| ())
            .with_context(|| format!("opening the worktree at {}", path.display())),
        Destination::Directory { path, .. } => open_dir(client, path),
    }
}

/// A directory has no identity in herdr's API until something opens it, so
/// this is the only route that needs a fallback chain.
fn open_dir(client: &Client, path: &Path) -> Result<()> {
    let dir =
        std::fs::canonicalize(path).with_context(|| format!("resolving {}", path.display()))?;

    // The presence of `.git` is exactly the test for a checkout root: a file
    // in a linked worktree, a directory in the main one, and absent in a
    // subdirectory, which is where `worktree.open` would pick the wrong root.
    if dir.join(".git").exists() {
        return client
            .worktree_open(&dir, &dir, None, true)
            .map(|_| ())
            .with_context(|| format!("opening a workspace for {}", dir.display()));
    }

    let snapshot = client.snapshot().context("reading the session")?;
    if let Some(pane) = snapshot
        .panes
        .iter()
        .find(|pane| pane.cwd.as_deref() == Some(dir.as_path()))
    {
        return client.workspace_focus(&pane.workspace_id).with_context(|| {
            format!(
                "focusing the workspace already sitting in {}",
                dir.display()
            )
        });
    }

    let label = dir.file_name().and_then(|name| name.to_str());
    client
        .workspace_create(&dir, label, true)
        .map(|_| ())
        .with_context(|| format!("creating a workspace for {}", dir.display()))
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;
    use herdrkit::api::{AgentStatus, Repository};
    use herdrkit::testing::{RecordedRequest, Server, Step};
    use herdrkit::{PaneId, WorkspaceId};
    use serde_json::json;

    const OK: &str = r#"{"id":"{id}","result":{"type":"ok"}}"#;
    static NEXT_TEMP: AtomicUsize = AtomicUsize::new(0);

    fn sent(server: &Server) -> Vec<RecordedRequest> {
        server.requests()
    }

    fn repository(name: &str, root: &str) -> Repository {
        Repository {
            key: name.to_owned(),
            name: name.to_owned(),
            root: root.into(),
        }
    }

    #[test]
    fn a_workspace_is_focused_by_its_id() {
        let server = Server::start(vec![Step::reply(OK)]);
        let workspace = Destination::Workspace {
            id: WorkspaceId::new("w2"),
            repository: Some(repository("dots", "/src/dots")),
            branch: "main".into(),
            display_path: "~/dots".into(),
            status: AgentStatus::Idle,
            focused: false,
        };
        open(&server.client(), &workspace).unwrap();
        assert_eq!(
            sent(&server),
            [RecordedRequest {
                method: "workspace.focus".into(),
                params: json!({"workspace_id": "w2"}),
            }]
        );
    }

    /// The pane id, not the display name: two agents can share a name once
    /// one of them has exited, and the pane is what the user pointed at.
    #[test]
    fn an_agent_is_focused_by_its_pane() {
        let server = Server::start(vec![Step::reply(OK)]);
        let agent = Destination::Agent {
            pane: PaneId::new("w2:p1"),
            name: "plugin-dev".into(),
            status: AgentStatus::Working,
            repository: Some(repository("dots", "/src/dots")),
            branch: "main".into(),
            task: "port".into(),
        };
        open(&server.client(), &agent).unwrap();
        assert_eq!(
            sent(&server),
            [RecordedRequest {
                method: "agent.focus".into(),
                params: json!({"target": "w2:p1"}),
            }]
        );
    }

    /// `cwd` names the repo and `path` the checkout, and the two are not
    /// interchangeable: herdr resolves the worktree list from `cwd`.
    #[test]
    fn a_worktree_opens_with_its_repo_root_as_cwd_and_its_path_as_path() {
        let server = Server::start(vec![Step::reply(
            r#"{"id":"{id}","result":{"type":"worktree_opened","workspace":{"workspace_id":"w3","label":"release","focused":true,"agent_status":"idle"},"tab":{"tab_id":"w3:t1","workspace_id":"w3"},"root_pane":{"pane_id":"w3:p1","workspace_id":"w3","tab_id":"w3:t1","cwd":"/wt/dots/release"},"worktree":{"path":"/wt/dots/release","label":"release","is_bare":false,"is_detached":false,"is_prunable":false,"is_linked_worktree":true,"branch":"release","open_workspace_id":"w3"},"already_open":false}}"#,
        )]);
        let worktree = Destination::Worktree {
            path: "/wt/dots/release".into(),
            repository: repository("dots", "/src/dots"),
            branch: "release".into(),
            display_path: "~/wt/dots/release".into(),
        };
        open(&server.client(), &worktree).unwrap();
        assert_eq!(
            sent(&server),
            [RecordedRequest {
                method: "worktree.open".into(),
                params: json!({"cwd": "/src/dots", "path": "/wt/dots/release", "focus": true}),
            }]
        );
    }

    #[test]
    fn a_rejection_names_what_was_being_opened() {
        let server = Server::start(vec![Step::reply(
            r#"{"id":"{id}","error":{"code":"not_found","message":"no such workspace"}}"#,
        )]);
        let workspace = Destination::Workspace {
            id: WorkspaceId::new("w9"),
            repository: None,
            branch: "-".into(),
            display_path: String::new(),
            status: AgentStatus::Unknown,
            focused: false,
        };
        let error = open(&server.client(), &workspace).unwrap_err();
        let message = format!("{error:#}");
        assert!(message.contains("focusing w9"), "{message}");
        assert!(message.contains("no such workspace"), "{message}");
    }

    #[test]
    fn an_existing_pane_directory_is_found_then_focused() {
        let sequence = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "herdr-find-dispatch-test-{}-{sequence}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).unwrap();
        let canonical = fs::canonicalize(&dir).unwrap();
        let snapshot = json!({
            "version": "0.9.0",
            "protocol": 22,
            "workspaces": [],
            "tabs": [],
            "panes": [{
                "pane_id": "w7:p2",
                "workspace_id": "w7",
                "tab_id": "w7:t1",
                "cwd": canonical,
            }],
            "agents": [],
        });
        let reply = format!(
            r#"{{"id":"{{id}}","result":{{"type":"session_snapshot","snapshot":{snapshot}}}}}"#
        );
        let server = Server::start(vec![Step::reply(reply), Step::reply(OK)]);

        open_dir(&server.client(), &dir).unwrap();
        assert_eq!(
            sent(&server),
            [
                RecordedRequest {
                    method: "session.snapshot".into(),
                    params: json!({}),
                },
                RecordedRequest {
                    method: "workspace.focus".into(),
                    params: json!({"workspace_id": "w7"}),
                },
            ]
        );

        let _ = fs::remove_dir_all(dir);
    }
}
