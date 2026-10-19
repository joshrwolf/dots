//! Ordinary editor -> real stdio bridge -> repository store -> reopened editor.
#![cfg(test)]
use std::fs;
use std::os::unix::fs::DirBuilderExt as _;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use herdrkit::{WorkspaceId, testing::Server};
use review_core::{
    CheckoutKind, ComparisonSpec, CreateBoundThread, DiffEndpoint, NewThread, Repository, Store,
    ThreadStatus,
};
use serde_json::json;

struct Checkout(PathBuf);
impl Checkout {
    fn new() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("review-editor-{}-{nonce}", std::process::id()));
        fs::DirBuilder::new().mode(0o700).create(&root).unwrap();
        let fixture = Self(fs::canonicalize(root).unwrap());
        fixture.git(&["init", "--quiet"]);
        fs::write(fixture.0.join("example.txt"), "original\n").unwrap();
        fixture.git(&["add", "example.txt"]);
        fixture.git(&["commit", "--quiet", "-m", "Initial fixture"]);
        fs::write(fixture.0.join("example.txt"), "changed\n").unwrap();
        fixture
    }
    fn git(&self, args: &[&str]) -> String {
        let mut command = Command::new("git");
        command
            .arg("-C")
            .arg(&self.0)
            .args([
                "-c",
                "user.name=Example",
                "-c",
                "user.email=example@example.invalid",
                "-c",
                "commit.gpgsign=false",
                "-c",
                "core.hooksPath=/dev/null",
            ])
            .args(args);
        let output = dots_process::output(command, Duration::from_secs(10), 64 * 1024).unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap().trim().into()
    }
}
impl Drop for Checkout {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn lua_behavior_suite() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let tests = manifest.join("tests");
    let lock: serde_json::Value =
        serde_json::from_slice(&fs::read(tests.join("nvim/nvim-pack-lock.json")).unwrap()).unwrap();
    let revision = lock
        .pointer("/plugins/mini.test/rev")
        .and_then(serde_json::Value::as_str)
        .unwrap();
    let dependency = manifest.join("../target/test-deps/nvim/site/pack/core/opt/mini.test");
    assert!(
        dependency.join("lua/mini/test.lua").is_file(),
        "run mise run //src/herdr/review:deps first"
    );
    for (args, expected) in [
        (vec!["rev-parse", "HEAD"], revision),
        (vec!["status", "--porcelain", "--untracked-files=all"], ""),
    ] {
        let mut command = Command::new("git");
        command.current_dir(&dependency).args(args);
        let output = dots_process::output(command, Duration::from_secs(10), 16 * 1024).unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let actual = String::from_utf8(output.stdout).unwrap();
        // vim.pack generates the help index; it is not dependency source.
        let actual = actual
            .lines()
            .filter(|line| *line != "?? doc/tags")
            .collect::<Vec<_>>()
            .join("\n");
        assert_eq!(actual.trim(), expected, "mini.test checkout changed");
    }
    let scratch = Checkout::new();
    let mut command = Command::new("nvim");
    command
        .args([
            "--headless",
            "-u",
            "NONE",
            "-i",
            "NONE",
            "-c",
            "lua dofile(vim.env.HERDR_REVIEW_TESTS .. '/lua/runner.lua')",
        ])
        .env("HERDR_REVIEW_TESTS", &tests)
        .env(
            "HERDR_REVIEW_CONFIG",
            manifest.join("../../../home/nvim/.config/nvim"),
        )
        .env("HERDR_REVIEW_SCRATCH", &scratch.0)
        .env("MINI_TEST_PATH", dependency)
        .env("NVIM_APPNAME", "nvim")
        .env("XDG_CONFIG_HOME", scratch.0.join("config"))
        .env("XDG_DATA_HOME", scratch.0.join("data"))
        .env("XDG_STATE_HOME", scratch.0.join("state"))
        .env("XDG_CACHE_HOME", scratch.0.join("cache"))
        .env("NVIM_LOG_FILE", scratch.0.join("nvim.log"));
    let output = dots_process::output(command, Duration::from_secs(90), 1024 * 1024).unwrap();
    assert!(
        output.status.success(),
        "mini.test failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

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
