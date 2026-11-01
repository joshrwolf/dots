use super::{Actor, Error, Project, Status, Store, Task};
use crate::store::{NewTask, Owner, Update};
use rusqlite::Connection;

fn project() -> Project {
    Project::try_new("p-00000000000000000000000000000001").unwrap()
}
fn actor(now: i64) -> Actor<'static> {
    Actor {
        name: Some(Owner::try_new("parent").unwrap()),
        now,
    }
}
fn store() -> Store {
    crate::test_store(Store::initialize(Connection::open_in_memory().unwrap()).unwrap())
}
fn add(store: &mut Store, title: &str, parent: Option<&str>) -> Task {
    store
        .add(
            &project(),
            &NewTask {
                title: title.into(),
                parent: parent.map(str::to_owned),
                ..NewTask::default()
            },
            actor(1),
        )
        .unwrap()
}

#[test]
fn claimed_goals_filter_claims_through_ancestors_and_project() {
    let mut store = store();
    let root = store
        .create_goal(&project(), "goal", "briefs/brief.md", actor(1))
        .unwrap();
    let group = add(&mut store, "group", Some(&root.root.id));
    let leaf = add(&mut store, "leaf", Some(&group.id));
    let sibling = add(&mut store, "sibling", Some(&group.id));
    let owner = Owner::try_new("session").unwrap();
    for id in [&leaf.id, &sibling.id] {
        store.claim(&project(), id, owner, false, 10).unwrap();
    }
    assert_eq!(
        store.goals(&project(), false, Some(owner),).unwrap().len(),
        1
    );
    assert!(
        store
            .goals(&project(), false, Some(Owner::try_new("other").unwrap()))
            .unwrap()
            .is_empty()
    );
    assert!(
        store
            .goals(
                &Project::try_new("p-00000000000000000000000000000002").unwrap(),
                false,
                Some(owner),
            )
            .unwrap()
            .is_empty()
    );
    store.release(&project(), &leaf.id, owner, 15).unwrap();
    store.release(&project(), &sibling.id, owner, 15).unwrap();
    assert!(
        store
            .goals(&project(), false, Some(owner),)
            .unwrap()
            .is_empty()
    );
    let empty = store
        .create_goal(&project(), "empty goal", "briefs/brief.md", actor(1))
        .unwrap();
    store
        .claim(&project(), &empty.root.id, owner, false, 10)
        .unwrap();
    assert_eq!(
        store
            .goals(&project(), false, Some(owner),)
            .unwrap()
            .first()
            .unwrap()
            .root
            .id,
        empty.root.id
    );
}

#[test]
fn creation_is_atomic_and_does_not_claim_or_execute() {
    let mut store = store();
    assert!(
        store
            .create_goal(&project(), "valid", "relative.md", actor(1))
            .is_err()
    );
    assert!(
        store
            .create_goal(&project(), " ", "briefs/brief.md", actor(1))
            .is_err()
    );
    assert!(
        store
            .list(&project(), &crate::Filter::default())
            .unwrap()
            .is_empty()
    );
    store.connection.execute_batch("CREATE TRIGGER reject_goal BEFORE INSERT ON goals BEGIN SELECT RAISE(ABORT, 'fixture rejection'); END;").unwrap();
    assert!(
        store
            .create_goal(&project(), "valid", "briefs/brief.md", actor(1))
            .is_err()
    );
    assert!(
        store
            .list(&project(), &crate::Filter::default())
            .unwrap()
            .is_empty()
    );
    assert!(store.events_since(&project(), 0, 100).unwrap().is_empty());
    store
        .connection
        .execute_batch("DROP TRIGGER reject_goal;")
        .unwrap();
    let goal = store
        .create_goal(&project(), "New goal", "briefs/brief.md", actor(2))
        .unwrap();
    assert_eq!(goal.root.status, Status::Open);
    assert!(goal.root.owner.is_none());
    assert_eq!(goal.root.children, 0);
    assert_eq!(
        store.events(&project(), &goal.root.id, 10).unwrap().len(),
        2
    );
}

