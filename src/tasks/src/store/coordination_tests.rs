use super::{Actor, Error, Filter, NewTask, Owner, Project, Status, Store, Task};
use crate::references::ExternalReference;
use rusqlite::Connection;

fn store() -> Store {
    crate::test_store(Store::initialize(Connection::open_in_memory().unwrap()).unwrap())
}
fn project() -> Project {
    Project::try_new("p-00000000000000000000000000000001").unwrap()
}
fn actor(now: i64) -> Actor<'static> {
    Actor {
        name: Some(Owner::try_new("parent").unwrap()),
        now,
    }
}
fn add(store: &mut Store, title: &str) -> Task {
    store
        .add(
            &project(),
            &NewTask {
                title: title.into(),
                ..NewTask::default()
            },
            actor(1),
        )
        .unwrap()
}

#[test]
fn repeated_review_replaces_result_and_releases_claim_atomically() {
    let mut store = store();
    let slice = add(&mut store, "slice");
    store
        .write_result(
            &project(),
            &slice.id,
            &crate::outcomes::Outcome {
                operation: crate::outcomes::Operation::Review,
                result: "first",
                evidence: &[],
                force: false,
            },
            actor(2),
        )
        .map(|receipt| receipt.task)
        .unwrap();
    let claimed = store
        .claim(&project(), &slice.id, actor(3).name.unwrap(), false, 3)
        .unwrap();
    let before = store.events(&project(), &slice.id, 100).unwrap().len();
    let other = Actor {
        name: Some(Owner::try_new("other").unwrap()),
        now: 4,
    };
    assert!(matches!(
        store
            .write_result(
                &project(),
                &slice.id,
                &crate::outcomes::Outcome {
                    operation: crate::outcomes::Operation::Review,
                    result: "overwrite",
                    evidence: &[],
                    force: false
                },
                other
            )
            .map(|receipt| receipt.task),
        Err(Error::OwnedByAnother { .. })
    ));
    for invalid in [None, Some(" ")] {
        assert!(matches!(
            store
                .write_result(
                    &project(),
                    &slice.id,
                    &crate::outcomes::Outcome {
                        operation: crate::outcomes::Operation::Review,
                        result: invalid.unwrap_or_default(),
                        evidence: &[],
                        force: true
                    },
                    actor(4)
                )
                .map(|receipt| receipt.task),
            Err(Error::Invalid(_))
        ));
    }
    let unchanged = store.get(&project(), &slice.id).unwrap();
    assert_eq!(unchanged.result, claimed.result);
    assert_eq!(unchanged.owner, claimed.owner);
    assert_eq!(
        store.events(&project(), &slice.id, 100).unwrap().len(),
        before
    );
    let handoff = store
        .write_result(
            &project(),
            &slice.id,
            &crate::outcomes::Outcome {
                operation: crate::outcomes::Operation::Review,
                result: "revised",
                evidence: &[],
                force: false,
            },
            actor(5),
        )
        .map(|receipt| receipt.task)
        .unwrap();
    assert_eq!(handoff.status, Status::Review);
    assert_eq!(handoff.result.as_deref(), Some("revised"));
    assert!(handoff.owner.is_none());
    assert!(handoff.closed_at.is_none());
    assert!(!handoff.ready());
    assert_eq!(handoff.updated_at, 5);
    assert_eq!(
        store.events(&project(), &slice.id, 100).unwrap().len(),
        before + 1
    );
    assert_eq!(handoff.last_event.as_ref().unwrap().kind, "review");
    assert_eq!(
        handoff.last_event.as_ref().unwrap().result.as_deref(),
        Some("revised")
    );
}

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "One transactional handoff scenario checks ownership, review, acceptance, and reopen"
)]
fn review_requires_evidence_and_explicit_claims_without_unblocking_dependents() {
    let mut store = store();
    let slice = add(&mut store, "slice");
    let dependent = add(&mut store, "dependent");
    store
        .add_dependency(&project(), &dependent.id, &slice.id, actor(1))
        .unwrap();
    store
        .claim(&project(), &slice.id, actor(1).name.unwrap(), false, 1)
        .unwrap();
    for status in [Status::Review, Status::Done] {
        for result in [None, Some(" ")] {
            assert!(matches!(
                store
                    .write_result(
                        &project(),
                        &slice.id,
                        &crate::outcomes::Outcome {
                            operation: if status == Status::Review {
                                crate::outcomes::Operation::Review
                            } else {
                                crate::outcomes::Operation::Done
                            },
                            result: result.unwrap_or_default(),
                            evidence: &[],
                            force: true
                        },
                        actor(2)
                    )
                    .map(|receipt| receipt.task),
                Err(Error::Invalid(_))
            ));
            let unchanged = store.get(&project(), &slice.id).unwrap();
            assert_eq!(unchanged.status, Status::Open);
            assert_eq!(unchanged.owner.as_deref(), Some("parent"));
        }
    }
    let outsider = Actor {
        name: Some(Owner::try_new("other").unwrap()),
        now: 2,
    };
    assert!(matches!(
        store
            .write_result(
                &project(),
                &slice.id,
                &crate::outcomes::Outcome {
                    operation: crate::outcomes::Operation::Review,
                    result: "ready",
                    evidence: &[],
                    force: false
                },
                outsider
            )
            .map(|receipt| receipt.task),
        Err(Error::OwnedByAnother { .. })
    ));
    let handoff = store
        .write_result(
            &project(),
            &slice.id,
            &crate::outcomes::Outcome {
                operation: crate::outcomes::Operation::Review,
                result: "Implemented on branch slice; tests pass; integration remains.",
                evidence: &[],
                force: false,
            },
            actor(3),
        )
        .map(|receipt| receipt.task)
        .unwrap();
    assert_eq!(
        handoff.result.as_deref(),
        Some("Implemented on branch slice; tests pass; integration remains.")
    );
    assert!(handoff.owner.is_none());
    assert!(handoff.closed_at.is_none());
    assert!(!handoff.ready());
    let reviewer = Owner::try_new("reviewer").unwrap();
    assert!(
        store
            .claim_next(&project(), None, reviewer, 100)
            .unwrap()
            .is_none()
    );
    assert_eq!(
        store.get(&project(), &dependent.id).unwrap().blocked_by,
        std::slice::from_ref(&slice.id)
    );
    let claimed = store
        .claim(&project(), &slice.id, reviewer, false, 100)
        .unwrap();
    assert_eq!(claimed.status, Status::Review);
    assert!(matches!(
        store
            .write_result(
                &project(),
                &slice.id,
                &crate::outcomes::Outcome {
                    operation: crate::outcomes::Operation::UpdateResult,
                    result: "overwrite",
                    evidence: &[],
                    force: false
                },
                actor(101)
            )
            .map(|receipt| receipt.task),
        Err(Error::OwnedByAnother { .. })
    ));
    let done = store
        .write_result(
            &project(),
            &slice.id,
            &crate::outcomes::Outcome {
                operation: crate::outcomes::Operation::Done,
                result: "Integrated and checked combined result.",
                evidence: &[],
                force: false,
            },
            Actor {
                name: Some(reviewer),
                now: 102,
            },
        )
        .map(|receipt| receipt.task)
        .unwrap();
    assert_eq!(
        done.result.as_deref(),
        Some("Integrated and checked combined result.")
    );
    assert!(store.get(&project(), &dependent.id).unwrap().ready());
    let reopened = store
        .set_status(
            &project(),
            &slice.id,
            Status::Open,
            Some("additional requirement"),
            actor(104),
            false,
        )
        .unwrap();
    assert_eq!(reopened.result, done.result);
    assert!(reopened.ready());
}

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "One association lifecycle checks scope, upserts, ownership, and removal"
)]
fn references_are_scoped_claim_protected_and_do_not_change_readiness() {
    let mut store = store();
    let root = add(&mut store, "goal");
    let slice = store
        .add(
            &project(),
            &NewTask {
                title: "slice".into(),
                parent: Some(root.id.clone()),
                ..NewTask::default()
            },
            actor(1),
        )
        .unwrap();
    let issue = ExternalReference {
        url: "https://linear.app/example/issue/ENG-123".into(),
        label: Some("ENG-123".into()),
    };
    store
        .set_reference(&project(), &root.id, &issue, actor(2), false)
        .unwrap();
    assert!(
        store
            .get(&project(), &slice.id)
            .unwrap()
            .references
            .is_empty()
    );
    let pr = ExternalReference {
        url: "https://github.com/example/project/pull/7".into(),
        label: None,
    };
    let linked = store
        .set_reference(&project(), &slice.id, &pr, actor(2), false)
        .unwrap();
    assert!(linked.ready());
    assert_eq!(linked.references.len(), 1);
    store
        .set_reference(&project(), &slice.id, &issue, actor(3), false)
        .unwrap();
    let mut revised = issue.clone();
    revised.label = Some("ENG-123 follow-up".into());
    let updated = store
        .set_reference(&project(), &slice.id, &revised, actor(4), false)
        .unwrap();
    assert_eq!(updated.references.len(), 2);
    assert_eq!(
        updated
            .references
            .iter()
            .find(|r| r.url == issue.url)
            .unwrap()
            .label
            .as_deref(),
        Some("ENG-123 follow-up")
    );
    store
        .claim(
            &project(),
            &slice.id,
            Owner::try_new("other").unwrap(),
            false,
            5,
        )
        .unwrap();
    assert!(matches!(
        store.set_reference(&project(), &slice.id, &pr, actor(6), false),
        Err(Error::OwnedByAnother { .. })
    ));
    assert!(matches!(
        store.remove_reference(&project(), &slice.id, &pr.url, actor(6), false),
        Err(Error::OwnedByAnother { .. })
    ));
    assert!(matches!(
        store.set_reference(
            &Project::try_new("p-00000000000000000000000000000002").unwrap(),
            &slice.id,
            &pr,
            actor(6),
            true
        ),
        Err(Error::NotFound(_))
    ));
    store
        .release(&project(), &slice.id, Owner::try_new("other").unwrap(), 7)
        .unwrap();
    let removed = store
        .remove_reference(&project(), &slice.id, &pr.url, actor(70), false)
        .unwrap();
    assert_eq!(removed.references.len(), 1);
    assert!(removed.ready());
    assert!(
        store
            .remove_reference(&project(), &slice.id, &pr.url, actor(71), false)
            .is_err()
    );
    for url in [
        "ENG-123",
        "https://user:password@example.com/issue/1",
        "https:///issue/1",
        "https://example.com/issue/1\n",
    ] {
        let bad = ExternalReference {
            url: url.into(),
            ..issue.clone()
        };
        assert!(
            store
                .set_reference(&project(), &slice.id, &bad, actor(72), false)
                .is_err()
        );
    }
}

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "one scenario covers review, acceptance and visibility"
)]
fn review_children_prevent_parent_acceptance_and_remain_visible() {
    let mut store = store();
    let root = store
        .create_goal(&project(), "goal", "briefs/brief.md", actor(1))
        .unwrap()
        .root;
    store
        .coordinate(
            &project(),
            &root.id,
            actor(1).name.unwrap(),
            false,
            false,
            1,
        )
        .unwrap();
    let slice = store
        .add(
            &project(),
            &NewTask {
                title: "slice".into(),
                parent: Some(root.id.clone()),
                ..NewTask::default()
            },
            actor(1),
        )
        .unwrap();
    store
        .write_result(
            &project(),
            &slice.id,
            &crate::outcomes::Outcome {
                operation: crate::outcomes::Operation::Review,
                result: "ready for integration",
                evidence: &[],
                force: false,
            },
            actor(2),
        )
        .map(|receipt| receipt.task)
        .unwrap();
    for status in [Status::Review, Status::Done] {
        assert!(matches!(
            store
                .write_result(
                    &project(),
                    &root.id,
                    &crate::outcomes::Outcome {
                        operation: if status == Status::Review {
                            crate::outcomes::Operation::Review
                        } else {
                            crate::outcomes::Operation::Done
                        },
                        result: "combined outcome",
                        evidence: &[],
                        force: false
                    },
                    actor(3)
                )
                .map(|receipt| receipt.task),
            Err(Error::Incomplete { .. })
        ));
    }
    assert_eq!(store.list(&project(), &Filter::default()).unwrap().len(), 2);
    let goal = store.goal_overview(&project(), &root.id, false).unwrap();
    assert_eq!(
        (
            goal.goal.progress.total,
            goal.goal.progress.review,
            goal.goal.progress.open,
            goal.goal.progress.ready
        ),
        (1, 1, 0, 0)
    );
    store
        .claim(&project(), &slice.id, actor(4).name.unwrap(), false, 4)
        .unwrap();
    assert_eq!(
        store
            .goal_overview(&project(), &root.id, false)
            .unwrap()
            .goal
            .progress
            .claimed,
        1
    );
    store
        .write_result(
            &project(),
            &slice.id,
            &crate::outcomes::Outcome {
                operation: crate::outcomes::Operation::Done,
                result: "integrated and checked",
                evidence: &[],
                force: false,
            },
            actor(6),
        )
        .map(|receipt| receipt.task)
        .unwrap();
    assert_eq!(
        store.get(&project(), &root.id).unwrap().status,
        Status::Open
    );
}

