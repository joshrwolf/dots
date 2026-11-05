use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Barrier};

use rusqlite::Connection;

use super::{
    AnswerContent, Choice, ClosureKind, DecisionContent, DecisionKind, GoalRef, HumanInput,
    Incorporation, RequestContent, RequestKind, Scope, Source, StateFilter,
};
use crate::store::{
    Actor, Error, Filter, Hold, NewTask, Owner, Project, RequestState, Status, Store, Update,
};

fn project() -> Project {
    Project::try_new("p-00000000000000000000000000000001").unwrap()
}
fn other_project() -> Project {
    Project::try_new("p-00000000000000000000000000000002").unwrap()
}
fn owner(name: &'static str) -> Owner<'static> {
    Owner::try_new(name).unwrap()
}
fn by(name: &'static str, now: i64) -> Actor<'static> {
    Actor {
        name: Some(owner(name)),
        now,
    }
}
fn store() -> Store {
    crate::test_store(Store::initialize(Connection::open_in_memory().unwrap()).unwrap())
}

/// A catalog file in a fresh directory, removed when dropped.
struct Catalog {
    root: PathBuf,
}

impl Catalog {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "dots-attention-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&root).unwrap();
        let store = crate::test_store(Store::create(&root.join("workspace.sqlite3")).unwrap());
        drop(store);
        Self { root }
    }

    fn path(&self) -> PathBuf {
        self.root.join("workspace.sqlite3")
    }

    fn open(&self) -> Store {
        Store::open(&self.path()).unwrap()
    }
}

impl Drop for Catalog {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.root).unwrap();
    }
}

/// A coordinated goal with `group > leaf`, a sibling leaf, in `project`.
struct Fixture {
    goal: String,
    group: String,
    leaf: String,
    sibling: String,
}

fn goal_in(store: &mut Store, project: &Project, coordinator: &'static str) -> Fixture {
    let goal = store
        .create_goal(project, "goal", "briefs/goal.md", by(coordinator, 1))
        .unwrap()
        .root
        .id;
    store
        .coordinate(project, &goal, owner(coordinator), false, false, 1)
        .unwrap();
    let add = |store: &mut Store, title: &str, parent: &str| {
        store
            .add(
                project,
                &NewTask {
                    title: title.into(),
                    parent: Some(parent.into()),
                    ..NewTask::default()
                },
                by(coordinator, 1),
            )
            .unwrap()
            .id
    };
    let group = add(store, "group", &goal);
    let leaf = add(store, "leaf", &group);
    let sibling = add(store, "sibling", &goal);
    Fixture {
        goal,
        group,
        leaf,
        sibling,
    }
}

fn goal(store: &mut Store) -> Fixture {
    goal_in(store, &project(), "coord")
}

fn question(tasks: &[&str]) -> RequestContent {
    RequestContent {
        kind: RequestKind::Decision,
        title: "Pick a storage layout".into(),
        question: "Which layout should the migration use?".into(),
        context: Some("Both are reversible.".into()),
        recommendation: Some("a".into()),
        choices: vec![
            Choice {
                id: "a".into(),
                label: "One table".into(),
                detail: None,
            },
            Choice {
                id: "b".into(),
                label: "Two tables".into(),
                detail: Some("More joins".into()),
            },
        ],
        tasks: tasks.iter().map(|task| (*task).to_owned()).collect(),
    }
}

fn ui_answer(choice: &str) -> AnswerContent {
    AnswerContent {
        choice: Some(choice.into()),
        text: None,
        source: Source::Ui,
        provenance: Some("viewer rex:session:1".into()),
        interpretation: None,
    }
}

fn decision(text: &str) -> Incorporation {
    Incorporation {
        kind: DecisionKind::Decision,
        decision: text.into(),
    }
}

fn done(store: &mut Store, id: &str, actor: Actor<'_>, force: bool) -> Result<(), Error> {
    store
        .write_result(
            &project(),
            id,
            &crate::outcomes::Outcome {
                operation: crate::outcomes::Operation::Done,
                result: "verified",
                evidence: &[],
                force,
            },
            actor,
        )
        .map(|_| ())
}

