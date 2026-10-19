use super::*;
use crate::engine::tests::{Scratch, focused, read, reply};
use herdrkit::testing::Server;

fn finish(control: &mut Control, ticket: &str) -> Reply {
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        let value = control
            .handle(Request::Result {
                ticket: ticket.into(),
            })
            .unwrap();
        if matches!(value, Reply::Complete { .. }) {
            return value;
        }
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn queued_back_presses_use_the_previous_confirmed_destination() {
    let root = Scratch::new();
    let server = Server::start(vec![
        read("w1:p1"),
        read("w2:p2"),
        read("w3:p3"),
        read("w3:p3"),
        focused("w2:p2"),
        read("w2:p2"),
        read("w2:p2"),
        focused("w1:p1"),
        read("w1:p1"),
    ]);
    let mut engine = Engine::open(server.client(), &root.path()).unwrap();
    engine.refresh().unwrap();
    engine.refresh().unwrap();
    let mut control = Control::start(engine).unwrap();
    let Reply::Submitted { ticket: a } = control
        .handle(Request::Navigate {
            operation: Operation::Travel {
                direction: Travel::Back,
            },
        })
        .unwrap()
    else {
        panic!("expected ticket")
    };
    let Reply::Submitted { ticket: b } = control
        .handle(Request::Navigate {
            operation: Operation::Travel {
                direction: Travel::Back,
            },
        })
        .unwrap()
    else {
        panic!("expected ticket")
    };
    assert_eq!(finish(&mut control, &a), Reply::Complete { result: Ok(()) });
    assert_eq!(finish(&mut control, &b), Reply::Complete { result: Ok(()) });
    assert_eq!(
        control
            .status
            .get("history")
            .unwrap()
            .get("current")
            .unwrap(),
        "A"
    );
}

#[test]
fn admission_is_bounded_and_observations_coalesce() {
    let (sender, _receiver) = mpsc::sync_channel(QUEUE);
    let (_updates, receiver) = mpsc::sync_channel(RESULTS);
    let mut control = Control {
        sender,
        receiver,
        observing: Arc::new(AtomicBool::new(false)),
        results: VecDeque::new(),
        epoch: "fixture".into(),
        next: 0,
        status: Value::Null,
        last_error: None,
    };
    for _ in 0..100 {
        control.handle(Request::Observe).unwrap();
    }
    for _ in 0..QUEUE - 1 {
        control
            .handle(Request::Navigate {
                operation: Operation::Travel {
                    direction: Travel::Back,
                },
            })
            .unwrap();
    }
    assert!(
        control
            .handle(Request::Navigate {
                operation: Operation::Travel {
                    direction: Travel::Back
                },
            })
            .is_err()
    );
    assert_eq!(control.results.len(), QUEUE - 1);
    assert!(
        control
            .handle(Request::Result {
                ticket: "foreign".into()
            })
            .is_err()
    );
}

#[test]
fn expired_requests_never_read_or_focus_herdr() {
    let root = Scratch::new();
    let server = Server::start(vec![read("w1:p1")]);
    let engine = Engine::open(server.client(), &root.path()).unwrap();
    let mut control = Control::start(engine).unwrap();
    control.results.push_back(("expired".into(), None));
    control
        .sender
        .send(Work::Navigate {
            ticket: "expired".into(),
            operation: Operation::Travel {
                direction: Travel::Back,
            },
            deadline: Instant::now().checked_sub(Duration::from_secs(1)).unwrap(),
        })
        .unwrap();
    assert!(matches!(
        finish(&mut control, "expired"),
        Reply::Complete { result: Err(_) }
    ));
    assert_eq!(server.requests().len(), 1);
}

#[test]
fn queued_workspace_toggles_alternate_confirmed_workspaces() {
    let root = Scratch::new();
    let server = Server::start(vec![
        read("w1:p1"),
        read("w2:p2"),
        read("w2:p2"),
        reply(&json!({"type":"ok"})),
        read("w1:p1"),
        read("w1:p1"),
        reply(&json!({"type":"ok"})),
        read("w2:p2"),
    ]);
    let mut engine = Engine::open(server.client(), &root.path()).unwrap();
    engine.refresh().unwrap();
    let mut control = Control::start(engine).unwrap();
    let tickets = [0, 1].map(|_| {
        let Reply::Submitted { ticket } = control
            .handle(Request::Navigate {
                operation: Operation::PreviousWorkspace,
            })
            .unwrap()
        else {
            panic!("expected ticket");
        };
        ticket
    });
    for ticket in tickets {
        assert_eq!(
            finish(&mut control, &ticket),
            Reply::Complete { result: Ok(()) }
        );
    }
    assert_eq!(control.status.pointer("/workspaces/current").unwrap(), "w2");
    assert_eq!(
        control.status.pointer("/workspaces/previous").unwrap(),
        "w1"
    );
}
