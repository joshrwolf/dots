//! Two machines publishing knowledge through one `file://` Git remote.
mod support;

use dots_memory::{Choice, Error, IntegrationKind, Landing, Resolution, Resource, Scope};
use support::{Machine, Remote, TestResult, global};

fn pair(remote: &Remote) -> (Machine, Machine, dots_memory::Snapshot) {
    let mut a = Machine::new(&remote.url());
    let now = a.now();
    let genesis = a.store.initialize(now).expect("initialize");
    (a, Machine::new(&remote.url()), genesis)
}

#[test]
fn a_publication_reaches_the_other_machine_on_its_next_pass() -> TestResult {
    let remote = Remote::new();
    let (mut a, mut b, genesis) = pair(&remote);
    assert!(matches!(b.store.head(), Err(Error::Unsynced)));
    assert_eq!(b.head().snapshot, genesis);
    let synced = b.store.head()?.freshness.as_of;
    let published = a.published(&genesis, &[global("MEMORY.md", "from a\n")]);
    // Reads serve the local copy until the next pass.
    let local = b.store.head()?;
    assert_eq!((local.snapshot, local.freshness.as_of), (genesis, synced));
    assert_eq!(local.freshness.failing, None);
    assert_eq!(b.sync(), None);
    let head = b.store.head()?;
    assert_eq!(head.snapshot, published);
    assert!(head.freshness.as_of > synced);
    assert_eq!(b.content(None, "MEMORY.md").as_deref(), Some("from a\n"));
    Ok(())
}

#[test]
fn concurrent_disjoint_publications_merge_into_two_parents() -> TestResult {
    let remote = Remote::new();
    let (mut a, mut b, genesis) = pair(&remote);
    assert_eq!(b.head().snapshot, genesis);
    let first = a.published(&genesis, &[global("a.md", "from a")]);
    // B publishes against its local copy at genesis; the merge happens on B.
    let Landing::Published { snapshot: merged } =
        b.publish(&genesis, &[global("b.md", "from b")])?.landing
    else {
        panic!("publication stayed pending");
    };
    let head = a.head();
    assert_eq!(head.snapshot, merged);
    let parents = a.store.view(&head, None)?.manifest().parents.clone();
    assert_eq!(parents.len(), 2);
    assert_eq!(parents[0], first);
    assert_eq!(a.content(None, "a.md").as_deref(), Some("from a"));
    assert_eq!(a.content(None, "b.md").as_deref(), Some("from b"));
    // The merged-in candidate's own snapshot stays readable on A.
    let candidate = parents[1];
    assert_eq!(
        a.content(Some(&candidate), "b.md").as_deref(),
        Some("from b")
    );
    assert_eq!(a.content(Some(&candidate), "a.md"), None);
    Ok(())
}

#[test]
fn contending_publishers_each_land_or_keep_a_pending_candidate() {
    let remote = Remote::new();
    let (mut a, _, genesis) = pair(&remote);
    let url = remote.url();
    let rounds = 4;
    // Each publisher lives on its own thread, so each opens its own state.
    let publisher = |label: &'static str| {
        let mut machine = Machine::new(&url);
        assert_eq!(machine.head().snapshot, genesis);
        let mut outcomes = (0, 0);
        for round in 0..rounds {
            let expected = machine.store.head().expect("head").snapshot;
            let change = global(&format!("{label}-{round}.md"), label);
            let publication = machine.publish(&expected, &[change]).expect("publish");
            match publication.landing {
                Landing::Published { .. } => outcomes.0 += 1,
                Landing::Pending { candidate, .. } => {
                    assert!(publication.pending.contains(&candidate));
                    outcomes.1 += 1;
                }
            }
        }
        // Later publications integrate whatever stayed pending.
        for _ in 0..rounds {
            if machine.store.pending().expect("pending").is_empty() {
                return outcomes;
            }
            let head = machine.head();
            machine.publish(&head.snapshot, &[]).expect("publish");
        }
        panic!("{label}: candidates stayed pending")
    };
    let (first, second) = std::thread::scope(|scope| {
        let first = scope.spawn(|| publisher("a"));
        let second = scope.spawn(|| publisher("b"));
        (first.join(), second.join())
    });
    let (first, second) = (first.expect("a panicked"), second.expect("b panicked"));
    assert_eq!(first.0 + first.1, rounds);
    assert_eq!(second.0 + second.1, rounds);
    assert!(first.0 + second.0 > 0, "neither publisher made progress");
    for label in ["a", "b"] {
        for round in 0..rounds {
            let path = format!("{label}-{round}.md");
            assert_eq!(a.content(None, &path).as_deref(), Some(label), "{path}");
        }
    }
}