fn kinds(store: &mut Store, id: &str) -> Vec<String> {
    store
        .events(&project(), id, 100)
        .unwrap()
        .into_iter()
        .map(|event| event.kind)
        .collect()
}

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "one scenario follows a request or claim through every step"
)]
fn coordination_permits_unfinished_children_and_takeover_moves_every_claim() {
    let mut store = store();
    let goal = goal(&mut store);
    assert_eq!(
        store.get(&project(), &goal.goal).unwrap().owner.as_deref(),
        Some("coord")
    );
    assert!(matches!(
        store.claim(&project(), &goal.goal, owner("coord"), false, 2),
        Err(Error::GoalRoot(_))
    ));
    store
        .claim(&project(), &goal.leaf, owner("coord"), false, 2)
        .unwrap();
    let outside = store
        .add(
            &project(),
            &NewTask {
                title: "outside".into(),
                ..NewTask::default()
            },
            by("coord", 2),
        )
        .unwrap()
        .id;
    store
        .claim(&project(), &outside, owner("coord"), false, 2)
        .unwrap();
    // Coordinating again is a no-op; another runtime needs an explicit takeover.
    let again = store
        .coordinate(&project(), &goal.goal, owner("coord"), false, false, 3)
        .unwrap();
    assert_eq!(again.transferred, []);
    let refused = store.coordinate(&project(), &goal.goal, owner("next"), false, false, 3);
    assert!(
        matches!(&refused, Err(Error::ClaimsHeld { claims, .. }) if claims.contains(&goal.leaf)),
        "{refused:?}"
    );
    let taken = store
        .coordinate(&project(), &goal.goal, owner("next"), true, false, 4)
        .unwrap();
    let moved: Vec<_> = taken
        .transferred
        .iter()
        .map(|transfer| (transfer.task.as_str(), transfer.from.as_str()))
        .collect();
    assert_eq!(
        moved,
        [(goal.goal.as_str(), "coord"), (goal.leaf.as_str(), "coord")]
    );
    assert_eq!(taken.goal.root.owner.as_deref(), Some("next"));
    assert_eq!(
        store.get(&project(), &goal.leaf).unwrap().owner.as_deref(),
        Some("next")
    );
    let root_events = store.events(&project(), &goal.goal, 100).unwrap();
    assert_eq!(
        root_events.last().unwrap().detail.as_deref(),
        Some("coordinating, taken from coord")
    );

    // The displaced coordinator's writes to the root and to its old leaf fail.
    let stale = by("coord", 5);
    assert!(matches!(
        store.release(&project(), &goal.leaf, owner("coord"), 5),
        Err(Error::OwnedByAnother { .. })
    ));
    assert!(matches!(
        done(&mut store, &goal.leaf, stale, true),
        Err(Error::NotCoordinator { coordinator, .. }) if coordinator == "next"
    ));
    assert!(matches!(
        store.set_status(
            &project(),
            &goal.goal,
            Status::Held,
            Some("pause"),
            stale,
            true
        ),
        Err(Error::NotCoordinator { .. })
    ));
    assert!(matches!(
        store.add(
            &project(),
            &NewTask {
                title: "late".into(),
                parent: Some(goal.goal.clone()),
                ..NewTask::default()
            },
            stale
        ),
        Err(Error::NotCoordinator { .. })
    ));
    assert!(matches!(
        store.add_dependency(&project(), &goal.sibling, &goal.group, stale),
        Err(Error::NotCoordinator { .. })
    ));
    assert!(matches!(
        store.claim(&project(), &goal.sibling, owner("coord"), true, 5),
        Err(Error::NotCoordinator { .. })
    ));
    // Cleanup releases only the stale actor's own remaining claims.
    assert_eq!(store.release_all(owner("coord"), 6).unwrap(), [outside]);
    assert_eq!(
        store.get(&project(), &goal.goal).unwrap().owner.as_deref(),
        Some("next")
    );
    assert_eq!(
        store.get(&project(), &goal.leaf).unwrap().owner.as_deref(),
        Some("next")
    );
}

#[test]
fn only_the_coordinator_dispatches_and_changes_goal_work() {
    let mut store = store();
    let goal = goal(&mut store);
    let worker = by("worker", 2);
    assert!(matches!(
        store.claim(&project(), &goal.leaf, owner("worker"), false, 2),
        Err(Error::NotCoordinator { .. })
    ));
    let outside = store
        .add(
            &project(),
            &NewTask {
                title: "outside".into(),
                ..NewTask::default()
            },
            worker,
        )
        .unwrap()
        .id;
    // Next-claim skips goal work the owner does not coordinate.
    assert_eq!(
        store
            .claim_next(&project(), None, owner("worker"), 2)
            .unwrap()
            .unwrap()
            .id,
        outside
    );
    assert!(
        store
            .claim_next(&project(), None, owner("worker"), 2)
            .unwrap()
            .is_none()
    );
    let update = Update {
        body: Some("rewrite".into()),
        ..Update::default()
    };
    assert!(matches!(
        store.update(&project(), &goal.leaf, &update, worker, true),
        Err(Error::NotCoordinator { .. })
    ));
    assert!(matches!(
        done(&mut store, &goal.leaf, worker, true),
        Err(Error::NotCoordinator { .. })
    ));
    let anonymous = Actor { name: None, now: 2 };
    assert!(matches!(
        store.update(&project(), &goal.leaf, &update, anonymous, true),
        Err(Error::NotCoordinator { .. })
    ));
    // The same actor coordinates and works its own leaves.
    let claimed = store
        .claim_next(&project(), Some(&goal.goal), owner("coord"), 3)
        .unwrap()
        .unwrap();
    assert_eq!(claimed.id, goal.leaf);
    done(&mut store, &goal.leaf, by("coord", 4), false).unwrap();
    // Without a coordinator nobody dispatches goal work.
    store
        .release(&project(), &goal.goal, owner("coord"), 5)
        .unwrap();
    assert!(matches!(
        store.claim(&project(), &goal.sibling, owner("coord"), false, 6),
        Err(Error::Uncoordinated { .. })
    ));
    assert!(
        store
            .claim_next(&project(), Some(&goal.goal), owner("coord"), 6)
            .unwrap()
            .is_none()
    );
}

