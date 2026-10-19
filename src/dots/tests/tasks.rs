//! Run the real CLI against an isolated state directory and a scratch Git repository.
#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::process::{Command, Output};
    use std::sync::atomic::{AtomicU64, Ordering};

    use serde_json::Value;

    struct Fixture {
        root: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let root = std::env::temp_dir().join(format!(
                "dots-tasks-cli-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            let checkout = root.join("checkout");
            fs::create_dir_all(&checkout).unwrap();
            git(&checkout, &["init", "-q", "-b", "main"]);
            git(&checkout, &["config", "user.email", "test@example.com"]);
            git(&checkout, &["config", "user.name", "Test"]);
            fs::write(checkout.join("README.md"), "scratch\n").unwrap();
            git(&checkout, &["add", "README.md"]);
            git(&checkout, &["commit", "-q", "-m", "init"]);
            git(
                &checkout,
                &["worktree", "add", "-q", "../linked", "-b", "slice"],
            );
            Self { root }
        }

        fn run_in(&self, directory: &str, args: &[&str]) -> Output {
            Command::new(env!("CARGO_BIN_EXE_dots"))
                .arg("--json")
                .arg("task")
                .args(args)
                .current_dir(self.root.join(directory))
                .env("XDG_STATE_HOME", self.root.join("state"))
                .output()
                .unwrap()
        }

        fn ok(&self, directory: &str, args: &[&str]) -> Value {
            let output = self.run_in(directory, args);
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            serde_json::from_slice(&output.stdout).unwrap()
        }

        fn fail(&self, directory: &str, args: &[&str]) -> String {
            let output = self.run_in(directory, args);
            assert!(!output.status.success());
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
        assert!(status.success(), "git {args:?}");
    }

    fn id(value: &Value) -> String {
        value
            .pointer("/task/id")
            .unwrap()
            .as_str()
            .unwrap()
            .to_owned()
    }

    #[test]
    fn worktrees_share_one_list_and_claims_are_exclusive() {
        let fixture = Fixture::new();
        let epic = fixture.ok(
            "checkout",
            &["add", "migrate policies", "--body", "pattern: docs/net.md"],
        );
        let epic = id(&epic);
        let first = id(&fixture.ok(
            "checkout",
            &["add", "first", "--parent", &epic, "--priority", "5"],
        ));
        let second = id(&fixture.ok(
            "linked",
            &["add", "second", "--parent", &epic, "--blocked-by", &first],
        ));
        let project = fixture.ok("linked", &["show", &epic]);
        assert_eq!(
            project.pointer("/project").unwrap(),
            fixture
                .ok("checkout", &["show", &epic])
                .pointer("/project")
                .unwrap()
        );
        assert_eq!(project.pointer("/task/children").unwrap(), 2);

        let ready = fixture.ok("linked", &["ready", "--parent", &epic]);
        assert_eq!(
            ready.pointer("/tasks").unwrap().as_array().unwrap().len(),
            1
        );
        assert_eq!(
            ready.pointer("/tasks/0/id").unwrap().as_str(),
            Some(first.as_str())
        );

        let claimed = fixture.ok(
            "checkout",
            &["--owner", "session-a", "claim", "--next", "--parent", &epic],
        );
        assert_eq!(id(&claimed), first);
        assert!(
            fixture
                .fail("linked", &["--owner", "session-b", "claim", &first])
                .contains("leased to session-a")
        );
        assert!(
            fixture
                .fail("linked", &["--owner", "session-b", "claim", "--next"])
                .contains("no ready task to claim")
        );
        assert_eq!(
            fixture
                .ok("checkout", &["ready"])
                .pointer("/tasks")
                .unwrap()
                .as_array()
                .unwrap()
                .len(),
            0,
            "a parent with open children is not itself ready"
        );
        assert!(
            fixture
                .fail("linked", &["--owner", "session-b", "done", &first])
                .contains("leased to session-a")
        );

        rejects_misuse(&fixture, &epic, &first);
        fixture.ok(
            "checkout",
            &[
                "--owner",
                "session-a",
                "update",
                &first,
                "--pr",
                "https://github.com/example/project/pull/1",
                "--branch",
                "netpol/first",
            ],
        );
        let all = fixture.ok("checkout", &["--owner", "session-a", "list", "--all"]);
        assert_eq!(all.pointer("/tasks").unwrap().as_array().unwrap().len(), 3);
        let mine = fixture.ok("checkout", &["list", "--all", "--claimed-by", "session-a"]);
        assert_eq!(mine.pointer("/tasks").unwrap().as_array().unwrap().len(), 1);
        let error = fixture.run_in("checkout", &["show", "t-nope"]);
        let envelope: Value = serde_json::from_slice(&error.stdout).unwrap();
        assert!(
            envelope
                .pointer("/error")
                .unwrap()
                .as_str()
                .unwrap()
                .contains("does not exist")
        );
        let done = fixture.ok(
            "checkout",
            &["--owner", "session-a", "done", &first, "--note", "merged"],
        );
        assert_eq!(done.pointer("/task/status").unwrap(), "done");
        assert!(done.pointer("/task/owner").unwrap().is_null());
        let unblocked = fixture.ok("linked", &["--owner", "session-b", "claim", &second]);
        assert_eq!(unblocked.pointer("/task/owner").unwrap(), "session-b");

        history_and_listing(&fixture, &epic, &first);
    }

    fn rejects_misuse(fixture: &Fixture, epic: &str, first: &str) {
        assert!(
            fixture
                .fail("checkout", &["claim", first])
                .contains("--owner is required")
        );
        assert!(
            fixture
                .fail("checkout", &["--owner", "session-a", "claim", epic])
                .contains("unclosed child")
        );
        assert!(
            fixture
                .fail("checkout", &["--owner", "session-b", "reopen", first])
                .contains("already open")
        );
        assert!(
            fixture
                .fail("checkout", &["--owner", "session-a", "done", epic])
                .contains("cannot be closed yet")
        );
        assert!(
            fixture
                .fail(
                    "checkout",
                    &[
                        "update",
                        first,
                        "--pr",
                        "https://github.com/example/project/pull/9"
                    ]
                )
                .contains("leased to session-a"),
            "field edits under another owner's lease are refused"
        );
        assert!(
            fixture
                .fail(
                    "checkout",
                    &["--owner", "session-a", "update", first, "--pr", "PR 1"]
                )
                .contains("full pull request URL")
        );
        assert!(
            fixture
                .fail("checkout", &["update", first])
                .contains("no fields")
        );
    }

    fn history_and_listing(fixture: &Fixture, epic: &str, first: &str) {
        let shown = fixture.ok("checkout", &["show", first]);
        let kinds: Vec<_> = shown
            .pointer("/events")
            .unwrap()
            .as_array()
            .unwrap()
            .iter()
            .map(|event| event.pointer("/kind").unwrap().as_str().unwrap().to_owned())
            .collect();
        assert_eq!(kinds, ["created", "claimed", "updated", "pr", "done"]);
        assert_eq!(shown.pointer("/task/branch").unwrap(), "netpol/first");
        assert_eq!(shown.pointer("/task/last_event/kind").unwrap(), "done");
        assert_eq!(shown.pointer("/task/last_event/detail").unwrap(), "merged");
        assert_eq!(
            shown.pointer("/task/pr").unwrap(),
            "https://github.com/example/project/pull/1"
        );

        let second = fixture.ok("checkout", &["list", "--parent", epic]);
        assert_eq!(
            second.pointer("/tasks/0/prerequisites/0/status").unwrap(),
            "done"
        );
        assert_eq!(
            second.pointer("/tasks/0/prerequisites/0/title").unwrap(),
            "first"
        );
        let events = fixture.ok("checkout", &["events", "--since", "2"]);
        let events = events.pointer("/events").unwrap().as_array().unwrap();
        assert!(events.len() > 3);
        assert!(
            events
                .iter()
                .all(|event| event.pointer("/seq").unwrap().as_i64().unwrap() > 2)
        );
        let listed = fixture.ok("checkout", &["list", "--parent", epic]);
        assert_eq!(
            listed.pointer("/tasks").unwrap().as_array().unwrap().len(),
            1
        );
        let all = fixture.ok("checkout", &["list", "--parent", epic, "--all"]);
        assert_eq!(all.pointer("/tasks").unwrap().as_array().unwrap().len(), 2);
    }

    fn nested_groups_are_one_subtree(fixture: &Fixture, epic: &str) {
        let group = id(&fixture.ok("checkout", &["add", "group", "--parent", epic]));
        let leaf = id(&fixture.ok("checkout", &["add", "leaf", "--parent", &group]));
        let ready = fixture.ok("checkout", &["ready", "--parent", epic]);
        assert_eq!(
            ready.pointer("/tasks/0/id").unwrap().as_str(),
            Some(leaf.as_str()),
            "ready --parent reaches leaves below nested groups and omits groups"
        );
        let below = fixture.ok("checkout", &["list", "--parent", epic]);
        assert!(below.pointer("/tasks").unwrap().as_array().unwrap().len() >= 3);
        let warned = fixture.ok("checkout", &["hold", &group, "--reason", "pause"]);
        assert!(
            warned
                .pointer("/warnings/0")
                .unwrap()
                .as_str()
                .unwrap()
                .contains("1 unclosed descendants")
        );
        fixture.ok("checkout", &["reopen", &group]);
        let claimed = fixture.ok(
            "linked",
            &["--owner", "session-c", "claim", "--next", "--parent", epic],
        );
        assert_eq!(id(&claimed), leaf);
        assert!(
            fixture
                .fail("checkout", &["dep", "add", &leaf, "--blocked-by", &leaf])
                .contains("wait on itself")
        );
    }

    #[test]
    fn explicit_project_keys_are_isolated_and_required_outside_git() {
        let fixture = Fixture::new();
        fs::create_dir_all(fixture.root.join("plain")).unwrap();
        assert!(fixture.fail("plain", &["list"]).contains("--project"));
        let created = fixture.ok("plain", &["--project", "notes", "add", "write docs"]);
        assert_eq!(created.pointer("/project").unwrap(), "notes");
        assert_eq!(
            fixture
                .ok("checkout", &["list"])
                .pointer("/tasks")
                .unwrap()
                .as_array()
                .unwrap()
                .len(),
            0
        );
        assert!(
            fixture
                .fail("checkout", &["show", &id(&created)])
                .contains("does not exist in this project")
        );
        let text = Command::new(env!("CARGO_BIN_EXE_dots"))
            .args(["task", "--project", "notes", "list"])
            .current_dir(&fixture.root)
            .env("XDG_STATE_HOME", fixture.root.join("state"))
            .output()
            .unwrap();
        let text = String::from_utf8(text.stdout).unwrap();
        assert!(
            text.contains("open") && text.contains("write docs"),
            "{text}"
        );
    }

    #[test]
    fn readiness_is_reported_and_checkout_flags_are_rejected() {
        let fixture = Fixture::new();
        let epic = id(&fixture.ok("checkout", &["add", "epic"]));
        let slice = id(&fixture.ok("checkout", &["add", "slice", "--parent", &epic]));
        let shown = fixture.ok("checkout", &["show", &epic]);
        assert_eq!(shown.pointer("/task/ready").unwrap(), false);
        assert_eq!(
            shown.pointer("/task/not_ready_because").unwrap(),
            "1 unclosed child"
        );
        assert!(
            fixture
                .fail("checkout", &["dep", "add", &slice, "--blocked-by", &epic])
                .contains("share a parent chain")
        );
        let roots = fixture.ok("linked", &["list", "--roots"]);
        assert_eq!(
            roots.pointer("/tasks").unwrap().as_array().unwrap().len(),
            1
        );
        fixture.ok("checkout", &["--owner", "session-a", "claim", &slice]);
        assert!(
            fixture
                .fail(
                    "linked",
                    &["--owner", "session-b", "hold", &slice, "--reason", "x"]
                )
                .contains("leased to session-a")
        );
        let other = id(&fixture.ok("checkout", &["add", "other"]));
        let warned = fixture.ok("checkout", &["dep", "add", &slice, "--blocked-by", &other]);
        assert!(
            warned
                .pointer("/warnings/0")
                .unwrap()
                .as_str()
                .unwrap()
                .contains("leased to session-a")
        );
        assert!(warned.pointer("/now").unwrap().is_i64());
        let held = fixture.ok(
            "linked",
            &[
                "--owner",
                "session-b",
                "hold",
                &slice,
                "--reason",
                "x",
                "--force",
            ],
        );
        assert!(held.pointer("/task/owner").unwrap().is_null());
        assert_eq!(held.pointer("/task/last_event/detail").unwrap(), "x");
        nested_groups_are_one_subtree(&fixture, &epic);
        for arguments in [
            vec!["list", "--repo", "example/project"],
            vec!["list", "--no-cache"],
        ] {
            let output = Command::new(env!("CARGO_BIN_EXE_dots"))
                .arg("task")
                .args(&arguments)
                .current_dir(fixture.root.join("checkout"))
                .env("XDG_STATE_HOME", fixture.root.join("state"))
                .output()
                .unwrap();
            assert!(!output.status.success());
            assert!(String::from_utf8_lossy(&output.stderr).contains("do not apply"));
        }
    }
}
