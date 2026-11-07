use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use dots_documents::staged_directory::StagedDirectory;
use dots_substrate::memory::MemoryBackend;
use dots_substrate::{Backend, Config, Fetched, Hash};

use super::*;
use crate::{Choice, MAX_FILE_BYTES, Resolution, Resource};

type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;

/// A shared in-memory remote that tests can take offline per machine.
#[derive(Clone, Debug)]
struct Link {
    remote: MemoryBackend,
    offline: Arc<AtomicBool>,
}

impl Backend for Link {
    fn list(&self) -> dots_substrate::Result<BTreeMap<RefName, Version>> {
        self.check()?;
        self.remote.list()
    }
    fn fetch(
        &self,
        name: &RefName,
        cached: &dyn Fn(&Hash) -> bool,
    ) -> dots_substrate::Result<Option<Fetched>> {
        self.check()?;
        self.remote.fetch(name, cached)
    }
    fn update(&self, updates: &[Update]) -> dots_substrate::Result<Vec<Outcome>> {
        self.check()?;
        self.remote.update(updates)
    }
}

impl Link {
    fn check(&self) -> dots_substrate::Result<()> {
        if self.offline.load(Ordering::SeqCst) {
            return Err(dots_substrate::Error::Unavailable("neutral outage".into()));
        }
        Ok(())
    }
}

struct Machine {
    _scratch: StagedDirectory,
    store: Store,
    offline: Arc<AtomicBool>,
    tick: u64,
}

impl Machine {
    fn new(remote: &MemoryBackend) -> Result<Self> {
        let scratch = StagedDirectory::create(&std::env::temp_dir())?;
        let offline = Arc::new(AtomicBool::new(false));
        let link = Link {
            remote: remote.clone(),
            offline: Arc::clone(&offline),
        };
        let substrate = Substrate::open(
            &scratch.path().join("substrate"),
            Box::new(link),
            Config::default(),
        )?;
        let store = Store::open(substrate, &scratch.path().join("memory"))?;
        Ok(Self {
            _scratch: scratch,
            store,
            offline,
            tick: 0,
        })
    }

    /// Each operation happens one second after the previous one.
    fn now(&mut self) -> SystemTime {
        self.tick += 1;
        UNIX_EPOCH + Duration::from_secs(1_900_000_000 + self.tick)
    }

    fn head(&mut self) -> Result<Head> {
        let now = self.now();
        self.store.head(Duration::ZERO, now)
    }

    fn snapshot(&mut self) -> Result<Snapshot> {
        Ok(self.head()?.snapshot)
    }

    fn publish(&mut self, expected: &Snapshot, changes: &[Change]) -> Result<Publication> {
        let now = self.now();
        self.store.publish(expected, changes, now)
    }

    fn published(&mut self, expected: &Snapshot, changes: &[Change]) -> Result<Snapshot> {
        match self.publish(expected, changes)?.landing {
            Landing::Published { snapshot } => Ok(snapshot),
            Landing::Pending { reason, .. } => Err(Error::Unavailable(reason)),
        }
    }

    fn content(&mut self, snapshot: Option<&Snapshot>, path: &str) -> Result<Option<String>> {
        let head = self.head()?;
        self.store
            .view(&head, snapshot)?
            .content(&Scope::Global, Path::new(path))
    }
}

fn pair() -> Result<(Machine, Machine, Snapshot)> {
    let remote = MemoryBackend::new();
    let mut a = Machine::new(&remote)?;
    let now = a.now();
    let genesis = a.store.initialize(now)?;
    let b = Machine::new(&remote)?;
    Ok((a, b, genesis))
}

fn put(scope: Scope, path: &str, content: &str) -> Change {
    Change {
        scope,
        path: path.into(),
        action: Action::Write {
            content: content.into(),
        },
    }
}

fn global(path: &str, content: &str) -> Change {
    put(Scope::Global, path, content)
}

fn remove(path: &str) -> Change {
    Change {
        scope: Scope::Global,
        path: path.into(),
        action: Action::Remove {},
    }
}