#[test]
fn pause_holds_every_descendant_until_explicit_resume() {
    let mut store = store();
    let goal = goal(&mut store);
    store
        .claim(&project(), &goal.leaf, owner("coord"), false, 2)
        .unwrap();
    store
        .set_status(
            &project(),
            &goal.goal,
            Status::Held,
            Some("pause"),
            by("coord", 3),
            false,
        )
        .unwrap();
    let root = store.get(&project(), &goal.goal).unwrap();
    assert!(root.owner.is_none());
    let leaf = store.get(&project(), &goal.leaf).unwrap();
    assert_eq!(leaf.owner.as_deref(), Some("coord"));
    assert_eq!(
        leaf.holds,
        [Hold::HeldAncestor {
            task: goal.goal.clone()
        }]
    );
    assert!(store.ready(&project(), None).unwrap().is_empty());
    assert!(matches!(
        store.coordinate(&project(), &goal.goal, owner("coord"), false, false, 4),
        Err(Error::GoalNotOpen { status: "held", .. })
    ));
    assert!(matches!(
        store.set_status(
            &project(),
            &goal.goal,
            Status::Open,
            None,
            by("coord", 4),
            false
        ),
        Err(Error::Uncoordinated { .. })
    ));
    let resumed = store
        .coordinate(&project(), &goal.goal, owner("coord"), false, true, 5)
        .unwrap();
    assert_eq!(resumed.goal.root.status, Status::Open);
    assert_eq!(resumed.transferred, []);
    assert_eq!(store.get(&project(), &goal.leaf).unwrap().holds, []);
    // Holding any group holds its subtree, on every claim and acceptance path.
    store
        .set_status(
            &project(),
            &goal.group,
            Status::Held,
            Some("wait"),
            by("coord", 6),
            false,
        )
        .unwrap();
    assert!(matches!(
        done(&mut store, &goal.leaf, by("coord", 7), true),
        Err(Error::NotReady { .. })
    ));
    store
        .release(&project(), &goal.leaf, owner("coord"), 7)
        .unwrap();
    assert!(matches!(
        store.claim(&project(), &goal.leaf, owner("coord"), true, 7),
        Err(Error::NotReady { .. })
    ));
    let ready: Vec<_> = store
        .ready(&project(), None)
        .unwrap()
        .into_iter()
        .map(|task| task.id)
        .collect();
    assert_eq!(ready, [goal.sibling]);
}

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "one scenario follows a request or claim through every step"
)]
fn requests_hold_named_work_until_the_answer_is_incorporated() {
    let mut store = store();
    let goal = goal(&mut store);
    store
        .claim(&project(), &goal.leaf, owner("coord"), false, 2)
        .unwrap();
    let raised = store
        .raise_request(
            &project(),
            &goal.goal,
            "layout",
            &question(&[&goal.group]),
            by("coord", 3),
        )
        .unwrap();
    assert!(raised.created);
    assert_eq!(raised.held_claims.len(), 1);
    assert_eq!(raised.held_claims[0].task, goal.leaf);
    let id = raised.request.id.clone();
    let hold = Hold::Request {
        request: id.clone(),
        task: goal.group.clone(),
        state: RequestState::Open,
    };
    assert_eq!(store.get(&project(), &goal.leaf).unwrap().holds, [hold]);
    assert_eq!(store.get(&project(), &goal.sibling).unwrap().holds, []);
    assert!(matches!(
        done(&mut store, &goal.leaf, by("coord", 4), true),
        Err(Error::NotReady { .. })
    ));
    store
        .release(&project(), &goal.leaf, owner("coord"), 4)
        .unwrap();
    assert!(matches!(
        store.claim(&project(), &goal.leaf, owner("coord"), true, 4),
        Err(Error::NotReady { .. })
    ));
    assert_eq!(
        store
            .claim_next(&project(), Some(&goal.goal), owner("coord"), 4)
            .unwrap()
            .unwrap()
            .id,
        goal.sibling
    );
    store
        .release(&project(), &goal.sibling, owner("coord"), 4)
        .unwrap();
    // Planning edits remain available while the answer is pending.
    store
        .update(
            &project(),
            &goal.leaf,
            &Update {
                body: Some("waiting on layout".into()),
                ..Update::default()
            },
            by("coord", 5),
            false,
        )
        .unwrap();

    let answered = store
        .answer_request(&project(), &id, &ui_answer("b"), by("human", 6))
        .unwrap();
    assert_eq!(answered.state, RequestState::Answered);
    assert_eq!(
        answered.answer.as_ref().unwrap().by.as_deref(),
        Some("human")
    );
    let leaf = store.get(&project(), &goal.leaf).unwrap();
    assert!(matches!(
        leaf.holds.as_slice(),
        [Hold::Request {
            state: RequestState::Answered,
            ..
        }]
    ));
    assert!(
        leaf.not_workable()
            .unwrap()
            .contains("not yet incorporated")
    );
    assert!(matches!(
        store.incorporate_request(
            &project(),
            &id,
            &decision("Use two tables."),
            by("worker", 7)
        ),
        Err(Error::NotCoordinator { .. })
    ));
    let incorporated = store
        .incorporate_request(
            &project(),
            &id,
            &decision("Use two tables."),
            by("coord", 7),
        )
        .unwrap();
    assert_eq!(incorporated.request.state, RequestState::Closed);
    let closure = incorporated.request.closure.as_ref().unwrap();
    assert_eq!(closure.kind, ClosureKind::Incorporated);
    assert_eq!(
        closure.decision.as_deref(),
        Some(incorporated.decision.id.as_str())
    );
    assert_eq!(incorporated.decision.request.as_deref(), Some(id.as_str()));
    assert_eq!(store.get(&project(), &goal.leaf).unwrap().holds, []);
    store
        .claim(&project(), &goal.leaf, owner("coord"), false, 8)
        .unwrap();
    assert_eq!(
        kinds(&mut store, &goal.goal)
            .into_iter()
            .filter(|kind| kind.starts_with("request-"))
            .collect::<Vec<_>>(),
        ["request-raised", "request-answered", "request-incorporated"]
    );
}

