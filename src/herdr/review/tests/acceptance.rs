//! Ordinary editor -> real stdio bridge -> repository store -> reopened editor.
#![cfg(test)]
mod support;

use std::path::Path;
use std::process::Command;
use std::time::Duration;

use herdrkit::{WorkspaceId, testing::Server};
use review_core::{
    CheckoutKind, ComparisonSpec, CreateBoundThread, DiffEndpoint, NewThread, Repository, Store,
    ThreadStatus,
};
use serde_json::json;
use support::Checkout;

#[test]
fn discussion_reply_and_resolution_survive_editor_restart() {
    let fixture = Checkout::new();
    let server = Server::start(vec![]);
    let client = server.client();
    let repository = Repository::discover(&fixture.0).unwrap();
    let mut store = Store::open(&repository).unwrap();
    let thread: NewThread = serde_json::from_value(json!({
        "anchor": { "original": { "path": "example.txt", "side": "target", "start_line": 1, "end_line": 1 },
                    "context": { "selected": ["changed"] } },
        "body": "Explain this change", "author": "reviewer"
    })).unwrap();
    let state = store
        .create_bound_thread(
            &CreateBoundThread::new(
                "Example review",
                vec![],
                ComparisonSpec::new(
                    DiffEndpoint::Commit {
                        oid: fixture.git(&["rev-parse", "HEAD"]),
                    },
                    DiffEndpoint::WorkingTree,
                )
                .unwrap(),
                client.server_id().unwrap(),
                WorkspaceId::new("w1"),
                None,
                CheckoutKind::Existing,
                thread,
            )
            .unwrap(),
        )
        .unwrap();
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    for phase in ["write", "read"] {
        let mut command = Command::new("nvim");
        command
            .args(["--headless", "-u", "NONE", "-i", "NONE", "-l"])
            .arg(manifest.join("tests/discussion.lua"))
            .current_dir(&fixture.0)
            .env(
                "REVIEW_CONFIG",
                manifest.join("../../../home/nvim/.config/nvim"),
            )
            .env("REVIEW_BINARY", env!("CARGO_BIN_EXE_herdr-review"))
            .env("REVIEW_PHASE", phase)
            .env("HERDR_ENV", "1")
            .env("HERDR_SOCKET_PATH", client.endpoint())
            // Deliberately stale inherited IDs: current-pane discovery must
            // override these in the bridge child, without changing Neovim's env.
            .env("HERDR_WORKSPACE_ID", "w-old")
            .env("HERDR_TAB_ID", "w-old:t1")
            .env("HERDR_PANE_ID", "w-old:p1")
            .env_remove("HERDR_REVIEW_CATALOG_DIR")
            .env("XDG_STATE_HOME", fixture.0.join("state"))
            .env("XDG_CACHE_HOME", fixture.0.join("cache"))
            .env("NVIM_LOG_FILE", fixture.0.join("nvim.log"));
        let output = dots_process::output(command, Duration::from_secs(30), 64 * 1024).unwrap();
        assert!(
            output.status.success(),
            "{phase}: {}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let loaded = store.binding_state_page(&state.binding.id, None).unwrap();
    let thread = loaded.threads.first().unwrap();
    assert_eq!(thread.status, ThreadStatus::Resolved);
    assert_eq!(thread.messages.len(), 2);
    assert_eq!(thread.messages.last().unwrap().body, "Reviewed café 👩‍💻");
}