fn resource(path: &str) -> Resource {
    Resource {
        scope: Scope::Global,
        path: path.into(),
    }
}

#[test]
fn serialized_changes_require_an_explicit_action() {
    for value in [
        r#"{"scope":{"kind":"global"},"path":"note.md"}"#,
        r#"{"scope":{"kind":"global"},"path":"note.md","action":null}"#,
        r#"{"scope":{"kind":"global"},"path":"note.md","action":{"kind":"write"}}"#,
        r#"{"scope":{"kind":"global"},"path":"note.md","action":{"kind":"remove","content":"unexpected"}}"#,
    ] {
        assert!(serde_json::from_str::<Change>(value).is_err(), "{value}");
    }
    for value in [
        r#"{"kind":"global","project":"p-00000000000000000000000000000000"}"#,
        r#"{"kind":"global","unknown":true}"#,
    ] {
        assert!(serde_json::from_str::<Scope>(value).is_err(), "{value}");
    }
}

#[test]
fn initialization_is_create_only_and_reads_need_published_knowledge() -> TestResult {
    let remote = MemoryBackend::new();
    let mut a = Machine::new(&remote)?;
    assert!(matches!(a.head(), Err(Error::Uninitialized)));
    let now = a.now();
    let genesis = a.store.initialize(now)?;
    let mut b = Machine::new(&remote)?;
    let now = b.now();
    assert!(matches!(b.store.initialize(now), Err(Error::AlreadyInitialized(s)) if s == genesis));
    let head = b.head()?;
    assert_eq!(head.snapshot, genesis);
    assert_eq!(head.freshness, Freshness::Current);
    let absent = b
        .store
        .view(&head, None)?
        .read(&Scope::Global, Path::new("MEMORY.md"), 1, 200)?;
    assert!(!absent.document.exists);
    assert_eq!(absent.document.revision, "missing");
    Ok(())
}

#[test]
fn publication_is_atomic_and_pinned_reads_remain_at_their_snapshot() -> TestResult {
    let (mut a, _, genesis) = pair()?;
    let first = a.published(
        &genesis,
        &[
            global("MEMORY.md", "# Memory\n"),
            global("wiki/topic.md", "topic\n"),
            global("empty.md", ""),
        ],
    )?;
    let second = a.published(
        &first,
        &[global("MEMORY.md", "# Updated\n"), remove("empty.md")],
    )?;
    assert_eq!(
        a.content(Some(&first), "MEMORY.md")?.as_deref(),
        Some("# Memory\n")
    );
    assert_eq!(a.content(Some(&first), "empty.md")?.as_deref(), Some(""));
    assert_eq!(
        a.content(None, "MEMORY.md")?.as_deref(),
        Some("# Updated\n")
    );
    assert_eq!(a.content(None, "empty.md")?, None);
    assert_eq!(a.store.parents(&second)?, vec![first]);
    let content = "café\r\nlast line";
    let third = a.published(&second, &[global("wiki/café.md", content)])?;
    assert_eq!(
        a.content(Some(&third), "wiki/café.md")?.as_deref(),
        Some(content)
    );
    // Repeating content already published changes nothing.
    let again = a.publish(&third, &[global("wiki/café.md", content)])?;
    assert_eq!(again.changed, 0);
    assert!(matches!(again.landing, Landing::Published { snapshot } if snapshot == third));
    Ok(())
}

