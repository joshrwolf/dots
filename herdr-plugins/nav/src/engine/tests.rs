use super::*;
use herdrkit::testing::{Server, Step};
use serde_json::{Value, json};
use std::sync::atomic::{AtomicU64, Ordering};

pub(crate) struct Scratch(std::path::PathBuf);
impl Scratch {
    pub(crate) fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "nav-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
    pub(crate) fn path(&self) -> std::path::PathBuf {
        self.0.join("history")
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
pub(crate) fn snapshot(focus: &str, destinations: &[(&str, &str)]) -> Step {
    // Fixture names encode workspace membership for brevity; production IDs
    // stay opaque and use the explicit workspace_id from Herdr.
    let workspace = |pane: &str| pane.split(':').next().unwrap().to_owned();
    let workspaces: std::collections::BTreeSet<_> = destinations
        .iter()
        .map(|(_, pane)| workspace(pane))
        .collect();
    reply(
        &json!({"type":"session_snapshot","snapshot":{"version":"0.9.1",
        "focused_workspace_id": workspace(focus),
        "workspaces": workspaces.iter().map(|id| json!({"workspace_id": id})).collect::<Vec<_>>(),
        "focused_pane_id": focus,"panes": destinations.iter().map(|(terminal, pane)|
            json!({"terminal_id":terminal,"pane_id":pane,"workspace_id":workspace(pane)})).collect::<Vec<_>>()}}),
    )
}
pub(crate) fn read(focus: &str) -> Step {
    snapshot(focus, &[("A", "w1:p1"), ("B", "w2:p2"), ("C", "w3:p3")])
}
pub(crate) fn reply(result: &Value) -> Step {
    Step::reply(json!({"id":"{id}","result":result}).to_string())
}
pub(crate) fn focused(pane: &str) -> Step {
    reply(&json!({"type":"pane_info","pane":{
        "pane_id":pane,"workspace_id":"w1","tab_id":"w1:t1"}}))
}
fn names(engine: &Engine) -> Value {
    engine.status().get("history").unwrap().clone()
}

#[test]
fn confirmed_traversal_and_late_wakes_do_not_record_jumps() {
    let root = Scratch::new();
    let server = Server::start(vec![
        read("w1:p1"),
        read("w2:p2"),
        read("w3:p3"),
        read("w3:p3"),
        focused("w2:p2"),
        read("w2:p2"),
        read("w2:p2"),
        read("w2:p2"),
        read("w2:p2"),
        focused("w3:p3"),
        read("w3:p3"),
    ]);
    let mut engine = Engine::open(server.client(), &root.path()).unwrap();
    engine.refresh().unwrap();
    engine.refresh().unwrap();
    engine.travel(Travel::Back).unwrap();
    engine.refresh().unwrap();
    engine.refresh().unwrap();
    assert_eq!(
        names(&engine),
        json!({"entries":["A","B","C"],"current":"B"})
    );
    engine.travel(Travel::Forward).unwrap();
    assert_eq!(
        names(&engine),
        json!({"entries":["A","B","C"],"current":"C"})
    );
    assert_eq!(
        server
            .requests()
            .iter()
            .filter(|r| r.method == "pane.focus")
            .count(),
        2
    );
}

#[test]
fn rejection_preserves_cursor_and_clears_reservation() {
    let root = Scratch::new();
    let server = Server::start(vec![
        read("w1:p1"),
        read("w2:p2"),
        read("w2:p2"),
        Step::reply(r#"{"id":"{id}","error":{"code":"pane_not_found","message":"closed"}}"#),
    ]);
    let mut engine = Engine::open(server.client(), &root.path()).unwrap();
    engine.refresh().unwrap();
    assert!(engine.travel(Travel::Back).is_err());
    assert_eq!(names(&engine), json!({"entries":["A","B"],"current":"B"}));
    assert_eq!(engine.status().get("pending").unwrap(), &Value::Null);
}

#[test]
fn lost_response_recovers_without_replaying_focus() {
    let root = Scratch::new();
    let server = Server::start(vec![
        read("w1:p1"),
        read("w2:p2"),
        read("w2:p2"),
        Step::Close,
        read("w1:p1"),
    ]);
    let mut engine = Engine::open(server.client(), &root.path()).unwrap();
    engine.refresh().unwrap();
    assert!(engine.travel(Travel::Back).is_err());
    assert_eq!(engine.status().get("pending").unwrap(), "A");
    drop(engine);
    let recovered = Engine::open(server.client(), &root.path()).unwrap();
    assert_eq!(
        names(&recovered),
        json!({"entries":["A","B"],"current":"A"})
    );
    assert_eq!(recovered.status().get("pending").unwrap(), &Value::Null);
    assert_eq!(server.requests().last().unwrap().method, "session.snapshot");
}

#[test]
fn competing_focus_is_recorded_and_reported() {
    let root = Scratch::new();
    let server = Server::start(vec![
        read("w1:p1"),
        read("w2:p2"),
        read("w3:p3"),
        read("w3:p3"),
        focused("w2:p2"),
        read("w1:p1"),
    ]);
    let mut engine = Engine::open(server.client(), &root.path()).unwrap();
    engine.refresh().unwrap();
    engine.refresh().unwrap();
    assert!(
        engine
            .travel(Travel::Back)
            .unwrap_err()
            .to_string()
            .contains("focus changed")
    );
    assert_eq!(
        names(&engine),
        json!({"entries":["B","C","A"],"current":"A"})
    );
}

#[test]
fn identity_follows_moves_and_prunes_closed_destinations() {
    let root = Scratch::new();
    let server = Server::start(vec![
        read("w1:p1"),
        read("w2:p2"),
        read("w3:p3"),
        snapshot("w3:p3", &[("A", "w9:p8"), ("C", "w3:p3")]),
        focused("w9:p8"),
        snapshot("w9:p8", &[("A", "w9:p8"), ("C", "w3:p3")]),
    ]);
    let mut engine = Engine::open(server.client(), &root.path()).unwrap();
    engine.refresh().unwrap();
    engine.refresh().unwrap();
    engine.travel(Travel::Back).unwrap();
    assert_eq!(names(&engine), json!({"entries":["A","C"],"current":"A"}));
    let requests = server.requests();
    assert_eq!(
        requests
            .iter()
            .find(|r| r.method == "pane.focus")
            .unwrap()
            .params
            .get("pane_id")
            .unwrap(),
        "w9:p8"
    );
}

#[test]
fn old_or_incoherent_snapshots_fail_before_focus() {
    for value in [
        json!({"version":"0.9.0","panes":[],"focused_pane_id":null}),
        json!({"version":"0.9.1","panes":[],"focused_pane_id":"missing"}),
        json!({"version":"0.9.1","panes":[{"pane_id":"w1:p1","terminal_id":""}],"focused_pane_id":"w1:p1"}),
        json!({"version":"0.9.1","panes":[{"pane_id":"w1:p1"}],"focused_pane_id":"w1:p1"}),
    ] {
        let root = Scratch::new();
        let server = Server::start(vec![reply(
            &json!({"type":"session_snapshot","snapshot":value}),
        )]);
        assert!(Engine::open(server.client(), &root.path()).is_err());
        assert_eq!(server.requests().len(), 1);
    }
}

#[test]
fn invalid_checkpoint_is_not_reset_or_used() {
    for record in [
        json!({"version":2,"history":{"entries":[],"current":null},"pending":null}),
        json!({"version":1,"history":{"entries":["A"],"current":"A"},"pending":"B"}),
    ] {
        let root = Scratch::new();
        let mut file = StateFile::lock(&root.path()).unwrap();
        file.write(&record).unwrap();
        drop(file);
        let server = Server::start(vec![]);
        assert!(Engine::open(server.client(), &root.path()).is_err());
        assert!(server.requests().is_empty());
    }
}

#[test]
fn replaced_server_is_not_queried_or_focused_with_old_history() {
    let root = Scratch::new();
    let original = Server::start(vec![read("w1:p1")]);
    let mut engine = Engine::open(original.client(), &root.path()).unwrap();
    let replacement = Server::start(vec![]);
    engine.client = replacement.client();
    assert!(engine.travel(Travel::Back).is_err());
    assert!(replacement.requests().is_empty());
}

#[test]
fn reservation_write_failure_prevents_focus() {
    let root = Scratch::new();
    let server = Server::start(vec![read("w1:p1"), read("w2:p2"), read("w2:p2")]);
    let mut engine = Engine::open(server.client(), &root.path()).unwrap();
    engine.refresh().unwrap();
    // A directory at the temporary-record path forces publication to fail.
    std::fs::create_dir(root.path().join("record.next")).unwrap();
    assert!(engine.travel(Travel::Back).is_err());
    assert!(
        server
            .requests()
            .iter()
            .all(|request| request.method != "pane.focus")
    );
}

#[test]
fn workspace_toggle_ignores_tabs_and_survives_worker_restart() {
    let root = Scratch::new();
    let read = |focus| snapshot(focus, &[("A", "w1:p1"), ("B", "w2:p2"), ("C", "w2:p3")]);
    let server = Server::start(vec![
        read("w1:p1"),
        read("w2:p2"),
        read("w2:p3"),
        read("w2:p3"),
        reply(&json!({"type":"ok"})),
        read("w1:p1"),
        read("w1:p1"), // restored worker
        read("w1:p1"),
        reply(&json!({"type":"ok"})),
        read("w2:p3"),
    ]);
    let mut engine = Engine::open(server.client(), &root.path()).unwrap();
    engine.refresh().unwrap();
    engine.refresh().unwrap();
    engine.previous_workspace().unwrap();
    drop(engine);
    let mut engine = Engine::open(server.client(), &root.path()).unwrap();
    engine.previous_workspace().unwrap();
    assert_eq!(
        engine.checkpoint.workspaces.current,
        Some(herdrkit::WorkspaceId::new("w2"))
    );
    assert_eq!(
        engine.checkpoint.workspaces.previous,
        Some(herdrkit::WorkspaceId::new("w1"))
    );
    let requests = server.requests();
    let focuses: Vec<_> = requests
        .iter()
        .filter(|r| matches!(r.method.as_str(), "pane.focus" | "workspace.focus"))
        .collect();
    assert_eq!(focuses.len(), 2);
    assert_eq!(
        focuses
            .iter()
            .map(|r| r.method.as_str())
            .collect::<Vec<_>>(),
        ["workspace.focus", "workspace.focus"]
    );
    assert_eq!(
        focuses.first().unwrap().params,
        json!({"workspace_id":"w1"})
    );
    assert_eq!(focuses.last().unwrap().params, json!({"workspace_id":"w2"}));
}

#[test]
fn uncertain_workspace_switch_recovers_actual_focus_without_replay() {
    for actual in ["w1:p1", "w2:p2", "w3:p3"] {
        let root = Scratch::new();
        let server = Server::start(vec![
            read("w1:p1"),
            read("w2:p2"),
            read("w2:p2"),
            Step::reply(r#"{"id":"{id}""#),
            read(actual),
        ]);
        let mut engine = Engine::open(server.client(), &root.path()).unwrap();
        engine.refresh().unwrap();
        assert!(engine.previous_workspace().is_err());
        assert_eq!(
            engine.checkpoint.workspaces.pending,
            Some(herdrkit::WorkspaceId::new("w1"))
        );
        drop(engine);
        let engine = Engine::open(server.client(), &root.path()).unwrap();
        assert_eq!(engine.checkpoint.workspaces.pending, None);
        assert_eq!(
            engine
                .checkpoint
                .workspaces
                .current
                .as_ref()
                .unwrap()
                .as_str(),
            actual.split(':').next().unwrap()
        );
        assert_eq!(
            server
                .requests()
                .iter()
                .filter(|r| r.method == "workspace.focus")
                .count(),
            1
        );
    }
}

#[test]
fn closed_previous_workspace_and_no_history_are_noops() {
    let root = Scratch::new();
    let server = Server::start(vec![
        read("w1:p1"),
        read("w1:p1"),
        read("w2:p2"),
        snapshot("w2:p2", &[("B", "w2:p2")]),
    ]);
    let mut engine = Engine::open(server.client(), &root.path()).unwrap();
    engine.previous_workspace().unwrap();
    engine.refresh().unwrap();
    engine.previous_workspace().unwrap();
    assert_eq!(engine.checkpoint.workspaces.previous, None);
    assert!(
        server
            .requests()
            .iter()
            .all(|r| r.method != "workspace.focus")
    );
}

#[test]
fn rejected_workspace_switch_keeps_pair_and_clears_pending() {
    let root = Scratch::new();
    let server = Server::start(vec![
        read("w1:p1"),
        read("w2:p2"),
        read("w2:p2"),
        Step::reply(r#"{"id":"{id}","error":{"code":"workspace_not_found","message":"closed"}}"#),
    ]);
    let mut engine = Engine::open(server.client(), &root.path()).unwrap();
    engine.refresh().unwrap();
    assert!(engine.previous_workspace().is_err());
    assert_eq!(engine.checkpoint.workspaces.pending, None);
    assert_eq!(
        engine.checkpoint.workspaces.current,
        Some(herdrkit::WorkspaceId::new("w2"))
    );
    assert_eq!(
        engine.checkpoint.workspaces.previous,
        Some(herdrkit::WorkspaceId::new("w1"))
    );
}

#[test]
fn failed_workspace_reservation_prevents_focus() {
    let root = Scratch::new();
    let server = Server::start(vec![read("w1:p1"), read("w2:p2"), read("w2:p2")]);
    let mut engine = Engine::open(server.client(), &root.path()).unwrap();
    engine.refresh().unwrap();
    std::fs::create_dir(root.path().join("record.next")).unwrap();
    assert!(engine.previous_workspace().is_err());
    assert!(
        server
            .requests()
            .iter()
            .all(|r| r.method != "workspace.focus")
    );
}

#[test]
fn pane_checkpoint_upgrade_preserves_history_and_seeds_workspace_pair() {
    let root = Scratch::new();
    let mut file = StateFile::lock(&root.path()).unwrap();
    file.write(&json!({"version":1,"history":{"entries":["A","B"],"current":"B"},"pending":null}))
        .unwrap();
    drop(file);
    let server = Server::start(vec![read("w2:p2")]);
    let engine = Engine::open(server.client(), &root.path()).unwrap();
    assert_eq!(names(&engine), json!({"entries":["A","B"],"current":"B"}));
    assert_eq!(
        engine.checkpoint.workspaces.current,
        Some(herdrkit::WorkspaceId::new("w2"))
    );
    assert_eq!(engine.checkpoint.workspaces.previous, None);
}

#[test]
fn competing_workspace_focus_is_recorded_and_reported() {
    let root = Scratch::new();
    let server = Server::start(vec![
        read("w1:p1"),
        read("w2:p2"),
        read("w2:p2"),
        reply(&json!({"type":"ok"})),
        read("w3:p3"),
    ]);
    let mut engine = Engine::open(server.client(), &root.path()).unwrap();
    engine.refresh().unwrap();
    assert!(
        engine
            .previous_workspace()
            .unwrap_err()
            .to_string()
            .contains("focus changed")
    );
    assert_eq!(
        engine.checkpoint.workspaces.current,
        Some(herdrkit::WorkspaceId::new("w3"))
    );
    assert_eq!(
        engine.checkpoint.workspaces.previous,
        Some(herdrkit::WorkspaceId::new("w2"))
    );
    assert_eq!(engine.checkpoint.workspaces.pending, None);
}

#[test]
fn empty_workspaces_participate_without_a_focused_pane() {
    let root = Scratch::new();
    let empty = |focus| {
        reply(&json!({"type":"session_snapshot","snapshot":{
            "version":"0.9.1","panes":[],"focused_pane_id":null,
            "focused_workspace_id":focus,"workspaces":[{"workspace_id":"w1"},{"workspace_id":"w2"}]
        }}))
    };
    let server = Server::start(vec![
        empty("w1"),
        empty("w2"),
        empty("w2"),
        reply(&json!({"type":"ok"})),
        empty("w1"),
    ]);
    let mut engine = Engine::open(server.client(), &root.path()).unwrap();
    engine.refresh().unwrap();
    engine.previous_workspace().unwrap();
    assert_eq!(
        engine.checkpoint.workspaces.current,
        Some(herdrkit::WorkspaceId::new("w1"))
    );
    assert_eq!(
        engine.checkpoint.workspaces.previous,
        Some(herdrkit::WorkspaceId::new("w2"))
    );
}
