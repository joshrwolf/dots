use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use super::{OutputContent, PublishMode, PublishOutput};
use crate::outcomes::{Evidence, Operation, Outcome};
use crate::{Actor, NewTask, Owner, Project, Store};

mod acceptance;

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Fixture {
    root: PathBuf,
    store: Store,
    project: Project,
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "dots-output-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        let root = fs::canonicalize(root).unwrap();
        let mut store = Store::create(&root.join("workspace.sqlite3")).unwrap();
        let project = store.register_project("fixture").unwrap();
        Self {
            root,
            store,
            project,
        }
    }
    fn source(&self, content: &str) -> PathBuf {
        let path = self.root.join("draft.md");
        fs::write(&path, content).unwrap();
        path
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}

fn actor() -> Actor<'static> {
    Actor {
        name: Some(Owner::try_new("parent").unwrap()),
        now: 100,
    }
}
fn input(source: &std::path::Path) -> PublishOutput<'_> {
    PublishOutput {
        slug: "report",
        filename: "report.md",
        media_type: "text/markdown",
        source,
        mode: PublishMode::Create,
        description: "report",
        provenance: "neutral fixture",
        task: None,
    }
}

#[test]
fn outcomes_pin_bytes_chain_results_and_reject_unavailable_evidence() {
    let mut fixture = Fixture::new();
    let source = fixture.source("reviewed bytes");
    let published = fixture
        .store
        .publish_output(&fixture.project, &input(&source), actor())
        .unwrap();
    let task = fixture
        .store
        .add(
            &fixture.project,
            &NewTask {
                title: "Accept exact bytes".into(),
                ..NewTask::default()
            },
            actor(),
        )
        .unwrap();
    let evidence = [Evidence {
        slug: "report".into(),
        digest: published.revision.digest.clone(),
    }];
    let request = Outcome {
        operation: Operation::Done,
        result: "Accepted",
        evidence: &evidence,
        force: false,
    };
    let first = fixture
        .store
        .write_result(&fixture.project, &task.id, &request, actor())
        .unwrap();
    assert_eq!(first.task.evidence, evidence);
    assert_eq!(first.task.result_event, Some(first.event.seq));
    assert_eq!(first.event.predecessor, None);
    fixture
        .store
        .set_status(
            &fixture.project,
            &task.id,
            crate::Status::Open,
            Some("new requirement"),
            actor(),
            false,
        )
        .unwrap();
    let second = fixture
        .store
        .write_result(
            &fixture.project,
            &task.id,
            &Outcome {
                operation: Operation::UpdateResult,
                result: "Result only",
                evidence: &[],
                force: false,
            },
            actor(),
        )
        .unwrap();
    assert_eq!(second.event.predecessor, Some(first.event.seq));
    assert_eq!(second.task.status, crate::Status::Open);
    assert_eq!(second.task.evidence, []);
    fs::remove_file(fixture.root.join(&published.revision.path)).unwrap();
    assert!(
        fixture
            .store
            .write_result(&fixture.project, &task.id, &request, actor())
            .is_err()
    );
    let current = fixture.store.get(&fixture.project, &task.id).unwrap();
    assert_eq!(current.result_event, Some(second.event.seq));
    assert_eq!(current.status, crate::Status::Open);
    assert_eq!(
        fixture
            .store
            .result_event(&fixture.project, &task.id, first.event.seq)
            .unwrap()
            .evidence,
        evidence
    );
}