#[test]
fn invalid_batches_and_unpublished_snapshots_change_nothing() -> TestResult {
    let (mut a, _, genesis) = pair()?;
    let huge = "x".repeat(MAX_FILE_BYTES + 1);
    for changes in [
        vec![global("valid.md", "valid"), global("../escape.md", "bad")],
        vec![global("valid.md", "valid"), global("large.md", &huge)],
        vec![global("same.md", "one"), global("same.md", "two")],
        vec![global("Case.md", "one"), global("case.md", "two")],
        vec![global("Wiki/one.md", "one"), global("wiki/two.md", "two")],
        vec![global("binary.md", "nul\0byte")],
        vec![global("file.md", "one"), global("file.md/child.md", "two")],
        vec![global("plain.txt", "not markdown")],
    ] {
        assert!(a.publish(&genesis, &changes).is_err());
        assert_eq!(a.snapshot()?, genesis);
    }
    let content = "x".repeat(MAX_FILE_BYTES);
    let changes: Vec<_> = (0..=(crate::MAX_CORPUS_BYTES / MAX_FILE_BYTES))
        .map(|index| global(&format!("document-{index}.md"), &content))
        .collect();
    assert!(a.publish(&genesis, &changes).is_err());
    let unknown: Snapshot = format!("sha256:{}", "0".repeat(64)).parse()?;
    assert!(a.publish(&unknown, &[global("a.md", "a")]).is_err());
    let head = a.head()?;
    assert!(a.store.view(&head, Some(&unknown)).is_err());
    assert_eq!(a.store.pending()?, Vec::new());
    assert_eq!(head.snapshot, genesis);
    Ok(())
}

#[test]
fn project_identity_is_explicit_and_display_names_do_not_select_it() -> TestResult {
    let (mut a, _, genesis) = pair()?;
    let now = a.now();
    let first = a.store.register_project(&genesis, "sample-project", now)?;
    let Landing::Published { snapshot } = first.publication.landing else {
        panic!("registration stayed pending");
    };
    let now = a.now();
    let other = a.store.register_project(&snapshot, "sample-project", now)?;
    assert_ne!(first.project, other.project);
    let Landing::Published { snapshot } = other.publication.landing else {
        panic!("registration stayed pending");
    };
    let scope = Scope::Project(first.project.clone());
    let written = a.published(
        &snapshot,
        &[put(scope.clone(), "MEMORY.md", "project note\n")],
    )?;
    let head = a.head()?;
    let view = a.store.view(&head, Some(&written))?;
    assert_eq!(
        view.read(&scope, Path::new("MEMORY.md"), 1, 20)?
            .document
            .content,
        "project note\n"
    );
    assert!(
        !view
            .read(
                &Scope::Project(other.project),
                Path::new("MEMORY.md"),
                1,
                20
            )?
            .document
            .exists
    );
    let unknown = ProjectId::allocate()?;
    assert!(matches!(
        view.read(&Scope::Project(unknown.clone()), Path::new("MEMORY.md"), 1, 20),
        Err(Error::UnknownProject(id)) if id == unknown
    ));
    assert!(matches!(
        a.store
            .view(&head, Some(&genesis))?
            .read(&scope, Path::new("MEMORY.md"), 1, 20),
        Err(Error::UnknownProject(_))
    ));
    Ok(())
}

#[test]
fn literal_search_is_scoped_bounded_and_uses_relative_resources() -> TestResult {
    let (mut a, _, genesis) = pair()?;
    let now = a.now();
    let project = a.store.register_project(&genesis, "sample", now)?;
    let base = a.snapshot()?;
    let published = a.published(
        &base,
        &[
            global("a.md", "First Needle\nSecond needle\n"),
            global("log.md", "needle in named topic\n"),
            put(
                Scope::Project(project.project.clone()),
                "a.md",
                "needle private to project\n",
            ),
        ],
    )?;
    let head = a.head()?;
    let view = a.store.view(&head, Some(&published))?;
    let first = view.search(&Scope::Global, "NEEDLE", 0, 1)?;
    assert_eq!(first.snapshot, published);
    assert_eq!(first.result.hits[0].path, Path::new("a.md"));
    assert_eq!(first.result.next_offset, Some(1));
    let last = view.search(&Scope::Global, "needle", 2, 1)?;
    assert_eq!(last.result.hits[0].path, Path::new("log.md"));
    assert!(!last.result.has_more);
    let scoped = view.search(&Scope::Project(project.project), "needle", 0, 20)?;
    assert_eq!(scoped.result.hits.len(), 1);
    assert_eq!(scoped.result.hits[0].excerpt, "needle private to project");
    Ok(())
}

