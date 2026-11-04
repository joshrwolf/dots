//! Run the real CLI against an isolated state directory and a scratch Git repository.
#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::process::{Command, Output};
    use std::sync::atomic::{AtomicU64, Ordering};

    use serde_json::Value;

    const HISTORY_FOOTER: &str = "dots task show ID shows a task's full result and history.";

    struct Fixture {
        root: PathBuf,
        project: String,
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
            let initialized = Command::new(env!("CARGO_BIN_EXE_dots"))
                .args(["--json", "project", "init", "fixture", "--fresh"])
                .current_dir(&checkout)
                .env("HOME", root.join("home"))
                .env("XDG_DATA_HOME", root.join("data"))
                .env("XDG_STATE_HOME", root.join("state"))
                .output()
                .unwrap();
            assert!(
                initialized.status.success(),
                "{}",
                String::from_utf8_lossy(&initialized.stderr)
            );
            let initialized: Value = serde_json::from_slice(&initialized.stdout).unwrap();
            let project = initialized
                .pointer("/project/id")
                .unwrap()
                .as_str()
                .unwrap()
                .to_owned();
            let fixture = Self { root, project };
            fs::create_dir_all(fixture.workspace().join("briefs")).unwrap();
            fs::write(fixture.workspace().join("briefs/README.md"), "# Goal\n").unwrap();
            fs::write(fixture.workspace().join("briefs/brief.md"), "# Goal\n").unwrap();
            fixture
        }

        fn workspace(&self) -> PathBuf {
            self.root.join("data/dots/workspaces").join(&self.project)
        }

        fn run_in(&self, directory: &str, args: &[&str]) -> Output {
            Command::new(env!("CARGO_BIN_EXE_dots"))
                .arg("--json")
                .arg("task")
                .args(args)
                .env_remove("PI_SESSION_ID")
                .env_remove("CLAUDE_CODE_SESSION_ID")
                .env_remove("CODEX_SESSION_ID")
                .current_dir(self.root.join(directory))
                .env("XDG_STATE_HOME", self.root.join("state"))
                .env("HOME", self.root.join("home"))
                .env("XDG_DATA_HOME", self.root.join("data"))
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

        fn readable(&self, args: &[&str]) -> String {
            let output = Command::new(env!("CARGO_BIN_EXE_dots"))
                .arg("task")
                .args(args)
                .env_remove("PI_SESSION_ID")
                .env_remove("CLAUDE_CODE_SESSION_ID")
                .env_remove("CODEX_SESSION_ID")
                .current_dir(self.root.join("checkout"))
                .env("XDG_STATE_HOME", self.root.join("state"))
                .env("HOME", self.root.join("home"))
                .env("XDG_DATA_HOME", self.root.join("data"))
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            String::from_utf8(output.stdout).unwrap()
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

    #[test]
    fn readable_task_inspection_labels_report_discovery_separately() {
        let fixture = Fixture::new();
        let created = fixture.ok("checkout", &["add", "Inspect reports"]);
        let id = created["task"]["id"].as_str().unwrap();
        let shown = fixture.readable(&["show", id]);
        assert!(shown.contains(&format!(
            "Reports: dots output list --task {id} --project {}\n",
            fixture.project
        )));
        assert!(!shown.contains("--json"));
        let linked = fixture.ok("linked", &["show", id]);
        assert_eq!(
            linked["discovery_command"],
            format!(
                "dots --json output list --task {id} --project {}",
                fixture.project
            )
        );
    }

    #[test]
    fn brief_inspection_reads_current_bytes_with_explicit_limits_and_failures() {
        let fixture = Fixture::new();
        let created = fixture.ok(
            "checkout",
            &["goal", "create", "Goal", "--brief", "briefs/README.md"],
        );
        let id = created.pointer("/goal/root/id").unwrap().as_str().unwrap();
        assert_eq!(
            created.pointer("/goal/brief/relative").unwrap(),
            "briefs/README.md"
        );
        let revision = created.pointer("/goal/brief/document/revision").unwrap();
        assert!(
            fixture
                .readable(&["goal", "show", id])
                .contains("Brief content:\n# Goal\n")
        );
        let path = fixture.workspace().join("briefs/README.md");
        fs::write(&path, "changed\n".repeat(250)).unwrap();
        let shown = fixture.ok("linked", &["goal", "show", id]);
        assert_ne!(
            shown
                .pointer("/overview/goal/brief/document/revision")
                .unwrap(),
            revision
        );
        assert_eq!(
            shown
                .pointer("/overview/goal/brief/document/next_line")
                .unwrap(),
            201
        );
        assert_eq!(
            shown
                .pointer("/overview/goal/brief/document/total_lines")
                .unwrap(),
            250
        );
        let listed = fixture.ok("linked", &["goal", "list"]);
        assert!(listed.pointer("/goals/0/brief/document").unwrap().is_null());
        for bytes in [vec![0xff], vec![b'x'; dots_memory::MAX_FILE_BYTES + 1]] {
            fs::write(&path, bytes).unwrap();
            let shown = fixture.ok("linked", &["goal", "show", id]);
            assert!(
                shown
                    .pointer("/overview/goal/brief/document")
                    .unwrap()
                    .is_null()
            );
            assert!(
                shown
                    .pointer("/overview/goal/brief/diagnostic")
                    .unwrap()
                    .is_string()
            );
        }
        fs::remove_file(&path).unwrap();
        let shown = fixture.ok("linked", &["goal", "show", id]);
        assert_eq!(
            shown
                .pointer("/overview/goal/brief/document/exists")
                .unwrap(),
            false
        );
        assert_eq!(
            shown
                .pointer("/overview/goal/brief/document/revision")
                .unwrap(),
            "missing"
        );
        fs::create_dir(&path).unwrap();
        let shown = fixture.ok("linked", &["goal", "show", id]);
        assert!(
            shown
                .pointer("/overview/goal/brief/diagnostic")
                .unwrap()
                .as_str()
                .unwrap()
                .contains("regular file")
        );
    }

    #[test]
    fn brief_associations_reject_unmanaged_paths_and_symlinked_ancestors() {
        let fixture = Fixture::new();
        for path in [
            "README.md",
            "/tmp/brief.md",
            "briefs/../README.md",
            "briefs//README.md",
            "briefs/./README.md",
            "knowledge/MEMORY.md",
            "briefs/bad\nname.md",
        ] {
            fixture.fail("checkout", &["goal", "create", "Rejected", "--brief", path]);
        }
        assert_eq!(
            fixture.ok("checkout", &["list"])["tasks"],
            serde_json::json!([])
        );
        let created = fixture.ok(
            "checkout",
            &["goal", "create", "Goal", "--brief", "briefs/README.md"],
        );
        let id = created.pointer("/goal/root/id").unwrap().as_str().unwrap();
        let workspaces = fixture.root.join("data/dots/workspaces");
        let displaced = fixture.root.join("displaced-workspaces");
        fs::rename(&workspaces, &displaced).unwrap();
        std::os::unix::fs::symlink(&displaced, &workspaces).unwrap();
        let shown = fixture.ok("linked", &["goal", "show", id]);
        assert!(
            shown
                .pointer("/overview/goal/brief/document")
                .unwrap()
                .is_null()
        );
        assert!(
            shown
                .pointer("/overview/goal/brief/diagnostic")
                .unwrap()
                .as_str()
                .unwrap()
                .contains("symlink")
        );
        assert!(
            fixture
                .fail(
                    "checkout",
                    &["goal", "attach", id, "--brief", "briefs/README.md"]
                )
                .contains("symlink")
        );
        assert_eq!(
            fs::read_to_string(displaced.join(&fixture.project).join("briefs/README.md")).unwrap(),
            "# Goal\n"
        );
    }

    #[test]
    fn pi_session_ownership_and_claimed_goal_discovery() {
        let f = Fixture::new();
        fs::write(f.workspace().join("briefs/brief.md"), "# Goal").unwrap();
        let goal = f.ok(
            "checkout",
            &["goal", "create", "goal", "--brief", "briefs/brief.md"],
        );
        assert!(
            goal.get("follow_hint")
                .unwrap()
                .as_str()
                .unwrap()
                .contains("goal_focus")
        );
        let root = goal.pointer("/goal/root/id").unwrap().as_str().unwrap();
        let leaf = f.ok("checkout", &["add", "leaf", "--parent", root]);
        let id = leaf.pointer("/task/id").unwrap().as_str().unwrap();
        let run = |session: &str, args: &[&str]| {
            Command::new(env!("CARGO_BIN_EXE_dots"))
                .args(["--json", "task"])
                .args(args)
                .current_dir(f.root.join("linked"))
                .env("XDG_STATE_HOME", f.root.join("state"))
                .env("XDG_DATA_HOME", f.root.join("data"))
                .env("PI_SESSION_ID", session)
                .output()
                .unwrap()
        };
        let output = run("fixture-session", &["claim", id]);
        assert!(output.status.success(), "{output:?}");
        let claimed: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(
            claimed.pointer("/task/owner").unwrap(),
            "pi-fixture-session"
        );
        assert_eq!(
            f.ok(
                "checkout",
                &["goal", "list", "--claimed-by", "pi-fixture-session"]
            )
            .pointer("/goals/0/root/id")
            .unwrap(),
            root
        );
        assert!(!run("other-session", &["release", id]).status.success());
        assert!(run("fixture-session", &["release", id]).status.success());
        assert_eq!(
            f.ok(
                "checkout",
                &["goal", "list", "--claimed-by", "pi-fixture-session"]
            )
            .get("goals")
            .unwrap(),
            &serde_json::json!([])
        );
        let output = run("fixture-session", &["claim", id, "--owner", "explicit"]);
        assert!(output.status.success());
        let claimed: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(claimed.pointer("/task/owner").unwrap(), "explicit");
        assert!(
            run("another-session", &["release", id, "--owner", "explicit"])
                .status
                .success()
        );
        assert!(!run("", &["claim", id]).status.success());
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
    fn default_overview_groups_slices_and_keeps_history_explicit() {
        let fixture = Fixture::new();
        assert!(
            fixture
                .fail(
                    "checkout",
                    &["goal", "create", "Broken", "--brief", "briefs/missing.md"]
                )
                .contains("brief is missing")
        );
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
        let created = fixture.ok(
            "checkout",
            &["goal", "create", "Goal", "--brief", "briefs/README.md"],
        );
        let root = created.pointer("/goal/root/id").unwrap().as_str().unwrap();
        let closed = id(&fixture.ok("checkout", &["add", "Finished", "--parent", root]));
        let active = id(&fixture.ok("checkout", &["add", "Next", "--parent", root]));
        fixture.ok("checkout", &["done", &closed, "--result", "verified"]);
        let roots = fixture.ok("linked", &["list"]);
        assert_eq!(
            roots.pointer("/tasks").unwrap().as_array().unwrap().len(),
            1
        );
        assert_eq!(roots.pointer("/tasks/0/id").unwrap(), root);
        let cases: &[(&[&str], usize)] = &[
            (&["list", "--flat"], 2),
            (&["list", "--all"], 1),
            (&["list", "--flat", "--all"], 3),
        ];
        for (args, expected) in cases {
            let listed = fixture.ok("checkout", args);
            assert_eq!(
                listed.pointer("/tasks").unwrap().as_array().unwrap().len(),
                *expected
            );
        }
        let below = fixture.ok("checkout", &["list", "--parent", root]);
        assert_eq!(below.pointer("/tasks/0/id").unwrap(), &active);
        let quiet = fixture.ok("checkout", &["goal", "show", root]);
        assert_eq!(
            quiet
                .pointer("/overview/tasks")
                .unwrap()
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert_eq!(quiet.pointer("/overview/hidden_closed").unwrap(), 1);
        assert_eq!(quiet.pointer("/overview/goal/progress/done").unwrap(), 1);
        let history = fixture.ok("checkout", &["goal", "show", root, "--all"]);
        assert_eq!(
            history
                .pointer("/overview/tasks")
                .unwrap()
                .as_array()
                .unwrap()
                .len(),
            2
        );
        assert_eq!(history.pointer("/overview/hidden_closed").unwrap(), 0);
        fixture.ok("checkout", &["done", &active, "--result", "verified"]);
        fixture.ok(
            "checkout",
            &["done", root, "--result", "combined outcome verified"],
        );
        assert_eq!(
            fixture
                .ok("checkout", &["goal", "list"])
                .pointer("/goals")
                .unwrap()
                .as_array()
                .unwrap()
                .len(),
            0
        );
        assert_eq!(
            fixture
                .ok("checkout", &["goal", "list", "--all"])
                .pointer("/goals")
                .unwrap()
                .as_array()
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn goal_overview_shows_branch_and_bounded_result() {
        let fixture = Fixture::new();
        let goal = fixture.ok(
            "checkout",
            &["goal", "create", "Goal", "--brief", "briefs/README.md"],
        );
        let root = goal.pointer("/goal/root/id").unwrap().as_str().unwrap();
        let fresh = id(&fixture.ok("checkout", &["add", "Fresh", "--parent", root]));
        let text = fixture.readable(&["goal", "show", root]);
        assert!(text.contains(&fresh), "{text}");
        assert!(!text.contains(HISTORY_FOOTER), "{text}");

        let slice = id(&fixture.ok("checkout", &["add", "Slice", "--parent", root]));
        fixture.ok("checkout", &["update", &slice, "--branch", "topic/slice"]);
        let result = format!(
            "{}👨\u{200d}👩\u{200d}👧 tail\nFurther verification details",
            "a".repeat(158)
        );
        fixture.ok("checkout", &["update", &slice, "--result", &result]);
        fixture.ok("checkout", &["hold", &slice, "--reason", "awaiting review"]);

        let text = fixture.readable(&["goal", "show", root]);
        assert!(text.contains("  Branch: topic/slice\n"), "{text}");
        assert!(
            text.contains(&format!("  Result: {}…\n", "a".repeat(158))),
            "{text}"
        );
        assert!(!text.contains("Further verification details"), "{text}");
        assert!(!text.contains("awaiting review"), "{text}");
        assert_eq!(text.matches(HISTORY_FOOTER).count(), 1, "{text}");
        let shown = fixture.ok("checkout", &["goal", "show", root]);
        assert_eq!(shown.pointer("/overview/tasks/1/result").unwrap(), &result);
        assert_eq!(shown.pointer("/overview/goal/progress/held").unwrap(), 1);
    }

    #[test]
    fn goal_overview_draws_leaf_dependencies_through_groups() {
        let fixture = Fixture::new();
        let goal = fixture.ok(
            "checkout",
            &["goal", "create", "Goal", "--brief", "briefs/README.md"],
        );
        let root = goal.pointer("/goal/root/id").unwrap().as_str().unwrap();
        let core = id(&fixture.ok("checkout", &["add", "Core", "--parent", root]));
        let types = id(&fixture.ok("checkout", &["add", "Types", "--parent", &core]));
        let exports = id(&fixture.ok(
            "checkout",
            &["add", "Exports", "--parent", &core, "--blocked-by", &types],
        ));
        let consumer = id(&fixture.ok(
            "checkout",
            &["add", "Consumer", "--parent", root, "--blocked-by", &core],
        ));
        fixture.ok("checkout", &["done", &types, "--result", "verified"]);

        let shown = fixture.ok("checkout", &["goal", "show", root, "--all"]);
        let rows = |pointer: &str| -> Vec<String> {
            shown
                .pointer(pointer)
                .unwrap()
                .as_array()
                .unwrap()
                .iter()
                .map(|row| row["task"].as_str().unwrap().to_owned())
                .collect()
        };
        assert_eq!(
            rows("/overview/graph/rows"),
            [types.as_str(), &exports, &consumer]
        );
        assert_eq!(
            rows("/overview/open_graph/rows"),
            [exports.as_str(), &consumer]
        );
        let first = &shown.pointer("/overview/graph/rows/0/lines").unwrap()[0];
        assert!(first.as_str().unwrap().starts_with('✓'), "{shown}");
        let layered = shown
            .pointer("/overview/graph/layered")
            .unwrap()
            .as_str()
            .unwrap();
        assert!(layered.contains("○ Exports · Core"), "{layered}");
        assert_eq!(
            shown.pointer("/overview/graph/notes").unwrap(),
            &Value::Array(vec![])
        );

        let open = fixture.ok("checkout", &["goal", "show", root]);
        assert_eq!(open.pointer("/overview/open_graph").unwrap(), &Value::Null);
        let text = fixture.readable(&["goal", "show", root, "--all"]);
        assert!(text.contains("Dependencies:\n  ✓"), "{text}");
        assert!(text.contains(&format!("{consumer} Consumer")), "{text}");
    }

    #[test]
    fn goals_share_workspace_briefs_and_report_missing_documents() {
        let fixture = Fixture::new();
        let root = id(&fixture.ok("checkout", &["add", "Goal"]));
        let slice = id(&fixture.ok("checkout", &["add", "Slice", "--parent", &root]));
        assert_eq!(
            fixture
                .ok("checkout", &["goal", "list"])
                .pointer("/goals")
                .unwrap(),
            &serde_json::json!([])
        );
        assert!(
            fixture
                .fail(
                    "checkout",
                    &["goal", "attach", &root, "--brief", "briefs/missing.md"]
                )
                .contains("brief is missing")
        );
        assert!(
            fixture
                .fail("checkout", &["goal", "attach", &root, "--brief", "."])
                .contains("relative components")
        );
        assert!(
            fixture
                .fail(
                    "checkout",
                    &["goal", "attach", &slice, "--brief", "briefs/README.md"]
                )
                .contains("root task")
        );
        let attached = fixture.ok(
            "linked",
            &[
                "--directory",
                "../checkout",
                "goal",
                "attach",
                &root,
                "--brief",
                "briefs/README.md",
            ],
        );
        let brief = fixture.workspace().join("briefs/README.md");
        assert_eq!(
            attached.pointer("/goal/brief/path").unwrap(),
            brief.to_str().unwrap()
        );
        assert_eq!(attached.pointer("/goal/root/id").unwrap(), &root);
        assert!(attached.pointer("/goal/root/owner").unwrap().is_null());
        // A missing document remains an inspectable reference, not an empty brief.
        fs::remove_file(&brief).unwrap();
        let overview = fixture.ok("linked", &["goal", "show", &root]);
        assert_eq!(
            overview.pointer("/overview/goal/brief/path").unwrap(),
            brief.to_str().unwrap()
        );
        assert_eq!(overview.pointer("/overview/tasks/0/id").unwrap(), &slice);
        assert_eq!(
            overview.pointer("/overview/goal/progress/ready").unwrap(),
            1
        );
        assert_eq!(
            overview.pointer("/overview/goal/progress/total").unwrap(),
            1
        );
        assert_eq!(
            fixture
                .ok("checkout", &["goal", "list"])
                .pointer("/goals")
                .unwrap()
                .as_array()
                .unwrap()
                .len(),
            1
        );
        let text = Command::new(env!("CARGO_BIN_EXE_dots"))
            .args(["task", "goal", "show", &root])
            .current_dir(fixture.root.join("linked"))
            .env("XDG_STATE_HOME", fixture.root.join("state"))
            .env("HOME", fixture.root.join("home"))
            .env("XDG_DATA_HOME", fixture.root.join("data"))
            .output()
            .unwrap();
        assert!(text.status.success());
        let text = String::from_utf8(text.stdout).unwrap();
        assert!(text.contains("  1 open · last activity "), "{text}");
        assert!(text.contains("Brief unavailable"), "{text}");
        fixture.ok("linked", &["goal", "detach", &root]);
        assert_eq!(id(&fixture.ok("checkout", &["show", &slice])), slice);
        assert!(
            fixture
                .fail("checkout", &["goal", "show", &root])
                .contains("no goal association")
        );
    }

    #[test]
    #[expect(
        clippy::too_many_lines,
        reason = "One cross-worktree scenario checks claims, reference recording, and accepted completion"
    )]
    fn worktrees_share_one_list_and_claims_are_exclusive() {
        let fixture = Fixture::new();
        let epic = fixture.ok(
            "checkout",
            &["add", "migrate policies", "--body", "pattern: docs/net.md"],
        );
        let epic = id(&epic);
        let first = id(&fixture.ok("checkout", &["add", "first", "--parent", &epic]));
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
                .contains("claimed by session-a")
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
                .fail(
                    "linked",
                    &[
                        "--owner",
                        "session-b",
                        "done",
                        &first,
                        "--result",
                        "verified"
                    ]
                )
                .contains("claimed by session-a")
        );

        rejects_misuse(&fixture, &epic, &first);
        fixture.ok(
            "checkout",
            &[
                "--owner",
                "session-a",
                "update",
                &first,
                "--branch",
                "netpol/first",
            ],
        );
        fixture.ok(
            "checkout",
            &[
                "--owner",
                "session-a",
                "ref",
                "set",
                &first,
                "https://github.com/example/project/pull/1",
            ],
        );
        let all = fixture.ok("checkout", &["list", "--flat", "--all"]);
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
            &["--owner", "session-a", "done", &first, "--result", "merged"],
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
                .contains("No session identity available; pass --owner")
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
                .fail(
                    "checkout",
                    &["--owner", "session-a", "done", epic, "--result", "verified"]
                )
                .contains("cannot be closed yet")
        );
        assert!(
            fixture
                .fail(
                    "checkout",
                    &[
                        "ref",
                        "set",
                        first,
                        "https://github.com/example/project/pull/9",
                    ]
                )
                .contains("claimed by session-a"),
            "field edits on another session's claim are refused"
        );
        assert!(
            fixture
                .fail(
                    "checkout",
                    &["--owner", "session-a", "ref", "set", first, "PR 1",]
                )
                .contains("full HTTP(S)")
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
        assert_eq!(
            kinds,
            ["created", "claimed", "updated", "reference_set", "done"]
        );
        assert_eq!(shown.pointer("/task/branch").unwrap(), "netpol/first");
        assert_eq!(shown.pointer("/task/last_event/kind").unwrap(), "done");
        assert_eq!(shown.pointer("/task/last_event/result").unwrap(), "merged");
        assert_eq!(
            shown.pointer("/task/references/0/url").unwrap(),
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
    fn local_review_and_issue_references_survive_cold_cli_invocations() {
        let fixture = Fixture::new();
        let task = id(&fixture.ok("checkout", &["add", "local implementation"]));
        assert!(
            fixture
                .fail("checkout", &["done", &task])
                .contains("--result")
        );
        assert!(
            fixture
                .fail("checkout", &["review", &task, "--result", " "])
                .contains("nonempty")
        );
        fixture.ok(
            "checkout",
            &[
                "ref",
                "set",
                &task,
                "https://linear.app/example/issue/ENG-123",
                "--label",
                "ENG-123",
            ],
        );
        assert_eq!(
            fixture
                .ok("linked", &["show", &task])
                .pointer("/task/ready")
                .unwrap(),
            true
        );
        let reviewed = fixture.ok(
            "checkout",
            &[
                "review",
                &task,
                "--result",
                "Unit checks pass; integration pending.",
            ],
        );
        assert_eq!(reviewed.pointer("/task/status").unwrap(), "review");
        assert_eq!(
            fixture.ok("linked", &["ready"]).pointer("/tasks").unwrap(),
            &serde_json::json!([])
        );
        fixture.ok("linked", &["claim", &task, "--owner", "reviewer"]);
        let done = fixture.ok(
            "linked",
            &[
                "done",
                &task,
                "--owner",
                "reviewer",
                "--result",
                "Integrated and verified.",
            ],
        );
        assert_eq!(
            done.pointer("/task/result").unwrap(),
            "Integrated and verified."
        );
        assert_eq!(done.pointer("/task/references/0/label").unwrap(), "ENG-123");
        fixture.ok(
            "checkout",
            &[
                "ref",
                "remove",
                &task,
                "https://linear.app/example/issue/ENG-123",
            ],
        );
        let reopened = fixture.ok("checkout", &["reopen", &task]);
        assert_eq!(
            reopened.pointer("/task/result").unwrap(),
            "Integrated and verified."
        );
        assert_eq!(
            reopened.pointer("/task/references").unwrap(),
            &serde_json::json!([])
        );
    }

    #[test]
    fn repeated_review_handoff_survives_cold_cli_invocations() {
        let fixture = Fixture::new();
        let task = id(&fixture.ok("checkout", &["add", "handoff"]));
        fixture.ok(
            "checkout",
            &["review", &task, "--result", "Initial handoff"],
        );
        fixture.ok("linked", &["claim", &task, "--owner", "reviewer"]);
        let handed_off = fixture.ok(
            "linked",
            &[
                "review",
                &task,
                "--owner",
                "reviewer",
                "--result",
                "Corrections checked; acceptance pending.",
            ],
        );
        assert_eq!(handed_off.pointer("/task/status").unwrap(), "review");
        assert_eq!(
            handed_off.pointer("/task/result").unwrap(),
            "Corrections checked; acceptance pending."
        );
        assert!(handed_off.pointer("/task/owner").unwrap().is_null());
        let saved = fixture.ok("checkout", &["show", &task]);
        assert_eq!(
            saved.pointer("/task/result"),
            handed_off.pointer("/task/result")
        );
    }

    #[test]
    fn explicit_project_keys_are_isolated_and_required_outside_git() {
        let fixture = Fixture::new();
        fs::create_dir_all(fixture.root.join("plain")).unwrap();
        assert!(fixture.fail("plain", &["list"]).contains("--project"));
        let registered = Command::new(env!("CARGO_BIN_EXE_dots"))
            .args(["--json", "project", "register", "notes"])
            .env("HOME", fixture.root.join("home"))
            .env("XDG_DATA_HOME", fixture.root.join("data"))
            .output()
            .unwrap();
        assert!(registered.status.success());
        let registered: Value = serde_json::from_slice(&registered.stdout).unwrap();
        let project = registered.pointer("/project/id").unwrap().as_str().unwrap();
        let created = fixture.ok("plain", &["--project", project, "add", "write docs"]);
        assert_eq!(created.pointer("/project").unwrap(), project);
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
            .args(["task", "--project", project, "list"])
            .current_dir(&fixture.root)
            .env("XDG_STATE_HOME", fixture.root.join("state"))
            .env("HOME", fixture.root.join("home"))
            .env("XDG_DATA_HOME", fixture.root.join("data"))
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
                .contains("claimed by session-a")
        );
        let other = id(&fixture.ok("checkout", &["add", "other"]));
        let warned = fixture.ok("checkout", &["dep", "add", &slice, "--blocked-by", &other]);
        assert!(
            warned
                .pointer("/warnings/0")
                .unwrap()
                .as_str()
                .unwrap()
                .contains("claimed by session-a")
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
    }

    #[test]
    fn event_kinds_need_no_catalog_and_follow_rejects_a_page_limit() {
        let fixture = Fixture::new();
        fs::remove_file(fixture.root.join("data/dots/workspace.sqlite3")).unwrap();
        let kinds = fixture.ok("checkout", &["event-kinds"]);
        let kinds: Vec<_> = kinds["kinds"]
            .as_array()
            .unwrap()
            .iter()
            .map(|kind| kind.as_str().unwrap())
            .collect();
        for kind in [
            "created",
            "claimed",
            "review",
            "done",
            "update-result",
            "goal_attached",
        ] {
            assert!(kinds.contains(&kind), "{kind} in {kinds:?}");
        }
        assert!(
            fixture
                .fail("checkout", &["events", "--follow", "--limit", "5"])
                .contains("cannot be used with")
        );
    }

    #[test]
    fn events_under_a_task_page_its_subtree_with_the_project_head() {
        let fixture = Fixture::new();
        let id = |value: &Value| {
            value
                .pointer("/task/id")
                .unwrap()
                .as_str()
                .unwrap()
                .to_owned()
        };
        let root = id(&fixture.ok("checkout", &["add", "root"]));
        let child = id(&fixture.ok("checkout", &["add", "child", "--parent", &root]));
        fixture.ok("checkout", &["add", "elsewhere"]);

        let head = fixture.ok("checkout", &["events", "--limit", "0"]);
        assert_eq!(
            head,
            serde_json::json!({ "events": [], "head": 3, "project": fixture.project, "now": head["now"] })
        );
        let page = fixture.ok("checkout", &["events", "--under", &root]);
        let tasks: Vec<_> = page["events"]
            .as_array()
            .unwrap()
            .iter()
            .map(|event| event["task"].as_str().unwrap())
            .collect();
        assert_eq!(tasks, [root.as_str(), child.as_str()]);
        assert_eq!(page["head"], 3, "the head stays the project's");

        let error = fixture.fail("checkout", &["events", "--under", "t-00"]);
        assert!(error.contains("t-00"), "{error}");
    }
}
