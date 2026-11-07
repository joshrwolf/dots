use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use super::{BATCH, Follower};
use crate::{Actor, EventKind, NewTask, Owner, Project, Store, Update};

/// No events.
const NONE: [i64; 0] = [];

struct Fixture {
    root: PathBuf,
    project: Project,
    other: Project,
}

impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "dots-follow-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        let mut store = Store::create(&root.join("workspace.sqlite3")).unwrap();
        let project = store.register_project("fixture").unwrap();
        let other = store.register_project("other").unwrap();
        Self {
            root,
            project,
            other,
        }
    }

    fn path(&self) -> PathBuf {
        self.root.join("workspace.sqlite3")
    }

    /// A separate writer, as another process would be.
    fn writer(&self) -> Store {
        Store::open(&self.path()).unwrap()
    }

    fn follow(&self, since: i64) -> Follower {
        Follower::open(&self.path(), self.project.clone(), None, since).unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}

fn writer_actor() -> Actor<'static> {
    Actor {
        name: Some(Owner::try_new("writer").unwrap()),
        now: 1,
    }
}

fn add(store: &mut Store, project: &Project, title: &str) -> i64 {
    add_task(store, project, title, None).1
}

/// A new task's ID and the seq of its `created` event.
fn add_task(
    store: &mut Store,
    project: &Project,
    title: &str,
    parent: Option<&str>,
) -> (String, i64) {
    let task = store
        .add(
            project,
            &NewTask {
                title: title.into(),
                parent: parent.map(str::to_owned),
                ..NewTask::default()
            },
            writer_actor(),
        )
        .unwrap();
    (task.id, task.last_event.unwrap().seq)
}

/// Move a task; the seq of its `updated` event.
fn reparent(store: &mut Store, project: &Project, id: &str, parent: Option<&str>) -> i64 {
    store
        .update(
            project,
            id,
            &Update {
                parent: Some(parent.map(str::to_owned)),
                ..Update::default()
            },
            writer_actor(),
            false,
        )
        .unwrap()
        .last_event
        .unwrap()
        .seq
}

fn seqs(follower: &mut Follower) -> Vec<i64> {
    follower
        .poll()
        .unwrap()
        .into_iter()
        .map(|event| event.seq)
        .collect()
}

/// Poll until a poll returns nothing, as a follower drains its backlog.
fn drain(follower: &mut Follower) -> Vec<i64> {
    let mut all = Vec::new();
    loop {
        let batch = seqs(follower);
        if batch.is_empty() {
            return all;
        }
        all.extend(batch);
    }
}

#[test]
fn backlog_then_commits_from_another_connection_arrive_once_in_order() {
    let fixture = Fixture::new();
    let mut writer = fixture.writer();
    let first = add(&mut writer, &fixture.project, "first");
    add(&mut writer, &fixture.other, "elsewhere");
    let second = add(&mut writer, &fixture.project, "second");

    let mut follower = fixture.follow(0);
    assert_eq!(drain(&mut follower), [first, second]);
    assert_eq!(seqs(&mut follower), NONE, "no commit, no events");

    let third = add(&mut writer, &fixture.project, "third");
    add(&mut writer, &fixture.other, "elsewhere again");
    let fourth = add(&mut writer, &fixture.project, "fourth");
    assert_eq!(seqs(&mut follower), [third, fourth]);
    assert_eq!(seqs(&mut follower), NONE);

    // Only the other project changes: nothing to report, and nothing missed.
    add(&mut writer, &fixture.other, "unrelated");
    assert_eq!(seqs(&mut follower), NONE);
    let fifth = add(&mut writer, &fixture.project, "fifth");
    assert_eq!(seqs(&mut follower), [fifth]);
}

#[test]
fn a_long_backlog_drains_in_bounded_batches_without_gaps() {
    let fixture = Fixture::new();
    let mut writer = fixture.writer();
    let task = add(&mut writer, &fixture.project, "busy");
    let id: String = writer
        .connection
        .query_row("SELECT task FROM events WHERE seq=?1", [task], |row| {
            row.get(0)
        })
        .unwrap();
    let extra = i64::try_from(2 * BATCH + 7).unwrap();
    writer
        .connection
        .execute(
            "WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i+1 FROM n WHERE i<?2)
             INSERT INTO events(task,at,kind,uid) SELECT ?1, i, 'updated', lower(hex(randomblob(16))) FROM n",
            rusqlite::params![id, extra],
        )
        .unwrap();

    let mut follower = fixture.follow(0);
    let first = seqs(&mut follower);
    assert_eq!(first.len(), BATCH);
    let mut all = first;
    all.extend(drain(&mut follower));
    assert_eq!(all, (task..=task + extra).collect::<Vec<_>>());
}

#[test]
fn resuming_from_the_last_seen_seq_has_no_gap_or_duplicate() {
    let fixture = Fixture::new();
    let mut writer = fixture.writer();
    let first = add(&mut writer, &fixture.project, "first");
    let mut follower = fixture.follow(0);
    assert_eq!(drain(&mut follower), [first]);
    drop(follower);

    let second = add(&mut writer, &fixture.project, "second");
    add(&mut writer, &fixture.other, "elsewhere");
    let third = add(&mut writer, &fixture.project, "third");
    let mut resumed = fixture.follow(first);
    assert_eq!(drain(&mut resumed), [second, third]);
}