#[test]
fn outcome_failure_rolls_back_status_event_result_and_associations() {
    let mut fixture = Fixture::new();
    let source = fixture.source("report bytes");
    let published = fixture
        .store
        .publish_output(&fixture.project, &input(&source), actor())
        .unwrap();
    let task = fixture
        .store
        .add(
            &fixture.project,
            &NewTask {
                title: "Atomic acceptance".into(),
                ..NewTask::default()
            },
            actor(),
        )
        .unwrap();
    let evidence = [Evidence {
        slug: "report".into(),
        digest: published.revision.digest,
    }];
    let request = Outcome {
        operation: Operation::Review,
        result: "ready",
        evidence: &evidence,
        force: false,
    };
    fixture.store.connection.execute_batch("CREATE TRIGGER fail_evidence BEFORE INSERT ON event_evidence BEGIN SELECT RAISE(ABORT,'injected evidence failure'); END;").unwrap();
    assert!(
        fixture
            .store
            .write_result(&fixture.project, &task.id, &request, actor())
            .is_err()
    );
    let task = fixture.store.get(&fixture.project, &task.id).unwrap();
    assert_eq!(task.status, crate::Status::Open);
    assert!(task.result.is_none());
    assert!(task.result_event.is_none());
    assert_eq!(task.evidence, []);
    fixture
        .store
        .connection
        .execute_batch("DROP TRIGGER fail_evidence;")
        .unwrap();
    let mut duplicate = evidence.to_vec();
    duplicate.extend_from_slice(&evidence);
    let request = Outcome {
        evidence: &duplicate,
        ..request
    };
    let receipt = fixture
        .store
        .write_result(&fixture.project, &task.id, &request, actor())
        .unwrap();
    assert_eq!(receipt.event.evidence, evidence);
}

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "one interrupted purge scenario verifies retained and excluded byte classes"
)]
fn purge_removes_duplicate_orphan_and_staging_bytes_but_preserves_catalog_aliases() {
    let mut fixture = Fixture::new();
    let source = fixture.source("selected bytes");
    let first = fixture
        .store
        .publish_output(&fixture.project, &input(&source), actor())
        .unwrap();
    let mut second = input(&source);
    second.slug = "another";
    fixture
        .store
        .publish_output(&fixture.project, &second, actor())
        .unwrap();
    let directory = fixture
        .root
        .join(&first.revision.path)
        .parent()
        .unwrap()
        .to_owned();
    let staged = directory.join(".staged-interrupted");
    fs::write(&staged, "selected bytes").unwrap();
    let orphan = directory.join("orphan.txt");
    fs::write(&orphan, "selected bytes").unwrap();
    let other = directory.join("partial.txt");
    fs::write(&other, "unrelated partial").unwrap();
    let alias = fixture.root.join(".staged-catalog-alias");
    fs::hard_link(fixture.root.join("workspace.sqlite3"), &alias).unwrap();
    let before = fixture.store.output_integrity(&fixture.project).unwrap();
    assert!(
        before
            .issues
            .iter()
            .any(|issue| issue.state == "catalog_staging_remnant")
    );
    assert!(
        before
            .issues
            .iter()
            .any(|issue| issue.state == "staged_output")
    );
    let purged = fixture
        .store
        .purge_output_digest(&fixture.project, &first.revision.digest, actor())
        .unwrap();
    assert!(purged.tombstoned && purged.removal_complete);
    assert_eq!(purged.removed.len(), 4);
    assert!(alias.exists());
    assert!(other.exists());
    assert!(source.exists());
    assert!(!staged.exists());
    assert!(!orphan.exists());
    let again = fixture
        .store
        .purge_output_digest(&fixture.project, &first.revision.digest, actor())
        .unwrap();
    assert!(again.tombstoned && again.removal_complete);
    assert_eq!(again.removed, Vec::<PathBuf>::new());
    assert!(matches!(
        fixture
            .store
            .read_output(&fixture.project, "report", &first.revision.digest, 1, 200)
            .unwrap()
            .content,
        OutputContent::Purged
    ));
    assert!(
        fixture
            .store
            .publish_output(&fixture.project, &input(&source), actor())
            .is_err()
    );
    // Model a crash after tombstone commit but before unlink: retry still removes bytes.
    fs::write(&orphan, "selected bytes").unwrap();
    assert!(
        fixture
            .store
            .output_integrity(&fixture.project)
            .unwrap()
            .issues
            .iter()
            .any(|issue| issue.path == orphan && issue.state == "purged_bytes_pending_removal")
    );
    assert_eq!(
        fixture
            .store
            .purge_output_digest(&fixture.project, &first.revision.digest, actor())
            .unwrap()
            .removed,
        vec![orphan]
    );
    let partial = directory.join(".staged-partial");
    fs::write(&partial, "selected").unwrap();
    let incomplete = fixture
        .store
        .purge_output_digest(&fixture.project, &first.revision.digest, actor())
        .unwrap();
    assert!(!incomplete.removal_complete);
    assert!(
        incomplete
            .issues
            .iter()
            .any(|issue| issue.path == partial && issue.state == "unverified_staging")
    );
    assert_eq!(fs::read_to_string(&partial).unwrap(), "selected");
    fs::remove_file(partial).unwrap();
    fs::write(
        directory.join("large-unrelated.txt"),
        "x".repeat(super::MAX_OUTPUT_BYTES + 1),
    )
    .unwrap();
    let complete = fixture
        .store
        .purge_output_digest(&fixture.project, &first.revision.digest, actor())
        .unwrap();
    assert!(complete.removal_complete);
    assert!(
        complete
            .issues
            .iter()
            .any(|issue| issue.state == "outside_payload_limit")
    );
}

