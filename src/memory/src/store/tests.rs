use super::*;
use std::fs::Permissions;

type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;

use crate::fixture::Fixture;

fn put(scope: Scope, path: &str, content: &str) -> Change {
    Change {
        scope,
        path: path.into(),
        action: Action::Write {
            content: content.into(),
        },
    }
}

#[test]
fn serialized_changes_require_an_explicit_action() {
    for value in [
        r#"{"scope":{"kind":"global"},"path":"note.md"}"#,
        r#"{"scope":{"kind":"global"},"path":"note.md","action":null}"#,
        r#"{"scope":{"kind":"global"},"path":"note.md","action":{"kind":"write"}}"#,
        r#"{"scope":{"kind":"global"},"path":"note.md","action":{"kind":"write","content":null}}"#,
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
    let remove = serde_json::from_str::<Change>(
        r#"{"scope":{"kind":"global"},"path":"note.md","action":{"kind":"remove"}}"#,
    )
    .expect("explicit removal");
    assert!(matches!(remove.action, Action::Remove {}));
}

#[test]
fn initialized_store_is_private_and_reads_do_not_create_a_lock() -> TestResult {
    let fixture = Fixture::new()?;
    let root = fixture.store.root();
    assert_eq!(fs::metadata(root)?.permissions().mode() & 0o777, 0o700);
    assert_eq!(
        fs::metadata(root.join(LOCAL))?.permissions().mode() & 0o777,
        0o600
    );
    assert_eq!(fixture.store.role(), Role::Authority);
    let opened = Store::open(root)?;
    assert_eq!(opened.id(), fixture.store.id());
    assert_eq!(opened.node_id(), fixture.store.node_id());
    let view = opened.latest()?;
    let absent = view.read(&Scope::Global, Path::new("MEMORY.md"), 1, 200)?;
    assert!(!absent.document.exists);
    assert_eq!(absent.document.revision, "missing");
    assert_eq!(absent.document.path, Path::new("MEMORY.md"));
    assert!(!root.join("mutation.lock").exists());
    assert!(!root.join("repo.git/index").exists());
    Ok(())
}

#[test]
fn publication_is_atomic_and_pinned_reads_remain_at_their_snapshot() -> TestResult {
    let fixture = Fixture::new()?;
    let store = &fixture.store;
    let base = store.snapshot()?;
    let published = store.publish(
        &base,
        &[
            put(Scope::Global, "MEMORY.md", "# Memory\nSee wiki/topic.md\n"),
            put(Scope::Global, "wiki/topic.md", "Complete topic\n"),
        ],
    )?;
    assert_eq!(published.changed, 2);
    assert!(
        !store
            .view(&base)?
            .read(&Scope::Global, Path::new("MEMORY.md"), 1, 200)?
            .document
            .exists
    );
    let view = store.view(&published.snapshot)?;
    assert_eq!(
        view.read(&Scope::Global, Path::new("wiki/topic.md"), 1, 200)?
            .document
            .content,
        "Complete topic\n"
    );
    assert_eq!(
        view.read(&Scope::Global, Path::new("MEMORY.md"), 1, 200)?
            .snapshot,
        published.snapshot
    );
    let no_op = store.publish(
        &published.snapshot,
        &[put(Scope::Global, "wiki/topic.md", "Complete topic\n")],
    )?;
    assert_eq!(no_op.changed, 0);
    assert_eq!(no_op.snapshot, published.snapshot);
    Ok(())
}

#[test]
fn full_snapshot_conflicts_do_not_overwrite_an_unrelated_change() -> TestResult {
    let fixture = Fixture::new()?;
    let base = fixture.store.snapshot()?;
    let published = fixture
        .store
        .publish(&base, &[put(Scope::Global, "one.md", "one")])?;
    let result = fixture
        .store
        .publish(&base, &[put(Scope::Global, "two.md", "two")]);
    assert!(
        matches!(result, Err(Error::Conflict { expected, actual }) if expected == base && actual == published.snapshot)
    );
    assert_eq!(fixture.store.snapshot()?, published.snapshot);
    assert!(
        !fixture
            .store
            .latest()?
            .read(&Scope::Global, Path::new("two.md"), 1, 200)?
            .document
            .exists
    );
    Ok(())
}

#[test]
fn racing_batches_publish_one_complete_snapshot() -> TestResult {
    use std::sync::Barrier;
    use std::thread;
    let fixture = Fixture::new()?;
    let store = &fixture.store;
    let base = store.snapshot()?;
    let gate = Barrier::new(2);
    let results = thread::scope(|scope| {
        let first = scope.spawn(|| {
            gate.wait();
            store.publish(
                &base,
                &[
                    put(Scope::Global, "one.md", "first"),
                    put(Scope::Global, "two.md", "first"),
                ],
            )
        });
        let second = scope.spawn(|| {
            gate.wait();
            store.publish(
                &base,
                &[
                    put(Scope::Global, "one.md", "second"),
                    put(Scope::Global, "two.md", "second"),
                ],
            )
        });
        [
            first.join().expect("first writer did not panic"),
            second.join().expect("second writer did not panic"),
        ]
    });
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    let rejected = results
        .iter()
        .find(|result| result.is_err())
        .expect("one writer is rejected");
    assert!(matches!(
        rejected,
        Err(Error::Conflict { .. } | Error::Documents(dots_documents::Error::Busy))
    ));
    let view = store.latest()?;
    let first = view.read(&Scope::Global, Path::new("one.md"), 1, 20)?;
    let second = view.read(&Scope::Global, Path::new("two.md"), 1, 20)?;
    assert_eq!(first.document.content, second.document.content);
    assert!(matches!(
        first.document.content.as_str(),
        "first\n" | "second\n"
    ));
    Ok(())
}

#[test]
fn empty_and_missing_documents_and_delete_have_distinct_contracts() -> TestResult {
    let fixture = Fixture::new()?;
    let base = fixture.store.snapshot()?;
    let written = fixture
        .store
        .publish(&base, &[put(Scope::Global, "empty.md", "")])?;
    let read = fixture.store.view(&written.snapshot)?.read(
        &Scope::Global,
        Path::new("empty.md"),
        1,
        20,
    )?;
    assert!(read.document.exists);
    assert_ne!(read.document.revision, "missing");
    assert_eq!(read.document.content, "");
    let removed = fixture.store.publish(
        &written.snapshot,
        &[Change {
            scope: Scope::Global,
            path: "empty.md".into(),
            action: Action::Remove {},
        }],
    )?;
    assert!(
        !fixture
            .store
            .view(&removed.snapshot)?
            .read(&Scope::Global, Path::new("empty.md"), 1, 20)?
            .document
            .exists
    );
    assert!(
        fixture
            .store
            .view(&written.snapshot)?
            .read(&Scope::Global, Path::new("empty.md"), 1, 20)?
            .document
            .exists
    );
    Ok(())
}

#[test]
fn exact_content_keeps_unicode_line_endings_and_unterminated_lines() -> TestResult {
    let fixture = Fixture::new()?;
    let content = "café\r\nlast line";
    let base = fixture.store.snapshot()?;
    let publication = fixture
        .store
        .publish(&base, &[put(Scope::Global, "wiki/café.md", content)])?;
    let view = fixture.store.view(&publication.snapshot)?;
    assert_eq!(
        view.content(&Scope::Global, Path::new("wiki/café.md"))?
            .as_deref(),
        Some(content)
    );
    assert_eq!(
        view.read(&Scope::Global, Path::new("wiki/café.md"), 1, 20)?
            .document
            .revision,
        dots_documents::text::revision(Some(content))
    );
    assert_eq!(view.content(&Scope::Global, Path::new("absent.md"))?, None);
    Ok(())
}

#[test]
fn project_identity_is_explicit_and_display_names_do_not_select_it() -> TestResult {
    let fixture = Fixture::new()?;
    let base = fixture.store.snapshot()?;
    let registration = fixture.store.register_project(&base, "sample-project")?;
    let other = fixture
        .store
        .register_project(&registration.snapshot, "sample-project")?;
    assert_ne!(registration.project, other.project);
    let scope = Scope::Project(registration.project.clone());
    let written = fixture.store.publish(
        &other.snapshot,
        &[put(scope.clone(), "MEMORY.md", "project note\n")],
    )?;
    let view = fixture.store.view(&written.snapshot)?;
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
    assert!(
        !view
            .read(&Scope::Global, Path::new("MEMORY.md"), 1, 20)?
            .document
            .exists
    );
    let unknown = ProjectId::allocate()?;
    assert!(
        matches!(view.read(&Scope::Project(unknown.clone()), Path::new("MEMORY.md"), 1, 20), Err(Error::UnknownProject(id)) if id == unknown)
    );
    assert!(matches!(
        fixture
            .store
            .view(&base)?
            .read(&scope, Path::new("MEMORY.md"), 1, 20),
        Err(Error::UnknownProject(_))
    ));
    Ok(())
}

#[test]
fn literal_search_is_scoped_bounded_and_uses_relative_resources() -> TestResult {
    let fixture = Fixture::new()?;
    let base = fixture.store.snapshot()?;
    let project = fixture.store.register_project(&base, "sample")?;
    let published = fixture.store.publish(
        &project.snapshot,
        &[
            put(Scope::Global, "a.md", "First Needle\nSecond needle\n"),
            put(Scope::Global, "log.md", "needle excluded\n"),
            put(
                Scope::Project(project.project.clone()),
                "a.md",
                "needle private to project\n",
            ),
        ],
    )?;
    let view = fixture.store.view(&published.snapshot)?;
    let first = view.search(&Scope::Global, "NEEDLE", 0, 1)?;
    assert_eq!(first.snapshot, published.snapshot);
    assert_eq!(first.result.hits.len(), 1);
    assert_eq!(first.result.hits[0].path, Path::new("a.md"));
    assert_eq!(first.result.hits[0].line, 1);
    assert_eq!(first.result.next_offset, Some(1));
    let last = view.search(&Scope::Global, "needle", 1, 1)?;
    assert_eq!(last.result.hits[0].line, 2);
    assert!(!last.result.has_more);
    let scoped = view.search(&Scope::Project(project.project), "needle", 0, 20)?;
    assert_eq!(scoped.result.hits.len(), 1);
    assert_eq!(scoped.result.hits[0].excerpt, "needle private to project");
    Ok(())
}

#[test]
fn invalid_batch_cannot_advance_the_published_reference() -> TestResult {
    let fixture = Fixture::new()?;
    let base = fixture.store.snapshot()?;
    let huge = "x".repeat(MAX_FILE_BYTES + 1);
    let objects = fixture.store.git.text(&["count-objects", "-v"], None)?;
    for changes in [
        vec![
            put(Scope::Global, "valid.md", "valid"),
            put(Scope::Global, "../escape.md", "bad"),
        ],
        vec![
            put(Scope::Global, "valid.md", "valid"),
            put(Scope::Global, "large.md", &huge),
        ],
        vec![
            put(Scope::Global, "same.md", "one"),
            put(Scope::Global, "same.md", "two"),
        ],
        vec![
            put(Scope::Global, "Case.md", "one"),
            put(Scope::Global, "case.md", "two"),
        ],
        vec![
            put(Scope::Global, "Wiki/one.md", "one"),
            put(Scope::Global, "wiki/two.md", "two"),
        ],
        vec![put(Scope::Global, "binary.md", "nul\0byte")],
        vec![
            put(Scope::Global, "file.md", "one"),
            put(Scope::Global, "file.md/child.md", "two"),
        ],
    ] {
        assert!(fixture.store.publish(&base, &changes).is_err());
        assert_eq!(fixture.store.snapshot()?, base);
        assert_eq!(
            fixture.store.git.text(&["count-objects", "-v"], None)?,
            objects
        );
    }
    Ok(())
}

#[test]
fn corpus_and_metadata_budgets_fail_before_writing_objects() -> TestResult {
    let fixture = Fixture::new()?;
    let base = fixture.store.snapshot()?;
    let objects = fixture.store.git.text(&["count-objects", "-v"], None)?;
    let content = "x".repeat(MAX_FILE_BYTES);
    let changes: Vec<_> = (0..=(MAX_CORPUS_BYTES / MAX_FILE_BYTES))
        .map(|index| put(Scope::Global, &format!("document-{index}.md"), &content))
        .collect();
    assert!(fixture.store.publish(&base, &changes).is_err());
    assert_eq!(fixture.store.snapshot()?, base);
    assert_eq!(
        fixture.store.git.text(&["count-objects", "-v"], None)?,
        objects
    );
    let prefix = format!("{}/", "a".repeat(240)).repeat(7);
    let changes: Vec<_> = (0..MAX_DOCUMENTS - 1)
        .map(|index| put(Scope::Global, &format!("{prefix}document-{index}.md"), ""))
        .collect();
    assert!(fixture.store.publish(&base, &changes).is_err());
    assert_eq!(fixture.store.snapshot()?, base);
    assert_eq!(
        fixture.store.git.text(&["count-objects", "-v"], None)?,
        objects
    );
    Ok(())
}

#[test]
fn unpublished_candidate_cannot_be_read_as_accepted_knowledge() -> TestResult {
    let fixture = Fixture::new()?;
    let base = fixture.store.snapshot()?;
    let entries = fixture.store.latest()?.entries.clone();
    let tree = fixture.store.git.tree(&entries)?;
    let candidate = fixture
        .store
        .git
        .commit(&tree, &[&base], "Unpublished fixture")?;
    assert!(fixture.store.view(&candidate).is_err());
    assert_eq!(fixture.store.snapshot()?, base);
    Ok(())
}

#[test]
fn unexpected_git_configuration_and_public_root_permissions_are_refused() -> TestResult {
    let fixture = Fixture::new()?;
    fixture.store.git.text(
        &["config", "url.https://example.invalid/.insteadOf", "/"],
        None,
    )?;
    assert!(Store::open(fixture.store.root()).is_err());
    let other = Fixture::new()?;
    fs::set_permissions(other.store.root(), Permissions::from_mode(0o755))?;
    assert!(Store::open(other.store.root()).is_err());
    fs::set_permissions(other.store.root(), Permissions::from_mode(0o700))?;
    assert!(Store::open(other.store.root()).is_ok());
    Ok(())
}

#[test]
fn relocation_preserves_identity_but_cannot_move_knowledge_into_a_checkout() -> TestResult {
    let fixture = Fixture::new()?;
    let base = fixture.store.snapshot()?;
    let destination = fixture.parent.join("relocated");
    fs::rename(fixture.store.root(), &destination)?;
    let opened = Store::open(&destination)?;
    assert_eq!(opened.id(), fixture.store.id());
    assert_eq!(opened.node_id(), fixture.store.node_id());
    assert_eq!(opened.snapshot()?, base);
    let checkout = fixture.parent.join("checkout");
    fs::create_dir(&checkout)?;
    fs::write(checkout.join(".git"), "gitdir: neutral-fixture")?;
    let invalid = checkout.join("knowledge");
    fs::rename(&destination, &invalid)?;
    assert!(Store::open(&invalid).is_err());
    Ok(())
}

#[test]
fn initialization_refuses_existing_or_checkout_destinations_without_adoption() -> TestResult {
    use std::os::unix::fs::DirBuilderExt as _;
    let fixture = Fixture::new()?;
    let existing = fixture.parent.join("existing");
    fs::DirBuilder::new().mode(0o700).create(&existing)?;
    fs::write(existing.join("keep"), "unchanged")?;
    assert!(Store::initialize(&existing, "other").is_err());
    assert_eq!(fs::read_to_string(existing.join("keep"))?, "unchanged");
    let checkout = fixture.parent.join("checkout");
    fs::create_dir(&checkout)?;
    fs::write(checkout.join(".git"), "gitdir: neutral-fixture")?;
    assert!(Store::initialize(&checkout.join("knowledge"), "other").is_err());
    assert!(!checkout.join("knowledge").exists());
    Ok(())
}

#[test]
fn native_push_cannot_bypass_owned_reference_validation() -> TestResult {
    let fixture = Fixture::new()?;
    let base = fixture.store.snapshot()?;
    let tree = fixture.store.git.tree(&fixture.store.latest()?.entries)?;
    let candidate = fixture
        .store
        .git
        .commit(&tree, &[&base], "Transport fixture")?;
    let remote = fixture.store.root().join("repo.git");
    let remote = remote.to_str().expect("neutral fixture path is Unicode");
    let staged = format!("{candidate}:refs/dots/incoming/test");
    let output =
        fixture
            .store
            .git
            .run(&["push", "--", remote, &staged], None, MAX_METADATA_BYTES)?;
    assert!(
        !output.status.success(),
        "raw push must not create candidate-cache references: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let published = format!("{candidate}:{}", crate::git::HEAD);
    let output = fixture.store.git.run(
        &["push", "--", remote, &published],
        None,
        MAX_METADATA_BYTES,
    )?;
    assert!(
        !output.status.success(),
        "raw push must not update the published branch"
    );
    assert_eq!(fixture.store.snapshot()?, base);
    fixture
        .store
        .git
        .text(&["config", "--unset", "receive.hiderefs"], None)?;
    assert!(Store::open(fixture.store.root()).is_err());
    let output = fixture.store.git.run(
        &["push", "--", remote, &published],
        None,
        MAX_METADATA_BYTES,
    )?;
    assert!(
        output.status.success(),
        "unprotected positive control: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    fixture
        .store
        .git
        .text(&["config", "receive.hiderefs", "refs/"], None)?;
    assert_eq!(Store::open(fixture.store.root())?.snapshot()?, candidate);
    Ok(())
}

#[test]
fn symbolic_publication_refs_and_broken_storage_indirections_are_rejected() -> TestResult {
    use std::os::unix::fs::symlink;
    let fixture = Fixture::new()?;
    let head = fixture.store.snapshot()?;
    fixture
        .store
        .git
        .text(&["update-ref", "refs/heads/other", head.as_str()], None)?;
    fixture.store.git.text(
        &["symbolic-ref", crate::git::HEAD, "refs/heads/other"],
        None,
    )?;
    assert!(Store::open(fixture.store.root()).is_err());
    let other = Fixture::new()?;
    symlink(
        "missing",
        other.store.root().join("repo.git/objects/info/alternates"),
    )?;
    assert!(Store::open(other.store.root()).is_err());
    let third = Fixture::new()?;
    let config = third.store.root().join("repo.git/config");
    fs::rename(&config, config.with_extension("saved"))?;
    symlink("config.saved", &config)?;
    assert!(Store::open(third.store.root()).is_err());
    Ok(())
}

#[test]
fn canonical_parent_checks_prevent_aliases_into_another_repository() -> TestResult {
    use std::os::unix::fs::symlink;
    let fixture = Fixture::new()?;
    let checkout = fixture.parent.join("checkout");
    fs::create_dir(&checkout)?;
    fs::create_dir(checkout.join("subdir"))?;
    fs::write(checkout.join(".git"), "gitdir: neutral-fixture")?;
    let alias = fixture.parent.join("alias");
    symlink(checkout.join("subdir"), &alias)?;
    assert!(Store::initialize(&alias.join("knowledge"), "other").is_err());
    assert!(!checkout.join("subdir/knowledge").exists());
    assert!(Store::initialize(&fixture.store.root().join("repo.git/nested"), "other").is_err());
    Ok(())
}

#[test]
fn malformed_published_manifest_fails_instead_of_falling_back() -> TestResult {
    let fixture = Fixture::new()?;
    let base = fixture.store.snapshot()?;
    let mut entries = fixture.store.latest()?.entries;
    let content = format!("{{\"store\":\"{}\",\"projects\":{{}}}}", fixture.store.id());
    let oid = fixture.store.git.write_blob(content.as_bytes())?;
    entries.insert(
        MANIFEST.into(),
        Entry {
            oid,
            size: content.len(),
        },
    );
    let tree = fixture.store.git.tree(&entries)?;
    let invalid = fixture
        .store
        .git
        .commit(&tree, &[&base], "Noncanonical fixture")?;
    fixture.store.git.publish(Some(&base), &invalid)?;
    assert!(fixture.store.latest().is_err());
    assert!(Store::open(fixture.store.root()).is_err());
    assert_eq!(fixture.store.view(&base)?.snapshot(), &base);
    Ok(())
}