#[test]
fn coordination_reconciles_a_root_request_without_dispatching_blocked_work() {
    let mut store = store();
    let goal = goal(&mut store);
    let id = store
        .raise_request(
            &project(),
            &goal.goal,
            "scope",
            &question(&[&goal.goal]),
            by("coord", 2),
        )
        .unwrap()
        .request
        .id;
    store.release_all(owner("coord"), 3).unwrap();
    store
        .answer_request(&project(), &id, &ui_answer("a"), by("human", 4))
        .unwrap();
    let taken = store
        .coordinate(&project(), &goal.goal, owner("next"), false, false, 5)
        .unwrap();
    assert!(
        taken
            .goal
            .root
            .holds
            .iter()
            .any(|hold| matches!(hold, Hold::Request { .. }))
    );
    assert!(matches!(
        store.claim(&project(), &goal.sibling, owner("next"), false, 6),
        Err(Error::NotReady { .. })
    ));
    assert!(matches!(
        store.withdraw_request(&project(), &id, "obsolete", by("next", 6)),
        Err(Error::RequestNotOpen {
            state: RequestState::Answered,
            ..
        })
    ));
    store
        .incorporate_request(&project(), &id, &decision("Keep scope."), by("next", 7))
        .unwrap();
    store
        .claim(&project(), &goal.sibling, owner("next"), false, 8)
        .unwrap();
}

#[test]
fn logical_keys_return_existing_requests_and_changes_need_a_replacement() {
    let mut store = store();
    let goal = goal(&mut store);
    let content = question(&[&goal.leaf]);
    let first = store
        .raise_request(&project(), &goal.goal, "layout", &content, by("coord", 2))
        .unwrap();
    let again = store
        .raise_request(&project(), &goal.goal, "layout", &content, by("coord", 3))
        .unwrap();
    assert!(!again.created);
    assert_eq!(again.request.id, first.request.id);
    let mut changed = content.clone();
    changed.question = "Which layout, given the new constraint?".into();
    assert!(matches!(
        store.raise_request(&project(), &goal.goal, "layout", &changed, by("coord", 3)),
        Err(Error::RequestConflict { .. })
    ));
    assert!(matches!(
        store.supersede_request(&project(), &first.request.id, &content, by("coord", 3)),
        Err(Error::Invalid(_))
    ));
    let replacement = store
        .supersede_request(&project(), &first.request.id, &changed, by("coord", 4))
        .unwrap()
        .request;
    assert_eq!(
        replacement.supersedes.as_deref(),
        Some(first.request.id.as_str())
    );
    let old = store.request(&project(), &first.request.id).unwrap();
    assert_eq!(old.superseded_by.as_deref(), Some(replacement.id.as_str()));
    assert_eq!(old.closure.unwrap().kind, ClosureKind::Superseded);
    // A stale answer names the replacement and is never applied to it.
    let stale = store.answer_request(
        &project(),
        &first.request.id,
        &ui_answer("a"),
        by("human", 5),
    );
    assert!(
        matches!(&stale, Err(Error::RequestNotOpen { replacement: Some(r), .. }) if *r == replacement.id),
        "{stale:?}"
    );
    assert_eq!(
        store.request(&project(), &replacement.id).unwrap().state,
        RequestState::Open
    );
    assert_eq!(
        store
            .raise_request(&project(), &goal.goal, "layout", &changed, by("coord", 6))
            .unwrap()
            .request
            .id,
        replacement.id
    );
    store
        .answer_request(&project(), &replacement.id, &ui_answer("a"), by("human", 7))
        .unwrap();
    store
        .incorporate_request(
            &project(),
            &replacement.id,
            &decision("One table."),
            by("coord", 8),
        )
        .unwrap();
    let closed = store
        .raise_request(&project(), &goal.goal, "layout", &changed, by("coord", 9))
        .unwrap();
    assert!(!closed.created);
    assert_eq!(
        closed.request.closure.unwrap().kind,
        ClosureKind::Incorporated
    );
    assert!(matches!(
        store.raise_request(&project(), &goal.goal, "layout", &content, by("coord", 9)),
        Err(Error::RequestConflict { reason, .. }) if reason.contains("new key")
    ));
}

