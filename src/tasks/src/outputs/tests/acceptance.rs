//! Real process death at publication and purge boundaries; no production failpoint interface.
use std::fs;
use std::io::Write as _;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use dots_documents::mutation_lock::MutationLock;
use dots_documents::staged_file::StagedFile;
use dots_documents::{directories, text};

use super::{Fixture, actor, input};
use crate::outputs::OutputContent;
use crate::{Project, Store};

#[test]
fn writer_fixture() {
    let Some(root) = std::env::var_os("DOTS_OUTPUT_FIXTURE") else {
        return;
    };
    let root = PathBuf::from(root);
    let project = Project::try_new(std::env::var("DOTS_OUTPUT_PROJECT").unwrap()).unwrap();
    let phase = std::env::var("DOTS_OUTPUT_STOP_AT").unwrap();
    let mut store = Store::open(&root.join("workspace.sqlite3")).unwrap();
    if phase == "staged" {
        let _lock = MutationLock::acquire(&root.join("mutation.lock")).unwrap();
        let digest = text::revision(Some("complete evidence"));
        let directory = root.join(format!(
            "workspaces/{}/outputs/report/sha256-{}",
            project.as_str(),
            &digest[7..]
        ));
        directories::create_within(&directory, &root).unwrap();
        let staged = StagedFile::create(&directory).unwrap();
        staged.file().write_all(b"complete").unwrap();
        staged.file().sync_all().unwrap();
        crate::test_pause(&root, "staged");
    } else if matches!(phase.as_str(), "published" | "committed") {
        store
            .publish_output(&project, &input(&root.join("draft.md")), actor())
            .unwrap();
    } else {
        store
            .purge_output_digest(
                &project,
                &text::revision(Some("complete evidence")),
                actor(),
            )
            .unwrap();
    }
    panic!("writer did not pause");
}

fn kill_at(fixture: &Fixture, phase: &str) {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args([
            "--exact",
            "outputs::tests::acceptance::writer_fixture",
            "--nocapture",
        ])
        .env("DOTS_OUTPUT_FIXTURE", &fixture.root)
        .env("DOTS_OUTPUT_PROJECT", fixture.project.as_str())
        .env("DOTS_OUTPUT_STOP_AT", phase)
        .env("HOME", fixture.root.join("home"))
        .env("XDG_DATA_HOME", fixture.root.join("data"))
        .env("XDG_STATE_HOME", fixture.root.join("state"));
    let result = dots_process::output(command, Duration::from_secs(2), 8192);
    assert!(
        matches!(result, Err(dots_process::Error::Timeout(_))),
        "{result:?}"
    );
    assert_eq!(
        fs::read_to_string(fixture.root.join("phase-reached")).unwrap(),
        phase
    );
}

#[test]
fn killed_publication_never_registers_partial_bytes_and_retries_after_lost_reply() {
    for phase in ["staged", "published", "committed"] {
        let mut fixture = Fixture::new();
        let source = fixture.source("complete evidence");
        kill_at(&fixture, phase);
        let before = fixture.store.output_integrity(&fixture.project).unwrap();
        if phase == "staged" {
            assert!(
                before
                    .issues
                    .iter()
                    .any(|issue| issue.state == "staged_output")
            );
        }
        if phase == "published" {
            assert!(
                before
                    .issues
                    .iter()
                    .any(|issue| issue.state == "unregistered_output")
            );
        }
        let published = fixture
            .store
            .publish_output(&fixture.project, &input(&source), actor())
            .unwrap();
        assert_eq!(published.created, phase != "committed");
        assert_eq!(
            fixture
                .store
                .output_manifest(&fixture.project, "report", 0)
                .unwrap()
                .revisions
                .len(),
            1
        );
        assert!(
            matches!(fixture.store.read_output(&fixture.project,"report",&published.revision.digest,1,200).unwrap().content,OutputContent::Available {document} if document.content=="complete evidence\n")
        );
    }
}

#[test]
fn killed_purge_is_logically_effective_and_retry_finishes_orphan_removal() {
    for phase in ["tombstoned", "removed"] {
        let mut fixture = Fixture::new();
        let source = fixture.source("complete evidence");
        let published = fixture
            .store
            .publish_output(&fixture.project, &input(&source), actor())
            .unwrap();
        let parent = fixture
            .root
            .join(&published.revision.path)
            .parent()
            .unwrap()
            .to_owned();
        fs::write(parent.join("orphan.md"), "complete evidence").unwrap();
        kill_at(&fixture, phase);
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
        assert!(
            fixture
                .store
                .output_integrity(&fixture.project)
                .unwrap()
                .issues
                .iter()
                .any(|issue| issue.state == "purged_bytes_pending_removal")
        );
        let retry = fixture
            .store
            .purge_output_digest(&fixture.project, &published.revision.digest, actor())
            .unwrap();
        assert!(retry.tombstoned && retry.removal_complete);
        assert_eq!(
            fixture
                .store
                .output_integrity(&fixture.project)
                .unwrap()
                .issues
                .len(),
            0
        );
    }
}
