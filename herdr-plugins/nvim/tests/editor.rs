//! mini.test behavior suite plus independent, typed cross-language contracts.
#[cfg(test)]
mod support;

use std::fs;
use std::process::Command;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use herdr_nvim::{Editors, RoutingError};
use serde::Deserialize as _;

use herdr_nvim::protocol::{ErrorCode, Identity, Location, Operation, Reply, Request, VERSION};
use herdrkit::testing::{RecordedRequest, Server, Step};
use herdrkit::{PaneId, TabId};
use serde_json::json;
use support::{Fixture, Scratch, tests_dir};

#[test]
fn lua_behavior_suite() {
    // Pin verification is offline. Installation is a separate explicit target.
    let mut verify = Command::new("python3");
    verify.arg(tests_dir().join("deps.py")).arg("check");
    let output = herdrkit::process::output(verify, Duration::from_secs(10), 16 * 1024).unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let dependency = String::from_utf8(output.stdout).unwrap();
    let scratch = Scratch::new();
    let mut command = support::command(&scratch.0);
    command.env("MINI_TEST_PATH", dependency.trim()).args([
        "-c",
        "lua dofile(vim.env.HERDR_NVIM_TESTS .. '/lua/runner.lua')",
    ]);
    let output = herdrkit::process::output(command, Duration::from_secs(90), 1024 * 1024).unwrap();
    let report = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    println!("{report}");
    assert!(output.status.success(), "mini.test failed:\n{report}");
}

#[test]
fn rust_requests_and_lua_replies_share_one_contract() {
    let fixture = Fixture::start();
    let context = fixture.ready.context.clone();
    let mut request = Request {
        protocol: VERSION,
        context: context.clone(),
        operation: Operation::Hello,
    };
    let identity = Identity {
        protocol: VERSION,
        context,
        pid: fixture.ready.pid,
    };
    assert_eq!(
        fixture.request(&request),
        Reply::Ready {
            identity: identity.clone()
        }
    );
    let path = fixture.scratch.0.join("a 'quoted\"\nλ.txt");
    fs::write(&path, "first\nsecond\nthird\n").unwrap();
    let target = Location {
        path,
        line: 2,
        column: 3,
    };
    request.operation = Operation::Open {
        target: target.clone(),
        not_after_ms: future_ms(20_000),
        lease: None,
    };
    assert_eq!(
        fixture.request(&request),
        Reply::Opened { identity, target }
    );

    // Mutate Rust-produced wire data, rather than maintain a parallel collection
    // of hand-written "valid" messages which can drift from the actual DTOs.
    let valid = serde_json::to_value(&request).unwrap();
    for (pointer, value, code) in [
        ("/protocol", json!(9), ErrorCode::ProtocolMismatch),
        (
            "/context/registration",
            json!("00000000000000000000000000000000"),
            ErrorCode::IdentityMismatch,
        ),
        (
            "/context/server",
            json!("/tmp/other.sock"),
            ErrorCode::IdentityMismatch,
        ),
        ("/operation/not_after_ms", json!(1), ErrorCode::Expired),
        (
            "/operation/target/path",
            json!("relative"),
            ErrorCode::InvalidTarget,
        ),
        (
            "/operation/target/line",
            json!(0),
            ErrorCode::InvalidPosition,
        ),
        (
            "/operation/target/line",
            json!(1.5),
            ErrorCode::InvalidPosition,
        ),
        (
            "/operation/target/column",
            json!(999),
            ErrorCode::InvalidPosition,
        ),
        (
            "/operation/method",
            json!("unknown"),
            ErrorCode::InvalidRequest,
        ),
    ] {
        let mut raw = valid.clone();
        *raw.pointer_mut(pointer).unwrap() = value;
        let Reply::Rejected { error } = fixture.raw(&raw.to_string()) else {
            panic!("expected rejection at {pointer}")
        };
        assert_eq!(error.code, code, "{pointer}: {error:?}");
    }
    for raw in ["null", "[]", "{}", "not JSON"] {
        let Reply::Rejected { error } = fixture.raw(raw) else {
            panic!("expected malformed request rejection")
        };
        assert_eq!(error.code, ErrorCode::InvalidRequest);
    }
}

