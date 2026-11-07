use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use dots_substrate::memory::MemoryBackend;
use dots_substrate::{Config, Flush, Pass, Substrate, Trigger};
use rusqlite::Connection;

use super::{Binding, Context, SharedProject, pass, take};
use crate::attention::{
    AnswerContent, Choice, DecisionKind, Incorporation, RequestContent, RequestKind, Source,
};
use crate::outputs::{PublishMode, PublishOutput};
use crate::store::{Error, RequestState, SCHEMA_9};
use crate::{Actor, NewTask, Owner, Project, Status, Store, Update};

static NEXT: AtomicU64 = AtomicU64::new(0);
const SHARED: &str = "p-5000000000000000000000000000000a";

/// Machines sharing one in-memory remote, each with its own catalog,
/// documents and substrate state under a scratch directory.
struct Lab {
    root: PathBuf,
    backend: MemoryBackend,
}

impl Lab {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "dots-share-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        Self {
            root: fs::canonicalize(root).unwrap(),
            backend: MemoryBackend::new(),
        }
    }

    fn machine(&self, name: &str) -> Machine {
        let home = self.root.join(name);
        fs::create_dir(&home).unwrap();
        let mut store = Store::create(&home.join("workspace.sqlite3")).unwrap();
        let project = store.register_project("fixture").unwrap();
        let substrate = Substrate::open(
            &home.join("substrate"),
            Box::new(self.backend.clone()),
            Config::default(),
        )
        .unwrap();
        let bindings = vec![Binding {
            local: project.clone(),
            shared: SharedProject::try_new(SHARED).unwrap(),
        }];
        Machine {
            home,
            store,
            substrate,
            bindings,
            project,
            clock: 1_000,
        }
    }
}

impl Drop for Lab {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

struct Machine {
    home: PathBuf,
    store: Store,
    substrate: Substrate,
    bindings: Vec<Binding>,
    project: Project,
    clock: u64,
}

fn actor(now: i64) -> Actor<'static> {
    Actor {
        name: Some(Owner::try_new("agent").unwrap()),
        now,
    }
}

impl Machine {
    /// Advance this machine's clock far enough to refill its push budget.
    fn tick(&mut self) -> SystemTime {
        self.clock += 120;
        UNIX_EPOCH + Duration::from_secs(self.clock)
    }

    fn enable(&mut self) {
        let machine = self.substrate.machine().clone();
        let bindings = self.bindings.clone();
        self.store.enable_sharing(&machine, &bindings, 1).unwrap();
    }

    fn sync(&mut self) -> Option<(super::Exported, super::Imported)> {
        let now = self.tick();
        let context = Context {
            substrate: &self.substrate,
            bindings: &self.bindings,
        };
        let (report, shared) = pass(&mut self.store, context, Trigger::Explicit, now).unwrap();
        assert!(matches!(report, Pass::Done(_)), "{report:?}");
        shared.map(|(exported, imported)| {
            assert!(imported.errors.is_empty(), "{:?}", imported.errors);
            (exported, imported)
        })
    }

    fn take(&mut self, root: &str) -> Result<super::Taken, Error> {
        let now = self.tick();
        let context = Context {
            substrate: &self.substrate,
            bindings: &self.bindings,
        };
        take(&mut self.store, context, root, now)
    }

    fn export(&mut self) {
        let now = self.tick();
        let context = Context {
            substrate: &self.substrate,
            bindings: &self.bindings,
        };
        super::export(&mut self.store, context, now).unwrap();
    }

    fn workspace(&self) -> PathBuf {
        self.home.join("workspaces").join(self.project.as_str())
    }

    fn goal(&mut self, brief: &str) -> String {
        let workspace = self.workspace();
        fs::create_dir_all(workspace.join("briefs")).unwrap();
        fs::write(workspace.join("briefs/plan.md"), brief).unwrap();
        let goal = self
            .store
            .create_goal(&self.project, "plan", "briefs/plan.md", actor(10))
            .unwrap();
        self.coordinate(&goal.root.id);
        goal.root.id
    }

    fn coordinate(&mut self, root: &str) {
        let owner = Owner::try_new("agent").unwrap();
        self.store
            .coordinate(&self.project, root, owner, false, false, 10)
            .unwrap();
    }

    fn add(&mut self, title: &str, parent: Option<&str>) -> String {
        self.store
            .add(
                &self.project,
                &NewTask {
                    title: title.into(),
                    parent: parent.map(str::to_owned),
                    ..NewTask::default()
                },
                actor(11),
            )
            .unwrap()
            .id
    }