#[test]
fn associations_are_optional_scoped_and_reversible() {
    let mut store = store();
    let root = add(&mut store, "goal", None);
    let child = add(&mut store, "slice", Some(&root.id));
    assert!(store.goals(&project(), true, None,).unwrap().is_empty());
    assert!(
        store
            .attach_goal(&project(), &child.id, "briefs/brief.md", actor(2))
            .is_err()
    );
    assert!(
        store
            .attach_goal(&project(), &root.id, "relative.md", actor(2))
            .is_err()
    );
    assert!(
        store
            .attach_goal(
                &Project::try_new("p-00000000000000000000000000000002").unwrap(),
                &root.id,
                "briefs/brief.md",
                actor(2)
            )
            .is_err()
    );
    store
        .attach_goal(&project(), &root.id, "briefs/brief.md", actor(2))
        .unwrap();
    let count = store.events(&project(), &root.id, 100).unwrap().len();
    store
        .attach_goal(&project(), &root.id, "briefs/brief.md", actor(3))
        .unwrap();
    assert_eq!(
        store.events(&project(), &root.id, 100).unwrap().len(),
        count
    );
    assert!(
        store
            .goals(
                &Project::try_new("p-00000000000000000000000000000002").unwrap(),
                true,
                None,
            )
            .unwrap()
            .is_empty()
    );
    assert!(
        store
            .goal_overview(
                &Project::try_new("p-00000000000000000000000000000002").unwrap(),
                &root.id,
                true
            )
            .is_err()
    );
    let other = add(&mut store, "other root", None);
    let reparent = Update {
        parent: Some(Some(other.id)),
        ..Update::default()
    };
    assert!(
        store
            .update(&project(), &root.id, &reparent, actor(3), true)
            .is_err()
    );
    store
        .attach_goal(&project(), &root.id, "briefs/new-brief.md", actor(4))
        .unwrap();
    assert_eq!(
        store
            .goal_overview(&project(), &root.id, true)
            .unwrap()
            .goal
            .brief,
        "briefs/new-brief.md"
    );
    store.detach_goal(&project(), &root.id, actor(5)).unwrap();
    store.detach_goal(&project(), &root.id, actor(6)).unwrap();
    assert!(store.goal_overview(&project(), &root.id, true).is_err());
    assert!(store.get(&project(), &child.id).is_ok());
    store
        .update(&project(), &root.id, &reparent, actor(7), false)
        .unwrap();
}

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "One goal overview scenario covers all states, history, and claim release"
)]
fn overview_counts_leaves_and_does_not_invent_verified_state() {
    let mut store = store();
    let root = add(&mut store, "goal", None);
    store
        .attach_goal(&project(), &root.id, "briefs/brief.md", actor(1))
        .unwrap();
    assert_eq!(
        store
            .goal_overview(&project(), &root.id, true)
            .unwrap()
            .goal
            .progress
            .total,
        0
    );
    let group = add(&mut store, "group", Some(&root.id));
    let done = add(&mut store, "done", Some(&group.id));
    store
        .write_result(
            &project(),
            &done.id,
            &crate::outcomes::Outcome {
                operation: crate::outcomes::Operation::Done,
                result: "verified",
                evidence: &[],
                force: false,
            },
            actor(2),
        )
        .map(|receipt| receipt.task)
        .unwrap();
    let dropped = add(&mut store, "dropped", Some(&group.id));
    store
        .set_status(
            &project(),
            &dropped.id,
            Status::Dropped,
            Some("out of scope"),
            actor(2),
            false,
        )
        .unwrap();
    let held = add(&mut store, "held", Some(&root.id));
    store
        .set_status(
            &project(),
            &held.id,
            Status::Held,
            Some("decision"),
            actor(2),
            false,
        )
        .unwrap();
    let active = add(&mut store, "claimed PR", Some(&root.id));
    store
        .claim(&project(), &active.id, actor(2).name.unwrap(), false, 2)
        .unwrap();
    store
        .write_result(
            &project(),
            &active.id,
            &crate::outcomes::Outcome {
                operation: crate::outcomes::Operation::UpdateResult,
                result: "implementation checked",
                evidence: &[],
                force: false,
            },
            actor(3),
        )
        .map(|receipt| receipt.task)
        .unwrap();
    add(&mut store, "ready", Some(&root.id));
    let overview = store.goal_overview(&project(), &root.id, true).unwrap();
    assert_eq!(overview.tasks.len(), 6);
    let p = overview.goal.progress;
    assert_eq!(
        (p.total, p.open, p.done, p.dropped, p.held),
        (5, 2, 1, 1, 1)
    );
    assert_eq!((p.ready, p.claimed, p.review), (1, 1, 0));
    let quiet = store.goal_overview(&project(), &root.id, false).unwrap();
    assert_eq!(quiet.hidden_closed, 2);
    assert_eq!(quiet.tasks.len(), 4);
    assert_eq!(quiet.goal.progress.done, 1);
    assert_eq!(quiet.goal.progress.dropped, 1);
    assert!(quiet.tasks.iter().all(|task| !task.status.is_closed()));
    store.release_all(actor(4).name.unwrap(), 4).unwrap();
    let released = store
        .goal_overview(&project(), &root.id, true)
        .unwrap()
        .goal
        .progress;
    assert_eq!(
        (released.ready, released.claimed, released.review),
        (2, 0, 0)
    );
    store
        .write_result(
            &project(),
            &root.id,
            &crate::outcomes::Outcome {
                operation: crate::outcomes::Operation::Done,
                result: "accepted",
                evidence: &[],
                force: true,
            },
            actor(21),
        )
        .map(|receipt| receipt.task)
        .unwrap();
    assert!(store.goals(&project(), false, None,).unwrap().is_empty());
    assert_eq!(store.goals(&project(), true, None,).unwrap().len(), 1);
}