#[test]
fn presentation_failures_have_a_typed_partial_reply() {
    let fixture = Fixture::start();
    let other = fixture.scratch.0.join("other.txt");
    fs::write(&other, "first\nsecond\nthird\n").unwrap();
    fixture.lua("fixture.fail_presentation(_A)", other.to_str().unwrap());
    let request = Request {
        protocol: VERSION,
        context: fixture.ready.context.clone(),
        operation: Operation::Open {
            target: Location {
                path: other,
                line: 2,
                column: 1,
            },
            not_after_ms: future_ms(20_000),
            lease: None,
        },
    };
    let Reply::Partial { error } = fixture.request(&request) else {
        panic!("expected partial presentation")
    };
    assert_eq!(error.code, ErrorCode::PresentationFailed);
}

#[test]
fn navigation_revalidates_and_focuses_without_process_scanning() {
    let fixture = Fixture::start();
    let token = &fixture.ready.context.registration;
    let pane = json!({"pane_id":"w1:p1","workspace_id":"w1","tab_id":"w1:t1","tokens":{"nvim_editor":token}});
    let moved = json!({"pane_id":"w1:p1","workspace_id":"w1","tab_id":"w1:t2","tokens":{"nvim_editor":token}});
    let listing = json!({"id":"{id}","result":{"type":"pane_list","panes":[pane]}}).to_string();
    let info = json!({"id":"{id}","result":{"type":"pane_info","pane":pane}}).to_string();
    let server = Server::start(vec![
        Step::reply(listing.clone()),
        Step::reply(info.clone()),
        Step::reply(info),
        Step::reply(listing),
        Step::reply(json!({"id":"{id}","result":{"type":"pane_info","pane":moved}}).to_string()),
    ]);
    let client = server.client();
    fixture.lua("fixture.configure(_A)", client.endpoint().to_str().unwrap());
    let path = fixture.scratch.0.join("example.txt");
    fs::write(&path, "first\nsecond\nthird\n").unwrap();
    let editors = Editors::new("nvim", fixture.ready.socket.parent().unwrap());
    let mut target = herdr_nvim::Target {
        path,
        line: 2,
        column: 3,
    };
    assert_eq!(
        editors
            .open(
                &client,
                &TabId::new("w1:t1"),
                Some(&PaneId::new("w1:p9")),
                &target
            )
            .unwrap(),
        PaneId::new("w1:p1")
    );
    let before = fixture.lua("fixture.snapshot()", "");
    target.path = fixture.scratch.0.join("should-not-open.txt");
    fs::write(&target.path, "different\nlocation\n").unwrap();
    target.line = 1;
    target.column = 1;
    let error = editors
        .open(&client, &TabId::new("w1:t1"), None, &target)
        .unwrap_err();
    assert_eq!(error.downcast_ref(), Some(&RoutingError::Changed));
    assert_eq!(fixture.lua("fixture.snapshot()", ""), before);
    assert_eq!(fixture.lua("fixture.received()", "").trim(), "1");
    assert_eq!(
        server.requests(),
        [
            RecordedRequest {
                method: "pane.list".into(),
                params: json!({})
            },
            RecordedRequest {
                method: "pane.get".into(),
                params: json!({"pane_id":"w1:p1"})
            },
            RecordedRequest {
                method: "pane.focus".into(),
                params: json!({"pane_id":"w1:p1"})
            },
            RecordedRequest {
                method: "pane.list".into(),
                params: json!({})
            },
            RecordedRequest {
                method: "pane.get".into(),
                params: json!({"pane_id":"w1:p1"})
            },
        ]
    );
}

#[cfg(test)]
fn future_ms(offset: u64) -> u64 {
    u64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis(),
    )
    .unwrap()
        + offset
}