#[test]
fn same_document_divergence_needs_an_explicit_reconcile() -> TestResult {
    let remote = Remote::new();
    let (mut a, mut b, genesis) = pair(&remote);
    let base = a.published(&genesis, &[global("MEMORY.md", "base\n")]);
    let theirs = a.published(&base, &[global("MEMORY.md", "a's line\n")]);
    let Err(Error::Diverged {
        expected,
        actual,
        candidate,
        conflicts,
    }) = b.publish(
        &base,
        &[global("MEMORY.md", "b's line\n"), global("other.md", "b")],
    )
    else {
        panic!("same-document edits did not diverge");
    };
    assert_eq!((expected, actual, conflicts), (base, theirs, 1));
    assert_eq!(b.head().snapshot, theirs);
    let head = b.head();
    let plans = b.store.pending_plans(&head)?;
    assert_eq!(plans.len(), 1);
    let plan = &plans[0].plan;
    assert_eq!(
        (plan.kind, plan.base, plan.local, plan.incoming),
        (IntegrationKind::Merge, base, candidate, theirs)
    );
    assert_eq!(
        plan.conflicts[0].resource.path,
        std::path::Path::new("MEMORY.md")
    );
    let resolution = [Resolution {
        resource: Resource {
            scope: Scope::Global,
            path: "MEMORY.md".into(),
        },
        choice: Choice::Write {
            content: "a's line\nb's line\n".into(),
        },
    }];
    let now = b.now();
    let merged = b.store.reconcile(&candidate, &theirs, &resolution, now)?;
    assert_eq!(b.store.pending()?, Vec::new());
    assert_eq!(a.head().snapshot, merged);
    assert_eq!(
        a.content(None, "MEMORY.md").as_deref(),
        Some("a's line\nb's line\n")
    );
    assert_eq!(a.content(None, "other.md").as_deref(), Some("b"));
    Ok(())
}

#[test]
fn an_offline_publication_is_retained_labeled_and_published_later() -> TestResult {
    let remote = Remote::new();
    let (mut a, mut b, genesis) = pair(&remote);
    let first = a.published(&genesis, &[global("first.md", "first")]);
    assert_eq!(b.head().snapshot, first);
    let synced = b.store.head()?.freshness.as_of;
    b.connect(&remote.unreachable());
    let reason = b.sync().expect("an unreachable remote synced");
    assert!(reason.contains("ls-remote"), "{reason}");
    let stale = b.store.head()?;
    assert_eq!((stale.snapshot, stale.freshness.as_of), (first, synced));
    let failing = stale.freshness.failing.expect("an outage read as healthy");
    assert!(failing.reason.contains("ls-remote"), "{}", failing.reason);
    assert_eq!(b.content(None, "first.md").as_deref(), Some("first"));
    let offline = b.publish(&first, &[global("offline.md", "offline")])?;
    let Landing::Pending { candidate, .. } = offline.landing else {
        panic!("offline publication was not retained");
    };
    assert_eq!(b.store.pending()?.len(), 1);
    let now = b.now();
    assert!(matches!(
        b.store.reconcile(&candidate, &first, &[], now),
        Err(Error::Unavailable(_))
    ));
    let second = a.published(&first, &[global("second.md", "second")]);
    b.connect(&remote.url());
    let online = b.publish(&second, &[global("online.md", "online")])?;
    assert_eq!(online.integrated, vec![candidate]);
    assert!(online.pending.is_empty() && b.store.pending()?.is_empty());
    let head = a.head();
    for path in ["first.md", "second.md", "offline.md", "online.md"] {
        assert!(a.content(None, path).is_some(), "{path}");
    }
    assert_eq!(head.freshness.failing, None);
    Ok(())
}

#[test]
fn a_machine_that_never_synced_has_no_knowledge_to_read() {
    let remote = Remote::new();
    let (_a, _b, _) = pair(&remote);
    let mut c = Machine::new(&remote.unreachable());
    assert!(matches!(c.store.head(), Err(Error::Unsynced)));
    assert!(c.sync().is_some());
    assert!(matches!(c.store.head(), Err(Error::Unsynced)));
}