    fn retitle(&mut self, id: &str, title: &str) -> Result<crate::Task, Error> {
        self.store.update(
            &self.project,
            id,
            &Update {
                title: Some(title.into()),
                ..Update::default()
            },
            actor(12),
            false,
        )
    }
}

fn flushed(machine: &mut Machine) -> usize {
    let now = machine.tick();
    let Pass::Done(report) = machine.substrate.sync(Trigger::Explicit, now).unwrap() else {
        panic!("pass did not run");
    };
    match report.flush {
        Flush::Sent(sent) => sent.applied.len(),
        _ => 0,
    }
}

#[test]
fn a_home_tree_arrives_read_only_with_identical_brief_and_outputs() {
    let lab = Lab::new();
    let (mut a, mut b) = (lab.machine("a"), lab.machine("b"));
    a.enable();
    b.enable();
    let brief = "# Plan\n\nShip the thing.\n";
    let root = a.goal(brief);
    let child = a.add("child", Some(&root));
    let draft = a.home.join("draft.md");
    let report = "report bytes\u{2014}exact\n";
    fs::write(&draft, report).unwrap();
    a.store
        .publish_output(
            &a.project,
            &PublishOutput {
                slug: "report",
                filename: "report.md",
                media_type: "text/markdown",
                source: &draft,
                mode: PublishMode::Create,
                description: "report",
                provenance: "neutral fixture",
                task: Some(&child),
            },
            actor(13),
        )
        .unwrap();
    let (exported, _) = a.sync().unwrap();
    assert_eq!(exported.trees, vec![root.clone()]);
    let (_, imported) = b.sync().unwrap();
    assert_eq!(imported.trees, vec![root.clone()]);

    let task = b.store.get(&b.project, &child).unwrap();
    assert_eq!(task.title, "child");
    let foreign = task.foreign.expect("imported task is foreign");
    assert_eq!(foreign.root, root);
    assert_eq!(foreign.home, a.substrate.machine().as_str());
    let goal = b.store.goal_overview(&b.project, &root, false).unwrap();
    assert_eq!(goal.goal.brief, "briefs/plan.md");
    let copy = super::imported_workspace(&b.workspace(), &root).join("briefs/plan.md");
    assert_eq!(fs::read(copy).unwrap(), brief.as_bytes());
    let digest = dots_documents::text::bytes_revision(report.as_bytes());
    let output = b
        .store
        .read_output(&b.project, "report", &digest, 1, 100)
        .unwrap();
    assert_eq!(fs::read(&output.path).unwrap(), report.as_bytes());
    let history = b.store.events(&b.project, &child, 50).unwrap();
    let mut a_history = a.store.events(&a.project, &child, 50).unwrap();
    a_history.retain(|event| event.kind != "claimed" && event.kind != "released");
    let uids = |events: &[crate::Event]| events.iter().map(|e| e.uid.clone()).collect::<Vec<_>>();
    assert_eq!(uids(&history), uids(&a_history));

    let refused = b.retitle(&child, "changed").unwrap_err();
    let Error::Foreign { root: named, .. } = &refused else {
        panic!("expected a foreign refusal, got {refused:?}");
    };
    assert_eq!(named, &root);
    assert!(
        refused
            .to_string()
            .contains(&format!("dots task share take {root}"))
    );
    let owner = Owner::try_new("agent").unwrap();
    assert!(matches!(
        b.store.claim(&b.project, &child, owner, false, 20),
        Err(Error::Foreign { .. })
    ));
    assert!(matches!(
        b.store
            .coordinate(&b.project, &root, owner, false, false, 20),
        Err(Error::Foreign { .. })
    ));
    assert!(b.store.ready(&b.project, None).unwrap().is_empty());

    // A later change replicates as a small update.
    a.retitle(&child, "renamed").unwrap();
    a.sync().unwrap();
    b.sync().unwrap();
    assert_eq!(b.store.get(&b.project, &child).unwrap().title, "renamed");
}