#[test]
fn unavailable_registered_caller_never_falls_back_to_a_live_sibling() {
    let fixture = Fixture::start();
    let directory = fixture.ready.socket.parent().unwrap();
    let stale = "00000000000000000000000000000000";
    // A real owned socket path with no listener, not an invalid token or an
    // invented transport failure. The other registration is genuinely live.
    drop(std::os::unix::net::UnixListener::bind(directory.join(format!("{stale}.sock"))).unwrap());
    let caller = json!({"pane_id":"w1:p1","workspace_id":"w1","tab_id":"w1:t1","tokens":{"nvim_editor":stale}});
    let sibling = json!({"pane_id":"w1:p2","workspace_id":"w1","tab_id":"w1:t1","tokens":{"nvim_editor":fixture.ready.context.registration}});
    let server = Server::start(vec![
        Step::reply(
            json!({"id":"{id}","result":{"type":"pane_list","panes":[caller,sibling]}}).to_string(),
        ),
        Step::reply(json!({"id":"{id}","result":{"type":"pane_info","pane":caller}}).to_string()),
    ]);
    let client = server.client();
    fixture.lua("fixture.configure(_A)", client.endpoint().to_str().unwrap());
    let before = fixture.lua("fixture.snapshot()", "");
    let path = fixture.scratch.0.join("target.txt");
    fs::write(&path, "hello\n").unwrap();
    let error = Editors::new("nvim", directory)
        .open(
            &client,
            &TabId::new("w1:t1"),
            Some(&PaneId::new("w1:p1")),
            &herdr_nvim::Target::at(path),
        )
        .unwrap_err();
    assert!(
        format!("{error:#}").contains("Neovim bridge exited"),
        "{error:#}"
    );
    assert_eq!(fixture.lua("fixture.snapshot()", ""), before);
    assert_eq!(fixture.lua("fixture.received()", "").trim(), "0");
    assert_eq!(
        server.requests(),
        [
            RecordedRequest {
                method: "pane.list".into(),
                params: json!({})
            },
            RecordedRequest {
                method: "pane.get".into(),
                params: json!({"pane_id":"w1:p1"})
            },
        ]
    );
}

#[test]
fn queued_open_expires_after_caller_timeout_without_late_navigation() {
    use nix::sys::signal::Signal;
    let fixture = Fixture::start();
    let before = fixture.lua("fixture.snapshot()", "");
    let path = fixture.scratch.0.join("late.txt");
    fs::write(&path, "must not open\n").unwrap();
    let request = Request {
        protocol: VERSION,
        context: fixture.ready.context.clone(),
        operation: Operation::Open {
            target: Location {
                path,
                line: 1,
                column: 1,
            },
            not_after_ms: future_ms(100),
            lease: None,
        },
    };
    fixture.signal(Signal::SIGSTOP);
    let command = fixture.command(
        "require('dots.herdr').request(_A)",
        &serde_json::to_string(&request).unwrap(),
    );
    let result = herdrkit::process::output(command, Duration::from_millis(300), 64 * 1024);
    fixture.signal(Signal::SIGCONT);
    assert!(
        matches!(result, Err(herdrkit::process::Error::Timeout(_))),
        "{result:?}"
    );
    let start = Instant::now();
    while fixture.lua("fixture.received()", "").trim() == "0"
        && start.elapsed() < Duration::from_secs(2)
    {
        std::thread::sleep(Duration::from_millis(10));
    }
    // Prove the request was delivered, rather than passing because the client
    // died before queuing it. Its missing acknowledgement cannot undo delivery.
    assert_eq!(fixture.lua("fixture.received()", "").trim(), "1");
    let reply: Reply = serde_json::from_str(&fixture.lua("fixture.last_reply()", "")).unwrap();
    assert!(
        matches!(&reply, Reply::Rejected { error } if error.code == ErrorCode::Expired),
        "{reply:?}"
    );
    assert_eq!(fixture.lua("fixture.snapshot()", ""), before);
}

// Obtain serde's real variant names, rather than maintaining a second Rust
// list that can drift independently of the enum being sent over the wire.
struct EnumNames<'a>(&'a mut Vec<&'static str>);
impl<'de> serde::Deserializer<'de> for EnumNames<'_> {
    type Error = serde::de::value::Error;
    fn deserialize_enum<V: serde::de::Visitor<'de>>(
        self,
        _: &'static str,
        variants: &'static [&'static str],
        _: V,
    ) -> Result<V::Value, Self::Error> {
        self.0.extend_from_slice(variants);
        Err(serde::de::Error::custom("metadata only"))
    }
    fn deserialize_any<V: serde::de::Visitor<'de>>(self, _: V) -> Result<V::Value, Self::Error> {
        Err(serde::de::Error::custom("expected enum"))
    }
    serde::forward_to_deserialize_any! { bool i8 i16 i32 i64 u8 u16 u32 u64 f32 f64 char str string bytes byte_buf option unit unit_struct newtype_struct seq tuple tuple_struct map struct identifier ignored_any }
}