#[test]
fn layout_collisions_require_explicit_choices_for_every_document() -> TestResult {
    for (ours, theirs) in [
        ("topic.md", "topic.md/child.md"),
        ("Topic.md", "topic.md"),
        ("Wiki/a.md", "wiki/b.md"),
    ] {
        let (mut a, mut b, genesis) = pair()?;
        let remote = b.published(&genesis, &[global(theirs, "theirs")])?;
        let Err(Error::Diverged {
            candidate, actual, ..
        }) = a.publish(&genesis, &[global(ours, "ours")])
        else {
            panic!("{ours} vs {theirs} did not diverge");
        };
        assert_eq!(actual, remote);
        let plan = a.store.plan(&candidate, &remote)?;
        assert_eq!(plan.conflicts.len(), 2, "{ours} vs {theirs}");
        let now = a.now();
        assert!(a.store.reconcile(&candidate, &remote, &[], now).is_err());
        let keep_both = [
            Resolution {
                resource: resource(ours),
                choice: Choice::Local {},
            },
            Resolution {
                resource: resource(theirs),
                choice: Choice::Incoming {},
            },
        ];
        let now = a.now();
        assert!(
            a.store
                .reconcile(&candidate, &remote, &keep_both, now)
                .is_err()
        );
        let choices = [
            Resolution {
                resource: resource(ours),
                choice: Choice::Remove {},
            },
            Resolution {
                resource: resource(theirs),
                choice: Choice::Incoming {},
            },
        ];
        let now = a.now();
        let merged = a.store.reconcile(&candidate, &remote, &choices, now)?;
        // Keeping only the incoming side publishes nothing new.
        assert_eq!(merged, remote);
        assert_eq!(a.content(None, ours)?, None);
        assert_eq!(a.content(None, theirs)?.as_deref(), Some("theirs"));
        assert_eq!(a.store.pending()?, Vec::new());
    }
    Ok(())
}

#[test]
fn deletion_conflicts_and_bad_resolutions_keep_everything_else() -> TestResult {
    let (mut a, mut b, genesis) = pair()?;
    let base = a.published(&genesis, &[global("MEMORY.md", "base")])?;
    let remote = b.published(&base, &[remove("MEMORY.md")])?;
    let Err(Error::Diverged { candidate, .. }) = a.publish(
        &base,
        &[global("MEMORY.md", "changed"), global("keep.md", "keep")],
    ) else {
        panic!("edit against removal did not diverge");
    };
    let plan = a.store.plan(&candidate, &remote)?;
    assert_eq!(plan.conflicts.len(), 1);
    assert!(plan.conflicts[0].incoming_blob.is_none());
    for resolutions in [
        vec![Resolution {
            resource: resource("keep.md"),
            choice: Choice::Remove {},
        }],
        vec![Resolution {
            resource: resource("MEMORY.md"),
            choice: Choice::Write {
                content: "bad\0text".into(),
            },
        }],
        vec![
            Resolution {
                resource: resource("MEMORY.md"),
                choice: Choice::Local {},
            },
            Resolution {
                resource: resource("MEMORY.md"),
                choice: Choice::Incoming {},
            },
        ],
    ] {
        let now = a.now();
        assert!(
            a.store
                .reconcile(&candidate, &remote, &resolutions, now)
                .is_err()
        );
        assert_eq!(a.snapshot()?, remote);
    }
    let now = a.now();
    let read = a
        .store
        .inspect(&candidate, &Scope::Global, Path::new("MEMORY.md"), 1, 20)?;
    assert_eq!(read.document.content, "changed\n");
    let resolution = [Resolution {
        resource: resource("MEMORY.md"),
        choice: Choice::Write {
            content: "written during reconcile".into(),
        },
    }];
    let merged = a.store.reconcile(&candidate, &remote, &resolution, now)?;
    assert_eq!(a.snapshot()?, merged);
    assert_eq!(
        a.content(None, "MEMORY.md")?.as_deref(),
        Some("written during reconcile")
    );
    assert_eq!(a.content(None, "keep.md")?.as_deref(), Some("keep"));
    assert!(
        a.store
            .inspect(&candidate, &Scope::Global, Path::new("MEMORY.md"), 1, 20)
            .is_err()
    );
    Ok(())
}

