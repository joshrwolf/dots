//! Task tree replication between two machines with neutral disposable homes
//! and one `file://` remote.
#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::process::{Command, Output};
    use std::rc::Rc;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::Duration;

    use serde_json::{Value, json};

    fn scratch(kind: &str) -> PathBuf {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "dots-share-cli-{kind}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).unwrap();
        fs::canonicalize(root).unwrap()
    }

    fn git(directory: &Path, args: &[&str]) {
        let mut command = Command::new("git");
        command
            .env_clear()
            .env("PATH", std::env::var_os("PATH").expect("test PATH"))
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .current_dir(directory)
            .args(args);
        assert!(
            dots_process::Runner::new(Duration::from_secs(10), 64 * 1024)
                .output(command)
                .unwrap()
                .status
                .success(),
            "git {args:?}"
        );
    }

    struct Remote {
        root: PathBuf,
    }
    impl Remote {
        fn new() -> Self {
            let root = scratch("remote");
            git(&root, &["init", "--bare", "-q", "remote.git"]);
            Self { root }
        }
        fn url(&self) -> String {
            format!("file://{}", self.root.join("remote.git").display())
        }
    }
    impl Drop for Remote {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    /// One machine: its own home, data, catalog and checkout.
    struct Machine {
        root: PathBuf,
        _remote: Rc<Remote>,
        project: String,
    }
    impl Machine {
        fn on(remote: &Rc<Remote>) -> Self {
            let root = scratch("machine");
            fs::create_dir_all(root.join("home")).unwrap();
            git(&root, &["init", "-q", "checkout"]);
            let mut machine = Self {
                root,
                _remote: Rc::clone(remote),
                project: String::new(),
            };
            machine.ok(&["substrate", "init", "--remote", &remote.url()]);
            let catalog = machine.ok(&["project", "init", "neutral", "--fresh"]);
            machine.project = str_at(&catalog, "/project/id").to_owned();
            machine
        }
        fn plain(&self) -> Command {
            let mut command = Command::new(env!("CARGO_BIN_EXE_dots"));
            command
                .env_clear()
                .env("PATH", std::env::var_os("PATH").expect("test PATH"))
                .env("HOME", self.root.join("home"))
                .env("XDG_DATA_HOME", self.root.join("data"))
                .env("XDG_STATE_HOME", self.root.join("state"))
                .current_dir(self.root.join("checkout"));
            command
        }
        fn command(&self, args: &[&str]) -> Command {
            let mut command = self.plain();
            command.args(["--json"]).args(args);
            command
        }
        /// Readable output, without --json.
        fn text(&self, args: &[&str]) -> String {
            let mut command = self.plain();
            command.args(args);
            let output = dots_process::Runner::new(Duration::from_secs(30), 4 * 1024 * 1024)
                .output(command)
                .unwrap();
            assert!(output.status.success(), "{args:?}");
            String::from_utf8(output.stdout).unwrap()
        }
        fn run(&self, args: &[&str], input: Option<&Value>) -> Output {
            let runner = dots_process::Runner::new(Duration::from_secs(30), 4 * 1024 * 1024);
            let runner = match input {
                Some(value) => {
                    let bytes = serde_json::to_vec(value).unwrap();
                    let limit = bytes.len() + 1;
                    runner.stdin(bytes, limit)
                }
                None => runner,
            };
            runner.output(self.command(args)).unwrap()
        }
        fn ok_with(&self, args: &[&str], input: Option<&Value>) -> Value {
            let output = self.run(args, input);
            assert!(
                output.status.success(),
                "{args:?}: {}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            serde_json::from_slice(&output.stdout).unwrap()
        }
        fn ok(&self, args: &[&str]) -> Value {
            self.ok_with(args, None)
        }
        fn fail(&self, args: &[&str]) -> String {
            let output = self.run(args, None);
            assert!(!output.status.success(), "{args:?}: unexpectedly succeeded");
            format!(
                "{}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            )
        }
        fn sync(&self) -> Value {
            self.ok(&["sync", "once"])
        }
        fn workspace(&self) -> PathBuf {
            self.root.join("data/dots/workspaces").join(&self.project)
        }
    }
    impl Drop for Machine {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    fn remote_refs(remote: &Remote) -> Vec<String> {
        let output = Command::new("git")
            .env_clear()
            .env("PATH", std::env::var_os("PATH").expect("test PATH"))
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .args(["for-each-ref", "--format=%(refname)"])
            .current_dir(remote.root.join("remote.git"))
            .output()
            .unwrap();
        String::from_utf8(output.stdout)
            .unwrap()
            .lines()
            .map(str::to_owned)
            .collect()
    }

    fn str_at<'value>(value: &'value Value, pointer: &str) -> &'value str {
        value
            .pointer(pointer)
            .unwrap_or_else(|| panic!("{pointer} in {value}"))
            .as_str()
            .unwrap()
    }

    #[test]
    #[expect(
        clippy::too_many_lines,
        reason = "one scenario follows a goal across two machines"
    )]
    fn goals_replicate_read_only_move_home_and_carry_answers() {
        let remote = Rc::new(Remote::new());
        let a = Machine::on(&remote);
        let b = Machine::on(&remote);
        let genesis = a.ok(&["memory", "init"]);
        let shared = a.ok(&[
            "memory",
            "project",
            "create",
            "neutral",
            "--expected-snapshot",
            str_at(&genesis, "/snapshot"),
        ]);
        let shared = str_at(&shared, "/project").to_owned();
        a.ok(&["memory", "project", "bind", &shared]);
        b.sync();
        b.ok(&["memory", "project", "bind", &shared]);

        // Disabled sharing exports nothing.
        let brief = "# Neutral goal\n\nMove the widget.\n";
        fs::create_dir_all(a.workspace().join("briefs")).unwrap();
        fs::write(a.workspace().join("briefs/goal.md"), brief).unwrap();
        let created = a.ok(&[
            "task",
            "goal",
            "create",
            "Goal",
            "--brief",
            "briefs/goal.md",
        ]);
        let root = str_at(&created, "/goal/root/id").to_owned();
        a.sync();
        assert!(
            remote_refs(&remote)
                .iter()
                .all(|name| !name.contains("tasks/")),
            "{:?}",
            remote_refs(&remote)
        );

        assert_eq!(a.ok(&["task", "share", "status"])["sharing"], Value::Null);
        a.ok(&["task", "share", "enable"]);
        b.ok(&["task", "share", "enable"]);
        a.ok(&["task", "--owner", "coord", "goal", "coordinate", &root]);
        let child = a.ok(&["task", "--owner", "coord", "add", "Leaf", "--parent", &root]);
        let child = str_at(&child, "/task/id").to_owned();
        // Pushes stay within the default budget of two per half minute.
        let raised = a.ok_with(
            &[
                "attention",
                "--owner",
                "coord",
                "raise",
                &root,
                "--key",
                "layout",
                "--content-file",
                "-",
            ],
            Some(&json!({
                "kind": "decision",
                "title": "Choose the layout",
                "question": "Which layout?",
                "choices": [{"id": "one", "label": "One"}, {"id": "two", "label": "Two"}],
            })),
        );
        let request = str_at(&raised, "/request/id").to_owned();
        let pending = a.ok(&["task", "share", "status"]);
        assert_eq!(pending["pending"].as_array().unwrap().len(), 1, "{pending}");
        assert!(
            std::path::Path::new(str_at(&pending, "/changes")).is_absolute(),
            "{pending}"
        );
        a.sync();

        // B imports with --fresh and refuses changes with the takeover hint.
        let shown = b.ok(&["task", "--fresh", "show", &child]);
        assert_eq!(str_at(&shown, "/task/foreign/root"), root);
        let goal = b.ok(&["task", "goal", "show", &root]);
        assert_eq!(
            str_at(&goal, "/overview/goal/brief/document/content"),
            brief
        );
        let refused = b.fail(&["task", "--owner", "other", "update", &child, "--title", "x"]);
        assert!(
            refused.contains(&format!("dots task share take {root}")),
            "{refused}"
        );

        // An answer on B reaches A, whose coordinator incorporates it.
        let answered = b.ok_with(
            &[
                "attention",
                "--fresh",
                "--owner",
                "human",
                "answer",
                &request,
                "--content-file",
                "-",
            ],
            Some(&json!({"choice": "two", "source": "ui", "provenance": "neutral viewer"})),
        );
        assert_eq!(str_at(&answered, "/request/state"), "answered");
        b.sync();
        let at_home = a.ok(&["attention", "--fresh", "show", &request]);
        assert_eq!(str_at(&at_home, "/request/state"), "answered");
        assert_eq!(str_at(&at_home, "/request/answer/choice"), "two");
        a.ok_with(
            &[
                "attention",
                "--owner",
                "coord",
                "incorporate",
                &request,
                "--content-file",
                "-",
            ],
            Some(&json!({"kind": "decision", "decision": "Use two."})),
        );
        a.sync();
        let closed = b.ok(&["attention", "--fresh", "show", &request]);
        assert_eq!(str_at(&closed, "/request/state"), "closed");

        // B takes over, writes, and A sees the tree read-only.
        let taken = b.ok(&["task", "share", "take", &root]);
        assert_eq!(str_at(&taken, "/taken/root"), root);
        assert_eq!(
            fs::read_to_string(b.workspace().join("briefs/goal.md")).unwrap(),
            brief
        );
        b.ok(&["task", "--owner", "coord-b", "goal", "coordinate", &root]);
        b.ok(&[
            "task", "--owner", "coord-b", "update", &child, "--title", "Renamed",
        ]);
        b.sync();
        let seen = a.ok(&["task", "--fresh", "show", &child]);
        assert_eq!(str_at(&seen, "/task/title"), "Renamed");
        assert!(seen.pointer("/task/foreign").is_some());
        let refused = a.fail(&["task", "--owner", "coord", "update", &child, "--title", "y"]);
        assert!(refused.contains("share take"), "{refused}");

        // Unbound projects never leave the machine.
        let refs = remote_refs(&remote);
        assert!(
            refs.iter()
                .any(|name| name.contains(&format!("tasks/{shared}/{root}")))
        );
        assert!(refs.iter().all(|name| !name.contains(&a.project)));
        assert!(refs.iter().all(|name| !name.contains(&b.project)));
    }

    #[test]
    fn an_update_refused_after_a_takeover_is_listed_with_what_never_published() {
        let remote = Rc::new(Remote::new());
        let a = Machine::on(&remote);
        let b = Machine::on(&remote);
        let genesis = a.ok(&["memory", "init"]);
        let shared = a.ok(&[
            "memory",
            "project",
            "create",
            "neutral",
            "--expected-snapshot",
            str_at(&genesis, "/snapshot"),
        ]);
        let shared = str_at(&shared, "/project").to_owned();
        a.ok(&["memory", "project", "bind", &shared]);
        b.sync();
        b.ok(&["memory", "project", "bind", &shared]);
        a.ok(&["task", "share", "enable"]);
        b.ok(&["task", "share", "enable"]);
        let root = str_at(&a.ok(&["task", "add", "Loose"]), "/task/id").to_owned();
        a.sync();
        b.sync();

        // B takes the tree over while A, offline, keeps writing.
        b.ok(&["task", "share", "take", &root]);
        b.ok(&["task", "update", &root, "--title", "By B"]);
        b.sync();
        a.ok(&["task", "update", &root, "--title", "Offline by A"]);
        a.sync();

        let substrate = a.ok(&["substrate", "status"]);
        let stranded = substrate["stranded"].as_array().unwrap();
        assert_eq!(stranded.len(), 1, "{substrate}");
        assert_eq!(
            str_at(&stranded[0], "/ref"),
            format!("tasks/{shared}/{root}")
        );
        let status = a.ok(&["task", "share", "status"]);
        let tree = &status["stranded"][0];
        assert_eq!(str_at(tree, "/root"), root, "{status}");
        assert_eq!(str_at(tree, "/tasks/0/title"), "Offline by A", "{status}");
        assert!(
            tree["events"]
                .as_array()
                .unwrap()
                .iter()
                .any(|event| event["kind"] == "updated"),
            "{status}"
        );
        let text = String::from_utf8(a.run(&["task", "share", "status"], None).stdout).unwrap();
        assert!(text.contains("Offline by A"), "{text}");
        let shown = a.ok(&["task", "show", &root]);
        assert_eq!(str_at(&shown, "/task/title"), "By B");
        assert!(shown.pointer("/task/foreign").is_some(), "{shown}");
        let refused = a.fail(&["task", "update", &root, "--title", "Again"]);
        assert!(refused.contains("share take"), "{refused}");
    }

    fn tree_published(remote: &Remote, shared: &str, root: &str) -> bool {
        remote_refs(remote)
            .iter()
            .any(|name| name.ends_with(&format!("tasks/{shared}/{root}")))
    }

    #[test]
    fn project_share_reports_declines_creates_and_binds_and_queues_existing_trees() {
        let remote = Rc::new(Remote::new());
        let a = Machine::on(&remote);
        a.ok(&["memory", "init"]);
        a.ok(&["task", "share", "enable"]);
        let root = str_at(&a.ok(&["task", "add", "Before binding"]), "/task/id").to_owned();

        let status = a.ok(&["project", "share"]);
        assert_eq!(status["sharing"], "unshared", "{status}");
        assert_eq!(str_at(&status, "/project/id"), a.project);
        assert_eq!(status["shared"], Value::Null);
        assert_eq!(status["machine_sharing"], true);
        assert_eq!(status["identities"], json!([]));
        assert_eq!(a.ok(&["memory", "context"])["sharing"], "unshared");
        let text = a.text(&["project", "share"]);
        assert!(text.contains("not shared across machines"), "{text}");
        assert!(text.contains("Set up: dots project share --bind"), "{text}");

        // Outside Git there is nothing to share.
        let home = a.root.join("home");
        let home = home.to_str().unwrap();
        let outside = a.fail(&["--directory", home, "project", "share"]);
        assert!(outside.contains("requires a Git checkout"), "{outside}");
        assert_eq!(
            a.ok(&["--directory", home, "memory", "context"])["sharing"],
            Value::Null
        );

        // An unregistered checkout still reports this machine's sharing.
        let unregistered = a.root.join("unregistered");
        fs::create_dir_all(&unregistered).unwrap();
        git(&unregistered, &["init", "-q"]);
        let unregistered = a.ok(&[
            "--directory",
            unregistered.to_str().unwrap(),
            "project",
            "share",
        ]);
        assert_eq!(unregistered["project"], Value::Null, "{unregistered}");
        assert_eq!(unregistered["sharing"], "unshared");
        assert_eq!(unregistered["machine_sharing"], true);

        let declined = a.ok(&["project", "share", "--decline"]);
        assert_eq!(declined["sharing"], "declined", "{declined}");
        assert_eq!(a.ok(&["memory", "context"])["sharing"], "declined");
        let conflicting = a.fail(&["project", "share", "--decline", "--create", "x"]);
        assert!(conflicting.contains("cannot be used with"), "{conflicting}");

        // Creating binds, clears the decline and queues the existing tree.
        let created = a.ok(&["project", "share", "--create", "neutral"]);
        assert_eq!(created["sharing"], "bound", "{created}");
        let shared = str_at(&created, "/shared/id").to_owned();
        assert_eq!(str_at(&created, "/shared/name"), "neutral");
        assert_eq!(a.ok(&["memory", "context"])["sharing"], "bound");
        a.sync();
        assert!(tree_published(&remote, &shared, &root));
        let again = a.fail(&["project", "share", "--create", "neutral"]);
        assert!(
            again.contains(&format!("already shared as {shared}")),
            "{again}"
        );
        let decline = a.fail(&["project", "share", "--decline"]);
        assert!(decline.contains("memory project unbind"), "{decline}");

        // Another machine sees the identity and binds to it; an unknown one
        // lists the valid choices.
        let b = Machine::on(&remote);
        b.sync();
        let listed = b.ok(&["project", "share"]);
        assert_eq!(
            listed["identities"],
            json!([{"id": shared, "name": "neutral"}]),
            "{listed}"
        );
        let unknown = b.fail(&[
            "project",
            "share",
            "--bind",
            "p-00000000000000000000000000000000",
        ]);
        assert!(
            unknown.contains(&format!("known identities: {shared} (neutral)")),
            "{unknown}"
        );
        let bound = b.ok(&["project", "share", "--bind", &shared]);
        assert_eq!(str_at(&bound, "/shared/id"), shared, "{bound}");

        // The memory primitive queues existing trees too.
        let c = Machine::on(&remote);
        c.sync();
        c.ok(&["task", "share", "enable"]);
        let loose = str_at(&c.ok(&["task", "add", "Loose on C"]), "/task/id").to_owned();
        let primitive = c.ok(&["memory", "project", "bind", &shared]);
        assert_eq!(primitive["queued"], 1, "{primitive}");
        c.sync();
        assert!(tree_published(&remote, &shared, &loose));
    }
}
