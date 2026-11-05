use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use dots_process::Runner;
use std::time::Duration;

use super::{Action, Change, Role, Scope, Store};
use crate::fixture::Fixture;
use crate::git::{Entry, Git};
use crate::{BundleReceipt, MAX_FILE_BYTES, MAX_METADATA_BYTES, Snapshot};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn raw_bundle(
    store: &Store,
    snapshot: &Snapshot,
    destination: &Path,
) -> crate::Result<BundleReceipt> {
    let reference = format!("refs/dots/export/{snapshot}");
    store
        .git
        .text(&["update-ref", &reference, snapshot.as_str()], None)?;
    let mut command = store.git.command(&["bundle", "create"])?;
    command.arg(destination).arg(&reference);
    Git::checked(
        Git::execute(
            command,
            "create neutral bundle fixture",
            Runner::new(Duration::from_secs(15), MAX_METADATA_BYTES),
        )?,
        "create neutral bundle fixture",
    )?;
    let bytes = dots_documents::text::read_bytes(destination, crate::MAX_BUNDLE_BYTES)?
        .expect("fixture bundle exists");
    Ok(BundleReceipt {
        source: store.identity(),
        snapshot: snapshot.clone(),
        digest: dots_documents::text::bytes_revision(&bytes),
    })
}

#[test]
fn full_bundle_restores_shared_identity_with_a_distinct_local_node() -> TestResult {
    let fixture = Fixture::new()?;
    let base = fixture.store.snapshot()?;
    let registered = fixture.store.register_project(&base, "sample")?;
    let published = fixture.store.publish(
        &registered.snapshot,
        &[Change {
            scope: Scope::Project(registered.project.clone()),
            path: "MEMORY.md".into(),
            action: Action::Write {
                content: "project knowledge\n".into(),
            },
        }],
    )?;
    let bundle = fixture.parent.join("knowledge.bundle");
    let receipt = fixture.store.export_bundle(&published.snapshot, &bundle)?;
    let replica = Store::restore(
        &fixture.parent.join("replica"),
        "replica",
        Role::Replica,
        &bundle,
        &receipt,
    )?;
    assert_eq!(replica.id(), fixture.store.id());
    assert_ne!(replica.node_id(), fixture.store.node_id());
    assert_eq!(replica.role(), Role::Replica);
    assert_eq!(replica.snapshot()?, published.snapshot);
    assert_eq!(
        replica
            .latest()?
            .content(&Scope::Project(registered.project), Path::new("MEMORY.md"))?
            .as_deref(),
        Some("project knowledge\n")
    );
    assert!(
        replica
            .register_project(&published.snapshot, "not-authorized")
            .is_err()
    );
    assert!(
        fixture
            .store
            .export_bundle(&published.snapshot, &bundle)
            .is_err(),
        "never overwrite an existing export"
    );
    Ok(())
}

#[test]
fn retained_candidate_survives_import_without_becoming_published() -> TestResult {
    let fixture = Fixture::new()?;
    let base = fixture.store.snapshot()?;
    let seed = fixture.parent.join("seed.bundle");
    let seed_receipt = fixture.store.export_bundle(&base, &seed)?;
    let replica = Store::restore(
        &fixture.parent.join("replica"),
        "replica",
        Role::Replica,
        &seed,
        &seed_receipt,
    )?;
    let published = fixture.store.publish(
        &base,
        &[Change {
            scope: Scope::Global,
            path: "MEMORY.md".into(),
            action: Action::Write {
                content: "new knowledge\n".into(),
            },
        }],
    )?;
    let bundle = fixture.parent.join("later.bundle");
    let receipt = fixture.store.export_bundle(&published.snapshot, &bundle)?;
    let candidate = replica.import_bundle(&bundle, &receipt)?;
    assert_eq!(candidate.snapshot, published.snapshot);
    assert_eq!(replica.snapshot()?, base);
    assert!(replica.view(&candidate.snapshot).is_err());
    assert!(
        !replica
            .latest()?
            .read(&Scope::Global, Path::new("MEMORY.md"), 1, 20)?
            .document
            .exists
    );
    assert_eq!(
        replica.git.reference(&candidate.reference())?,
        Some(published.snapshot.clone())
    );
    let reopened = Store::open(replica.root())?;
    assert_eq!(
        reopened.git.reference(&candidate.reference())?,
        Some(published.snapshot)
    );
    assert_eq!(
        reopened.import_bundle(&bundle, &receipt)?.snapshot,
        candidate.snapshot
    );
    assert_eq!(reopened.snapshot()?, base);
    let leftovers: Vec<_> = fs::read_dir(replica.root())?
        .filter_map(Result::ok)
        .filter(|entry| entry.file_name().to_string_lossy().starts_with(".staged"))
        .collect();
    assert!(leftovers.is_empty());
    Ok(())
}