#[test]
fn all_error_codes_match_between_rust_and_lua() {
    let mut rust = Vec::new();
    ErrorCode::deserialize(EnumNames(&mut rust)).unwrap_err();
    assert!(!rust.is_empty());
    let fixture = Fixture::start();
    let mut lua: Vec<String> = serde_json::from_str(
        &fixture.lua("vim.json.encode(require('dots.herdr.protocol').codes)", ""),
    )
    .unwrap();
    rust.sort_unstable();
    lua.sort_unstable();
    assert_eq!(rust, lua);
}

#[cfg(test)]
fn ownership(reply: Reply) -> herdr_nvim::protocol::Ownership {
    let Reply::Ownership {
        ownership: Some(state),
        ..
    } = reply
    else {
        panic!("expected managed ownership, got {reply:?}")
    };
    state
}

#[test]
fn typed_owner_controls_expire_and_fence_stale_controllers() {
    use herdr_nvim::protocol::{LeaseId, OwnerControl, OwnerPhase};
    let fixture = Fixture::start();
    let first = LeaseId {
        owner: "a".repeat(32),
        generation: 1,
    };
    let next_owner = "b".repeat(32);
    fixture.lua("fixture.manage(_A)", &first.owner);
    fixture.lua(
        "(function() vim.api.nvim_buf_set_lines(0, 0, -1, false, {'unsaved work'}); return true end)()",
        "",
    );
    let before = fixture.lua("fixture.snapshot()", "");
    assert_eq!(
        ownership(fixture.owner(OwnerControl::Inspect)).phase,
        OwnerPhase::Active
    );
    fixture.lua("fixture.advance_owner_clock(_A)", "5000");
    assert_eq!(
        ownership(fixture.owner(OwnerControl::Inspect)).phase,
        OwnerPhase::Orphaned
    );
    let late = fixture.owner(OwnerControl::Renew {
        lease: first.clone(),
        not_after_ms: future_ms(2000),
    });
    assert!(matches!(late, Reply::Rejected { error } if error.code == ErrorCode::OwnerExpired));
    let recovery = OwnerControl::Recover {
        expected: first.clone(),
        owner: next_owner,
        not_after_ms: future_ms(20_000),
    };
    let next = ownership(fixture.owner(recovery.clone()));
    assert_eq!(next.phase, OwnerPhase::Active);
    assert_eq!(next.lease.generation, 2);
    fixture.lua("fixture.advance_owner_clock(_A)", "1000");
    assert_eq!(ownership(fixture.owner(recovery)).remaining_ms, 4000);
    for control in [
        OwnerControl::Renew {
            lease: first.clone(),
            not_after_ms: future_ms(2000),
        },
        OwnerControl::Release {
            lease: first.clone(),
            not_after_ms: future_ms(2000),
        },
        OwnerControl::Recover {
            expected: first.clone(),
            owner: "c".repeat(32),
            not_after_ms: future_ms(2000),
        },
        OwnerControl::Recover {
            expected: LeaseId {
                owner: "c".repeat(32),
                generation: 1,
            },
            owner: next.lease.owner.clone(),
            not_after_ms: future_ms(2000),
        },
    ] {
        let reply = fixture.owner(control);
        assert!(
            matches!(&reply, Reply::Rejected { error } if error.code == ErrorCode::OwnerMismatch),
            "{reply:?}"
        );
    }
    assert_eq!(fixture.lua("fixture.snapshot()", ""), before);
    assert_eq!(
        fixture.lua("vim.json.encode(vim.bo.modified)", "").trim(),
        "true"
    );
}

