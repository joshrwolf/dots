use std::path::Path;

use crate::fixture::Fixture;
use crate::{
    Action, Candidate, Change, Choice, Coordinator, Delivery, Error, IntegrationKind, Resolution,
    Resource, Role, Scope, Store,
};

type TestResult = Result<(), Box<dyn std::error::Error>>;

struct Pair {
    home: Fixture,
    replica: Store,
}
impl Pair {
    fn new() -> crate::Result<Self> {
        let home = Fixture::new()?;
        let snapshot = home.store.snapshot()?;
        let snapshot = home
            .store
            .publish(&snapshot, &[put("MEMORY.md", "first\nsecond\n")])?
            .snapshot;
        let bundle = home.parent.join("seed.bundle");
        let receipt = home.store.export_bundle(&snapshot, &bundle)?;
        let replica = Store::restore(
            &home.parent.join("replica"),
            "replica",
            Role::Replica,
            &bundle,
            &receipt,
        )?;
        Ok(Self { home, replica })
    }

    fn transfer(&self, source: &Store, destination: &Store) -> crate::Result<Candidate> {
        let path = self
            .home
            .parent
            .join(format!("transfer-{:016x}.bundle", getrandom::u64()?));
        let receipt = source.export_bundle(&source.snapshot()?, &path)?;
        destination.import_bundle(&path, &receipt)
    }
}

fn put(path: &str, content: &str) -> Change {
    Change {
        scope: Scope::Global,
        path: path.into(),
        action: Action::Write {
            content: content.into(),
        },
    }
}

fn resource(path: &str) -> Resource {
    Resource {
        scope: Scope::Global,
        path: path.into(),
    }
}

#[test]
fn disjoint_contributions_merge_both_histories_and_return_append_only() -> TestResult {
    let pair = Pair::new()?;
    let authority = &pair.home.store;
    let initial = authority.snapshot()?;
    let local = authority
        .publish(&initial, &[put("local.md", "local")])?
        .snapshot;
    let remote = pair
        .replica
        .publish(&initial, &[put("remote.md", "remote")])?
        .snapshot;
    let incoming = pair.transfer(&pair.replica, authority)?;
    let coordinator = Coordinator::new(authority)?;
    let plan = coordinator.plan(&local, &incoming)?;
    assert_eq!(plan.kind, IntegrationKind::Merge);
    assert!(plan.conflicts.is_empty());
    let merged = coordinator.accept(&plan, &[])?;
    assert!(authority.git().is_ancestor(&local, &merged)?);
    assert!(authority.git().is_ancestor(&remote, &merged)?);
    assert_eq!(
        authority
            .latest()?
            .content(&Scope::Global, Path::new("local.md"))?
            .as_deref(),
        Some("local")
    );
    assert_eq!(
        authority
            .latest()?
            .content(&Scope::Global, Path::new("remote.md"))?
            .as_deref(),
        Some("remote")
    );
    let candidate = pair.transfer(authority, &pair.replica)?;
    let delivery = Delivery {
        recipient: pair.replica.identity(),
        expected: remote,
        candidate,
    };
    let receipt = pair.replica.accept_return(&delivery)?;
    assert_eq!(receipt.snapshot, merged);
    assert_eq!(receipt.observed, merged);
    assert!(!receipt.already_present);
    assert_eq!(pair.replica.snapshot()?, merged);
    assert!(Coordinator::new(&pair.replica).is_err());
    Ok(())
}

#[test]
fn clean_text_merge_still_requires_explicit_document_reconciliation() -> TestResult {
    let pair = Pair::new()?;
    let authority = &pair.home.store;
    let base = authority.snapshot()?;
    let local = authority
        .publish(&base, &[put("MEMORY.md", "first changed\nsecond\n")])?
        .snapshot;
    pair.replica
        .publish(&base, &[put("MEMORY.md", "first\nsecond changed\n")])?;
    let candidate = pair.transfer(&pair.replica, authority)?;
    let coordinator = Coordinator::new(authority)?;
    let plan = coordinator.plan(&local, &candidate)?;
    assert_eq!(plan.conflicts.len(), 1);
    assert_eq!(plan.conflicts[0].resource, resource("MEMORY.md"));
    assert!(coordinator.accept(&plan, &[]).is_err());
    assert_eq!(authority.snapshot()?, local);
    let merged = coordinator.accept(
        &plan,
        &[Resolution {
            resource: resource("MEMORY.md"),
            choice: Choice::Write {
                content: "first changed\nsecond changed\n".into(),
            },
        }],
    )?;
    assert_eq!(
        authority
            .latest()?
            .content(&Scope::Global, Path::new("MEMORY.md"))?
            .as_deref(),
        Some("first changed\nsecond changed\n")
    );
    assert_ne!(merged, local);
    Ok(())
}