#[test]
fn offline_snapshot_restores_task_identity_events_evidence_and_current_documents() {
    let mut fixture = Fixture::new();
    let source = fixture.source("durable evidence");
    let published = fixture
        .store
        .publish_output(&fixture.project, &input(&source), actor())
        .unwrap();
    let task = fixture
        .store
        .add(
            &fixture.project,
            &NewTask {
                title: "Keep exact evidence".into(),
                ..NewTask::default()
            },
            actor(),
        )
        .unwrap();
    let selected = [Evidence {
        slug: "report".into(),
        digest: published.revision.digest.clone(),
    }];
    let request = Outcome {
        operation: Operation::Done,
        result: "Verified",
        evidence: &selected,
        force: false,
    };
    let accepted = fixture
        .store
        .write_result(&fixture.project, &task.id, &request, actor())
        .unwrap();
    let knowledge = fixture.root.join(format!(
        "workspaces/{}/knowledge/MEMORY.md",
        fixture.project.as_str()
    ));
    fs::create_dir_all(knowledge.parent().unwrap()).unwrap();
    fs::write(&knowledge, "Current guidance\n").unwrap();
    // No other writers exist. SQLite creates the consistent catalog snapshot;
    // copy managed files only after that, retaining ordinary document bytes.
    let backup = fixture.root.join("backup");
    fs::create_dir(&backup).unwrap();
    fixture
        .store
        .connection
        .execute(
            "VACUUM INTO ?1",
            [backup.join("workspace.sqlite3").to_str().unwrap()],
        )
        .unwrap();
    copy_tree(&fixture.root.join("workspaces"), &backup.join("workspaces"));
    let mut restored = Store::open(&backup.join("workspace.sqlite3")).unwrap();
    let loaded = restored.get(&fixture.project, &task.id).unwrap();
    assert_eq!(loaded.id, task.id);
    assert_eq!(loaded.result_event, Some(accepted.event.seq));
    assert_eq!(loaded.evidence, selected);
    assert_eq!(
        restored
            .result_event(&fixture.project, &task.id, accepted.event.seq)
            .unwrap()
            .result,
        "Verified"
    );
    assert!(
        matches!(restored.read_output(&fixture.project,"report",&published.revision.digest,1,200).unwrap().content,OutputContent::Available {document} if document.content=="durable evidence\n")
    );
    assert_eq!(
        fs::read_to_string(backup.join(knowledge.strip_prefix(&fixture.root).unwrap())).unwrap(),
        "Current guidance\n"
    );
    assert_eq!(
        restored
            .output_integrity(&fixture.project)
            .unwrap()
            .issues
            .len(),
        0
    );
}

fn copy_tree(source: &std::path::Path, destination: &std::path::Path) {
    fs::create_dir(destination).unwrap();
    for entry in fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let target = destination.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).unwrap();
        }
    }
}

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "one claim tests both association changes and no-op retries"
)]
fn association_changes_respect_claims_but_exact_noops_do_not() {
    use crate::output_discovery::Association;
    let mut fixture = Fixture::new();
    let source = fixture.source("report");
    let task = fixture
        .store
        .add(
            &fixture.project,
            &NewTask {
                title: "Claimed".into(),
                ..NewTask::default()
            },
            actor(),
        )
        .unwrap();
    fixture
        .store
        .claim(
            &fixture.project,
            &task.id,
            actor().name.unwrap(),
            false,
            100,
        )
        .unwrap();
    let other = Actor {
        name: Some(Owner::try_new("other").unwrap()),
        now: 101,
    };
    let mut request = input(&source);
    request.task = Some(&task.id);
    assert!(matches!(
        fixture
            .store
            .publish_output(&fixture.project, &request, other),
        Err(crate::Error::OwnedByAnother { .. })
    ));
    fixture
        .store
        .publish_output(&fixture.project, &request, actor())
        .unwrap();
    let events = fixture
        .store
        .events(&fixture.project, &task.id, 100)
        .unwrap()
        .len();
    assert!(
        !fixture
            .store
            .publish_output(&fixture.project, &request, other)
            .unwrap()
            .created
    );
    assert!(
        !fixture
            .store
            .associate_output(
                &fixture.project,
                &task.id,
                "report",
                Association::Attach,
                other
            )
            .unwrap()
    );
    assert_eq!(
        fixture
            .store
            .events(&fixture.project, &task.id, 100)
            .unwrap()
            .len(),
        events
    );
    assert!(matches!(
        fixture.store.associate_output(
            &fixture.project,
            &task.id,
            "report",
            Association::Detach,
            other
        ),
        Err(crate::Error::OwnedByAnother { .. })
    ));
    assert!(
        fixture
            .store
            .associate_output(
                &fixture.project,
                &task.id,
                "report",
                Association::Detach,
                actor()
            )
            .unwrap()
    );
    assert!(
        !fixture
            .store
            .associate_output(
                &fixture.project,
                &task.id,
                "report",
                Association::Detach,
                other
            )
            .unwrap()
    );
}