#[test]
fn competing_answer_and_supersede_have_exactly_one_winner() {
    let catalog = Catalog::new();
    let (goal, id) = {
        let mut store = catalog.open();
        let goal = goal(&mut store);
        let id = store
            .raise_request(
                &project(),
                &goal.goal,
                "layout",
                &question(&[&goal.leaf]),
                by("coord", 2),
            )
            .unwrap()
            .request
            .id;
        (goal, id)
    };
    let barrier = Arc::new(Barrier::new(2));
    let answering = {
        let (barrier, path, id) = (barrier.clone(), catalog.path(), id.clone());
        std::thread::spawn(move || {
            let mut store = Store::open(&path).unwrap();
            barrier.wait();
            store.answer_request(&project(), &id, &ui_answer("a"), by("human", 3))
        })
    };
    let superseding = {
        let (path, id, leaf) = (catalog.path(), id.clone(), goal.leaf.clone());
        std::thread::spawn(move || {
            let mut store = Store::open(&path).unwrap();
            let mut changed = question(&[&leaf]);
            changed.title = "Pick a storage layout now".into();
            barrier.wait();
            store.supersede_request(&project(), &id, &changed, by("coord", 3))
        })
    };
    let answered = answering.join().unwrap();
    let superseded = superseding.join().unwrap();
    assert!(
        answered.is_ok() != superseded.is_ok(),
        "{answered:?} {superseded:?}"
    );
    let state = catalog.open().request(&project(), &id).unwrap();
    if answered.is_ok() {
        assert_eq!(state.state, RequestState::Answered);
        assert!(matches!(
            superseded,
            Err(Error::RequestNotOpen {
                state: RequestState::Answered,
                ..
            })
        ));
    } else {
        assert_eq!(state.closure.unwrap().kind, ClosureKind::Superseded);
        assert!(matches!(
            answered,
            Err(Error::RequestNotOpen {
                replacement: Some(_),
                ..
            })
        ));
    }
}

#[test]
fn saved_answers_survive_owner_replacement_and_reopening() {
    let catalog = Catalog::new();
    let (goal, id) = {
        let mut store = catalog.open();
        let goal = goal(&mut store);
        let id = store
            .raise_request(
                &project(),
                &goal.goal,
                "layout",
                &question(&[&goal.leaf]),
                by("coord", 2),
            )
            .unwrap()
            .request
            .id;
        store
            .answer_request(&project(), &id, &ui_answer("b"), by("human", 3))
            .unwrap();
        // The session ends before the coordinator sees the answer.
        store.release_all(owner("coord"), 4).unwrap();
        (goal, id)
    };
    let mut store = catalog.open();
    let saved = store.request(&project(), &id).unwrap();
    assert_eq!(saved.state, RequestState::Answered);
    assert_eq!(saved.answer.unwrap().content.choice.as_deref(), Some("b"));
    assert!(matches!(
        store.answer_request(&project(), &id, &ui_answer("a"), by("human", 5)),
        Err(Error::RequestNotOpen { .. })
    ));
    assert!(matches!(
        store.incorporate_request(&project(), &id, &decision("Two tables."), by("coord", 5)),
        Err(Error::Uncoordinated { .. })
    ));
    store
        .coordinate(&project(), &goal.goal, owner("next"), false, false, 6)
        .unwrap();
    assert!(matches!(
        store.incorporate_request(&project(), &id, &decision("Two tables."), by("coord", 7)),
        Err(Error::NotCoordinator { .. })
    ));
    let decided = store
        .incorporate_request(&project(), &id, &decision("Two tables."), by("next", 7))
        .unwrap()
        .decision;
    drop(store);
    let listed = catalog
        .open()
        .decisions(
            &Scope::Goals(vec![GoalRef {
                project: project(),
                goal: goal.goal.clone(),
            }]),
            None,
        )
        .unwrap();
    assert_eq!(listed.goals[0].decisions[0].id, decided.id);
    assert_eq!(listed.goals[0].decisions[0].by.as_deref(), Some("next"));
}