#[test]
fn managed_navigation_requires_the_recovered_generation() {
    use herdr_nvim::protocol::{LeaseId, OwnerControl};
    let fixture = Fixture::start();
    let first = LeaseId {
        owner: "a".repeat(32),
        generation: 1,
    };
    fixture.lua("fixture.manage(_A)", &first.owner);
    fixture.owner(OwnerControl::Release {
        lease: first.clone(),
        not_after_ms: future_ms(2000),
    });
    let next = ownership(fixture.owner(OwnerControl::Recover {
        expected: first.clone(),
        owner: "b".repeat(32),
        not_after_ms: future_ms(2000),
    }));
    let before = fixture.lua("fixture.snapshot()", "");
    let path = fixture.scratch.0.join("managed.txt");
    fs::write(&path, "target\n").unwrap();
    for (lease, expected) in [
        (None, ErrorCode::OwnerRequired),
        (Some(first), ErrorCode::OwnerMismatch),
    ] {
        let reply = fixture.request(&Request {
            protocol: VERSION,
            context: fixture.ready.context.clone(),
            operation: Operation::Open {
                target: Location {
                    path: path.clone(),
                    line: 1,
                    column: 1,
                },
                not_after_ms: future_ms(2000),
                lease,
            },
        });
        assert!(
            matches!(&reply, Reply::Rejected { error } if error.code == expected),
            "{reply:?}"
        );
    }
    assert_eq!(fixture.lua("fixture.snapshot()", ""), before);
    let reply = fixture.request(&Request {
        protocol: VERSION,
        context: fixture.ready.context.clone(),
        operation: Operation::Open {
            target: Location {
                path,
                line: 1,
                column: 1,
            },
            not_after_ms: future_ms(2000),
            lease: Some(next.lease),
        },
    });
    assert!(matches!(reply, Reply::Opened { .. }));
}

#[test]
fn durable_reservations_reconcile_interrupted_launch_and_lost_recovery_ack() {
    use herdr_nvim::protocol::{LeaseId, OwnerControl, OwnerPhase};
    use herdr_nvim::recovery::{self, Binding, Ledger, Phase, Reconciled, RecoveryIntent, Scope};
    let scratch = Scratch::new();
    let registration = recovery::fresh_id().unwrap();
    let owner = recovery::fresh_id().unwrap();
    let directory = scratch.0.join("ledger");
    let binding = Binding {
        scope: Scope {
            server_id: "fixture-server@1:1".into(),
            plugin_root: scratch.0.join("plugin"),
        },
        context: herdr_nvim::protocol::Context {
            registration: registration.clone(),
            server: "/tmp/example-herdr.sock".into(),
        },
        socket: scratch.0.join("s").join(format!("{registration}.sock")),
        workspace: herdrkit::WorkspaceId::new("w1"),
        directory: scratch.0.clone(),
    };
    let mut ledger = Ledger::lock(&directory).unwrap();
    ledger
        .reserve_launch(binding.clone(), owner.clone())
        .unwrap();
    // Reservation is synced before the native process starts, with the exact
    // identity it must bind. No socket/PID discovery is used after the interruption.
    let fixture = Fixture::start_reserved(scratch, Some(&registration));
    assert_eq!(fixture.ready.context, binding.context);
    assert_eq!(fixture.ready.socket, binding.socket);
    fixture.lua("fixture.manage(_A)", &owner);
    drop(ledger); // interrupted before recording readiness
    let mut ledger = Ledger::lock(&directory).unwrap();
    assert!(matches!(
        ledger.read().unwrap().unwrap().phase,
        Phase::LaunchPending { .. }
    ));
    assert_eq!(
        ledger.reconcile(&binding.scope, None).unwrap(),
        Reconciled::Unreachable
    );
    let observed = fixture.owner(OwnerControl::Inspect);
    assert_eq!(
        ledger.reconcile(&binding.scope, Some(&observed)).unwrap(),
        Reconciled::Observed(OwnerPhase::Active)
    );
    fixture.lua("fixture.advance_owner_clock(_A)", "5000");
    let intent = RecoveryIntent {
        expected: LeaseId {
            owner,
            generation: 1,
        },
        owner: recovery::fresh_id().unwrap(),
        not_after_ms: future_ms(20_000),
    };
    ledger
        .reserve_recovery(&binding.scope, intent.clone())
        .unwrap();
    let _lost_acknowledgement = fixture.owner(intent.control());
    drop(ledger); // applied remotely, but never recorded locally
    let mut ledger = Ledger::lock(&directory).unwrap();
    assert!(matches!(
        ledger.read().unwrap().unwrap().phase,
        Phase::RecoveryPending { .. }
    ));
    fixture.lua("fixture.advance_owner_clock(_A)", "1000");
    let observed = fixture.owner(OwnerControl::Inspect);
    assert_eq!(
        ledger.reconcile(&binding.scope, Some(&observed)).unwrap(),
        Reconciled::Observed(OwnerPhase::Active)
    );
    assert!(
        matches!(ledger.read().unwrap().unwrap().phase, Phase::Observed { lease } if lease.owner == intent.owner && lease.generation == 2)
    );
    fixture.lua("fixture.advance_owner_clock(_A)", "4000");
    assert_eq!(
        ownership(fixture.owner(OwnerControl::Inspect)).phase,
        OwnerPhase::Orphaned
    );
    drop(ledger); // release the stable lock before the fixture removes its scratch tree
}

