//! Shared knowledge CLI contracts with neutral disposable homes and a `file://` remote.
#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::process::{Command, Output};
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::Duration;

    use serde_json::{Value, json};

    fn scratch(kind: &str) -> PathBuf {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "dots-knowledge-cli-{kind}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).unwrap();
        root
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

    /// A bare repository reached over `file://`, shared by every machine.
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
        /// Make the remote unreachable until `restore`.
        fn disconnect(&self) {
            fs::rename(self.root.join("remote.git"), self.root.join("aside.git")).unwrap();
        }
        fn restore(&self) {
            fs::rename(self.root.join("aside.git"), self.root.join("remote.git")).unwrap();
        }
    }
    impl Drop for Remote {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    /// One machine: its own home and data directory, configured for `remote`.
    struct Fixture {
        root: PathBuf,
        remote: std::rc::Rc<Remote>,
    }
    impl Fixture {
        fn new() -> Self {
            Self::on(std::rc::Rc::new(Remote::new()))
        }
        fn on(remote: std::rc::Rc<Remote>) -> Self {
            let root = scratch("machine");
            fs::create_dir_all(root.join("checkout")).unwrap();
            fs::create_dir_all(root.join("home")).unwrap();
            let fixture = Self { root, remote };
            fixture.ok(&["substrate", "init", "--remote", &fixture.remote.url()]);
            fixture
        }
        /// Another machine on the same remote.
        fn machine(&self) -> Self {
            Self::on(self.remote.clone())
        }
        fn command(&self, args: &[&str]) -> Command {
            let mut command = Command::new(env!("CARGO_BIN_EXE_dots"));
            command
                .env_clear()
                .env("PATH", std::env::var_os("PATH").expect("test PATH"))
                .env("HOME", self.root.join("home"))
                .env("XDG_DATA_HOME", self.root.join("data"))
                .env("XDG_STATE_HOME", self.root.join("state"))
                .current_dir(self.root.join("checkout"))
                .args(["--json"])
                .args(args);
            command
        }
        fn run(&self, args: &[&str]) -> Output {
            dots_process::Runner::new(Duration::from_secs(20), 4 * 1024 * 1024)
                .output(self.command(args))
                .unwrap()
        }
        fn run_input(&self, args: &[&str], input: &[u8]) -> Output {
            dots_process::Runner::new(Duration::from_secs(20), 4 * 1024 * 1024)
                .stdin(input.to_vec(), input.len() + 1)
                .output(self.command(args))
                .unwrap()
        }
        fn ok(&self, args: &[&str]) -> Value {
            let output = self.run(args);
            assert!(
                output.status.success(),
                "{args:?}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            serde_json::from_slice(&output.stdout).unwrap()
        }
        fn fail(&self, args: &[&str]) -> String {
            let output = self.run(args);
            assert!(!output.status.success(), "{args:?}: unexpectedly succeeded");
            if !output.stdout.is_empty() {
                let failure: Value = serde_json::from_slice(&output.stdout).unwrap();
                assert!(
                    failure["error"].is_string(),
                    "failed command wrote success data: {failure}"
                );
                assert!(failure.get("snapshot").is_none());
            }
            format!(
                "{}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            )
        }
        fn init(&self) -> Value {
            self.ok(&["memory", "init"])
        }
        fn input(&self, name: &str, value: &Value) -> String {
            let path = self.root.join(name);
            fs::write(&path, serde_json::to_vec(value).unwrap()).unwrap();
            path.to_str().unwrap().to_owned()
        }
        fn publish(&self, expected: &str, changes: &Value) -> Value {
            let path = self.input("changes.json", changes);
            self.ok(&[
                "memory",
                "publish",
                "--expected-snapshot",
                expected,
                "--input-file",
                &path,
            ])
        }
        fn catalog(&self) -> Value {
            git(&self.root, &["init", "-q", "checkout"]);
            self.ok(&["project", "init", "neutral", "--fresh"])
        }
        fn local(&self) -> PathBuf {
            self.root.join("data/dots/memory")
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }
    fn write(scope: &Value, path: &str, content: &str) -> Value {
        json!({"scope": scope, "path": path, "action": {"kind": "write", "content": content}})
    }
    fn snapshot(value: &Value) -> &str {
        value["snapshot"].as_str().unwrap()
    }

    #[test]
    fn explicit_initialization_pinned_reads_atomic_batches_and_missing_empty_removed() {
        let f = Fixture::new();
        assert!(f.fail(&["memory", "context"]).contains("never synced"));
        assert!(
            f.fail(&["memory", "--fresh", "context"])
                .contains("memory init")
        );
        let genesis = f.init();
        assert!(snapshot(&genesis).starts_with("sha256:"));
        f.fail(&["memory", "init"]);
        let before = f.ok(&["memory", "read", "MEMORY.md", "--scope", "global"]);
        assert_eq!(before["document"]["exists"], false);
        let first = f.publish(
            snapshot(&genesis),
            &json!([
                write(&json!({"kind":"global"}), "MEMORY.md", "Read topic.\n"),
                write(&json!({"kind":"global"}), "wiki/topic.md", "durable fact\n"),
                write(&json!({"kind":"global"}), "empty.md", "")
            ]),
        );
        let context = f.ok(&["memory", "context"]);
        assert_eq!(context["global"]["snapshot"], first["snapshot"]);
        assert_eq!(context["global"]["document"]["path"], "MEMORY.md");
        assert!(context["project"].is_null());
        assert!(context["diagnostic"].is_string());
        let empty = f.ok(&["memory", "read", "empty.md", "--scope", "global"]);
        assert_eq!(empty["document"]["exists"], true);
        assert_eq!(empty["document"]["content"], "");
        let second = f.publish(
            snapshot(&first),
            &json!([
                write(&json!({"kind":"global"}), "MEMORY.md", "Updated index\n"),
                {"scope":{"kind":"global"},"path":"empty.md","action":{"kind":"remove"}}
            ]),
        );
        let old = f.ok(&[
            "memory",
            "read",
            "MEMORY.md",
            "--scope",
            "global",
            "--snapshot",
            snapshot(&first),
        ]);
        assert_eq!(old["document"]["content"], "Read topic.\n");
        let gone = f.ok(&["memory", "read", "empty.md", "--scope", "global"]);
        assert_eq!(gone["document"]["exists"], false);
        let stale = f.input(
            "stale.json",
            &json!([write(&json!({"kind":"global"}), "MEMORY.md", "stale")]),
        );
        f.fail(&[
            "memory",
            "publish",
            "--expected-snapshot",
            snapshot(&first),
            "--input-file",
            &stale,
        ]);
        assert_eq!(f.ok(&["memory", "status"])["snapshot"], second["snapshot"]);
        let malformed = f.input(
            "bad.json",
            &json!([{"scope":{"kind":"global"},"path":"MEMORY.md"}]),
        );
        f.fail(&[
            "memory",
            "publish",
            "--expected-snapshot",
            snapshot(&second),
            "--input-file",
            &malformed,
        ]);
        assert_eq!(f.ok(&["memory", "status"])["snapshot"], second["snapshot"]);
    }

    #[test]
    fn single_page_writes_accept_bounded_stdin_and_preserve_preconditions() {
        let f = Fixture::new();
        let genesis = f.init();
        let args = [
            "memory",
            "write",
            "wiki/topic.md",
            "--scope",
            "global",
            "--expected-snapshot",
            snapshot(&genesis),
            "--content-file",
            "-",
        ];
        let output = f.run_input(&args, "# Topic\n\nCafé.\n".as_bytes());
        assert!(output.status.success(), "stdin write: {output:?}");
        let first: Value = serde_json::from_slice(&output.stdout).unwrap();
        let read = f.ok(&["memory", "read", "wiki/topic.md", "--scope", "global"]);
        assert_eq!(read["document"]["content"], "# Topic\n\nCafé.\n");
        let stale = f.run_input(&args, b"stale overwrite\n");
        assert!(!stale.status.success(), "stale stdin write succeeded");
        assert_eq!(f.ok(&["memory", "status"])["snapshot"], first["snapshot"]);

        for input in [
            vec![0xff],
            b"invalid\0text".to_vec(),
            vec![b'x'; dots_memory::MAX_FILE_BYTES + 1],
        ] {
            let output = f.run_input(
                &[
                    "memory",
                    "write",
                    "wiki/topic.md",
                    "--scope",
                    "global",
                    "--expected-snapshot",
                    snapshot(&first),
                    "--content-file",
                    "-",
                ],
                &input,
            );
            assert!(!output.status.success(), "invalid stdin write succeeded");
            assert_eq!(f.ok(&["memory", "status"])["snapshot"], first["snapshot"]);
        }
        f.fail(&[
            "memory",
            "write",
            "wiki/topic.md",
            "--scope",
            "global",
            "--expected-snapshot",
            snapshot(&first),
            "--content-file",
            "absent",
        ]);
        let large = vec![b'\n'; dots_memory::MAX_FILE_BYTES];
        let output = f.run_input(
            &[
                "memory",
                "write",
                "large.md",
                "--scope",
                "global",
                "--expected-snapshot",
                snapshot(&first),
                "--content-file",
                "-",
            ],
            &large,
        );
        assert!(output.status.success(), "exact-budget write: {output:?}");
        let large: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(f.ok(&["memory", "status"])["snapshot"], large["snapshot"]);
    }

    #[test]
    fn file_input_empty_pages_and_removal_keep_distinct_history() {
        let f = Fixture::new();
        let genesis = f.init();
        fs::write(f.root.join("checkout/-"), "literal dash file\n").unwrap();
        let literal = f.ok(&[
            "memory",
            "write",
            "literal.md",
            "--scope",
            "global",
            "--expected-snapshot",
            snapshot(&genesis),
            "--content-file",
            "./-",
        ]);
        assert_eq!(
            f.ok(&["memory", "read", "literal.md", "--scope", "global"])["document"]["content"],
            "literal dash file\n"
        );
        let output = f.run_input(
            &[
                "memory",
                "write",
                "empty.md",
                "--scope",
                "global",
                "--expected-snapshot",
                snapshot(&literal),
                "--content-file",
                "-",
            ],
            b"",
        );
        assert!(output.status.success(), "empty stdin write: {output:?}");
        let empty: Value = serde_json::from_slice(&output.stdout).unwrap();
        let present = f.ok(&["memory", "read", "empty.md", "--scope", "global"]);
        assert_eq!(present["document"]["exists"], true);
        assert_eq!(present["document"]["content"], "");
        let removed = f.ok(&[
            "memory",
            "remove",
            "empty.md",
            "--scope",
            "global",
            "--expected-snapshot",
            snapshot(&empty),
        ]);
        assert_eq!(
            f.ok(&["memory", "read", "empty.md", "--scope", "global"])["document"]["exists"],
            false
        );
        assert_eq!(
            f.ok(&[
                "memory",
                "read",
                "empty.md",
                "--scope",
                "global",
                "--snapshot",
                snapshot(&empty)
            ])["document"]["exists"],
            true
        );
        // A removal built on an older snapshot merges with the disjoint removal since.
        let merged = f.ok(&[
            "memory",
            "remove",
            "literal.md",
            "--scope",
            "global",
            "--expected-snapshot",
            snapshot(&empty),
        ]);
        assert_ne!(merged["snapshot"], removed["snapshot"]);
        for path in ["literal.md", "empty.md"] {
            let read = f.ok(&["memory", "read", path, "--scope", "global"]);
            assert_eq!(read["document"]["exists"], false, "{path}");
        }
    }

    #[test]
    fn stdin_batches_are_atomic_across_scopes_and_reject_malformed_changes() {
        let f = Fixture::new();
        let genesis = f.init();
        let registered = f.ok(&[
            "memory",
            "project",
            "create",
            "neutral",
            "--expected-snapshot",
            snapshot(&genesis),
        ]);
        let project = registered["project"].as_str().unwrap();
        let changes = serde_json::to_vec(&json!([
            write(&json!({"kind":"global"}), "MEMORY.md", "global index\n"),
            write(
                &json!({"kind":"project", "project": project}),
                "MEMORY.md",
                "project index\n"
            )
        ]))
        .unwrap();
        let args = [
            "memory",
            "publish",
            "--expected-snapshot",
            snapshot(&registered),
            "--input-file",
            "-",
        ];
        let output = f.run_input(&args, &changes);
        assert!(output.status.success(), "stdin batch: {output:?}");
        let published: Value = serde_json::from_slice(&output.stdout).unwrap();
        for scope in ["global", project] {
            let page = f.ok(&["memory", "read", "MEMORY.md", "--scope", scope]);
            assert_eq!(page["snapshot"], published["snapshot"]);
            assert_eq!(page["document"]["exists"], true);
        }
        let diverging = serde_json::to_vec(&json!([
            write(&json!({"kind":"global"}), "MEMORY.md", "diverging\n"),
            write(&json!({"kind":"global"}), "unrelated.md", "unrelated\n")
        ]))
        .unwrap();
        let stale = f.run_input(&args, &diverging);
        assert!(!stale.status.success(), "diverging stdin batch succeeded");
        assert_eq!(
            f.ok(&["memory", "read", "unrelated.md", "--scope", "global"])["document"]["exists"],
            false
        );
        for input in [
            b"{".as_slice(),
            b"[{}]".as_slice(),
            b"[] trailing".as_slice(),
        ] {
            let output = f.run_input(
                &[
                    "memory",
                    "publish",
                    "--expected-snapshot",
                    snapshot(&published),
                    "--input-file",
                    "-",
                ],
                input,
            );
            assert!(!output.status.success(), "malformed stdin batch succeeded");
            assert_eq!(
                f.ok(&["memory", "status"])["snapshot"],
                published["snapshot"]
            );
        }
    }

    #[test]
    fn shared_scope_selectors_and_local_bindings_do_not_leak_or_retarget() {
        let f = Fixture::new();
        let local = f.catalog();
        let genesis = f.init();
        let a = f.ok(&[
            "memory",
            "project",
            "create",
            "same-name",
            "--expected-snapshot",
            snapshot(&genesis),
        ]);
        let b = f.ok(&[
            "memory",
            "project",
            "create",
            "same-name",
            "--expected-snapshot",
            snapshot(&a),
        ]);
        let aid = a["project"].as_str().unwrap();
        let bid = b["project"].as_str().unwrap();
        assert_ne!(aid, bid);
        assert_ne!(a["project"], local["project"]);
        f.ok(&["memory", "project", "bind", aid]);
        let next = f.publish(
            snapshot(&b),
            &json!([
                write(&json!({"kind":"global"}), "MEMORY.md", "global needle"),
                write(
                    &json!({"kind":"project","project":aid}),
                    "MEMORY.md",
                    "chosen needle"
                ),
                write(
                    &json!({"kind":"project","project":bid}),
                    "MEMORY.md",
                    "other needle"
                )
            ]),
        );
        let context = f.ok(&["memory", "context"]);
        assert_eq!(context["project"]["scope"]["project"], aid);
        assert_eq!(
            context["global"]["snapshot"],
            context["project"]["snapshot"]
        );
        assert!(context["diagnostic"].is_null());
        let hits = f.ok(&["memory", "search", "needle", "--snapshot", snapshot(&next)]);
        assert_eq!(hits["results"].as_array().unwrap().len(), 2);
        assert!(!hits.to_string().contains("other needle"));
        f.fail(&["memory", "project", "bind", bid]);
        f.fail(&["memory", "project", "unbind", "--expected", bid]);
        f.ok(&["memory", "project", "unbind", "--expected", aid]);
        f.ok(&["memory", "project", "bind", bid]);
        // A context resource's exact shared selector remains valid after a local rebind.
        let pinned = f.ok(&[
            "memory",
            "read",
            "MEMORY.md",
            "--scope",
            aid,
            "--snapshot",
            snapshot(&next),
        ]);
        assert_eq!(pinned["document"]["content"], "chosen needle\n");
        f.fail(&["memory", "read", "MEMORY.md", "--scope", "project"]);
        f.fail(&[
            "memory",
            "read",
            "MEMORY.md",
            "--scope",
            "global",
            "--project",
            aid,
        ]);
        fs::write(f.local().join("bindings.json"), b"{invalid").unwrap();
        let degraded = f.ok(&["memory", "context"]);
        assert_eq!(degraded["global"]["document"]["content"], "global needle\n");
        assert!(degraded["project"].is_null());
        assert!(
            degraded["diagnostic"]
                .as_str()
                .unwrap()
                .contains("unavailable")
        );
    }

    #[test]
    fn a_machine_reads_its_local_copy_until_a_pass_or_fresh_read_syncs_it() {
        let a = Fixture::new();
        let b = a.machine();
        let genesis = a.init();
        assert_eq!(b.ok(&["sync", "once"])["pass"], "done");
        let global = json!({"kind":"global"});
        let base = a.publish(
            snapshot(&genesis),
            &json!([write(&global, "MEMORY.md", "base\n")]),
        );
        // Reads serve B's local copy, with its age, until B syncs.
        let local = b.ok(&["memory", "read", "MEMORY.md", "--scope", "global"]);
        assert_eq!(local["snapshot"], genesis["snapshot"]);
        assert!(local["freshness"]["as_of"].is_u64());
        assert!(local["freshness"]["age_seconds"].is_u64());
        assert_eq!(local["freshness"]["stale"], false);
        let pass = b.ok(&["sync", "once"]);
        assert_eq!(pass["pass"], "done");
        assert_eq!(pass["fetched"], json!(["memory"]));
        assert_eq!(
            b.ok(&["memory", "read", "MEMORY.md", "--scope", "global"])["document"]["content"],
            "base\n"
        );
        let next = a.publish(
            snapshot(&base),
            &json!([write(&global, "MEMORY.md", "next\n")]),
        );
        assert_eq!(b.ok(&["memory", "context"])["snapshot"], base["snapshot"]);
        // --fresh runs the pass inline.
        let fresh = b.ok(&["memory", "--fresh", "context"]);
        assert_eq!(fresh["snapshot"], next["snapshot"]);
        assert_eq!(fresh["global"]["document"]["content"], "next\n");
    }

    #[test]
    fn machines_share_one_remote_merge_disjoint_work_and_reconcile_divergence() {
        let a = Fixture::new();
        let b = a.machine();
        let genesis = a.init();
        b.fail(&["memory", "init"]);
        assert_eq!(b.ok(&["memory", "status"])["snapshot"], genesis["snapshot"]);
        let global = json!({"kind":"global"});
        let base = a.publish(
            snapshot(&genesis),
            &json!([write(&global, "MEMORY.md", "base\n")]),
        );
        assert_eq!(
            b.ok(&[
                "memory",
                "--fresh",
                "read",
                "MEMORY.md",
                "--scope",
                "global"
            ])["document"]["content"],
            "base\n"
        );
        let first = a.publish(
            snapshot(&base),
            &json!([write(&global, "a.md", "from a\n")]),
        );
        let merged = b.publish(
            snapshot(&base),
            &json!([write(&global, "b.md", "from b\n")]),
        );
        assert_eq!(merged["state"], "published");
        assert_ne!(merged["snapshot"], first["snapshot"]);
        for path in ["a.md", "b.md"] {
            let read = a.ok(&["memory", "--fresh", "read", path, "--scope", "global"]);
            assert_eq!(read["snapshot"], merged["snapshot"]);
            assert_eq!(read["document"]["exists"], true, "{path}");
        }

        let theirs = a.publish(
            snapshot(&merged),
            &json!([write(&global, "MEMORY.md", "a's line\n")]),
        );
        fs::write(b.root.join("checkout/mine.md"), "b's line\n").unwrap();
        let failure = b.fail(&[
            "memory",
            "write",
            "MEMORY.md",
            "--scope",
            "global",
            "--expected-snapshot",
            snapshot(&merged),
            "--content-file",
            "mine.md",
        ]);
        assert!(failure.contains("reconcile"), "{failure}");
        let status = b.ok(&["memory", "status"]);
        assert_eq!(status["snapshot"], theirs["snapshot"]);
        let plan = &status["pending"][0]["plan"];
        assert_eq!(plan["kind"], "merge");
        assert_eq!(plan["base"], merged["snapshot"]);
        assert_eq!(plan["incoming"], theirs["snapshot"]);
        assert_eq!(plan["conflicts"][0]["resource"]["path"], "MEMORY.md");
        let candidate = plan["local"].as_str().unwrap();
        let inspected = b.ok(&[
            "memory",
            "inspect",
            "--candidate",
            candidate,
            "MEMORY.md",
            "--scope",
            "global",
        ]);
        assert_eq!(inspected["document"]["content"], "b's line\n");
        let resolution = b.input(
            "reconcile.json",
            &json!({
                "local": candidate,
                "incoming": plan["incoming"],
                "resolutions": [{
                    "resource": plan["conflicts"][0]["resource"],
                    "choice": {"kind": "write", "content": "both lines\n"}
                }]
            }),
        );
        let reconciled = b.ok(&["memory", "reconcile", "--input-file", &resolution]);
        assert_eq!(reconciled["pending"], json!([]));
        b.fail(&["memory", "reconcile", "--input-file", &resolution]);
        let read = a.ok(&[
            "memory",
            "--fresh",
            "read",
            "MEMORY.md",
            "--scope",
            "global",
        ]);
        assert_eq!(read["snapshot"], reconciled["snapshot"]);
        assert_eq!(read["document"]["content"], "both lines\n");
    }

    #[test]
    fn offline_reads_are_labeled_stale_and_offline_publications_wait() {
        let a = Fixture::new();
        let b = a.machine();
        let genesis = a.init();
        let global = json!({"kind":"global"});
        let first = a.publish(
            snapshot(&genesis),
            &json!([write(&global, "MEMORY.md", "index\n")]),
        );
        let synced = b.ok(&["memory", "--fresh", "status"]);
        assert_eq!(synced["freshness"]["stale"], false);
        assert!(synced["freshness"]["failing"].is_null());
        a.remote.disconnect();
        let pass = b.ok(&["sync", "once"]);
        assert_eq!(pass["pass"], "offline");
        assert!(pass["sync"]["failing"]["retry_at"].is_u64());
        let context = b.ok(&["memory", "context"]);
        assert_eq!(context["snapshot"], first["snapshot"]);
        let read = b.ok(&["memory", "read", "MEMORY.md", "--scope", "global"]);
        assert_eq!(read["freshness"]["stale"], true);
        assert_eq!(read["freshness"]["as_of"], synced["freshness"]["as_of"]);
        assert!(read["freshness"]["failing"]["last_error"].is_string());
        assert_eq!(read["document"]["content"], "index\n");
        let plain = dots_process::Runner::new(Duration::from_secs(20), 1024 * 1024)
            .output({
                let mut command = Command::new(env!("CARGO_BIN_EXE_dots"));
                command
                    .env_clear()
                    .env("PATH", std::env::var_os("PATH").expect("test PATH"))
                    .env("HOME", b.root.join("home"))
                    .env("XDG_DATA_HOME", b.root.join("data"))
                    .current_dir(b.root.join("checkout"))
                    .args(["memory", "read", "MEMORY.md", "--scope", "global"]);
                command
            })
            .unwrap();
        assert!(plain.status.success());
        assert!(String::from_utf8(plain.stdout).unwrap().contains("Stale:"));

        let offline = b.publish(
            snapshot(&first),
            &json!([write(&global, "offline.md", "offline\n")]),
        );
        assert_eq!(offline["state"], "pending");
        let candidate = offline["candidate"].clone();
        let status = b.ok(&["memory", "status"]);
        assert_eq!(status["freshness"]["stale"], true);
        assert_eq!(status["pending"][0]["candidate"], candidate);
        let substrate = b.ok(&["substrate", "status"]);
        let failing = &substrate["sync"]["failing"];
        assert!(failing["since"].is_u64());
        assert!(failing["failures"].as_u64().unwrap() >= 2);
        assert!(failing["last_error"].is_string());
        assert!(failing["retry_at"].is_u64());
        assert!(substrate["counters"]["failed_passes"].as_u64().unwrap() >= 2);
        a.remote.restore();

        let second = a.publish(
            snapshot(&first),
            &json!([write(&global, "second.md", "second\n")]),
        );
        let online = b.publish(
            snapshot(&second),
            &json!([write(&global, "online.md", "online\n")]),
        );
        assert_eq!(online["state"], "published");
        assert_eq!(online["integrated"], json!([candidate]));
        assert_eq!(online["pending"], json!([]));
        for path in ["offline.md", "second.md", "online.md"] {
            let read = a.ok(&["memory", "--fresh", "read", path, "--scope", "global"]);
            assert_eq!(read["document"]["exists"], true, "{path}");
        }
        assert_eq!(b.ok(&["memory", "status"])["pending"], json!([]));
    }

    #[test]
    fn substrate_configuration_is_explicit_and_unchanging() {
        let f = Fixture::new();
        let url = f.remote.url();
        let config = f.root.join("data/dots/substrate/config.json");
        let written = fs::read(&config).unwrap();
        f.ok(&["substrate", "init", "--remote", &url]);
        assert_eq!(fs::read(&config).unwrap(), written);
        f.fail(&["substrate", "init", "--remote", "file:///elsewhere.git"]);
        f.fail(&[
            "substrate",
            "init",
            "--remote",
            &url,
            "--credential-helper",
            "!true",
        ]);
        let status = f.ok(&["substrate", "status"]);
        assert_eq!(status["remote"], url.as_str());
        assert_eq!(status["pending"], json!([]));
        assert!(status["sync"]["listed_at"].is_null());
        assert!(status["sync"]["last_pass_started_at"].is_null());
        f.init();
        let status = f.ok(&["substrate", "status"]);
        assert!(status["sync"]["listed_at"].is_u64());
        assert!(status["sync"]["last_pass_finished_at"].is_u64());
        assert!(status["sync"]["failing"].is_null());
        assert_eq!(status["counters"]["passes"], 1);
        assert_eq!(status["counters"]["pushes"], 1);
        assert!(status["machine"].is_string());
        assert_eq!(status["stranded"], json!([]));

        let unconfigured = scratch("unconfigured");
        let output = dots_process::Runner::new(Duration::from_secs(20), 1024 * 1024)
            .output({
                let mut command = Command::new(env!("CARGO_BIN_EXE_dots"));
                command
                    .env_clear()
                    .env("PATH", std::env::var_os("PATH").expect("test PATH"))
                    .env("HOME", unconfigured.join("home"))
                    .env("XDG_DATA_HOME", unconfigured.join("data"))
                    .current_dir(&unconfigured)
                    .args(["memory", "status"]);
                command
            })
            .unwrap();
        assert!(!output.status.success());
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("dots substrate init"),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(!unconfigured.join("data").exists());
        let _ = fs::remove_dir_all(&unconfigured);
    }
}