#[test]
fn integrity_reports_catalog_and_revision_errors_without_hiding_other_issues() {
    let mut fixture = Fixture::new();
    let source = fixture.source("evidence");
    fixture
        .store
        .publish_output(&fixture.project, &input(&source), actor())
        .unwrap();
    fixture
        .store
        .connection
        .execute("UPDATE output_revisions SET digest='malformed'", [])
        .unwrap();
    fixture.store.connection.execute_batch("PRAGMA foreign_keys=OFF; INSERT INTO task_outputs VALUES('t-absent','p-absent','absent'); PRAGMA foreign_keys=ON; CREATE TABLE integrity_fixture(value INTEGER CHECK(value>0)); PRAGMA ignore_check_constraints=ON; INSERT INTO integrity_fixture VALUES(0); PRAGMA ignore_check_constraints=OFF;").unwrap();
    let report = fixture.store.output_integrity(&fixture.project).unwrap();
    for state in [
        "catalog_revision_error",
        "catalog_foreign_key_error",
        "catalog_integrity_error",
    ] {
        assert!(
            report.issues.iter().any(|issue| issue.state == state),
            "missing {state}: {report:?}"
        );
    }
}

#[test]
fn publication_retry_and_a_b_a_have_no_head() {
    let mut fixture = Fixture::new();
    let source = fixture.source("first\n");
    let mut request = input(&source);
    let first = fixture
        .store
        .publish_output(&fixture.project, &request, actor())
        .unwrap();
    assert!(first.created);
    let later = fixture
        .store
        .publish_output(
            &fixture.project,
            &request,
            Actor {
                now: 101,
                ..actor()
            },
        )
        .unwrap();
    assert!(!later.metadata_ignored);
    assert_eq!(later.revision.created_at, 100);
    let task = fixture
        .store
        .add(
            &fixture.project,
            &NewTask {
                title: "Inspect report".into(),
                ..NewTask::default()
            },
            actor(),
        )
        .unwrap();
    request.task = Some(&task.id);
    request.description = "ignored retry description";
    let retry = fixture
        .store
        .publish_output(&fixture.project, &request, actor())
        .unwrap();
    assert!(!retry.created);
    assert!(retry.metadata_ignored);
    assert_eq!(retry.revision.description, "report");
    assert_eq!(
        fixture
            .store
            .connection
            .query_row("SELECT count(*) FROM task_outputs", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        1
    );
    fixture.source("second\n");
    assert!(
        fixture
            .store
            .publish_output(&fixture.project, &request, actor())
            .is_err()
    );
    request.mode = PublishMode::Extend;
    let second = fixture
        .store
        .publish_output(&fixture.project, &request, actor())
        .unwrap();
    assert_ne!(first.revision.digest, second.revision.digest);
    fixture.source("first\n");
    let again = fixture
        .store
        .publish_output(&fixture.project, &request, actor())
        .unwrap();
    assert_eq!(first.revision.digest, again.revision.digest);
    assert!(!again.created);
    assert_eq!(
        fixture
            .store
            .output_manifest(&fixture.project, "report", 0)
            .unwrap()
            .revisions
            .len(),
        2
    );
    request.mode = PublishMode::Create;
    assert!(
        fixture
            .store
            .publish_output(&fixture.project, &request, actor())
            .is_err()
    );
}

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "one revision is inspected through each distinct availability state"
)]
fn exact_read_detects_missing_corrupt_and_purged_without_fallback() {
    let mut fixture = Fixture::new();
    let source = fixture.source("original\n");
    let published = fixture
        .store
        .publish_output(&fixture.project, &input(&source), actor())
        .unwrap();
    fixture.source("draft changed\n");
    let read = fixture
        .store
        .read_output(
            &fixture.project,
            "report",
            &published.revision.digest,
            1,
            200,
        )
        .unwrap();
    assert!(
        matches!(read.content,OutputContent::Available {document} if document.content=="original\n")
    );
    assert!(
        fixture
            .store
            .read_output(
                &fixture.project,
                "report",
                &published.revision.digest,
                0,
                200
            )
            .is_err()
    );
    assert!(
        fixture
            .store
            .read_output(
                &fixture.project,
                "report",
                &published.revision.digest,
                1,
                201
            )
            .is_err()
    );
    fs::remove_file(&read.path).unwrap();
    fs::write(&read.path, [255]).unwrap();
    assert!(matches!(
        fixture
            .store
            .read_output(
                &fixture.project,
                "report",
                &published.revision.digest,
                1,
                200
            )
            .unwrap()
            .content,
        OutputContent::Corrupt
    ));
    fs::write(&read.path, "x".repeat(super::MAX_OUTPUT_BYTES + 1)).unwrap();
    assert!(matches!(
        fixture
            .store
            .read_output(
                &fixture.project,
                "report",
                &published.revision.digest,
                1,
                200
            )
            .unwrap()
            .content,
        OutputContent::Corrupt
    ));
    fs::write(&read.path, "tampered").unwrap();
    assert!(matches!(
        fixture
            .store
            .read_output(
                &fixture.project,
                "report",
                &published.revision.digest,
                1,
                200
            )
            .unwrap()
            .content,
        OutputContent::Corrupt
    ));
    fs::remove_file(&read.path).unwrap();
    assert!(matches!(
        fixture
            .store
            .read_output(
                &fixture.project,
                "report",
                &published.revision.digest,
                1,
                200
            )
            .unwrap()
            .content,
        OutputContent::Missing
    ));
    fixture
        .store
        .connection
        .execute(
            "INSERT INTO purged_digests VALUES(?1,?2,100,'parent')",
            rusqlite::params![fixture.project.as_str(), published.revision.digest],
        )
        .unwrap();
    assert!(matches!(
        fixture
            .store
            .read_output(
                &fixture.project,
                "report",
                &published.revision.digest,
                1,
                200
            )
            .unwrap()
            .content,
        OutputContent::Purged
    ));
    fixture.source("original\n");
    assert!(
        fixture
            .store
            .publish_output(&fixture.project, &input(&source), actor())
            .is_err()
    );
}