#[test]
fn stale_local_plan_and_late_replica_write_preserve_both_new_tips() -> TestResult {
    let pair = Pair::new()?;
    let authority = &pair.home.store;
    let base = authority.snapshot()?;
    let local = authority
        .publish(&base, &[put("local.md", "local")])?
        .snapshot;
    let remote = pair
        .replica
        .publish(&base, &[put("remote.md", "remote")])?
        .snapshot;
    let candidate = pair.transfer(&pair.replica, authority)?;
    let coordinator = Coordinator::new(authority)?;
    let old = coordinator.plan(&local, &candidate)?;
    let updated = authority
        .publish(&local, &[put("late-local.md", "late local")])?
        .snapshot;
    assert!(
        matches!(coordinator.accept(&old, &[]), Err(Error::Conflict { expected, actual }) if expected == local && actual == updated)
    );
    assert_eq!(authority.snapshot()?, updated);
    let plan = coordinator.plan(&updated, &candidate)?;
    let merged = coordinator.accept(&plan, &[])?;
    let returned = pair.transfer(authority, &pair.replica)?;
    let late = pair
        .replica
        .publish(&remote, &[put("late-remote.md", "late remote")])?
        .snapshot;
    let delivery = Delivery {
        recipient: pair.replica.identity(),
        expected: remote.clone(),
        candidate: returned,
    };
    assert!(
        matches!(pair.replica.accept_return(&delivery), Err(Error::Conflict { expected, actual }) if expected == remote && actual == late)
    );
    assert_eq!(pair.replica.snapshot()?, late);
    assert_eq!(authority.snapshot()?, merged);
    Ok(())
}

#[test]
fn lost_reply_replay_acknowledges_history_without_rewinding_later_writes() -> TestResult {
    let pair = Pair::new()?;
    let authority = &pair.home.store;
    let base = authority.snapshot()?;
    let local = authority
        .publish(&base, &[put("local.md", "local")])?
        .snapshot;
    let candidate = pair.transfer(authority, &pair.replica)?;
    let delivery = Delivery {
        recipient: pair.replica.identity(),
        expected: base,
        candidate,
    };
    pair.replica.accept_return(&delivery)?;
    let replay = pair.replica.accept_return(&delivery)?;
    assert!(replay.already_present);
    assert_eq!(replay.observed, local);
    let later = pair
        .replica
        .publish(&local, &[put("later.md", "later")])?
        .snapshot;
    let replay = pair.replica.accept_return(&delivery)?;
    assert!(replay.already_present);
    assert_eq!(replay.snapshot, local);
    assert_eq!(replay.observed, later);
    assert_eq!(pair.replica.snapshot()?, later);
    Ok(())
}

#[test]
fn identical_edits_preserve_both_histories_and_idle_integration_is_stable() -> TestResult {
    let pair = Pair::new()?;
    let authority = &pair.home.store;
    let base = authority.snapshot()?;
    let local = authority
        .publish(&base, &[put("MEMORY.md", "same")])?
        .snapshot;
    let remote = pair
        .replica
        .publish(&base, &[put("MEMORY.md", "same")])?
        .snapshot;
    let candidate = pair.transfer(&pair.replica, authority)?;
    let coordinator = Coordinator::new(authority)?;
    let plan = coordinator.plan(&local, &candidate)?;
    assert!(plan.conflicts().is_empty());
    let merged = coordinator.accept(&plan, &[])?;
    assert!(authority.git().is_ancestor(&local, &merged)?);
    assert!(authority.git().is_ancestor(&remote, &merged)?);
    let idle = coordinator.plan(&merged, &candidate)?;
    assert_eq!(idle.kind(), IntegrationKind::UpToDate);
    assert_eq!(coordinator.accept(&idle, &[])?, merged);
    Ok(())
}

