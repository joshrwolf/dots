//! Run `dots attention` against an isolated state directory and a scratch Git repository.
#[cfg(test)]
mod tests {
    use std::fs;
    use std::io::Write as _;
    use std::path::{Path, PathBuf};
    use std::process::{Command, Output, Stdio};
    use std::sync::atomic::{AtomicU64, Ordering};

    use serde_json::{Value, json};

    struct Fixture {
        root: PathBuf,
        project: String,
    }

    impl Fixture {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let root = std::env::temp_dir().join(format!(
                "dots-attention-cli-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            let checkout = root.join("checkout");
            fs::create_dir_all(&checkout).unwrap();
            git(&checkout, &["init", "-q", "-b", "main"]);
            let mut fixture = Self {
                root,
                project: String::new(),
            };
            let initialized = fixture.json(&["project", "init", "fixture", "--fresh"], None);
            fixture.project = initialized
                .pointer("/project/id")
                .unwrap()
                .as_str()
                .unwrap()
                .to_owned();
            let briefs = fixture
                .root
                .join("data/dots/workspaces")
                .join(&fixture.project)
                .join("briefs");
            fs::create_dir_all(&briefs).unwrap();
            fs::write(briefs.join("README.md"), "# Goal\n").unwrap();
            fixture
        }

        fn run(&self, args: &[&str], stdin: Option<&Value>) -> Output {
            let mut child = Command::new(env!("CARGO_BIN_EXE_dots"))
                .args(args)
                .env_remove("PI_SESSION_ID")
                .env_remove("CLAUDE_CODE_SESSION_ID")
                .env_remove("CODEX_SESSION_ID")
                .env_remove("DOTS_AGENT_OWNER")
                .env_remove("DOTS_WORKER_ID")
                .current_dir(self.root.join("checkout"))
                .env("HOME", self.root.join("home"))
                .env("XDG_DATA_HOME", self.root.join("data"))
                .env("XDG_STATE_HOME", self.root.join("state"))
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap();
            let mut input = child.stdin.take().unwrap();
            if let Some(content) = stdin {
                input.write_all(content.to_string().as_bytes()).unwrap();
            }
            drop(input);
            child.wait_with_output().unwrap()
        }

        fn json(&self, args: &[&str], stdin: Option<&Value>) -> Value {
            let mut all = vec!["--json"];
            all.extend_from_slice(args);
            let output = self.run(&all, stdin);
            assert!(
                output.status.success(),
                "{args:?}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            serde_json::from_slice(&output.stdout).unwrap()
        }

        fn text(&self, args: &[&str]) -> String {
            let output = self.run(args, None);
            assert!(
                output.status.success(),
                "{args:?}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            String::from_utf8(output.stdout).unwrap()
        }

        fn fail(&self, args: &[&str], stdin: Option<&Value>) -> String {
            let output = self.run(args, stdin);
            assert!(!output.status.success(), "{args:?} succeeded");
            String::from_utf8_lossy(&output.stderr).into_owned()
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    fn git(directory: &Path, args: &[&str]) {
        let status = Command::new("git")
            .args(args)
            .current_dir(directory)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .status()
            .unwrap();
        assert!(status.success());
    }

    fn str_at<'value>(value: &'value Value, pointer: &str) -> &'value str {
        value.pointer(pointer).unwrap().as_str().unwrap()
    }

    #[test]
    #[expect(
        clippy::too_many_lines,
        reason = "one scenario follows a request from raise through incorporation"
    )]
    fn requests_hold_work_until_incorporated_and_scopes_stay_explicit() {
        let f = Fixture::new();
        let created = f.json(
            &[
                "task",
                "goal",
                "create",
                "Goal",
                "--brief",
                "briefs/README.md",
            ],
            None,
        );
        let root = str_at(&created, "/goal/root/id").to_owned();
        f.json(
            &["task", "--owner", "coord", "goal", "coordinate", &root],
            None,
        );
        let added = f.json(
            &["task", "--owner", "coord", "add", "Leaf", "--parent", &root],
            None,
        );
        let leaf = str_at(&added, "/task/id").to_owned();
        let content = json!({
            "kind": "decision",
            "title": "Choose the layout",
            "question": "Which layout?",
            "choices": [{"id": "one", "label": "One table"}, {"id": "two", "label": "Two tables"}],
            "tasks": [leaf],
        });
        let raise = [
            "attention",
            "--owner",
            "coord",
            "raise",
            &root,
            "--key",
            "layout",
            "--content-file",
            "-",
        ];
        let raised = f.json(&raise, Some(&content));
        assert_eq!(raised["created"], true);
        let request = str_at(&raised, "/request/id").to_owned();
        assert_eq!(f.json(&raise, Some(&content))["created"], false);
        assert!(
            f.fail(&raise, Some(&json!({"extra": 1})))
                .contains("does not match this command's JSON shape")
        );
        let mut by_worker = raise;
        by_worker[2] = "worker";
        assert!(f.fail(&by_worker, Some(&content)).contains("coordinator"));
        let ready = f.json(&["task", "ready", "--parent", &root], None);
        assert_eq!(ready["tasks"], json!([]));
        let shown = f.json(&["task", "show", &leaf], None);
        assert_eq!(
            shown.pointer("/task/holds/0/request").unwrap(),
            &request.as_str()
        );

        let answer = [
            "attention",
            "--owner",
            "human",
            "answer",
            &request,
            "--content-file",
            "-",
        ];
        // A model account without its origin is not a human answer.
        assert!(
            f.fail(&answer, Some(&json!({"choice": "one", "source": "ui"})))
                .contains("JSON shape")
        );
        let answered = f.json(
            &answer,
            Some(&json!({"choice": "one", "source": "ui", "provenance": "attention view, test terminal"})),
        );
        assert_eq!(str_at(&answered, "/request/state"), "answered");
        assert!(
            f.fail(
                &["task", "--owner", "coord", "done", &root, "--result", "x"],
                None
            )
            .contains("outstanding requests")
        );

        let goal_ref = format!("{}:{root}", f.project);
        let global = f.json(&["attention", "list", "--scope", "global"], None);
        let exact = f.json(
            &["attention", "list", "--scope", "goals", "--goal", &goal_ref],
            None,
        );
        assert_eq!(global["goals"], exact["goals"]);
        assert_eq!(str_at(&global, "/goals/0/requests/0/id"), request);
        assert_eq!(str_at(&global, "/goals/0/coordinator"), "coord");
        assert_eq!(
            f.json(&["attention", "list", "--scope", "goals"], None)["goals"],
            json!([])
        );
        assert!(
            f.fail(
                &[
                    "attention",
                    "list",
                    "--scope",
                    "global",
                    "--goal",
                    &goal_ref
                ],
                None
            )
            .contains("--scope goals")
        );
        assert!(
            f.fail(
                &[
                    "attention",
                    "list",
                    "--scope",
                    "global",
                    "--project",
                    &f.project
                ],
                None
            )
            .contains("--project does not apply")
        );
        assert!(f.fail(&["attention", "list"], None).contains("--scope"));

        let incorporated = f.json(
            &[
                "attention",
                "--owner",
                "coord",
                "incorporate",
                &request,
                "--content-file",
                "-",
            ],
            Some(&json!({"kind": "decision", "decision": "Use one table."})),
        );
        assert_eq!(str_at(&incorporated, "/request/state"), "closed");
        assert_eq!(str_at(&incorporated, "/decision/request"), request.as_str());
        let ready = f.json(&["task", "ready", "--parent", &root], None);
        assert_eq!(str_at(&ready, "/tasks/0/id"), leaf);

        let decide = [
            "attention",
            "--owner",
            "coord",
            "decide",
            &root,
            "--content-file",
            "-",
        ];
        assert!(
            f.fail(
                &decide,
                Some(&json!({"kind": "mandate", "decision": "Local commits only."}))
            )
            .contains("JSON shape")
        );
        f.json(
            &decide,
            Some(&json!({
                "kind": "mandate",
                "decision": "Local commits only.",
                "human": {"text": "commit locally", "source": "chat", "provenance": "pi session s entry e"},
            })),
        );
        let mandates = f.json(
            &[
                "attention",
                "decisions",
                "--scope",
                "global",
                "--kind",
                "mandate",
            ],
            None,
        );
        assert_eq!(str_at(&mandates, "/goals/0/decisions/0/kind"), "mandate");
        let listed = f.text(&["attention", "list", "--scope", "global"]);
        assert!(listed.contains("Mandate "), "{listed}");
        assert!(listed.contains("root claimed by coord"), "{listed}");
    }

    #[test]
    fn a_missing_catalog_fails_instead_of_reading_nothing() {
        let f = Fixture::new();
        fs::remove_dir_all(f.root.join("data/dots")).unwrap();
        assert!(
            f.fail(&["attention", "list", "--scope", "global"], None)
                .contains("workspace absent")
        );
    }
}