#[test]
fn unmanaged_endpoint_reports_no_owner_and_rejects_implicit_adoption() {
    use herdr_nvim::protocol::{LeaseId, OwnerControl};
    let fixture = Fixture::start();
    let Reply::Ownership {
        identity,
        ownership: None,
    } = fixture.owner(OwnerControl::Inspect)
    else {
        panic!("expected an unmanaged endpoint");
    };
    assert_eq!(identity.pid, fixture.ready.pid);
    assert_eq!(identity.context, fixture.ready.context);
    let reply = fixture.owner(OwnerControl::Recover {
        expected: LeaseId {
            owner: "a".repeat(32),
            generation: 1,
        },
        owner: "b".repeat(32),
        not_after_ms: future_ms(2000),
    });
    assert!(matches!(reply, Reply::Rejected { error } if error.code == ErrorCode::NotManaged));
}

#[test]
fn owner_wire_validation_fails_before_renewal() {
    use herdr_nvim::protocol::{LeaseId, OwnerControl};
    let fixture = Fixture::start();
    let lease = LeaseId {
        owner: "a".repeat(32),
        generation: 1,
    };
    fixture.lua("fixture.manage(_A)", &lease.owner);
    fixture.lua("fixture.advance_owner_clock(_A)", "1000");
    let before = ownership(fixture.owner(OwnerControl::Inspect));
    let request = Request {
        protocol: VERSION,
        context: fixture.ready.context.clone(),
        operation: Operation::Owner {
            control: OwnerControl::Renew {
                lease,
                not_after_ms: future_ms(20_000),
            },
        },
    };
    let valid = serde_json::to_value(request).unwrap();
    for (pointer, value, code) in [
        (
            "/operation/control/action",
            json!("unknown"),
            ErrorCode::InvalidRequest,
        ),
        (
            "/operation/control/lease",
            json!(null),
            ErrorCode::InvalidRequest,
        ),
        (
            "/operation/control/lease/owner",
            json!("invalid"),
            ErrorCode::InvalidRequest,
        ),
        (
            "/operation/control/lease/generation",
            json!(0),
            ErrorCode::InvalidRequest,
        ),
        (
            "/operation/control/lease/generation",
            json!(1.5),
            ErrorCode::InvalidRequest,
        ),
        (
            "/operation/control/lease/generation",
            json!(4_294_967_296_u64),
            ErrorCode::InvalidRequest,
        ),
        (
            "/operation/control/not_after_ms",
            json!(1),
            ErrorCode::Expired,
        ),
        (
            "/operation/control/not_after_ms",
            json!(-1),
            ErrorCode::InvalidRequest,
        ),
    ] {
        let mut raw = valid.clone();
        *raw.pointer_mut(pointer).unwrap() = value;
        let reply = fixture.raw(&raw.to_string());
        assert!(
            matches!(&reply, Reply::Rejected { error } if error.code == code),
            "{pointer}: {reply:?}"
        );
    }
    for pointer in [
        "/operation",
        "/operation/control",
        "/operation/control/lease",
    ] {
        let mut raw = valid.clone();
        raw.pointer_mut(pointer)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .insert("extra".into(), json!(true));
        let reply = fixture.raw(&raw.to_string());
        assert!(
            matches!(reply, Reply::Rejected { error } if error.code == ErrorCode::InvalidRequest)
        );
    }
    assert_eq!(ownership(fixture.owner(OwnerControl::Inspect)), before);
}
