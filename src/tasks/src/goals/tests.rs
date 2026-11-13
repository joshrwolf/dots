use super::{Actor, Error, Project, Status, Store, Task};
use crate::GoalRef;
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
fn coordinate(store: &mut Store, root: &str, now: i64) {
    store
        .coordinate(
            &project(),
            root,
            actor(now).name.unwrap(),
            false,
            false,
            now,
        )
        .unwrap();
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
    let owner = actor(1).name.unwrap();
    coordinate(&mut store, &root.root.id, 1);
    let group = add(&mut store, "group", Some(&root.root.id));
    let leaf = add(&mut store, "leaf", Some(&group.id));
    let sibling = add(&mut store, "sibling", Some(&group.id));
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
    store.release(&project(), &root.root.id, owner, 15).unwrap();
    assert!(
        store
            .goals(&project(), false, Some(owner),)
            .unwrap()
            .is_empty()
    );
    let empty = store
        .create_goal(&project(), "empty goal", "briefs/brief.md", actor(1))
        .unwrap();
    assert!(matches!(
        store.claim(&project(), &empty.root.id, owner, false, 10),
        Err(Error::GoalRoot(_))
    ));
    coordinate(&mut store, &empty.root.id, 10);
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
    assert!(
        store
            .history(&project(), None, 0, 100)
            .unwrap()
            .events
            .is_empty()
    );
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
    coordinate(&mut store, &root.id, 2);
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
    coordinate(&mut store, &root.id, 1);
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
    coordinate(&mut store, &root.id, 20);
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
        .coordinate(
            &project(),
            &root.id,
            Owner::try_new("other").unwrap(),
            false,
            false,
            2,
        )
        .unwrap();
    assert!(
        store
            .attach_goal(&project(), &root.id, "briefs/changed.md", actor(3))
            .is_err()
    );
    assert!(matches!(
        store.detach_goal(&project(), &root.id, actor(3)),
        Err(Error::NotCoordinator { .. })
    ));
    store
        .release(&project(), &root.id, Owner::try_new("other").unwrap(), 4)
        .unwrap();
    assert!(matches!(
        store.detach_goal(&project(), &root.id, actor(5)),
        Err(Error::Uncoordinated { .. })
    ));
    coordinate(&mut store, &root.id, 5);
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
    coordinate(&mut store, &older.root.id, 1);
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
    for version in [2, 3, 8, 9, 10, 99] {
        let other = Connection::open_in_memory().unwrap();
        other.pragma_update(None, "user_version", version).unwrap();
        assert!(matches!(Store::initialize(other), Err(Error::Schema(v)) if v == version));
    }
}