#[test]
fn association_changes_respect_live_root_claims() {
    let mut store = store();
    let root = add(&mut store, "empty goal", None);
    store
        .attach_goal(&project(), &root.id, "briefs/brief.md", actor(1))
        .unwrap();
    store
        .claim(
            &project(),
            &root.id,
            Owner::try_new("other").unwrap(),
            false,
            2,
        )
        .unwrap();
    assert!(
        store
            .attach_goal(&project(), &root.id, "briefs/changed.md", actor(3))
            .is_err()
    );
    assert!(store.detach_goal(&project(), &root.id, actor(3)).is_err());
    store
        .release(&project(), &root.id, Owner::try_new("other").unwrap(), 4)
        .unwrap();
    store.detach_goal(&project(), &root.id, actor(5)).unwrap();
}

#[test]
fn goals_list_most_recently_active_first() {
    let mut store = store();
    let older = store
        .create_goal(&project(), "older", "briefs/older.md", actor(1))
        .unwrap();
    let newer = store
        .create_goal(&project(), "newer", "briefs/newer.md", actor(2))
        .unwrap();
    let listed_ids = |store: &mut Store| -> Vec<String> {
        store
            .goals(&project(), false, None)
            .unwrap()
            .into_iter()
            .map(|goal| goal.root.id)
            .collect()
    };
    assert_eq!(
        listed_ids(&mut store),
        [newer.root.id.clone(), older.root.id.clone()]
    );
    add(&mut store, "slice", Some(&older.root.id));
    let slice = store
        .goal_overview(&project(), &older.root.id, false)
        .unwrap()
        .tasks[0]
        .id
        .clone();
    store
        .update(
            &project(),
            &slice,
            &Update {
                body: Some("scope".into()),
                ..Update::default()
            },
            actor(9),
            false,
        )
        .unwrap();
    let goals = store.goals(&project(), false, None).unwrap();
    assert_eq!(goals[0].root.id, older.root.id);
    assert_eq!(goals[0].last_activity, 9);
    assert_eq!(listed_ids(&mut store), [older.root.id, newer.root.id]);
}

#[test]
fn stores_reopen_and_other_schema_versions_fail() {
    let mut first = store();
    let root = add(&mut first, "existing", None);
    first
        .attach_goal(&project(), &root.id, "briefs/brief.md", actor(2))
        .unwrap();
    let mut reopened = Store::initialize(first.connection).unwrap();
    assert_eq!(
        reopened
            .goal_overview(&project(), &root.id, true)
            .unwrap()
            .goal
            .brief,
        "briefs/brief.md"
    );
    assert_eq!(reopened.events(&project(), &root.id, 100).unwrap().len(), 2);
    for version in [2, 3, 99] {
        let other = Connection::open_in_memory().unwrap();
        other.pragma_update(None, "user_version", version).unwrap();
        assert!(matches!(Store::initialize(other), Err(Error::Schema(v)) if v == version));
    }
}