#[test]
fn interrupted_writes_leave_no_partial_state() {
    let mut store = store();
    let goal = goal(&mut store);
    let id = store
        .raise_request(
            &project(),
            &goal.goal,
            "layout",
            &question(&[&goal.leaf]),
            by("coord", 2),
        )
        .unwrap()
        .request
        .id;
    store
        .answer_request(&project(), &id, &ui_answer("a"), by("human", 3))
        .unwrap();
    let events = kinds(&mut store, &goal.goal).len();
    store
        .connection
        .execute_batch(
            "CREATE TRIGGER fail_close BEFORE UPDATE OF state ON requests WHEN NEW.state='closed'
             BEGIN SELECT RAISE(ABORT, 'injected'); END;",
        )
        .unwrap();
    assert!(matches!(
        store.incorporate_request(&project(), &id, &decision("One table."), by("coord", 4)),
        Err(Error::Storage(_))
    ));
    assert_eq!(
        store.request(&project(), &id).unwrap().state,
        RequestState::Answered
    );
    let decisions: i64 = store
        .connection
        .query_row("SELECT COUNT(*) FROM decisions", [], |row| row.get(0))
        .unwrap();
    assert_eq!(decisions, 0);
    assert_eq!(kinds(&mut store, &goal.goal).len(), events);
    store
        .connection
        .execute_batch("DROP TRIGGER fail_close;")
        .unwrap();

    // A takeover that fails partway moves no claim.
    store
        .claim(&project(), &goal.sibling, owner("coord"), false, 5)
        .unwrap();
    store
        .connection
        .execute_batch(&format!(
            "CREATE TRIGGER fail_transfer BEFORE UPDATE OF owner ON tasks WHEN OLD.id='{}'
             BEGIN SELECT RAISE(ABORT, 'injected'); END;",
            goal.sibling
        ))
        .unwrap();
    assert!(
        store
            .coordinate(&project(), &goal.goal, owner("next"), true, false, 6)
            .is_err()
    );
    assert_eq!(
        store.get(&project(), &goal.goal).unwrap().owner.as_deref(),
        Some("coord")
    );
    assert_eq!(
        store
            .get(&project(), &goal.sibling)
            .unwrap()
            .owner
            .as_deref(),
        Some("coord")
    );
}

#[test]
fn goal_roots_close_only_once_no_request_is_outstanding() {
    let mut store = store();
    let goal = goal(&mut store);
    done(&mut store, &goal.leaf, by("coord", 2), false).unwrap();
    // A request may name work accepted before it was raised.
    let id = store
        .raise_request(
            &project(),
            &goal.goal,
            "late",
            &question(&[&goal.leaf]),
            by("coord", 3),
        )
        .unwrap()
        .request
        .id;
    let coord = by("coord", 4);
    assert!(matches!(
        done(&mut store, &goal.goal, coord, true),
        Err(Error::Outstanding { .. })
    ));
    assert!(matches!(
        store.set_status(
            &project(),
            &goal.goal,
            Status::Dropped,
            Some("stop"),
            coord,
            true
        ),
        Err(Error::Outstanding { .. })
    ));
    assert!(matches!(
        store.detach_goal(&project(), &goal.goal, coord),
        Err(Error::Invalid(_))
    ));
    let other_root = store
        .add(
            &project(),
            &NewTask {
                title: "elsewhere".into(),
                ..NewTask::default()
            },
            coord,
        )
        .unwrap()
        .id;
    assert!(matches!(
        store.update(
            &project(),
            &goal.group,
            &Update {
                parent: Some(Some(other_root)),
                ..Update::default()
            },
            coord,
            false
        ),
        Err(Error::Outstanding { .. })
    ));
    // Pausing is not terminal.
    store
        .set_status(
            &project(),
            &goal.goal,
            Status::Held,
            Some("pause"),
            coord,
            false,
        )
        .unwrap();
    store
        .coordinate(&project(), &goal.goal, owner("coord"), false, true, 5)
        .unwrap();
    store
        .withdraw_request(&project(), &id, "accepted earlier", by("coord", 6))
        .unwrap();
    store
        .set_status(
            &project(),
            &goal.goal,
            Status::Dropped,
            Some("stop"),
            by("coord", 7),
            true,
        )
        .unwrap();
}

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "one scenario follows a request or claim through every step"
)]
fn content_and_answers_are_validated_against_the_request() {
    let mut store = store();
    let goal = goal(&mut store);
    let coord = by("coord", 2);
    for key in ["", "Upper", "-lead", "has space"] {
        assert!(matches!(
            store.raise_request(&project(), &goal.goal, key, &question(&[]), coord),
            Err(Error::Invalid(_))
        ));
    }
    let mut action = question(&[]);
    action.kind = RequestKind::Action;
    assert!(matches!(
        store.raise_request(&project(), &goal.goal, "login", &action, coord),
        Err(Error::Invalid(_))
    ));
    let mut duplicate = question(&[]);
    duplicate.choices[1].id = "a".into();
    assert!(matches!(
        store.raise_request(&project(), &goal.goal, "dup", &duplicate, coord),
        Err(Error::Invalid(_))
    ));
    let outside = store
        .add(
            &project(),
            &NewTask {
                title: "outside".into(),
                ..NewTask::default()
            },
            coord,
        )
        .unwrap()
        .id;
    assert!(matches!(
        store.raise_request(
            &project(),
            &goal.goal,
            "outside",
            &question(&[&outside]),
            coord
        ),
        Err(Error::Invalid(_))
    ));
    assert!(matches!(
        store.raise_request(
            &project(),
            &goal.goal,
            "missing",
            &question(&["t-missing"]),
            coord
        ),
        Err(Error::NotFound(_))
    ));
    assert!(matches!(
        store.raise_request(
            &project(),
            &goal.goal,
            "worker",
            &question(&[]),
            by("worker", 2)
        ),
        Err(Error::NotCoordinator { .. })
    ));
    let id = store
        .raise_request(&project(), &goal.goal, "layout", &question(&[]), coord)
        .unwrap()
        .request
        .id;
    let invalid = [
        AnswerContent {
            choice: None,
            text: None,
            ..ui_answer("a")
        },
        AnswerContent {
            interpretation: Some("the user meant a".into()),
            ..ui_answer("a")
        },
        AnswerContent {
            source: Source::Chat,
            text: Some("go with a".into()),
            ..ui_answer("a")
        },
        AnswerContent {
            source: Source::Chat,
            interpretation: Some("a".into()),
            ..ui_answer("a")
        },
    ];
    for answer in invalid {
        assert!(matches!(
            store.answer_request(&project(), &id, &answer, by("human", 3)),
            Err(Error::Invalid(_))
        ));
    }
    assert!(matches!(
        store.answer_request(&project(), &id, &ui_answer("c"), by("human", 3)),
        Err(Error::RequestConflict { .. })
    ));
    let chat = AnswerContent {
        choice: Some("a".into()),
        text: Some("one table is fine".into()),
        source: Source::Chat,
        provenance: Some("pi session s entry e".into()),
        interpretation: Some("Selects choice a".into()),
    };
    let answered = store
        .answer_request(&project(), &id, &chat, by("coord", 4))
        .unwrap();
    assert_eq!(answered.answer.unwrap().content, chat);
    assert!(matches!(
        store.request(&other_project(), &id),
        Err(Error::RequestNotFound(_))
    ));
}

