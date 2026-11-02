//! Run `dots review` against scratch repositories for its session contract.
#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use std::sync::atomic::{AtomicU64, Ordering};

    use serde_json::Value;

    struct Repo {
        root: PathBuf,
    }

    impl Repo {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let root = std::env::temp_dir().join(format!(
                "dots-review-cli-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            let _ = fs::remove_dir_all(&root);
            let checkout = root.join("checkout");
            fs::create_dir_all(&checkout).unwrap();
            git(&checkout, &["init", "-q", "-b", "main"]);
            fs::write(checkout.join("README.md"), "scratch\n").unwrap();
            git(&checkout, &["add", "README.md"]);
            git(&checkout, &["commit", "-q", "--no-gpg-sign", "-m", "init"]);
            git(
                &checkout,
                &["worktree", "add", "-q", "../linked", "-b", "slice"],
            );
            for worktree in ["checkout", "linked"] {
                fs::write(root.join(worktree).join("README.md"), "changed\n").unwrap();
            }
            Self { root }
        }

        /// `dots --json review ARGS` in `worktree`, as agent session `session`.
        fn run(&self, worktree: &str, session: Option<&str>, args: &[&str]) -> Value {
            let mut command = Command::new(env!("CARGO_BIN_EXE_dots"));
            command
                .args(["--json", "review"])
                .args(args)
                .current_dir(self.root.join(worktree))
                .env_remove("PI_SESSION_ID")
                .env_remove("CLAUDE_CODE_SESSION_ID")
                .env_remove("CODEX_SESSION_ID");
            if let Some(session) = session {
                command.env("PI_SESSION_ID", session);
            }
            let output = command.output().unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            serde_json::from_slice(&output.stdout).unwrap()
        }
    }

    impl Drop for Repo {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    fn git(directory: &Path, args: &[&str]) {
        let output = Command::new("git")
            .args(args)
            .current_dir(directory)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_AUTHOR_NAME", "Fixture")
            .env("GIT_AUTHOR_EMAIL", "fixture@example.com")
            .env("GIT_COMMITTER_NAME", "Fixture")
            .env("GIT_COMMITTER_EMAIL", "fixture@example.com")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn ids(listed: &Value) -> Vec<&str> {
        listed["reviews"]
            .as_array()
            .unwrap()
            .iter()
            .map(|review| review["id"].as_str().unwrap())
            .collect()
    }

    #[test]
    fn a_review_is_owned_by_the_starting_session_unless_named() {
        let repo = Repo::new();
        let started = repo.run("checkout", Some("parent"), &["start", "--worktree"]);
        assert_eq!(started["review"]["owner"], "pi-parent");
        let parent = started["review"]["id"].as_str().unwrap();

        let named = repo.run(
            "linked",
            Some("worker"),
            &["start", "--worktree", "--owner", "pi-parent"],
        );
        assert_eq!(named["review"]["owner"], "pi-parent");
        let delegated = named["review"]["id"].as_str().unwrap();

        let unowned = repo.run("linked", None, &["start", "--worktree"]);
        assert_eq!(unowned["review"]["owner"], Value::Null);
        let unowned = unowned["review"]["id"].as_str().unwrap();
        assert_eq!(
            repo.run("checkout", None, &["show", unowned])["review"]["owner"],
            Value::Null
        );

        let mut owned = vec![parent, delegated];
        owned.sort_unstable();
        let listed = repo.run("checkout", None, &["list", "--all", "--owner", "pi-parent"]);
        let mut listed_ids = ids(&listed);
        listed_ids.sort_unstable();
        assert_eq!(listed_ids, owned);
        assert!(
            listed["reviews"]
                .as_array()
                .unwrap()
                .iter()
                .all(|review| review["owner"] == "pi-parent")
        );
        assert_eq!(
            ids(&repo.run("linked", None, &["list", "--owner", "pi-parent"])),
            [delegated],
            "without --all the filter applies to this worktree's reviews"
        );
        assert_eq!(
            ids(&repo.run("checkout", None, &["list", "--all"])).len(),
            3
        );
        assert_eq!(
            ids(&repo.run("checkout", None, &["list", "--all", "--owner", "pi-other"])),
            Vec::<&str>::new()
        );
    }

    fn keys(value: &Value) -> Vec<&str> {
        value
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect()
    }

    #[test]
    fn drafts_and_submissions_round_trip_as_json() {
        let repo = Repo::new();
        let started = repo.run("linked", None, &["start", "--worktree"]);
        let id = started["review"]["id"].as_str().unwrap();

        let drafted = repo.run(
            "checkout",
            None,
            &[
                "draft",
                id,
                "--path",
                "README.md",
                "--side",
                "new",
                "--start",
                "1",
                "--body",
                "Why?",
            ],
        );
        assert_eq!(keys(&drafted), ["draft", "review"]);
        assert_eq!(
            drafted["draft"],
            serde_json::json!({
                "id": "d1",
                "thread": null,
                "revision": 0,
                "anchor": {"path": "README.md", "side": "new", "start": 1, "end": 1},
                "body": "Why?",
                "at": drafted["draft"]["at"],
            })
        );
        let file = repo.run(
            "checkout",
            None,
            &["draft", id, "--path", "README.md", "--body", "Whole file"],
        );
        assert_eq!(
            file["draft"]["anchor"],
            serde_json::json!({"path": "README.md"})
        );

        let shown = repo.run("checkout", None, &["show", id]);
        assert!(shown["review"].get("drafts").is_none(), "{shown}");
        assert!(shown.get("view").is_none(), "{shown}");
        let listed = repo.run("checkout", None, &["list", "--all"]);
        assert!(listed["reviews"][0].get("drafts").is_none(), "{listed}");
        let listed = repo.run("checkout", None, &["list", "--all", "--drafts"]);
        assert_eq!(listed["reviews"][0]["drafts"], 2);

        let shown = repo.run(
            "checkout",
            None,
            &["show", id, "--drafts", "--preset", "everything"],
        );
        assert_eq!(shown["review"]["drafts"].as_array().unwrap().len(), 2);
        let view = &shown["view"];
        assert_eq!(
            keys(view),
            ["anchors", "available", "base", "files", "head", "preset"]
        );
        assert_eq!(view["preset"], "everything");
        assert_eq!(
            view["available"],
            serde_json::json!(["everything", "uncommitted"])
        );
        assert_eq!(
            view["anchors"],
            serde_json::json!({
                "d1": {"path": "README.md", "side": "new", "start": 1, "end": 1},
                "d2": {"path": "README.md"},
            })
        );
        assert_eq!(
            keys(&shown["files"][0]),
            [
                "added",
                "binary",
                "changed_since_looked",
                "path",
                "removed",
                "reviewed",
                "status"
            ]
        );

        let submitted = repo.run("checkout", None, &["submit", id, "--body", "Summary"]);
        assert_eq!(keys(&submitted), ["review", "submission", "threads"]);
        assert_eq!(submitted["submission"]["body"], "Summary");
        assert_eq!(
            submitted["submission"]["comments"],
            serde_json::json!([{"thread": "t1", "comment": 0}, {"thread": "t2", "comment": 0}])
        );
        let shown = repo.run("checkout", None, &["show", id, "--drafts"]);
        assert!(shown["review"].get("drafts").is_none(), "{shown}");
        assert_eq!(
            shown["review"]["threads"][0]["comments"][0]["author"],
            "user"
        );
    }

    #[test]
    fn marks_and_looks_round_trip_as_json() {
        let repo = Repo::new();
        let started = repo.run("linked", None, &["start", "--worktree"]);
        let id = started["review"]["id"].as_str().unwrap();
        let marked = repo.run("checkout", None, &["mark", id, "README.md"]);
        assert_eq!(keys(&marked), ["file", "review"]);
        assert_eq!(marked["file"]["reviewed"], true);
        let unmarked = repo.run("checkout", None, &["unmark", id, "README.md"]);
        assert_eq!(unmarked["file"]["reviewed"], false);
        let looked = repo.run("checkout", None, &["look", id]);
        assert_eq!(keys(&looked), ["looked", "review"]);
        assert_eq!(looked["looked"]["revision"], 0);
    }
}