#[test]
fn takeover_has_one_winner_and_moves_the_home() {
    let lab = Lab::new();
    let (mut a, mut b, mut c) = (lab.machine("a"), lab.machine("b"), lab.machine("c"));
    for machine in [&mut a, &mut b, &mut c] {
        machine.enable();
    }
    let root = a.goal("brief\n");
    let child = a.add("child", Some(&root));
    a.sync();
    b.sync();
    c.sync();

    let won = b.take(&root).unwrap();
    assert_eq!(won.previous_home, a.substrate.machine().as_str());
    let lost = c.take(&root).unwrap_err();
    assert!(
        lost.to_string()
            .contains("changed since this machine last saw it"),
        "{lost}"
    );
    assert!(
        lost.to_string().contains(b.substrate.machine().as_str()),
        "{lost}"
    );

    // The winner writes; the old home sees the tree read-only.
    assert_eq!(
        fs::read(b.workspace().join("briefs/plan.md")).unwrap(),
        b"brief\n"
    );
    b.coordinate(&root);
    b.retitle(&child, "by b").unwrap();
    b.sync();
    a.sync();
    let seen = a.store.get(&a.project, &child).unwrap();
    assert_eq!(seen.title, "by b");
    assert!(seen.foreign.is_some());
    assert!(matches!(
        a.retitle(&child, "by a"),
        Err(Error::Foreign { .. })
    ));

    // B wrote again after the refusal, so the rerun refuses once more; the
    // version that refusal fetched is then taken.
    assert!(c.take(&root).is_err());
    c.take(&root).unwrap();
    c.coordinate(&root);
    c.retitle(&child, "by c").unwrap();
    c.sync();
    b.sync();
    assert_eq!(b.store.get(&b.project, &child).unwrap().title, "by c");
    assert!(matches!(b.retitle(&child, "x"), Err(Error::Foreign { .. })));
}

#[test]
fn a_burst_of_writes_flushes_as_one_push() {
    let lab = Lab::new();
    let (mut a, mut b) = (lab.machine("a"), lab.machine("b"));
    a.enable();
    b.enable();
    let root = a.goal("brief\n");
    let child = a.add("child", Some(&root));
    a.sync();
    for round in 0..40 {
        a.retitle(&child, &format!("title {round}")).unwrap();
        a.export();
    }
    assert_eq!(a.substrate.status().unwrap().pending.len(), 1);
    assert_eq!(flushed(&mut a), 1);
    b.sync();
    assert_eq!(b.store.get(&b.project, &child).unwrap().title, "title 39");
    let history = b.store.events(&b.project, &child, 100).unwrap();
    assert_eq!(history.iter().filter(|e| e.kind == "updated").count(), 40);
}

#[test]
fn an_answer_elsewhere_is_incorporated_at_home_and_closes_everywhere() {
    let lab = Lab::new();
    let (mut a, mut b) = (lab.machine("a"), lab.machine("b"));
    a.enable();
    b.enable();
    let root = a.goal("brief\n");
    let raised = a
        .store
        .raise_request(
            &a.project,
            &root,
            "scope",
            &RequestContent {
                kind: RequestKind::Decision,
                title: "Scope".into(),
                question: "Which scope?".into(),
                context: None,
                recommendation: None,
                choices: vec![
                    Choice {
                        id: "small".into(),
                        label: "Small".into(),
                        detail: None,
                    },
                    Choice {
                        id: "large".into(),
                        label: "Large".into(),
                        detail: None,
                    },
                ],
                tasks: Vec::new(),
            },
            actor(21),
        )
        .unwrap();
    let id = raised.request.id;
    a.sync();
    b.sync();
    assert_eq!(
        b.store.request(&b.project, &id).unwrap().state,
        RequestState::Open
    );

    let answered = b
        .store
        .answer_request(
            &b.project,
            &id,
            &AnswerContent {
                choice: Some("small".into()),
                text: None,
                source: Source::Ui,
                provenance: "neutral viewer".into(),
                interpretation: None,
            },
            Actor {
                name: None,
                now: 30,
            },
        )
        .unwrap();
    assert_eq!(answered.state, RequestState::Answered);
    b.sync();
    let (_, imported) = a.sync().unwrap();
    assert_eq!(imported.answers, vec![id.clone()]);
    let at_home = a.store.request(&a.project, &id).unwrap();
    assert_eq!(at_home.state, RequestState::Answered);
    assert_eq!(
        at_home.answer.unwrap().content.choice.as_deref(),
        Some("small")
    );
    a.store
        .incorporate_request(
            &a.project,
            &id,
            &Incorporation {
                kind: DecisionKind::Decision,
                decision: "Keep it small.".into(),
            },
            actor(40),
        )
        .unwrap();
    a.sync();
    b.sync();
    let closed = b.store.request(&b.project, &id).unwrap();
    assert_eq!(closed.state, RequestState::Closed);
    assert!(closed.closure.is_some());
    assert_eq!(b.store.share_status().unwrap().sent_answers, 0);
    // The emptied answer list replicates too, and the home ignores it.
    b.sync();
    let (_, imported) = a.sync().unwrap();
    assert_eq!(imported.answers, Vec::<String>::new());
}

