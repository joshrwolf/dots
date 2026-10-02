//! Run the real CLI against hand-authored Herdr responses and isolated SQLite state.
#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::process::{Command, Output};
    use std::sync::atomic::{AtomicU64, Ordering};

    use herdr_prs::{PullRequestUrl, Registry, Scope, Source};
    use herdrkit::testing::{Server, Step};
    use serde_json::{Value, json};

    const FIRST: &str = "11111111-1111-4111-8111-111111111111";
    const SECOND: &str = "22222222-2222-4222-8222-222222222222";
    const PR: &str = "https://github.com/example/project/pull/7";
    const NEXT_PR: &str = "https://github.com/example/project/pull/8";

    struct Fixture {
        root: PathBuf,
        server: Server,
    }

    impl Fixture {
        fn new(replies: usize) -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let root = std::env::temp_dir().join(format!(
                "dots-prs-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(root.join("checkout")).unwrap();
            // Both workspaces deliberately share a checkout. Focus and inherited IDs
            // consistently name w1, while the explicit task target will be w2.
            let workspaces: Vec<_> = [("w1", FIRST), ("w2", SECOND)].into_iter().map(|(id, uuid)| {
            json!({"workspace_id":id,"identity":uuid,"label":id,"focused":id=="w1","agent_status":"idle",
                "worktree":{"repo_key":"example","repo_name":"project","repo_root":root.join("checkout"),"checkout_path":root.join("checkout"),"is_linked_worktree":false}})
        }).collect();
            let snapshot = json!({"type":"session_snapshot","snapshot":{
            "version":"test","protocol":22,"workspaces":workspaces,
            "tabs":[{"tab_id":"w1:t1","workspace_id":"w1"},{"tab_id":"w2:t1","workspace_id":"w2"}],
            "panes":[{"pane_id":"w1:p1","workspace_id":"w1","tab_id":"w1:t1","cwd":root.join("checkout")},
                     {"pane_id":"w2:p1","workspace_id":"w2","tab_id":"w2:t1","cwd":root.join("checkout")}],
            "agents":[],"focused_workspace_id":"w1"}});
            let mut steps = Vec::new();
            for _ in 0..replies {
                steps.push(Step::reply(
                    json!({"id":"{id}","result":snapshot}).to_string(),
                ));
                // Registration must survive a watcher failure, independently of its target.
                steps.push(Step::reply(
                json!({"id":"{id}","error":{"code":"unavailable","message":"watcher unavailable"}})
                    .to_string(),
            ));
            }
            Self {
                root,
                server: Server::start(steps),
            }
        }

        fn run(&self, arguments: &[&str]) -> Output {
            Command::new(env!("CARGO_BIN_EXE_dots"))
                .args(["herdr", "prs"])
                .args(arguments)
                .arg("--json")
                .current_dir(self.root.join("checkout"))
                .env("HERDR_SOCKET_PATH", self.server.client().endpoint())
                .env("HERDR_WORKSPACE_ID", "w1")
                .env("HERDR_TAB_ID", "w1:t1")
                .env("HERDR_PANE_ID", "w1:p1")
                .env("XDG_STATE_HOME", self.root.join("state"))
                .output()
                .unwrap()
        }

        fn scope(&self, uuid: &str) -> Scope {
            Scope::new(
                self.server.client().endpoint().to_str().unwrap().into(),
                uuid.into(),
            )
            .unwrap()
        }

        fn registry(&self) -> Registry {
            let path = self.root.join("state").join(herdr_prs::REGISTRY_PATH);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            Registry::open(&path).unwrap()
        }

        fn seed(&self) {
            self.registry()
                .register(
                    &self.scope(FIRST),
                    &PullRequestUrl::parse(PR).unwrap(),
                    Source::Manual,
                    1,
                )
                .unwrap();
        }

        fn success(&self, args: &[&str]) -> Value {
            let result = self.run(args);
            assert!(
                result.status.success(),
                "{}",
                String::from_utf8_lossy(&result.stderr)
            );
            serde_json::from_slice(&result.stdout).unwrap()
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn stale_consistent_ids_and_matching_checkout_cannot_write_without_explicit_target() {
        let fixture = Fixture::new(3);
        fixture.seed();
        let before =
            serde_json::to_value(fixture.registry().list(&fixture.scope(FIRST)).unwrap()).unwrap();
        for args in [
            vec!["register", NEXT_PR],
            vec!["unregister", PR],
            vec!["list"],
        ] {
            let result = fixture.run(&args);
            assert_eq!(
                serde_json::to_value(fixture.registry().list(&fixture.scope(FIRST)).unwrap())
                    .unwrap(),
                before
            );
            assert!(!result.status.success());
            assert!(String::from_utf8_lossy(&result.stderr).contains("--workspace is required"));
            assert!(
                fixture
                    .registry()
                    .list(&fixture.scope(SECOND))
                    .unwrap()
                    .pulls
                    .is_empty()
            );
        }
        assert!(
            fixture.server.requests().is_empty(),
            "missing target must fail before contacting Herdr"
        );
    }

    #[test]
    fn missing_target_does_not_even_create_registry() {
        let fixture = Fixture::new(0);
        assert!(!fixture.run(&["register", PR]).status.success());
        assert!(!fixture.root.join("state").exists());
    }

    #[test]
    fn explicit_target_overrides_stale_environment_and_focus_and_preserves_other_workspace() {
        let fixture = Fixture::new(5);
        fixture.seed();
        let first = fixture.success(&["register", PR, "--workspace", "w2"]);
        assert_eq!(first.get("workspace_id").unwrap(), "w2");
        assert_eq!(first.get("scope").unwrap().get("identity").unwrap(), SECOND);
        assert_eq!(first.get("changed").unwrap(), true);
        assert!(first.get("watcher_error").unwrap().is_string());
        let repeat = fixture.success(&["register", PR, "--workspace", "w2"]);
        assert_eq!(repeat.get("changed").unwrap(), false);
        fixture.success(&["register", NEXT_PR, "--workspace", "w2"]);
        let listed = fixture.success(&["list", "--workspace", "w2"]);
        assert_eq!(
            listed
                .get("associations")
                .unwrap()
                .get("pulls")
                .unwrap()
                .as_array()
                .unwrap()
                .len(),
            2
        );
        fixture.success(&["unregister", PR, "--workspace", "w2"]);
        let mut registry = fixture.registry();
        assert_eq!(
            registry
                .list(&fixture.scope(FIRST))
                .unwrap()
                .pulls
                .first()
                .unwrap()
                .url
                .as_str(),
            PR
        );
        let remaining = registry.list(&fixture.scope(SECOND)).unwrap();
        assert_eq!(remaining.pulls.len(), 1);
        assert_eq!(remaining.pulls.first().unwrap().url.as_str(), NEXT_PR);
        let requests = fixture.server.requests();
        assert_eq!(requests.len(), 10);
        for [snapshot, resources] in requests.as_chunks::<2>().0 {
            assert_eq!(snapshot.method, "session.snapshot");
            assert_eq!(resources.method, "workspace.resources.get");
            assert_eq!(resources.params, json!({"workspace_id":"w2"}));
        }
    }

    #[test]
    fn nonexistent_explicit_workspace_cannot_mutate_registry() {
        let fixture = Fixture::new(1);
        fixture.seed();
        let before =
            serde_json::to_value(fixture.registry().list(&fixture.scope(FIRST)).unwrap()).unwrap();
        let result = fixture.run(&["unregister", PR, "--workspace", "missing"]);
        assert!(!result.status.success());
        assert!(
            String::from_utf8_lossy(&result.stderr).contains("explicit workspace no longer exists")
        );
        assert_eq!(
            serde_json::to_value(fixture.registry().list(&fixture.scope(FIRST)).unwrap()).unwrap(),
            before
        );
        assert_eq!(fixture.server.requests().len(), 1);
    }
}
