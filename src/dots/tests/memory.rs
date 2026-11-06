//! Native snapshot CLI contracts with neutral disposable homes.
#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::process::{Command, Output};
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::Duration;

    use serde_json::{Value, json};

    struct Fixture {
        root: PathBuf,
    }
    impl Fixture {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let root = std::env::temp_dir().join(format!(
                "dots-knowledge-cli-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(root.join("checkout")).unwrap();
            fs::create_dir_all(root.join("home")).unwrap();
            Self { root }
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
        fn ok(&self, args: &[&str]) -> Value {
            let output = self.run(args);
            assert!(
                output.status.success(),
                "{args:?}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            serde_json::from_slice(&output.stdout).unwrap()
        }
        fn fail(&self, args: &[&str]) {
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
        }
        fn init(&self) -> Value {
            self.ok(&["memory", "init", "--actor", "neutral"])
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
            let mut command = Command::new("git");
            command
                .env_clear()
                .env("PATH", std::env::var_os("PATH").expect("test PATH"))
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .args(["init", "-q"])
                .arg(self.root.join("checkout"));
            assert!(
                dots_process::Runner::new(Duration::from_secs(10), 64 * 1024)
                    .output(command)
                    .unwrap()
                    .status
                    .success()
            );
            self.ok(&["project", "init", "neutral", "--fresh"])
        }
        fn replica(&self) -> Self {
            let replica = Self::new();
            let bundle = self.root.join("seed.bundle");
            let bundle = bundle.to_str().unwrap();
            let receipt = self.ok(&["memory", "export", "--output", bundle]);
            let receipt_file = self.input("seed-receipt.json", &receipt);
            replica.ok(&[
                "memory",
                "restore",
                "--actor",
                "replica",
                "--role",
                "replica",
                "--bundle",
                bundle,
                "--receipt-file",
                &receipt_file,
            ]);
            replica
        }
        fn store(&self) -> PathBuf {
            self.root.join("data/dots/knowledge")
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
        f.fail(&["memory", "context"]);
        assert!(!f.store().exists());
        let genesis = f.init();
        f.fail(&["memory", "init", "--actor", "neutral"]);
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
        fs::write(f.store().join("bindings.json"), b"{invalid").unwrap();
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
    fn restore_import_inspection_and_explicit_reconciliation_preserve_ownership() {
        let authority = Fixture::new();
        let seed = authority.init();
        let replica = authority.replica();
        let restored = replica.ok(&["memory", "status"]);
        assert_eq!(restored["identity"]["store"], seed["identity"]["store"]);
        assert_ne!(restored["identity"]["node"], seed["identity"]["node"]);
        let remote = replica.publish(
            snapshot(&restored),
            &json!([write(&json!({"kind":"global"}), "MEMORY.md", "remote fact")]),
        );
        let local = authority.publish(
            snapshot(&seed),
            &json!([write(&json!({"kind":"global"}), "MEMORY.md", "local fact")]),
        );
        let remote_bundle = replica.root.join("remote.bundle");
        let remote_bundle = remote_bundle.to_str().unwrap();
        let remote_receipt = replica.ok(&["memory", "export", "--output", remote_bundle]);
        let remote_receipt_file = replica.input("remote-receipt.json", &remote_receipt);
        let candidate = authority.ok(&[
            "memory",
            "import",
            "--bundle",
            remote_bundle,
            "--receipt-file",
            &remote_receipt_file,
        ]);
        assert_eq!(candidate["snapshot"], remote["snapshot"]);
        assert_eq!(
            authority.ok(&["memory", "status"])["snapshot"],
            local["snapshot"]
        );
        authority.fail(&[
            "memory",
            "read",
            "MEMORY.md",
            "--scope",
            "global",
            "--snapshot",
            snapshot(&remote),
        ]);
        let candidate_file = authority.input("candidate.json", &candidate);
        let inspected = authority.ok(&[
            "memory",
            "inspect",
            "--candidate-file",
            &candidate_file,
            "MEMORY.md",
            "--scope",
            "global",
        ]);
        assert_eq!(inspected["document"]["content"], "remote fact\n");
        assert!(inspected.get("candidate").is_some());
        let plan = authority.ok(&[
            "memory",
            "plan",
            "--expected-snapshot",
            snapshot(&local),
            "--candidate-file",
            &candidate_file,
        ]);
        assert_eq!(plan["base"], seed["snapshot"]);
        assert_eq!(plan["local"], local["snapshot"]);
        assert_eq!(plan["recipient"], seed["identity"]);
        assert_eq!(plan["conflicts"].as_array().unwrap().len(), 1);
        assert_eq!(plan["conflicts"][0]["resource"]["path"], "MEMORY.md");
        assert_eq!(
            authority.ok(&["memory", "status"])["snapshot"],
            local["snapshot"]
        );
        let wrong = authority.input("wrong-node.json", &json!({"recipient":restored["identity"],"local":local["snapshot"],"incoming":candidate,"resolutions":[]}));
        authority.fail(&["memory", "reconcile", "--input-file", &wrong]);
        assert_eq!(
            authority.ok(&["memory", "status"])["snapshot"],
            local["snapshot"]
        );
        let input = authority.input("reconcile.json", &json!({"recipient":plan["recipient"],"local":plan["local"],"incoming":plan["incoming"],"resolutions":[{"resource":plan["conflicts"][0]["resource"],"choice":{"kind":"incoming"}}]}));
        let accepted = authority.ok(&["memory", "reconcile", "--input-file", &input]);
        assert_ne!(accepted["snapshot"], remote["snapshot"]);
        authority.fail(&[
            "memory",
            "plan",
            "--expected-snapshot",
            snapshot(&local),
            "--candidate-file",
            &candidate_file,
        ]);
        assert_eq!(
            authority.ok(&["memory", "read", "MEMORY.md", "--scope", "global"])["document"]["content"],
            "remote fact\n"
        );
        replica.fail(&[
            "memory",
            "project",
            "create",
            "not-authority",
            "--expected-snapshot",
            snapshot(&remote),
        ]);
    }

    #[test]
    fn one_shot_control_protocol_checks_real_cli_input_and_location() {
        let f = Fixture::new();
        let initialized = f.init();
        let request = serde_json::to_vec(&json!({"kind":"status", "root":f.store()})).unwrap();
        let output = dots_process::Runner::new(Duration::from_secs(10), 1024 * 1024)
            .stdin(request, 1024 * 1024)
            .output(f.command(&["memory", "serve"]))
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let response: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(response["status"]["identity"], initialized["identity"]);
        assert_eq!(response["status"]["snapshot"], initialized["snapshot"]);
        let output = dots_process::Runner::new(Duration::from_secs(10), 1024 * 1024)
            .stdin(b"{bad".to_vec(), 1024)
            .output(f.command(&["memory", "serve"]))
            .unwrap();
        assert!(!output.status.success());
        let failure: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert!(failure["error"].is_string());
    }
}
