use super::*;
use crate::{NewTask, Owner, Update};
use rusqlite::Connection;
use std::time::Duration;

fn project() -> Project {
    Project::new("example").unwrap()
}
fn actor(now: i64) -> Actor<'static> {
    Actor {
        name: Some(Owner::new("parent").unwrap()),
        now,
    }
}
fn store() -> Store {
    Store::initialize(Connection::open_in_memory().unwrap()).unwrap()
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
fn creation_is_atomic_and_does_not_claim_or_execute() {
    let mut store = store();
    assert!(
        store
            .create_goal(&project(), "valid", "relative.md", actor(1))
            .is_err()
    );
    assert!(
        store
            .create_goal(&project(), " ", "/brief.md", actor(1))
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
            .create_goal(&project(), "valid", "/brief.md", actor(1))
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
        .create_goal(&project(), "New goal", "/brief.md", actor(2))
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
    assert!(store.goals(&project(), true).unwrap().is_empty());
    assert!(
        store
            .attach_goal(&project(), &child.id, "/brief.md", actor(2))
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
                &Project::new("other").unwrap(),
                &root.id,
                "/brief.md",
                actor(2)
            )
            .is_err()
    );
    store
        .attach_goal(&project(), &root.id, "/brief.md", actor(2))
        .unwrap();
    let count = store.events(&project(), &root.id, 100).unwrap().len();
    store
        .attach_goal(&project(), &root.id, "/brief.md", actor(3))
        .unwrap();
    assert_eq!(
        store.events(&project(), &root.id, 100).unwrap().len(),
        count
    );
    assert!(
        store
            .goals(&Project::new("other").unwrap(), true)
            .unwrap()
            .is_empty()
    );
    assert!(
        store
            .goal_overview(&Project::new("other").unwrap(), &root.id, 3, true)
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
        .attach_goal(&project(), &root.id, "/new-brief.md", actor(4))
        .unwrap();
    assert_eq!(
        store
            .goal_overview(&project(), &root.id, 5, true)
            .unwrap()
            .goal
            .brief,
        "/new-brief.md"
    );
    store.detach_goal(&project(), &root.id, actor(5)).unwrap();
    store.detach_goal(&project(), &root.id, actor(6)).unwrap();
    assert!(store.goal_overview(&project(), &root.id, 6, true).is_err());
    assert!(store.get(&project(), &child.id).is_ok());
    store
        .update(&project(), &root.id, &reparent, actor(7), false)
        .unwrap();
}

#[test]
fn overview_counts_leaves_and_does_not_invent_verified_state() {
    let mut store = store();
    let root = add(&mut store, "goal", None);
    store
        .attach_goal(&project(), &root.id, "/brief.md", actor(1))
        .unwrap();
    assert_eq!(
        store
            .goal_overview(&project(), &root.id, 1, true)
            .unwrap()
            .progress
            .total,
        0
    );
    let group = add(&mut store, "group", Some(&root.id));
    let done = add(&mut store, "done", Some(&group.id));
    store
        .set_status(&project(), &done.id, Status::Done, None, actor(2), false)
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
        .claim(
            &project(),
            &active.id,
            actor(2).name.unwrap(),
            Duration::from_secs(10),
            2,
        )
        .unwrap();
    store
        .update(
            &project(),
            &active.id,
            &Update {
                pr: Some("https://github.com/example/project/pull/1".into()),
                ..Update::default()
            },
            actor(3),
            false,
        )
        .unwrap();
    add(&mut store, "ready", Some(&root.id));
    let overview = store.goal_overview(&project(), &root.id, 4, true).unwrap();
    assert_eq!(overview.tasks.len(), 6);
    let p = overview.progress;
    assert_eq!(
        (p.total, p.open, p.done, p.dropped, p.held),
        (5, 2, 1, 1, 1)
    );
    assert_eq!((p.ready, p.claimed, p.with_pr), (1, 1, 1));
    let quiet = store.goal_overview(&project(), &root.id, 4, false).unwrap();
    assert_eq!(quiet.hidden_closed, 2);
    assert_eq!(quiet.tasks.len(), 4);
    assert_eq!(quiet.progress.done, 1);
    assert_eq!(quiet.progress.dropped, 1);
    assert!(quiet.tasks.iter().all(|task| !task.status.is_closed()));
    let expired = store
        .goal_overview(&project(), &root.id, 20, true)
        .unwrap()
        .progress;
    assert_eq!((expired.ready, expired.claimed, expired.with_pr), (1, 0, 1));
    store
        .set_status(&project(), &root.id, Status::Done, None, actor(21), true)
        .unwrap();
    assert!(store.goals(&project(), false).unwrap().is_empty());
    assert_eq!(store.goals(&project(), true).unwrap().len(), 1);
}

#[test]
fn association_changes_respect_live_root_claims() {
    let mut store = store();
    let root = add(&mut store, "empty goal", None);
    store
        .attach_goal(&project(), &root.id, "/brief.md", actor(1))
        .unwrap();
    store
        .claim(
            &project(),
            &root.id,
            Owner::new("other").unwrap(),
            Duration::from_secs(10),
            2,
        )
        .unwrap();
    assert!(
        store
            .attach_goal(&project(), &root.id, "/changed.md", actor(3))
            .is_err()
    );
    assert!(store.detach_goal(&project(), &root.id, actor(3)).is_err());
    store.detach_goal(&project(), &root.id, actor(20)).unwrap();
}

#[test]
fn old_schema_migrations_preserve_tasks_and_reopen_goals() {
    for version in [1, 2, 3] {
        let mut old = store();
        let root = add(&mut old, "existing", None);
        if version < 3 {
            old.connection.execute_batch("DROP TABLE goals;").unwrap();
        }
        if version == 1 {
            old.connection
                .execute_batch("ALTER TABLE tasks DROP COLUMN branch;")
                .unwrap();
        }
        old.connection
            .pragma_update(None, "user_version", version)
            .unwrap();
        let mut upgraded = Store::initialize(old.connection).unwrap();
        assert_eq!(
            upgraded.get(&project(), &root.id).unwrap().title,
            "existing"
        );
        upgraded
            .attach_goal(&project(), &root.id, "/brief.md", actor(2))
            .unwrap();
        let mut reopened = Store::initialize(upgraded.connection).unwrap();
        assert_eq!(
            reopened
                .goal_overview(&project(), &root.id, 3, true)
                .unwrap()
                .goal
                .brief,
            "/brief.md"
        );
        assert_eq!(reopened.events(&project(), &root.id, 100).unwrap().len(), 2);
    }
    let unknown = Connection::open_in_memory().unwrap();
    unknown.pragma_update(None, "user_version", 99).unwrap();
    assert!(matches!(Store::initialize(unknown), Err(Error::Schema(99))));
}