#[test]
fn publication_rejects_identity_scope_and_source_errors() {
    let mut fixture = Fixture::new();
    let source = fixture.source("bytes");
    let mut request = input(&source);
    request.mode = PublishMode::Extend;
    assert!(
        fixture
            .store
            .publish_output(&fixture.project, &request, actor())
            .is_err()
    );
    request.mode = PublishMode::Create;
    fixture
        .store
        .publish_output(&fixture.project, &request, actor())
        .unwrap();
    request.filename = "other.md";
    assert!(
        fixture
            .store
            .publish_output(&fixture.project, &request, actor())
            .is_err()
    );
    request.filename = "../bad.md";
    assert!(
        fixture
            .store
            .publish_output(&fixture.project, &request, actor())
            .is_err()
    );
    request.filename = "report.md";
    request.task = Some("t-absent");
    assert!(
        fixture
            .store
            .publish_output(&fixture.project, &request, actor())
            .is_err()
    );
    request.task = None;
    let other = fixture.store.register_project("other").unwrap();
    assert!(fixture.store.output_manifest(&other, "report", 0).is_err());
    fs::write(&source, [255]).unwrap();
    assert!(
        fixture
            .store
            .publish_output(&fixture.project, &request, actor())
            .is_err()
    );
    fs::write(&source, "x".repeat(super::MAX_OUTPUT_BYTES + 1)).unwrap();
    assert!(
        fixture
            .store
            .publish_output(&fixture.project, &request, actor())
            .is_err()
    );
    #[cfg(unix)]
    {
        fs::remove_file(&source).unwrap();
        std::os::unix::fs::symlink("absent", &source).unwrap();
        assert!(
            fixture
                .store
                .publish_output(&fixture.project, &request, actor())
                .is_err()
        );
    }
}

#[test]
fn failed_catalog_commit_leaves_a_complete_retriable_orphan() {
    let mut fixture = Fixture::new();
    let source = fixture.source("orphan bytes");
    fixture.store.connection.execute_batch("CREATE TRIGGER fail_publication BEFORE INSERT ON output_revisions BEGIN SELECT RAISE(ABORT,'injected failure'); END;").unwrap();
    assert!(
        fixture
            .store
            .publish_output(&fixture.project, &input(&source), actor())
            .is_err()
    );
    assert!(
        fixture
            .store
            .output_manifest(&fixture.project, "report", 0)
            .is_err()
    );
    fixture
        .store
        .connection
        .execute_batch("DROP TRIGGER fail_publication;")
        .unwrap();
    let published = fixture
        .store
        .publish_output(&fixture.project, &input(&source), actor())
        .unwrap();
    assert!(published.created);
    assert_eq!(
        fs::read_to_string(fixture.root.join(published.revision.path)).unwrap(),
        "orphan bytes"
    );
}