#[test]
fn coordinated_goals_are_exactly_the_open_roots_an_owner_claims() {
    let mut store = store();
    let other_project = Project::try_new("p-00000000000000000000000000000002").unwrap();
    let first = Owner::try_new("first").unwrap();
    let second = Owner::try_new("second").unwrap();
    let as_ = |name, now| Actor {
        name: Some(name),
        now,
    };
    let goal = |store: &mut Store, project: &Project, owner| {
        let root = store
            .create_goal(project, "goal", "briefs/brief.md", as_(owner, 1))
            .unwrap()
            .root
            .id;
        store
            .coordinate(project, &root, owner, false, false, 2)
            .unwrap();
        root
    };
    let here = goal(&mut store, &project(), first);
    let there = goal(&mut store, &other_project, first);
    let theirs = goal(&mut store, &project(), second);
    let refs = |pairs: &[(&Project, &str)]| {
        pairs
            .iter()
            .map(|(project, goal)| GoalRef {
                project: (*project).clone(),
                goal: (*goal).to_owned(),
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(
        store.coordinated_goals(first).unwrap(),
        refs(&[(&project(), &here), (&other_project, &there)])
    );
    assert_eq!(
        store.coordinated_goals(second).unwrap(),
        refs(&[(&project(), &theirs)])
    );
    let nobody = Owner::try_new("nobody").unwrap();
    assert_eq!(store.coordinated_goals(nobody).unwrap(), []);

    // A claim below a root is work, not coordination.
    let leaf = store
        .add(
            &project(),
            &NewTask {
                title: "leaf".into(),
                parent: Some(theirs.clone()),
                ..NewTask::default()
            },
            as_(second, 3),
        )
        .unwrap()
        .id;
    store.release(&project(), &theirs, second, 4).unwrap();
    store
        .coordinate(&project(), &theirs, first, false, false, 5)
        .unwrap();
    store.claim(&project(), &leaf, first, false, 6).unwrap();
    store.release(&project(), &theirs, first, 7).unwrap();
    assert_eq!(store.coordinated_goals(first).unwrap().len(), 2);

    // Takeover moves responsibility; releasing, pausing and closing end it.
    store
        .coordinate(&project(), &here, second, true, false, 8)
        .unwrap();
    assert_eq!(
        store.coordinated_goals(second).unwrap(),
        refs(&[(&project(), &here)])
    );
    store.release(&project(), &here, second, 9).unwrap();
    store
        .set_status(
            &other_project,
            &there,
            Status::Held,
            Some("pause"),
            as_(first, 10),
            false,
        )
        .unwrap();
    assert_eq!(store.coordinated_goals(first).unwrap(), []);
    assert_eq!(store.coordinated_goals(second).unwrap(), []);
    store
        .coordinate(&other_project, &there, first, false, true, 11)
        .unwrap();
    assert_eq!(
        store.coordinated_goals(first).unwrap(),
        refs(&[(&other_project, &there)])
    );
    store
        .set_status(
            &other_project,
            &there,
            Status::Dropped,
            Some("stop"),
            as_(first, 12),
            false,
        )
        .unwrap();
    assert_eq!(store.coordinated_goals(first).unwrap(), []);
}

/// Columns of every table and the names of every index and trigger.
fn shape(connection: &Connection) -> Vec<String> {
    let mut statement = connection
        .prepare(
            "SELECT type || ' ' || name FROM sqlite_master
             WHERE name NOT LIKE 'sqlite_%' ORDER BY type, name",
        )
        .unwrap();
    let objects: Vec<String> = statement
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    let mut shape = Vec::new();
    for object in objects {
        if let Some(table) = object.strip_prefix("table ") {
            let mut columns = connection
                .prepare(&format!("SELECT name, type, \"notnull\", dflt_value, pk FROM pragma_table_info('{table}')"))
                .unwrap();
            for column in columns
                .query_map([], |row| {
                    Ok(format!(
                        "{table}.{} {} {} {:?} {}",
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, Option<String>>(3)?,
                        row.get::<_, i64>(4)?
                    ))
                })
                .unwrap()
            {
                shape.push(column.unwrap());
            }
        }
        shape.push(object);
    }
    shape
}

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "one fixture covers every way a mandate reaches its brief"
)]
fn schema_11_moves_mandates_into_briefs_and_drops_attention() {
    let root = std::env::temp_dir().join(format!("dots-goals-migrate-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir(&root).unwrap();
    let root = std::fs::canonicalize(root).unwrap();
    let path = root.join("workspace.sqlite3");
    let mut store = Store::create(&path).unwrap();
    let project = store.register_project("fixture").unwrap();
    let mut goal = |title: &str, brief: &str| {
        store
            .create_goal(&project, title, brief, actor(1))
            .unwrap()
            .root
            .id
    };
    let written = goal("written", "briefs/written.md");
    let missing = goal("missing", "briefs/missing.md");
    let carried = goal("carried", "briefs/carried.md");
    let foreign = goal("foreign", "briefs/foreign.md");
    drop(store);
    let briefs = root
        .join("workspaces")
        .join(project.as_str())
        .join("briefs");
    std::fs::create_dir_all(&briefs).unwrap();
    std::fs::write(briefs.join("written.md"), "# Written\n\nOutcome.").unwrap();
    std::fs::write(briefs.join("carried.md"), "# Carried\n\nShip to main.\n").unwrap();
    std::fs::write(briefs.join("foreign.md"), "# Foreign\n").unwrap();

    let connection = Connection::open(&path).unwrap();
    connection
        .execute_batch(
            "CREATE TABLE requests(
                id TEXT PRIMARY KEY, project TEXT NOT NULL REFERENCES projects(id), goal TEXT NOT NULL REFERENCES tasks(id),
                key TEXT NOT NULL, kind TEXT NOT NULL, title TEXT NOT NULL, question TEXT NOT NULL, context TEXT, recommendation TEXT,
                state TEXT NOT NULL, created_at INTEGER NOT NULL, created_by TEXT, supersedes TEXT UNIQUE REFERENCES requests(id),
                answer_choice TEXT, answer_text TEXT, answer_source TEXT, answer_provenance TEXT, answer_interpretation TEXT,
                answered_at INTEGER, answered_by TEXT,
                closure TEXT, closed_at INTEGER, closed_by TEXT, close_reason TEXT);
             CREATE UNIQUE INDEX requests_key ON requests(goal,key) WHERE closure IS NOT 'superseded';
             CREATE INDEX requests_project ON requests(project,state);
             CREATE TABLE request_choices(request TEXT NOT NULL REFERENCES requests(id), position INTEGER NOT NULL, id TEXT NOT NULL, label TEXT NOT NULL, detail TEXT, PRIMARY KEY(request,position), UNIQUE(request,id));
             CREATE TABLE request_tasks(request TEXT NOT NULL REFERENCES requests(id), task TEXT NOT NULL REFERENCES tasks(id), PRIMARY KEY(request,task));
             CREATE INDEX request_tasks_task ON request_tasks(task);
             CREATE TABLE decisions(
                id TEXT PRIMARY KEY, project TEXT NOT NULL REFERENCES projects(id), goal TEXT NOT NULL REFERENCES tasks(id),
                kind TEXT NOT NULL, decision TEXT NOT NULL, human_text TEXT, human_source TEXT, human_provenance TEXT,
                request TEXT UNIQUE REFERENCES requests(id), at INTEGER NOT NULL, actor TEXT);
             CREATE INDEX decisions_goal ON decisions(goal,kind);
             ALTER TABLE sharing ADD COLUMN answers_dirty INTEGER NOT NULL DEFAULT 0;
             CREATE TABLE sent_answers(
                request TEXT PRIMARY KEY REFERENCES requests(id), goal TEXT NOT NULL, answer TEXT NOT NULL,
                at INTEGER NOT NULL, exported INTEGER);
             INSERT INTO sharing(singleton, machine, enabled, changed_at) VALUES(0, 'm-here', 0, 1);
             PRAGMA user_version=11;",
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO shared_trees(root, project, shared_project, home) VALUES(?1, ?2, ?2, 'm-there')",
            rusqlite::params![foreign, project.as_str()],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO requests(id, project, goal, key, kind, title, question, state, created_at)
             VALUES('r-1', ?1, ?2, 'scope', 'decision', 'Scope', 'Which?', 'open', 1)",
            rusqlite::params![project.as_str(), written],
        )
        .unwrap();
    let mut insert = connection
        .prepare(
            "INSERT INTO decisions(id, project, goal, kind, decision, human_text, human_source, human_provenance, at)
             VALUES(?1, ?2, ?3, ?4, ?5, 'yes', 'chat', 'session', ?6)",
        )
        .unwrap();
    for (id, goal, kind, decision, at) in [
        ("d-2", &written, "mandate", "Later mandate.\n", 5),
        (
            "d-1",
            &written,
            "mandate",
            "Commit locally; do not push.",
            2,
        ),
        ("d-3", &written, "decision", "Not a mandate.", 3),
        ("d-4", &missing, "mandate", "Work in a worktree.", 2),
        ("d-5", &carried, "mandate", "Ship to main.", 2),
        ("d-6", &foreign, "mandate", "Homed elsewhere.", 2),
    ] {
        insert
            .execute(rusqlite::params![
                id,
                project.as_str(),
                goal,
                kind,
                decision,
                at
            ])
            .unwrap();
    }
    drop(insert);
    drop(connection);

    let migrated = Store::open(&path).unwrap();
    let read = |name: &str| std::fs::read_to_string(briefs.join(name)).unwrap();
    assert_eq!(
        read("written.md"),
        "# Written\n\nOutcome.\n\n## Mandate\n\nCommit locally; do not push.\n\nLater mandate.\n"
    );
    assert_eq!(read("missing.md"), "## Mandate\n\nWork in a worktree.\n");
    assert_eq!(read("carried.md"), "# Carried\n\nShip to main.\n");
    assert_eq!(read("foreign.md"), "# Foreign\n");
    let version: i64 = migrated
        .connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .unwrap();
    assert_eq!(version, 12);
    let fresh = Store::create(&root.join("fresh.sqlite3")).unwrap();
    assert_eq!(shape(&migrated.connection), shape(&fresh.connection));
    drop(migrated);
    Store::open(&path).unwrap();
    assert_eq!(read("missing.md"), "## Mandate\n\nWork in a worktree.\n");
    std::fs::remove_dir_all(&root).unwrap();
}