#[test]
fn corrupt_or_wrong_receipts_and_failed_restores_preserve_existing_state() -> TestResult {
    let fixture = Fixture::new()?;
    let base = fixture.store.snapshot()?;
    let bundle = fixture.parent.join("knowledge.bundle");
    let receipt = fixture.store.export_bundle(&base, &bundle)?;
    let objects = fixture.store.git.text(&["count-objects", "-v"], None)?;
    let mut wrong = receipt.clone();
    wrong.digest = format!("sha256:{}", "0".repeat(64));
    assert!(fixture.store.import_bundle(&bundle, &wrong).is_err());
    let mut wrong = receipt.clone();
    wrong.source.store = crate::StoreId::allocate()?;
    assert!(fixture.store.import_bundle(&bundle, &wrong).is_err());
    let broken = fixture.parent.join("truncated.bundle");
    let bytes = fs::read(&bundle)?;
    fs::write(&broken, &bytes[..bytes.len() / 2])?;
    assert!(fixture.store.import_bundle(&broken, &receipt).is_err());
    let destination = fixture.parent.join("failed-restore");
    assert!(Store::restore(&destination, "replica", Role::Replica, &broken, &receipt).is_err());
    assert!(!destination.exists());
    assert_eq!(fixture.store.snapshot()?, base);
    assert_eq!(
        fixture.store.git.text(&["count-objects", "-v"], None)?,
        objects
    );
    Ok(())
}

#[test]
fn a_clean_tip_cannot_hide_invalid_ancestor_files_modes_or_content() -> TestResult {
    let fixture = Fixture::new()?;
    let base = fixture.store.snapshot()?;
    let seed = fixture.parent.join("seed.bundle");
    let seed_receipt = fixture.store.export_bundle(&base, &seed)?;
    let replica = Store::restore(
        &fixture.parent.join("replica"),
        "replica",
        Role::Replica,
        &seed,
        &seed_receipt,
    )?;
    let original = fixture.store.latest()?.entries;
    let valid_tree = fixture.store.git.tree(&original)?;
    let ordinary = fixture.store.git.write_blob(b"neutral content")?;
    let invalid_utf8 = fixture.store.git.write_blob(&[0xff])?;
    let oversized = fixture
        .store
        .git
        .write_blob(&vec![b'x'; MAX_FILE_BYTES + 1])?;
    let empty_tree = fixture.store.git.tree(&BTreeMap::new())?;
    let manifest = &original[super::MANIFEST].oid;
    for (index, (mode, kind, oid, path)) in [
        ("100644", "blob", ordinary.as_str(), "private.txt"),
        ("120000", "blob", ordinary.as_str(), "global/link.md"),
        ("100644", "blob", invalid_utf8.as_str(), "global/binary.md"),
        ("100644", "blob", oversized.as_str(), "global/large.md"),
        ("040000", "tree", empty_tree.as_str(), "empty"),
    ]
    .into_iter()
    .enumerate()
    {
        let input =
            format!("100644 blob {manifest}\tknowledge.json\0{mode} {kind} {oid}\t{path}\0")
                .into_bytes();
        let tree = if path.contains('/') {
            let mut entries = original.clone();
            entries.insert(
                path.into(),
                Entry {
                    oid: oid.into(),
                    size: if index == 3 { MAX_FILE_BYTES + 1 } else { 1 },
                },
            );
            if mode == "100644" {
                fixture.store.git.tree(&entries)?
            } else {
                let inner = fixture.store.git.text(
                    &["mktree", "-z"],
                    Some(format!("{mode} {kind} {oid}\tlink.md\0").into_bytes()),
                )?;
                fixture
                    .store
                    .git
                    .text(
                        &["mktree", "-z"],
                        Some(
                            format!(
                                "100644 blob {manifest}\tknowledge.json\0040000 tree {}\tglobal\0",
                                inner.trim()
                            )
                            .into_bytes(),
                        ),
                    )?
                    .trim()
                    .into()
            }
        } else {
            fixture
                .store
                .git
                .text(&["mktree", "-z"], Some(input))?
                .trim()
                .into()
        };
        let bad = fixture
            .store
            .git
            .commit(&tree, &[&base], "Invalid ancestor fixture")?;
        let clean = fixture
            .store
            .git
            .commit(&valid_tree, &[&bad], "Clean tip fixture")?;
        let bundle = fixture.parent.join(format!("invalid-{index}.bundle"));
        let receipt = raw_bundle(&fixture.store, &clean, &bundle)?;
        let before = replica.git.text(&["count-objects", "-v"], None)?;
        assert!(replica.import_bundle(&bundle, &receipt).is_err(), "{path}");
        assert_eq!(replica.snapshot()?, base, "{path}");
        assert_eq!(
            replica.git.text(&["count-objects", "-v"], None)?,
            before,
            "{path}"
        );
    }
    Ok(())
}