#[test]
fn disabled_sharing_and_unbound_projects_export_nothing() {
    let lab = Lab::new();
    let (mut a, mut b) = (lab.machine("a"), lab.machine("b"));
    b.enable();
    a.goal("brief\n");
    assert!(a.sync().is_none());
    b.sync();
    assert_eq!(b.substrate.names("tasks/").unwrap().len(), 0);

    // Enabling exports the bound project, never the unbound one.
    let unbound = a.store.register_project("private").unwrap();
    let secret = a
        .store
        .add(
            &unbound,
            &NewTask {
                title: "secret".into(),
                ..NewTask::default()
            },
            actor(5),
        )
        .unwrap();
    a.enable();
    a.add("loose", None);
    let (exported, _) = a.sync().unwrap();
    assert_eq!(exported.trees.len(), 2);
    a.store
        .set_status(
            &unbound,
            &secret.id,
            Status::Held,
            Some("wait"),
            actor(6),
            false,
        )
        .unwrap();
    a.sync();
    b.sync();
    let names = b.substrate.names("tasks/").unwrap();
    assert_eq!(names.len(), 2);
    assert!(names.iter().all(|name| !name.as_str().contains(&secret.id)));

    // Disabling stops exports.
    a.store.disable_sharing(7).unwrap();
    a.add("later", None);
    assert!(a.sync().is_none());
    assert!(!a.store.share_pending().unwrap());
}

#[test]
fn a_schema_9_catalog_migrates_once() {
    let lab = Lab::new();
    let path = lab.root.join("old.sqlite3");
    {
        let connection = Connection::open(&path).unwrap();
        connection.execute_batch(SCHEMA_9).unwrap();
        connection
            .execute_batch(
                "INSERT INTO projects VALUES('p-00000000000000000000000000000001','fixture');
                 INSERT INTO tasks(id,project,title,status,created_at,updated_at)
                   VALUES('t-00000000000000000000000000000001','p-00000000000000000000000000000001','old','open',1,1);
                 INSERT INTO events(task,at,kind) VALUES('t-00000000000000000000000000000001',1,'created');
                 INSERT INTO events(task,at,kind) VALUES('t-00000000000000000000000000000001',2,'updated');",
            )
            .unwrap();
    }
    let mut store = Store::open(&path).unwrap();
    let project = Project::try_new("p-00000000000000000000000000000001").unwrap();
    let history = store
        .events(&project, "t-00000000000000000000000000000001", 10)
        .unwrap();
    assert_eq!(history.len(), 2);
    let uids: std::collections::BTreeSet<_> =
        history.iter().map(|event| event.uid.clone()).collect();
    assert_eq!(uids.len(), 2);
    assert!(uids.iter().all(|uid| uid.len() == 32));
    drop(store);
    let mut store = Store::open(&path).unwrap();
    let again = store
        .events(&project, "t-00000000000000000000000000000001", 10)
        .unwrap();
    assert_eq!(
        again
            .iter()
            .map(|event| event.uid.clone())
            .collect::<std::collections::BTreeSet<_>>(),
        uids
    );
    assert!(store.sharing().unwrap().is_none());
}

#[test]
fn growth_of_a_tree_with_500_events_stays_incremental() {
    let lab = Lab::new();
    let (mut a, mut b) = (lab.machine("a"), lab.machine("b"));
    a.enable();
    b.enable();
    let root = a.goal("brief\n");
    let child = a.add("child", Some(&root));
    a.sync();
    let mut pushed = Vec::new();
    for batch in 0..50 {
        for round in 0..10 {
            a.retitle(&child, &format!("title {batch}-{round}"))
                .unwrap();
        }
        a.export();
        let pending = a.substrate.status().unwrap().pending;
        let bytes: usize = pending
            .iter()
            .flat_map(|pending| pending.blobs.iter())
            .map(|hash| a.substrate.blob(hash).unwrap().content().len())
            .sum();
        pushed.push(bytes);
        assert_eq!(flushed(&mut a), 1);
    }
    b.sync();
    let imported: i64 = b
        .store
        .connection
        .query_row(
            "SELECT COUNT(*) FROM events WHERE task=?1",
            [&child],
            |row| row.get(0),
        )
        .unwrap();
    assert!(imported >= 500, "{imported}");
    let total: usize = pushed.iter().sum();
    let largest = pushed.iter().copied().max().unwrap_or_default();
    eprintln!(
        "500 events in 50 pushes: {total} bytes pushed, largest push {largest} bytes, mean {} bytes",
        total / pushed.len()
    );
    // Each push carries its new events and small records, not the history.
    assert!(largest < 16 * 1024, "largest push {largest}");
}