#[test]
fn explicit_deletion_and_resolution_validation_do_not_discard_other_documents() -> TestResult {
    let pair = Pair::new()?;
    let authority = &pair.home.store;
    let base = authority.snapshot()?;
    let local = authority
        .publish(
            &base,
            &[put("MEMORY.md", "changed"), put("keep.md", "keep")],
        )?
        .snapshot;
    pair.replica.publish(
        &base,
        &[Change {
            scope: Scope::Global,
            path: "MEMORY.md".into(),
            action: Action::Remove {},
        }],
    )?;
    let candidate = pair.transfer(&pair.replica, authority)?;
    let coordinator = Coordinator::new(authority)?;
    let plan = coordinator.plan(&local, &candidate)?;
    assert_eq!(plan.conflicts().len(), 1);
    assert!(plan.conflicts()[0].incoming_blob.is_none());
    let objects = authority.git().text(&["count-objects", "-v"], None)?;
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
        assert!(coordinator.accept(&plan, &resolutions).is_err());
        assert_eq!(authority.snapshot()?, local);
        assert_eq!(
            authority.git().text(&["count-objects", "-v"], None)?,
            objects
        );
    }
    let read = candidate.inspect(authority, &resource("MEMORY.md"), 1, 20)?;
    assert_eq!(read.candidate.snapshot, candidate.snapshot);
    assert!(!read.document.exists);
    let merged = coordinator.accept(
        &plan,
        &[Resolution {
            resource: resource("MEMORY.md"),
            choice: Choice::Incoming {},
        }],
    )?;
    assert_eq!(authority.snapshot()?, merged);
    assert_eq!(
        authority
            .latest()?
            .content(&Scope::Global, Path::new("MEMORY.md"))?,
        None
    );
    assert_eq!(
        authority
            .latest()?
            .content(&Scope::Global, Path::new("keep.md"))?
            .as_deref(),
        Some("keep")
    );
    Ok(())
}

#[test]
fn delivery_is_bound_to_the_exact_receiver_not_just_the_shared_store() -> TestResult {
    let pair = Pair::new()?;
    let authority = &pair.home.store;
    let base = authority.snapshot()?;
    let seed = pair.home.parent.join("other-seed.bundle");
    let receipt = authority.export_bundle(&base, &seed)?;
    let other = Store::restore(
        &pair.home.parent.join("other-replica"),
        "other",
        Role::Replica,
        &seed,
        &receipt,
    )?;
    let local = authority.publish(&base, &[put("new.md", "new")])?.snapshot;
    let candidate = pair.transfer(authority, &other)?;
    let wrong = Delivery {
        recipient: pair.replica.identity(),
        expected: base.clone(),
        candidate,
    };
    assert!(other.accept_return(&wrong).is_err());
    assert_eq!(other.snapshot()?, base);
    assert_ne!(other.node_id(), pair.replica.node_id());
    assert_eq!(other.id(), pair.replica.id());
    let right = Delivery {
        recipient: other.identity(),
        ..wrong
    };
    assert_eq!(other.accept_return(&right)?.observed, local);
    Ok(())
}

#[test]
fn a_plan_cannot_be_applied_to_another_authority_instance() -> TestResult {
    let pair = Pair::new()?;
    let authority = &pair.home.store;
    let base = authority.snapshot()?;
    let seed = pair.home.parent.join("authority-seed.bundle");
    let receipt = authority.export_bundle(&base, &seed)?;
    let other = Store::restore(
        &pair.home.parent.join("other-authority"),
        "other",
        Role::Authority,
        &seed,
        &receipt,
    )?;
    pair.replica.publish(&base, &[put("remote.md", "remote")])?;
    let candidate = pair.transfer(&pair.replica, authority)?;
    pair.transfer(&pair.replica, &other)?;
    let plan = Coordinator::new(authority)?.plan(&base, &candidate)?;
    assert!(Coordinator::new(&other)?.accept(&plan, &[]).is_err());
    assert_eq!(other.snapshot()?, base);
    Ok(())
}