#[test]
fn a_stale_plan_is_refused_and_identical_edits_need_no_merge() -> TestResult {
    let (mut a, mut b, genesis) = pair()?;
    let remote = b.published(&genesis, &[global("MEMORY.md", "theirs")])?;
    let Err(Error::Diverged { candidate, .. }) =
        a.publish(&genesis, &[global("MEMORY.md", "ours")])
    else {
        panic!("did not diverge");
    };
    let later = b.published(&remote, &[global("later.md", "later")])?;
    let choice = [Resolution {
        resource: resource("MEMORY.md"),
        choice: Choice::Local {},
    }];
    let now = a.now();
    assert!(matches!(
        a.store.reconcile(&candidate, &remote, &choice, now),
        Err(Error::Conflict { expected, actual }) if expected == remote && actual == later
    ));
    assert_eq!(a.store.pending_candidates()?, vec![candidate]);
    let same = a.published(&genesis, &[global("later.md", "later")])?;
    assert_eq!(same, later);
    Ok(())
}

#[test]
fn manifests_are_canonical_and_validated_on_decode() -> TestResult {
    let (mut a, _, genesis) = pair()?;
    let blob = a.store.substrate().blob(&genesis.hash())?;
    let manifest = Manifest::decode(&blob)?;
    assert_eq!(manifest.parents, Vec::new());
    let text = std::str::from_utf8(blob.content())?;
    assert!(text.starts_with(r#"{"parents":[],"store":"k-"#));
    let pretty = serde_json::to_vec_pretty(&manifest)?;
    assert!(Manifest::decode(&Blob::new(pretty)?).is_err());
    let mut outside = manifest.clone();
    outside.entries.insert(
        "elsewhere/MEMORY.md".into(),
        Entry {
            hash: Hash::of(b""),
            size: 0,
        },
    );
    assert!(outside.encode().is_err());
    let mut parents = manifest;
    parents.parents = vec![genesis, genesis];
    assert!(parents.encode().is_err());
    let _ = a.now();
    Ok(())
}

#[test]
fn offline_publications_stay_pending_until_the_next_online_publication() -> TestResult {
    let (mut a, mut b, genesis) = pair()?;
    let first = a.published(&genesis, &[global("first.md", "first")])?;
    assert_eq!(b.snapshot()?, first);
    b.offline.store(true, Ordering::SeqCst);
    let stale = b.head()?;
    assert_eq!(stale.snapshot, first);
    assert!(matches!(
        stale.freshness,
        Freshness::Stale { since: Some(_), .. }
    ));
    let offline = b.publish(&first, &[global("offline.md", "offline")])?;
    let Landing::Pending { candidate, .. } = offline.landing else {
        panic!("offline publication was not retained");
    };
    assert_eq!(offline.pending, vec![candidate]);
    assert_eq!(b.content(None, "offline.md")?, None);
    let second = a.published(&first, &[global("second.md", "second")])?;
    b.offline.store(false, Ordering::SeqCst);
    let head = b.head()?;
    assert_eq!(head.snapshot, second);
    let plans = b.store.pending_plans(&head)?;
    assert_eq!(plans.len(), 1);
    assert_eq!(plans[0].plan.kind, IntegrationKind::Merge);
    assert!(plans[0].plan.conflicts.is_empty());
    let online = b.publish(&second, &[global("online.md", "online")])?;
    assert_eq!(online.integrated, vec![candidate]);
    assert_eq!(online.pending, Vec::new());
    let Landing::Published { snapshot } = online.landing else {
        panic!("online publication stayed pending");
    };
    for path in ["first.md", "second.md", "offline.md", "online.md"] {
        assert!(a.content(Some(&snapshot), path)?.is_some(), "{path}");
    }
    assert!(a.store.ancestors(&snapshot)?.contains(&candidate));
    Ok(())
}