#[test]
fn closed_results_stay_until_reopened_even_when_forced() {
    use crate::outcomes::Operation::{Done, Review, UpdateResult};
    let mut store = store();
    let slice = add(&mut store, "slice");
    let write = |store: &mut Store, operation, result, force, name: &str, now| {
        store.write_result(
            &project(),
            &slice.id,
            &crate::outcomes::Outcome {
                operation,
                result,
                evidence: &[],
                force,
            },
            Actor {
                name: Some(Owner::try_new(name).unwrap()),
                now,
            },
        )
    };
    write(&mut store, Review, "Ready from A", false, "a", 2).unwrap();
    write(&mut store, Done, "Accepted by B", false, "b", 3).unwrap();
    let accepted = store.get(&project(), &slice.id).unwrap();
    let events = store.events(&project(), &slice.id, 100).unwrap().len();
    for (operation, force, name) in [
        (Review, false, "a"),
        (Review, true, "a"),
        (UpdateResult, false, "c"),
        (Done, true, "c"),
    ] {
        let late = write(&mut store, operation, "late", force, name, 4);
        assert!(
            matches!(late, Err(Error::Closed { status: "done", .. })),
            "late {operation:?} by {name} (force {force}): got {late:?}, want Closed"
        );
    }
    let current = store.get(&project(), &slice.id).unwrap();
    assert_eq!(current.status, Status::Done);
    assert_eq!(current.result, accepted.result);
    assert_eq!(current.result_event, accepted.result_event);
    assert_eq!(
        store.events(&project(), &slice.id, 100).unwrap().len(),
        events
    );
    store
        .set_status(
            &project(),
            &slice.id,
            Status::Open,
            Some("Rework"),
            actor(5),
            false,
        )
        .unwrap();
    let revised = write(&mut store, Review, "Reworked", false, "a", 6).unwrap();
    assert_eq!(revised.task.status, Status::Review);
    assert_eq!(revised.task.result.as_deref(), Some("Reworked"));
}