#[test]
fn native_return_transfer_stays_unpublished_until_the_receiver_accepts_it() -> TestResult {
    use crate::{PeerRequest, PeerResponse};
    let pair = Pair::new()?;
    let authority = &pair.home.store;
    let base = authority.snapshot()?;
    let snapshot = authority.publish(&base, &[put("new.md", "new")])?.snapshot;
    let export = authority.prepare_export(&snapshot)?;
    let candidate = Candidate {
        source: authority.identity(),
        snapshot: snapshot.clone(),
    };
    let delivery = Delivery {
        recipient: pair.replica.identity(),
        expected: base.clone(),
        candidate,
    };
    let request = || PeerRequest::Accept {
        root: pair.replica.root().to_owned(),
        delivery: Delivery {
            recipient: delivery.recipient.clone(),
            expected: delivery.expected.clone(),
            candidate: delivery.candidate.clone(),
        },
    };
    let PeerResponse::NeedsTransfer {
        reference,
        recipient,
        expected,
    } = crate::serve_peer(request())?
    else {
        panic!("receiver must request missing transfer data");
    };
    assert_eq!(recipient, pair.replica.identity());
    assert_eq!(expected, base);
    let target = pair.replica.root().join("repo.git");
    let target = target.to_str().expect("neutral fixture path");
    let mapping = format!("{}:{reference}", export.reference);
    let output = authority.git().run(
        &[
            "push",
            "--receive-pack=git -c receive.hideRefs=!refs/dots/transfer/ receive-pack",
            "--",
            target,
            &mapping,
        ],
        None,
        crate::MAX_METADATA_BYTES,
    )?;
    assert!(
        output.status.success(),
        "stage native transfer: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(pair.replica.snapshot()?, base);
    assert!(pair.replica.view(&snapshot).is_err());
    let PeerResponse::Accepted { receipt } = crate::serve_peer(request())? else {
        panic!("receiver must accept staged data");
    };
    assert_eq!(receipt.snapshot, snapshot);
    assert_eq!(pair.replica.snapshot()?, snapshot);
    let PeerResponse::Accepted { receipt } = crate::serve_peer(request())? else {
        panic!("receiver must acknowledge replay");
    };
    assert!(receipt.already_present);
    Ok(())
}

#[test]
fn receiver_rechecks_the_head_after_native_transfer_and_validates_its_node() -> TestResult {
    use crate::{PeerRequest, PeerResponse};
    let pair = Pair::new()?;
    let authority = &pair.home.store;
    let base = authority.snapshot()?;
    let snapshot = authority.publish(&base, &[put("new.md", "new")])?.snapshot;
    let export = authority.prepare_export(&snapshot)?;
    let candidate = Candidate {
        source: authority.identity(),
        snapshot: snapshot.clone(),
    };
    let delivery = Delivery {
        recipient: pair.replica.identity(),
        expected: base.clone(),
        candidate,
    };
    let request = PeerRequest::Accept {
        root: pair.replica.root().to_owned(),
        delivery: Delivery {
            recipient: delivery.recipient.clone(),
            expected: delivery.expected.clone(),
            candidate: delivery.candidate.clone(),
        },
    };
    let PeerResponse::NeedsTransfer { reference, .. } = crate::serve_peer(request)? else {
        panic!("request transfer");
    };
    let target = pair.replica.root().join("repo.git");
    let target = target.to_str().expect("neutral fixture path");
    let mapping = format!("{}:{reference}", export.reference);
    let output = authority.git().run(
        &[
            "push",
            "--receive-pack=git -c receive.hideRefs=!refs/dots/transfer/ receive-pack",
            "--",
            target,
            &mapping,
        ],
        None,
        crate::MAX_METADATA_BYTES,
    )?;
    assert!(
        output.status.success(),
        "stage native transfer: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let later = pair
        .replica
        .publish(&base, &[put("late.md", "late")])?
        .snapshot;
    assert!(
        matches!(crate::serve_peer(PeerRequest::Accept { root: pair.replica.root().to_owned(), delivery }),
        Err(Error::Conflict { expected, actual }) if expected == base && actual == later)
    );
    assert_eq!(pair.replica.snapshot()?, later);
    let wrong = PeerRequest::Export {
        root: pair.replica.root().to_owned(),
        recipient: authority.identity(),
        snapshot: later.clone(),
    };
    assert!(crate::serve_peer(wrong).is_err());
    assert_eq!(pair.replica.snapshot()?, later);
    Ok(())
}
