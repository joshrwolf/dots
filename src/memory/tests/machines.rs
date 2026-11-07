//! Two machines publishing knowledge through one `file://` Git remote.
mod support;

use std::time::Duration;

use dots_memory::{
    Choice, Error, Freshness, IntegrationKind, Landing, Resolution, Resource, Scope,
};
use support::{Machine, Remote, TestResult, global};

fn pair(remote: &Remote) -> (Machine, Machine, dots_memory::Snapshot) {
    let mut a = Machine::new(&remote.url());
    let now = a.now();
    let genesis = a.store.initialize(now).expect("initialize");
    (a, Machine::new(&remote.url()), genesis)
}

#[test]
fn a_publication_reaches_the_other_machine_within_the_freshness_bound() -> TestResult {
    let remote = Remote::new();
    let (mut a, mut b, genesis) = pair(&remote);
    let window = Duration::from_secs(30);
    let now = b.now();
    assert_eq!(b.store.head(window, now)?.snapshot, genesis);
    let published = a.published(&genesis, &[global("MEMORY.md", "from a\n")]);
    // B listed moments ago, so context reads reuse that listing.
    let now = b.now();
    let cached = b.store.head(window, now)?;
    assert_eq!(cached.snapshot, genesis);
    assert_eq!(cached.freshness, Freshness::Current);
    b.advance(30);
    let now = b.now();
    assert_eq!(b.store.head(window, now)?.snapshot, published);
    assert_eq!(b.content(None, "MEMORY.md").as_deref(), Some("from a\n"));
    // Explicit reads list every time.
    let later = a.published(&published, &[global("MEMORY.md", "again\n")]);
    assert_eq!(b.head().snapshot, later);
    Ok(())
}

#[test]
fn concurrent_disjoint_publications_merge_into_two_parents() -> TestResult {
    let remote = Remote::new();
    let (mut a, mut b, genesis) = pair(&remote);
    let listed = b.now();
    assert_eq!(b.store.head(Duration::ZERO, listed)?.snapshot, genesis);
    let first = a.published(&genesis, &[global("a.md", "from a")]);
    // Publishing at the instant B listed reuses that listing, so B proposes a
    // fast-forward from genesis; the remote rejects it and B merges.
    let Landing::Published { snapshot: merged } = b
        .store
        .publish(&genesis, &[global("b.md", "from b")], listed)?
        .landing
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
    b.connect(&remote.unreachable());
    let stale = b.head();
    assert_eq!(stale.snapshot, first);
    let Freshness::Stale { since, reason } = &stale.freshness else {
        panic!("an unreachable remote read as current");
    };
    assert!(since.is_some());
    assert!(reason.contains("ls-remote"), "{reason}");
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
    assert_eq!(head.freshness, Freshness::Current);
    Ok(())
}

#[test]
fn a_machine_that_never_listed_cannot_read_offline() {
    let remote = Remote::new();
    let (_a, _b, _) = pair(&remote);
    let mut c = Machine::new(&remote.unreachable());
    let now = c.now();
    assert!(matches!(
        c.store.head(Duration::ZERO, now),
        Err(Error::Unavailable(_))
    ));
}
