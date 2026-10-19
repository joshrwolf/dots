#![cfg(feature = "testing")]

use herdrkit::testing::{Server, Step};
use serde_json::Value;
use std::process::{Command, Stdio};

#[test]
fn simultaneous_starts_share_one_ready_process_and_control_socket() {
    let root = std::env::temp_dir().join(format!("herdrkit-service-test-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let server = Server::start(vec![Step::reply(
        r#"{"id":"{id}","result":{"type":"plugin_list","plugins":[{"plugin_id":"fixture","plugin_root":"/fixture","enabled":true}]}}"#,
    )]);
    let command = || {
        let mut command = Command::new(env!("CARGO_BIN_EXE_herdrkit-service-fixture"));
        command
            .env("HERDR_SOCKET_PATH", server.client().endpoint())
            .env("HERDR_PLUGIN_ID", "fixture")
            .env("HERDR_PLUGIN_ROOT", "/fixture")
            .env("HERDR_PLUGIN_CONFIG_DIR", &root)
            .env("HERDR_PLUGIN_STATE_DIR", &root)
            .env("HERDR_PLUGIN_CONTEXT_JSON", "{}")
            .env("HERDR_PLUGIN_EVENT", "startup")
            .env_remove("HERDR_PLUGIN_EVENT_JSON")
            .env_remove("HERDR_PLUGIN_ACTION_ID")
            .env_remove("HERDR_PLUGIN_ENTRYPOINT_ID")
            .env_remove("HERDRKIT_SERVICE_CHILD")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        command
    };
    let children: Vec<_> = (0..4).map(|_| command().spawn().unwrap()).collect();
    let results: Vec<_> = children
        .into_iter()
        .map(|child| child.wait_with_output().unwrap())
        .collect();
    // Always stop our owned worker before asserting, including on test failures.
    let wake = command().arg("wake").output().unwrap();
    let stopped = command().arg("stop").output().unwrap();
    assert!(
        stopped.status.success(),
        "{}",
        String::from_utf8_lossy(&stopped.stderr)
    );
    assert!(
        wake.status.success(),
        "{}",
        String::from_utf8_lossy(&wake.stderr)
    );
    let wake: Value = serde_json::from_slice(&wake.stdout).unwrap();
    assert_eq!(wake.get("requests").and_then(Value::as_u64), Some(1));
    for result in results {
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        let ready: Value = serde_json::from_slice(&result.stdout).unwrap();
        assert_eq!(ready.get("pid"), wake.get("pid"));
    }
    // Only fixture-owned files; control shutdown acknowledged above.
    std::fs::remove_dir_all(root).unwrap();
}
