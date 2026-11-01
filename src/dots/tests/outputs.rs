//! Cold CLI evidence discovery and exact-revision acceptance with isolated storage.
#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::process::{Command, Output};
    use std::sync::atomic::{AtomicU64, Ordering};

    use serde_json::Value;

    struct Fixture {
        root: PathBuf,
        project: String,
    }
    impl Fixture {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let root = std::env::temp_dir().join(format!(
                "dots-output-cli-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(root.join("data/dots")).unwrap();
            let mut store =
                dots_tasks::Store::create(&root.join("data/dots/workspace.sqlite3")).unwrap();
            let project = store
                .register_project("example")
                .unwrap()
                .as_str()
                .to_owned();
            Self { root, project }
        }
        fn run(&self, group: &str, args: &[&str]) -> Output {
            Command::new(env!("CARGO_BIN_EXE_dots"))
                .args(["--json", group, "--project", &self.project])
                .args(args)
                .current_dir(&self.root)
                .env("HOME", self.root.join("home"))
                .env("XDG_DATA_HOME", self.root.join("data"))
                .env("XDG_STATE_HOME", self.root.join("state"))
                .env_remove("PI_SESSION_ID")
                .env_remove("CLAUDE_CODE_SESSION_ID")
                .env_remove("CODEX_SESSION_ID")
                .output()
                .unwrap()
        }
        fn ok(&self, group: &str, args: &[&str]) -> Value {
            let output = self.run(group, args);
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            serde_json::from_slice(&output.stdout).unwrap()
        }
        fn publish(&self, text: &str) -> Value {
            let source = self.root.join("draft.md");
            fs::write(&source, text).unwrap();
            self.ok(
                "output",
                &[
                    "publish",
                    "report",
                    "--filename",
                    "report.md",
                    "--media-type",
                    "text/markdown",
                    "--content-file",
                    source.to_str().unwrap(),
                    "--create",
                ],
            )
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.root).unwrap();
        }
    }

    #[test]
    fn task_discovery_command_preserves_project_and_json_without_accepting_reports() {
        let fixture = Fixture::new();
        let created = fixture.ok("task", &["add", "Discover reports"]);
        let id = created["task"]["id"].as_str().unwrap();
        fixture.publish("# Unaccepted report\n");
        for attached in [false, true] {
            if attached {
                fixture.ok("output", &["attach", "report", "--task", id]);
            }
            let shown = fixture.ok("task", &["show", id]);
            assert_eq!(shown["task"]["evidence"], serde_json::json!([]));
            assert!(shown["task"]["result_event"].is_null());
            let command = shown["discovery_command"].as_str().unwrap();
            assert_eq!(
                command,
                format!(
                    "dots --json output list --task {id} --project {}",
                    fixture.project
                )
            );
            // Execute the returned arguments outside Git: the command must carry its scope.
            let output = Command::new(env!("CARGO_BIN_EXE_dots"))
                .args(command.split_ascii_whitespace().skip(1))
                .current_dir(&fixture.root)
                .env("HOME", fixture.root.join("home"))
                .env("XDG_DATA_HOME", fixture.root.join("data"))
                .env("XDG_STATE_HOME", fixture.root.join("state"))
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            let discovered: Value = serde_json::from_slice(&output.stdout).unwrap();
            assert_eq!(discovered["project"], fixture.project);
            let expected = if attached { vec!["report"] } else { vec![] };
            assert_eq!(discovered["slugs"], serde_json::json!(expected));
            let after = fixture.ok("task", &["show", id]);
            assert_eq!(after["task"], shown["task"]);
        }
    }

    #[test]
    fn cold_task_discovery_pins_reviewed_bytes_and_detach_preserves_history() {
        let fixture = Fixture::new();
        let created = fixture.ok("task", &["add", "Inspect retained report"]);
        let id = created["task"]["id"].as_str().unwrap();
        assert_eq!(id.len(), 34);
        let published = fixture.publish("# Report\nVerified against neutral fixture.\n");
        let digest = published["publication"]["revision"]["digest"]
            .as_str()
            .unwrap();
        let inspected = fixture.ok("output", &["show", "report", "--digest", digest]);
        assert_eq!(inspected["output"]["content"]["state"], "available");
        let accepted = fixture.ok(
            "task",
            &[
                "done",
                id,
                "--result",
                "accepted",
                "--evidence",
                "report",
                digest,
            ],
        );
        let event = accepted["event"]["seq"].to_string();
        fs::write(fixture.root.join("draft.md"), "changed draft").unwrap();
        let shown = fixture.ok("task", &["show", id]);
        let evidence = &shown["task"]["evidence"][0];
        assert_eq!(evidence["digest"], digest);
        let opened = fixture.ok(
            "output",
            &[
                "show",
                evidence["slug"].as_str().unwrap(),
                "--digest",
                evidence["digest"].as_str().unwrap(),
            ],
        );
        assert_eq!(
            opened["output"]["content"]["document"]["content"],
            "# Report\nVerified against neutral fixture.\n"
        );
        assert_eq!(
            fixture.ok("output", &["list", "--task", id])["slugs"],
            serde_json::json!(["report"])
        );
        fixture.ok("output", &["detach", "report", "--task", id]);
        assert_eq!(
            fixture.ok("output", &["list", "--task", id])["slugs"],
            serde_json::json!([])
        );
        assert_eq!(
            fixture.ok("task", &["result-event", id, "--event", &event])["event"]["evidence"][0],
            *evidence
        );
        fixture.ok("task", &["reopen", id]);
        fixture.ok("output", &["purge", "--digest", digest]);
        assert_eq!(
            fixture.ok("task", &["show", id])["task"]["purged_evidence"][0],
            *evidence
        );
        let purged = fixture.run(
            "task",
            &[
                "done",
                id,
                "--result",
                "accepted",
                "--evidence",
                "report",
                digest,
            ],
        );
        assert!(!purged.status.success());
        assert!(String::from_utf8_lossy(&purged.stderr).contains("purged"));
        assert_eq!(fixture.ok("task", &["show", id])["task"]["status"], "open");
        assert_eq!(
            fixture.ok("output", &["show", "report", "--digest", digest])["output"]["content"]["state"],
            "purged"
        );
    }

    #[test]
    fn unavailable_evidence_leaves_the_current_result() {
        let fixture = Fixture::new();
        let created = fixture.ok("task", &["add", "Result only"]);
        let id = created["task"]["id"].as_str().unwrap();
        let first = fixture.ok("task", &["update", id, "--result", "first"]);
        let fake = format!("sha256:{}", "0".repeat(64));
        let missing = fixture.run(
            "task",
            &[
                "done",
                id,
                "--result",
                "next",
                "--evidence",
                "missing",
                &fake,
            ],
        );
        assert!(!missing.status.success());
        assert!(String::from_utf8_lossy(&missing.stderr).contains("not registered"));
        let shown = fixture.ok("task", &["show", id]);
        assert_eq!(shown["task"]["result"], "first");
        assert_eq!(shown["task"]["result_event"], first["event"]["seq"]);
        let applied = fixture.ok("task", &["done", id, "--result", "next"]);
        assert_eq!(applied["task"]["status"], "done");
        assert_eq!(applied["event"]["predecessor"], first["event"]["seq"]);
    }
}