#[test]
fn checkpoints_neither_hide_nor_repeat_commits() {
    let fixture = Fixture::new();
    let mut writer = fixture.writer();
    let mut follower = fixture.follow(0);
    assert_eq!(seqs(&mut follower), NONE);
    for (round, mode) in ["PASSIVE", "RESTART", "TRUNCATE"].into_iter().enumerate() {
        let before = add(&mut writer, &fixture.project, &format!("before {round}"));
        writer
            .connection
            .pragma_update(None, "wal_checkpoint", mode)
            .unwrap();
        let after = add(&mut writer, &fixture.project, &format!("after {round}"));
        writer
            .connection
            .pragma_update(None, "wal_checkpoint", mode)
            .unwrap();
        assert_eq!(seqs(&mut follower), [before, after], "{mode}");
        assert_eq!(seqs(&mut follower), NONE, "{mode}");
    }
}

#[test]
fn a_replaced_catalog_is_followed_from_the_cursor() {
    let fixture = Fixture::new();
    let mut writer = fixture.writer();
    let first = add(&mut writer, &fixture.project, "first");
    let mut follower = fixture.follow(0);
    assert_eq!(drain(&mut follower), [first]);

    // An offline restore: a consistent copy with history the follower has
    // not seen replaces the catalog and its WAL files.
    let staged = fixture.root.join("restored.sqlite3");
    writer
        .connection
        .execute("VACUUM INTO ?1", [staged.to_str().unwrap()])
        .unwrap();
    let mut restored = Store::open(&staged).unwrap();
    let second = add(&mut restored, &fixture.project, "second");
    restored
        .connection
        .pragma_update(None, "journal_mode", "DELETE")
        .unwrap();
    drop(restored);
    drop(writer);
    for suffix in ["", "-wal", "-shm"] {
        let mut name = fixture.path().into_os_string();
        name.push(suffix);
        fs::remove_file(name).unwrap();
    }
    assert_eq!(seqs(&mut follower), NONE, "waits while absent");
    fs::rename(&staged, fixture.path()).unwrap();

    assert_eq!(seqs(&mut follower), [second]);
    let mut writer = fixture.writer();
    let third = add(&mut writer, &fixture.project, "third");
    assert_eq!(seqs(&mut follower), [third]);
}

#[test]
fn an_absent_catalog_is_waited_for() {
    let fixture = Fixture::new();
    let mut writer = fixture.writer();
    let first = add(&mut writer, &fixture.project, "first");
    let mut follower = fixture.follow(0);
    assert_eq!(drain(&mut follower), [first]);
    let parked = fixture.root.join("parked.sqlite3");
    fs::rename(fixture.path(), &parked).unwrap();
    assert_eq!(seqs(&mut follower), NONE);
    fs::rename(&parked, fixture.path()).unwrap();
    let second = add(&mut writer, &fixture.project, "second");
    assert_eq!(seqs(&mut follower), [second]);
}

#[test]
fn a_subtree_follower_judges_each_commit_against_the_current_tree() {
    let fixture = Fixture::new();
    let mut writer = fixture.writer();
    let project = &fixture.project;
    let (root, root_created) = add_task(&mut writer, project, "root", None);
    let (child, child_created) = add_task(&mut writer, project, "child", Some(&root));
    let (stray, _) = add_task(&mut writer, project, "stray", None);
    let mut follower =
        Follower::open(&fixture.path(), project.clone(), Some(root.clone()), 0).unwrap();
    assert_eq!(drain(&mut follower), [root_created, child_created]);

    // Created below a descendant while following.
    let (late, late_created) = add_task(&mut writer, project, "late", Some(&child));
    assert_eq!(seqs(&mut follower), [late_created]);
    // Moved in by another process: no earlier event repeats, later ones count.
    let moved_in = reparent(&mut writer, project, &stray, Some(&late));
    assert_eq!(seqs(&mut follower), [moved_in]);
    let (_, below_stray) = add_task(&mut writer, project, "below stray", Some(&stray));
    assert_eq!(seqs(&mut follower), [below_stray]);
    // Moved out: its move and everything after it no longer count.
    reparent(&mut writer, project, &child, None);
    add_task(&mut writer, project, "below child", Some(&child));
    let (_, direct) = add_task(&mut writer, project, "direct", Some(&root));
    assert_eq!(seqs(&mut follower), [direct]);
    assert_eq!(seqs(&mut follower), NONE);
}

#[test]
fn a_subtree_follower_resumes_without_gap_or_duplicate() {
    let fixture = Fixture::new();
    let mut writer = fixture.writer();
    let project = &fixture.project;
    let (root, root_created) = add_task(&mut writer, project, "root", None);
    let mut follower =
        Follower::open(&fixture.path(), project.clone(), Some(root.clone()), 0).unwrap();
    assert_eq!(drain(&mut follower), [root_created]);
    drop(follower);

    let (_, first) = add_task(&mut writer, project, "first", Some(&root));
    add(&mut writer, project, "elsewhere");
    let (_, second) = add_task(&mut writer, project, "second", Some(&root));
    let mut resumed =
        Follower::open(&fixture.path(), project.clone(), Some(root), root_created).unwrap();
    assert_eq!(drain(&mut resumed), [first, second]);
}

#[test]
fn a_subtree_follower_of_an_unknown_task_fails_its_first_poll() {
    let fixture = Fixture::new();
    let mut follower = Follower::open(
        &fixture.path(),
        fixture.project.clone(),
        Some("t-00".into()),
        0,
    )
    .unwrap();
    assert!(matches!(
        follower.poll(),
        Err(crate::Error::NotFound(id)) if id == "t-00"
    ));
}

#[test]
fn opening_a_missing_catalog_fails() {
    let fixture = Fixture::new();
    let missing = fixture.root.join("missing.sqlite3");
    assert!(matches!(
        Follower::open(&missing, fixture.project.clone(), None, 0),
        Err(crate::Error::CatalogPath { .. })
    ));
}

#[test]
fn event_kind_names_are_distinct() {
    let mut names: Vec<_> = EventKind::ALL.iter().map(|kind| kind.as_str()).collect();
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), EventKind::ALL.len());
}