#[test]
fn standalone_decisions_need_the_coordinator_and_human_input() {
    let mut store = store();
    let goal = goal(&mut store);
    let mandate = DecisionContent {
        kind: DecisionKind::Mandate,
        decision: "Implement and verify locally; no publication.".into(),
        human: HumanInput {
            text: "go ahead, local commits only".into(),
            source: Source::Chat,
            provenance: Some("pi session s entry e".into()),
        },
    };
    assert!(matches!(
        store.record_decision(&project(), &goal.goal, &mandate, by("worker", 2)),
        Err(Error::NotCoordinator { .. })
    ));
    assert!(matches!(
        store.record_decision(&project(), &goal.leaf, &mandate, by("coord", 2)),
        Err(Error::Invalid(_))
    ));
    let blank = DecisionContent {
        human: HumanInput {
            text: " ".into(),
            ..mandate.human.clone()
        },
        ..mandate.clone()
    };
    assert!(matches!(
        store.record_decision(&project(), &goal.goal, &blank, by("coord", 2)),
        Err(Error::Invalid(_))
    ));
    let recorded = store
        .record_decision(&project(), &goal.goal, &mandate, by("coord", 2))
        .unwrap();
    assert_eq!(recorded.human.as_ref(), Some(&mandate.human));
    assert!(recorded.request.is_none());
    assert_eq!(
        kinds(&mut store, &goal.goal).last().unwrap(),
        "decision-recorded"
    );
    let scope = Scope::Goals(vec![GoalRef {
        project: project(),
        goal: goal.goal.clone(),
    }]);
    let listed = store.attention(&scope, StateFilter::Outstanding).unwrap();
    assert_eq!(listed.goals[0].mandate.as_ref().unwrap().id, recorded.id);
    assert!(
        store
            .decisions(&scope, Some(DecisionKind::Decision))
            .unwrap()
            .goals[0]
            .decisions
            .is_empty()
    );
}

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "one scenario follows a request or claim through every step"
)]
fn global_and_exact_scopes_read_the_same_records() {
    let mut store = store();
    let first = goal(&mut store);
    let second = goal_in(&mut store, &other_project(), "coord");
    let draft = store
        .create_goal(&project(), "draft", "briefs/draft.md", by("coord", 1))
        .unwrap()
        .root
        .id;
    let stopped = store
        .create_goal(&project(), "stopped", "briefs/stopped.md", by("coord", 1))
        .unwrap()
        .root
        .id;
    store
        .coordinate(&project(), &stopped, owner("coord"), false, false, 1)
        .unwrap();
    store
        .record_decision(
            &project(),
            &stopped,
            &DecisionContent {
                kind: DecisionKind::Mandate,
                decision: "Ship when verified.".into(),
                human: HumanInput {
                    text: "ship it".into(),
                    source: Source::Chat,
                    provenance: None,
                },
            },
            by("coord", 2),
        )
        .unwrap();
    store.release_all(owner("coord"), 3).unwrap();
    let raise = |store: &mut Store, project: &Project, goal: &str| {
        store
            .coordinate(project, goal, owner("coord"), false, false, 4)
            .unwrap();
        store
            .raise_request(project, goal, "layout", &question(&[]), by("coord", 4))
            .unwrap()
            .request
            .id
    };
    let one = raise(&mut store, &project(), &first.goal);
    let two = raise(&mut store, &other_project(), &second.goal);

    let global = store
        .attention(&Scope::Global, StateFilter::Outstanding)
        .unwrap();
    assert!(global.errors.is_empty());
    let listed: Vec<_> = global
        .goals
        .iter()
        .map(|goal| {
            (
                goal.goal.as_str(),
                goal.requests.len(),
                goal.coordinator.is_some(),
            )
        })
        .collect();
    assert_eq!(
        listed,
        [
            (first.goal.as_str(), 1, true),
            (stopped.as_str(), 0, false),
            (second.goal.as_str(), 1, true)
        ]
    );
    assert!(global.goals.iter().all(|goal| goal.goal != draft));

    assert!(
        store
            .attention(&Scope::Goals(Vec::new()), StateFilter::All)
            .unwrap()
            .goals
            .is_empty()
    );
    let exact = store
        .attention(
            &Scope::Goals(vec![
                GoalRef {
                    project: other_project(),
                    goal: second.goal.clone(),
                },
                GoalRef {
                    project: project(),
                    goal: draft.clone(),
                },
                GoalRef {
                    project: project(),
                    goal: first.leaf.clone(),
                },
                GoalRef {
                    project: Project::try_new("p-00000000000000000000000000000009").unwrap(),
                    goal: first.goal.clone(),
                },
            ]),
            StateFilter::Outstanding,
        )
        .unwrap();
    let listed: Vec<_> = exact.goals.iter().map(|goal| goal.goal.as_str()).collect();
    assert_eq!(listed, [second.goal.as_str(), draft.as_str()]);
    assert_eq!(exact.goals[0].requests[0].id, two);
    assert!(exact.goals[1].requests.is_empty());
    let failed: Vec<_> = exact
        .errors
        .iter()
        .map(|error| (error.project.as_str(), error.goal.as_deref()))
        .collect();
    assert_eq!(
        failed,
        [
            (
                "p-00000000000000000000000000000009",
                Some(first.goal.as_str())
            ),
            (project().as_str(), Some(first.leaf.as_str())),
        ]
    );

    // A corrupt record in one project is reported; other projects still list.
    store
        .connection
        .execute("UPDATE requests SET kind='bogus' WHERE id=?1", [&two])
        .unwrap();
    let global = store
        .attention(&Scope::Global, StateFilter::Outstanding)
        .unwrap();
    assert_eq!(global.errors.len(), 1);
    assert_eq!(global.errors[0].project, other_project().as_str());
    assert!(
        global.errors[0].error.contains("kind"),
        "{}",
        global.errors[0].error
    );
    assert_eq!(global.goals[0].requests[0].id, one);
    assert!(store.get(&other_project(), &second.leaf).is_ok());
    assert!(
        store
            .list(&project(), &Filter::default())
            .unwrap()
            .iter()
            .all(|task| task.holds.is_empty())
    );
}

