use super::{Actor, Error, Filter, NewTask, Owner, Project, Status, Store, Task, Update};
use rusqlite::Connection;

fn store() -> Store {
    crate::test_store(Store::initialize(Connection::open_in_memory().unwrap()).unwrap())
}

fn project() -> Project {
    Project::try_new("p-00000000000000000000000000000001").unwrap()
}

fn owner(name: &str) -> Owner<'_> {
    Owner::try_new(name).unwrap()
}

fn actor(now: i64) -> Actor<'static> {
    Actor {
        name: Some(owner("parent")),
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

fn counts(store: &Store) -> (i64, i64, i64) {
    store.connection.query_row(
        "SELECT (SELECT count(*) FROM tasks), (SELECT count(*) FROM deps), (SELECT count(*) FROM events)",
        [], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    ).unwrap()
}

#[test]
fn insertion_rejects_parent_and_indirect_ancestor_dependencies_atomically() {
    let mut store = store();
    let root = add(&mut store, "root");
    let parent = store
        .add(
            &project(),
            &NewTask {
                title: "parent".into(),
                parent: Some(root.id.clone()),
                ..NewTask::default()
            },
            actor(2),
        )
        .unwrap();
    let unrelated = add(&mut store, "unrelated");
    for blocker in [&parent.id, &root.id] {
        let before = counts(&store);
        assert!(matches!(
            store.add(
                &project(),
                &NewTask {
                    title: "child".into(),
                    parent: Some(parent.id.clone()),
                    blocked_by: vec![unrelated.id.clone(), blocker.clone()],
                    ..NewTask::default()
                },
                actor(3)
            ),
            Err(Error::Hierarchy { .. })
        ));
        assert_eq!(
            counts(&store),
            before,
            "task, first dependency, and events roll back"
        );
    }
    store
        .add(
            &project(),
            &NewTask {
                title: "valid child".into(),
                parent: Some(parent.id),
                blocked_by: vec![unrelated.id],
                ..NewTask::default()
            },
            actor(4),
        )
        .unwrap();
}

#[test]
fn reparenting_checks_edges_in_both_directions_for_the_whole_subtree() {
    for nested in [false, true] {
        for incoming in [false, true] {
            let mut store = store();
            let moving = add(&mut store, "moving");
            let target = add(&mut store, "target");
            let descendant = store
                .add(
                    &project(),
                    &NewTask {
                        title: "descendant".into(),
                        parent: Some(moving.id.clone()),
                        ..NewTask::default()
                    },
                    actor(2),
                )
                .unwrap();
            let endpoint = if nested { &descendant.id } else { &moving.id };
            let (dependent, blocker) = if incoming {
                (&target.id, endpoint)
            } else {
                (endpoint, &target.id)
            };
            store
                .add_dependency(&project(), dependent, blocker, actor(3))
                .unwrap();
            let before = counts(&store);
            assert!(matches!(
                store.update(
                    &project(),
                    &moving.id,
                    &Update {
                        parent: Some(Some(target.id.clone())),
                        title: Some("must roll back".into()),
                        ..Update::default()
                    },
                    actor(4),
                    false
                ),
                Err(Error::Hierarchy { .. })
            ));
            assert_eq!(counts(&store), before);
            let restored = store.get(&project(), &moving.id).unwrap();
            assert_eq!(restored.parent, None);
            assert_eq!(restored.title, "moving");
            assert_eq!(
                store
                    .get(&project(), &descendant.id)
                    .unwrap()
                    .parent
                    .as_deref(),
                Some(moving.id.as_str())
            );
            let unrelated = add(&mut store, "unrelated");
            store
                .update(
                    &project(),
                    &moving.id,
                    &Update {
                        parent: Some(Some(unrelated.id)),
                        ..Update::default()
                    },
                    actor(5),
                    false,
                )
                .unwrap();
        }
    }
}

#[test]
fn readiness_follows_dependencies_status_and_claims() {
    let mut store = store();
    let first = add(&mut store, "first");
    let second = add(&mut store, "second");
    store
        .add_dependency(&project(), &second.id, &first.id, actor(2))
        .unwrap();
    assert_eq!(store.ready(&project(), None).unwrap().len(), 1);
    // The reason names each blocker's status, so a caller knows what clears it.
    match store.claim(&project(), &second.id, owner("a"), false, 3) {
        Err(Error::NotReady { reason, .. }) => assert!(
            reason.starts_with(&format!("blocked by {} (open);", first.id)),
            "claim blocked task: got reason {reason:?}"
        ),
        other => panic!("claim blocked task: got {other:?}, want NotReady"),
    }
    assert!(matches!(
        store.add_dependency(&project(), &first.id, &second.id, actor(3)),
        Err(Error::Cycle { .. })
    ));
    let claimed = store
        .claim(&project(), &first.id, owner("a"), false, 4)
        .unwrap();
    assert_eq!(claimed.owner.as_deref(), Some("a"));
    assert!(store.ready(&project(), None).unwrap().is_empty());
    assert!(matches!(
        store.claim(&project(), &first.id, owner("b"), false, 5),
        Err(Error::Claimed { .. })
    ));
    let unchanged = store
        .claim(&project(), &first.id, owner("a"), false, 6)
        .unwrap();
    assert_eq!(unchanged.updated_at, claimed.updated_at);
    let reclaimed = store
        .claim(&project(), &first.id, owner("b"), true, 70)
        .unwrap();
    assert_eq!(reclaimed.owner.as_deref(), Some("b"));
    assert!(matches!(
        store
            .write_result(
                &project(),
                &first.id,
                &crate::outcomes::Outcome {
                    operation: crate::outcomes::Operation::Done,
                    result: "valid result",
                    evidence: &[],
                    force: false
                },
                actor(71)
            )
            .map(|receipt| receipt.task),
        Err(Error::OwnedByAnother { .. })
    ));
    let done = store
        .write_result(
            &project(),
            &first.id,
            &crate::outcomes::Outcome {
                operation: crate::outcomes::Operation::Done,
                result: "merged",
                evidence: &[],
                force: false,
            },
            Actor {
                name: Some(owner("b")),
                now: 72,
            },
        )
        .map(|receipt| receipt.task)
        .unwrap();
    assert_eq!(done.closed_at, Some(72));
    assert!(done.owner.is_none());
    let second = store.get(&project(), &second.id).unwrap();
    assert!(second.blocked_by.is_empty() && second.ready());
    let held = store
        .set_status(
            &project(),
            &second.id,
            Status::Held,
            Some("waiting"),
            actor(74),
            false,
        )
        .unwrap();
    assert!(!held.ready());
    assert!(store.ready(&project(), None).unwrap().is_empty());
    history_is_recorded(&mut store, &first.id);
}

fn history_is_recorded(store: &mut Store, first: &str) {
    let kinds: Vec<_> = store
        .events(&project(), first, 10)
        .unwrap()
        .into_iter()
        .map(|event| event.kind)
        .collect();
    assert_eq!(kinds, ["created", "claimed", "claimed", "done"]);
    assert!(matches!(
        store.get(
            &Project::try_new("p-00000000000000000000000000000002").unwrap(),
            first
        ),
        Err(Error::NotFound(_))
    ));
}

#[test]
fn child_counts_and_descendants_track_closure() {
    let mut store = store();
    let root = add(&mut store, "root");
    assert_eq!(root.children, 0);
    assert_eq!(root.unclosed_children, 0);
    assert_eq!(store.unclosed_descendants(&project(), &root.id).unwrap(), 0);

    let child = store
        .add(
            &project(),
            &NewTask {
                title: "child".into(),
                parent: Some(root.id.clone()),
                ..NewTask::default()
            },
            actor(2),
        )
        .unwrap();
    let grandchild = store
        .add(
            &project(),
            &NewTask {
                title: "grandchild".into(),
                parent: Some(child.id.clone()),
                ..NewTask::default()
            },
            actor(3),
        )
        .unwrap();
    let root = store.get(&project(), &root.id).unwrap();
    assert_eq!((root.children, root.unclosed_children), (1, 1));
    assert_eq!(store.unclosed_descendants(&project(), &root.id).unwrap(), 2);
    assert!(!root.ready());

    store
        .write_result(
            &project(),
            &grandchild.id,
            &crate::outcomes::Outcome {
                operation: crate::outcomes::Operation::Done,
                result: "checked",
                evidence: &[],
                force: false,
            },
            actor(4),
        )
        .map(|receipt| receipt.task)
        .unwrap();
    assert_eq!(store.unclosed_descendants(&project(), &root.id).unwrap(), 1);
    store
        .write_result(
            &project(),
            &child.id,
            &crate::outcomes::Outcome {
                operation: crate::outcomes::Operation::Done,
                result: "checked",
                evidence: &[],
                force: false,
            },
            actor(5),
        )
        .map(|receipt| receipt.task)
        .unwrap();
    let root = store.get(&project(), &root.id).unwrap();
    assert_eq!((root.children, root.unclosed_children), (1, 0));
    assert_eq!(store.unclosed_descendants(&project(), &root.id).unwrap(), 0);
    assert!(root.ready());
}

#[test]
fn hierarchy_and_updates_are_scoped() {
    let mut store = store();
    let epic = add(&mut store, "migration");
    let slice = store
        .add(
            &project(),
            &NewTask {
                title: "slice".into(),
                parent: Some(epic.id.clone()),
                ..NewTask::default()
            },
            actor(1),
        )
        .unwrap();
    assert_eq!(store.get(&project(), &epic.id).unwrap().children, 1);
    assert_eq!(
        store
            .list(
                &project(),
                &Filter {
                    parent: Some(&epic.id),
                    ..Filter::default()
                }
            )
            .unwrap()
            .len(),
        1
    );
    let updated = store
        .write_result(
            &project(),
            &slice.id,
            &crate::outcomes::Outcome {
                operation: crate::outcomes::Operation::UpdateResult,
                result: "Implemented and checked the change",
                evidence: &[],
                force: false,
            },
            actor(2),
        )
        .map(|receipt| receipt.task)
        .unwrap();
    assert!(updated.result.is_some());
    assert!(matches!(
        store.update(
            &project(),
            &slice.id,
            &Update {
                parent: Some(Some(slice.id.clone())),
                ..Update::default()
            },
            actor(3),
            false,
        ),
        Err(Error::AncestorCycle { .. })
    ));
    store
        .set_status(
            &project(),
            &slice.id,
            Status::Dropped,
            Some("obsolete"),
            actor(4),
            false,
        )
        .unwrap();
    assert!(
        store
            .list(&project(), &Filter::default())
            .unwrap()
            .iter()
            .all(|task| task.id == epic.id)
    );
    assert_eq!(
        store
            .list(
                &project(),
                &Filter {
                    include_closed: true,
                    ..Filter::default()
                }
            )
            .unwrap()
            .len(),
        2
    );
    let reopened = store
        .set_status(&project(), &slice.id, Status::Open, None, actor(5), false)
        .unwrap();
    assert!(reopened.closed_at.is_none());
}

#[test]
fn competing_sessions_never_share_a_claim() {
    let directory = std::env::temp_dir().join(format!("dots-tasks-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("tasks.sqlite3");
    let _ = std::fs::remove_file(&path);
    let mut store = crate::test_store(Store::create(&path).unwrap());
    let contested = add(&mut store, "contested");
    for number in 0..4 {
        add(&mut store, &format!("slice {number}"));
    }
    drop(store);
    let workers: Vec<_> = (0..8)
        .map(|number| {
            let path = path.clone();
            let contested = contested.id.clone();
            std::thread::spawn(move || {
                let mut store = Store::open(&path).unwrap();
                let name = format!("session-{number}");
                let won = store
                    .claim(&project(), &contested, owner(&name), false, 10)
                    .is_ok();
                let next = store
                    .claim_next(&project(), None, owner(&name), 10)
                    .unwrap();
                (won, next.map(|task| task.id))
            })
        })
        .collect();
    let results: Vec<_> = workers
        .into_iter()
        .map(|worker| worker.join().unwrap())
        .collect();
    assert_eq!(results.iter().filter(|(won, _)| *won).count(), 1);
    let mut taken: Vec<_> = results
        .iter()
        .filter_map(|(_, next)| next.clone())
        .collect();
    let distinct = taken.len();
    taken.sort();
    taken.dedup();
    assert_eq!(distinct, 4);
    assert_eq!(taken.len(), 4);
    let mut store = Store::open(&path).unwrap();
    assert!(store.ready(&project(), None).unwrap().is_empty());
    let released: usize = (0..8)
        .map(|number| {
            let name = format!("session-{number}");
            store.release_all(owner(&name), 11).unwrap().len()
        })
        .sum();
    assert_eq!(released, 5);
    assert_eq!(store.ready(&project(), None).unwrap().len(), 5);
    drop(store);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn release_reopen_and_explicit_claims_respect_owners_and_children() {
    let mut store = store();
    let epic = add(&mut store, "epic");
    let slice = store
        .add(
            &project(),
            &NewTask {
                title: "slice".into(),
                parent: Some(epic.id.clone()),
                ..NewTask::default()
            },
            actor(1),
        )
        .unwrap();
    assert!(matches!(
        store.claim(&project(), &epic.id, owner("a"), false, 2),
        Err(Error::NotReady { .. })
    ));
    store
        .claim(&project(), &slice.id, owner("a"), false, 2)
        .unwrap();
    assert!(matches!(
        store.release(&project(), &slice.id, owner("b"), 3),
        Err(Error::OwnedByAnother { .. })
    ));
    assert!(matches!(
        store.set_status(&project(), &slice.id, Status::Open, None, actor(3), false),
        Err(Error::Unchanged { .. })
    ));
    let held = store
        .set_status(
            &project(),
            &slice.id,
            Status::Held,
            Some("wait"),
            actor(3),
            true,
        )
        .unwrap();
    assert!(held.owner.is_none());
    store
        .set_status(&project(), &slice.id, Status::Open, None, actor(4), false)
        .unwrap();
    assert!(matches!(
        store.release(&project(), &slice.id, owner("a"), 4),
        Err(Error::Unchanged { .. })
    ));
    store
        .claim(&project(), &slice.id, owner("a"), false, 4)
        .unwrap();
    let released = store.release(&project(), &slice.id, owner("a"), 5).unwrap();
    assert!(released.owner.is_none());
    store
        .claim(&project(), &slice.id, owner("a"), false, 6)
        .unwrap();
    assert!(matches!(
        store.release(&project(), &slice.id, owner("b"), 100),
        Err(Error::OwnedByAnother { .. })
    ));
    assert_eq!(
        store.release_all(owner("a"), 100).unwrap(),
        std::slice::from_ref(&slice.id)
    );
    assert!(store.get(&project(), &slice.id).unwrap().owner.is_none());
    assert_eq!(
        store.release_all(owner("a"), 101).unwrap(),
        Vec::<String>::new()
    );
    assert!(matches!(
        store.update(
            &project(),
            &epic.id,
            &Update {
                parent: Some(Some(slice.id.clone())),
                ..Update::default()
            },
            actor(7),
            false,
        ),
        Err(Error::AncestorCycle { .. })
    ));
    let roots = store
        .list(
            &project(),
            &Filter {
                roots: true,
                ..Filter::default()
            },
        )
        .unwrap();
    assert_eq!(roots.len(), 1);
    assert!(Owner::try_new(" ").is_err());
    assert_eq!(Owner::try_new(" a ").unwrap().as_str(), "a");
}
