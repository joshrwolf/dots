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
}