#[test]
fn missing_and_corrupt_catalogs_are_errors() {
    let catalog = Catalog::new();
    assert!(Store::open(&catalog.root.join("absent.sqlite3")).is_err());
    let garbage = catalog.root.join("garbage.sqlite3");
    std::fs::write(&garbage, b"not a database").unwrap();
    assert!(Store::open(&garbage).is_err());
    let old = catalog.root.join("old.sqlite3");
    let connection = Connection::open(&old).unwrap();
    connection.pragma_update(None, "user_version", 8).unwrap();
    drop(connection);
    assert!(matches!(Store::open(&old), Err(Error::Schema(8))));
}

#[test]
fn attaching_a_claim_never_confers_coordination_and_history_keeps_its_goal() {
    let mut store = store();
    let task = store
        .add(
            &project(),
            &NewTask {
                title: "claimed".into(),
                ..NewTask::default()
            },
            by("worker", 1),
        )
        .unwrap()
        .id;
    store
        .claim(&project(), &task, owner("worker"), false, 1)
        .unwrap();
    for actor in ["worker", "coord"] {
        assert!(
            store
                .attach_goal(&project(), &task, "briefs/goal.md", by(actor, 2))
                .is_err()
        );
    }
    assert!(!store.get(&project(), &task).unwrap().is_goal_root());
    store
        .release(&project(), &task, owner("worker"), 3)
        .unwrap();
    let attached = store
        .attach_goal(&project(), &task, "briefs/goal.md", by("worker", 4))
        .unwrap();
    assert!(attached.root.owner.is_none());

    // Requests and decisions stay discoverable: their goal cannot be detached.
    let goal = goal(&mut store);
    let id = store
        .raise_request(
            &project(),
            &goal.goal,
            "layout",
            &question(&[]),
            by("coord", 5),
        )
        .unwrap()
        .request
        .id;
    store
        .withdraw_request(&project(), &id, "not needed", by("coord", 6))
        .unwrap();
    assert!(matches!(
        store.detach_goal(&project(), &goal.goal, by("coord", 7)),
        Err(Error::Invalid(_))
    ));
    store
        .attach_goal(&project(), &goal.goal, "briefs/renamed.md", by("coord", 8))
        .unwrap();
    let scope = Scope::Goals(vec![GoalRef {
        project: project(),
        goal: goal.goal.clone(),
    }]);
    assert_eq!(
        store.attention(&scope, StateFilter::All).unwrap().goals[0].requests[0].id,
        id
    );
}
