//! Real CLI memory behavior with neutral fixtures and isolated homes.
#[cfg(test)]
mod tests {
    use serde_json::Value;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::process::{Command, Output};
    use std::sync::atomic::{AtomicU64, Ordering};

    struct Fixture {
        root: PathBuf,
    }
    impl Fixture {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let root = std::env::temp_dir().join(format!(
                "dots-memory-cli-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(root.join("checkout")).unwrap();
            Self { root }
        }
        fn run(&self, directory: &str, args: &[&str]) -> Output {
            Command::new(env!("CARGO_BIN_EXE_dots"))
                .args(["--json", "memory"])
                .args(args)
                .current_dir(self.root.join(directory))
                .env("HOME", self.root.join("home"))
                .env("XDG_DATA_HOME", self.root.join("data"))
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .output()
                .unwrap()
        }
        fn ok(&self, args: &[&str]) -> Value {
            self.ok_in("checkout", args)
        }
        fn ok_in(&self, directory: &str, args: &[&str]) -> Value {
            let output = self.run(directory, args);
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            serde_json::from_slice(&output.stdout).unwrap()
        }
        fn fail(&self, args: &[&str]) -> String {
            let output = self.run("checkout", args);
            assert!(
                !output.status.success(),
                "unexpected success: {:?}",
                output.stdout
            );
            String::from_utf8_lossy(&output.stderr).into_owned()
        }
        fn write(&self, path: &str, scope: &str, expected: &str, content: &str) -> Value {
            let input = self.root.join("input.md");
            fs::write(&input, content).unwrap();
            self.ok(&[
                "write",
                path,
                "--scope",
                scope,
                "--expected",
                expected,
                "--content-file",
                input.to_str().unwrap(),
            ])
        }
        fn global(&self) -> PathBuf {
            self.root.join("data/dots/memory/_global")
        }
        fn git(&self, directory: &str, args: &[&str]) {
            let output = Command::new("git")
                .args(args)
                .current_dir(self.root.join(directory))
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn lazy_read_first_write_conflict_and_removal() {
        let f = Fixture::new();
        assert!(f.ok(&["scope"]).get("project").unwrap().is_null());
        let initial = f.ok(&["read", "wiki/preferences.md", "--scope", "global"]);
        assert_eq!(initial.pointer("/document/revision").unwrap(), "missing");
        assert_eq!(initial.pointer("/document/exists").unwrap(), false);
        assert!(!f.global().exists());
        let published = f.write(
            "wiki/preferences.md",
            "global",
            "missing",
            "# Preferences\nPrefer small changes.\n",
        );
        let revision = published
            .pointer("/published/revision")
            .unwrap()
            .as_str()
            .unwrap();
        assert!(revision.starts_with("sha256:"));
        assert!(!f.global().join(".write-lock").exists());
        fs::write(f.root.join("input.md"), "stale update").unwrap();
        assert!(
            f.fail(&[
                "write",
                "wiki/preferences.md",
                "--scope",
                "global",
                "--expected",
                "missing",
                "--content-file",
                "../input.md"
            ])
            .contains("conflict")
        );
        let read = f.ok(&["read", "wiki/preferences.md", "--scope", "global"]);
        assert_eq!(read.pointer("/document/revision").unwrap(), revision);
        assert_eq!(
            read.pointer("/document/content").unwrap(),
            "# Preferences\nPrefer small changes.\n"
        );
        f.ok(&[
            "remove",
            "wiki/preferences.md",
            "--scope",
            "global",
            "--expected",
            revision,
        ]);
        assert!(!f.global().join("wiki/preferences.md").exists());
        assert!(
            f.fail(&["read", "MEMORY.md", "--scope", "project"])
                .contains("outside Git")
        );
    }

    #[test]
    fn bounded_unicode_search_excludes_sources_and_paginates() {
        let f = Fixture::new();
        f.write("wiki/retries.md", "global", "missing", "# Retries\nCurrent: at most three attempts.\nRetries include the first attempt.\nCafe\u{301} 👨‍👩‍👧 — preferred spelling.\n");
        f.write(
            "sources/proposal.md",
            "global",
            "missing",
            "Old retries proposal: ten attempts.\n",
        );
        f.write("log.md", "global", "missing", "Retries changed.\n");
        let first = f.ok(&["search", "RETRIES", "--limit", "1"]);
        assert_eq!(
            first.pointer("/searches/0/result/backend").unwrap(),
            "literal"
        );
        assert_eq!(first.pointer("/searches/0/result/has_more").unwrap(), true);
        let second = f.ok(&["search", "retries", "--limit", "1", "--offset", "1"]);
        assert_eq!(second.pointer("/searches/0/result/hits/0/line").unwrap(), 3);
        assert_eq!(
            second.pointer("/searches/0/result/has_more").unwrap(),
            false
        );
        let sources = f.ok(&["search", "retries", "--include-sources"]);
        assert_eq!(
            sources
                .pointer("/searches/0/result/hits")
                .unwrap()
                .as_array()
                .unwrap()
                .len(),
            4
        );
        let unicode = f.ok(&["search", "👨‍👩‍👧"]);
        assert_eq!(
            unicode.pointer("/searches/0/result/hits/0/line").unwrap(),
            4
        );
        let page = f.ok(&[
            "read",
            "wiki/retries.md",
            "--scope",
            "global",
            "--lines",
            "1",
        ]);
        assert_eq!(page.pointer("/document/next_line").unwrap(), 2);
        assert!(f.fail(&["search", " "]).contains("query"));
        assert!(f.fail(&["search", "x", "--limit", "0"]).contains("invalid"));
    }

    #[test]
    fn locks_and_unsafe_paths_do_not_clobber_files() {
        let f = Fixture::new();
        f.write("MEMORY.md", "global", "missing", "# Global memory\n");
        fs::create_dir(f.global().join(".write-lock")).unwrap();
        fs::write(f.global().join(".write-lock/owner"), "other session").unwrap();
        assert!(
            f.fail(&[
                "remove",
                "MEMORY.md",
                "--scope",
                "global",
                "--expected",
                "missing"
            ])
            .contains("acquire memory lock")
        );
        assert_eq!(
            fs::read_to_string(f.global().join(".write-lock/owner")).unwrap(),
            "other session"
        );
        for path in [
            "../escape.md",
            "/tmp/escape.md",
            ".write-lock/owner.md",
            "not-markdown.txt",
        ] {
            assert!(
                !f.run("checkout", &["read", path, "--scope", "global"])
                    .status
                    .success()
            );
        }
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(f.root.join("input.md"), f.global().join("link.md"))
                .unwrap();
            assert!(
                f.fail(&["read", "link.md", "--scope", "global"])
                    .contains("symlink")
            );
            assert!(f.fail(&["search", "anything"]).contains("symlink"));
        }
    }

    #[test]
    fn worktrees_and_symlink_checkouts_share_scope_but_other_repos_do_not() {
        let f = Fixture::new();
        f.git("checkout", &["init", "-q", "-b", "main"]);
        f.git(
            "checkout",
            &[
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.com",
                "-c",
                "commit.gpgsign=false",
                "commit",
                "--allow-empty",
                "-qm",
                "init",
            ],
        );
        f.git(
            "checkout",
            &["worktree", "add", "-qb", "slice", "../linked"],
        );
        let original = f.ok(&["scope"]);
        assert_eq!(original, f.ok_in("linked", &["scope"]));
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(f.root.join("checkout"), f.root.join("alias")).unwrap();
            assert_eq!(original, f.ok_in("alias", &["scope"]));
        }
        f.write(
            "wiki/decision.md",
            "project",
            "missing",
            "A scoped decision.\n",
        );
        assert_eq!(
            f.ok_in("linked", &["search", "scoped"])
                .pointer("/searches/1/result/hits")
                .unwrap()
                .as_array()
                .unwrap()
                .len(),
            1
        );
        fs::create_dir(f.root.join("other")).unwrap();
        f.git("other", &["init", "-q"]);
        let other = f.ok_in("other", &["search", "scoped"]);
        assert_eq!(
            other.pointer("/searches/1/result/hits").unwrap(),
            &serde_json::json!([])
        );
        assert!(Path::new(original.pointer("/project").unwrap().as_str().unwrap()).is_absolute());
    }

    #[test]
    fn oversized_inputs_and_broken_git_fail_explicitly() {
        let f = Fixture::new();
        fs::write(f.root.join("input.md"), "x".repeat(256 * 1024 + 1)).unwrap();
        assert!(
            f.fail(&[
                "write",
                "MEMORY.md",
                "--scope",
                "global",
                "--expected",
                "missing",
                "--content-file",
                "../input.md"
            ])
            .contains("exceeds")
        );
        assert!(!f.global().exists());
        f.write(
            "wiki/long.md",
            "global",
            "missing",
            &format!("{}👨‍👩‍👧 needle\n", "é".repeat(299)),
        );
        let hits = f.ok(&["search", "needle"]);
        let hit = hits.pointer("/searches/0/result/hits/0").unwrap();
        assert_eq!(hit.get("truncated").unwrap(), true);
        assert_eq!(
            hit.get("excerpt").unwrap().as_str().unwrap(),
            "é".repeat(299)
        );
        fs::write(f.global().join("wiki/long.md"), "a".repeat(17000)).unwrap();
        assert!(
            f.fail(&["read", "wiki/long.md", "--scope", "global"])
                .contains("line exceeds")
        );
        fs::write(
            f.root.join("checkout/.git"),
            "gitdir: /nonexistent-neutral-fixture\n",
        )
        .unwrap();
        assert!(f.fail(&["scope"]).contains("cannot resolve"));
        // A broken project must not prevent an explicitly global read.
        assert_eq!(
            f.ok(&["read", "MEMORY.md", "--scope", "global"])
                .pointer("/document/exists")
                .unwrap(),
            false
        );
    }

    #[test]
    fn relative_xdg_falls_back_to_home_without_initializing_memory() {
        let f = Fixture::new();
        let output = Command::new(env!("CARGO_BIN_EXE_dots"))
            .args(["--json", "memory", "scope"])
            .current_dir(f.root.join("checkout"))
            .env("HOME", f.root.join("home"))
            .env("XDG_DATA_HOME", "relative-is-ignored")
            .output()
            .unwrap();
        assert!(output.status.success());
        let result: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(
            result.get("global").unwrap().as_str().unwrap(),
            f.root
                .join("home/.local/share/dots/memory/_global")
                .to_str()
                .unwrap()
        );
        assert!(!f.root.join("home").exists());
    }

    #[test]
    fn concurrent_writers_cannot_both_replace_the_same_revision() {
        let f = Fixture::new();
        let published = f.write("MEMORY.md", "global", "missing", "before\n");
        let revision = published
            .pointer("/published/revision")
            .unwrap()
            .as_str()
            .unwrap();
        fs::write(f.root.join("input.md"), "after\n").unwrap();
        let args = [
            "write",
            "MEMORY.md",
            "--scope",
            "global",
            "--expected",
            revision,
            "--content-file",
            "../input.md",
        ];
        let outcomes = std::thread::scope(|scope| {
            let first = scope.spawn(|| f.run("checkout", &args));
            let second = scope.spawn(|| f.run("checkout", &args));
            [first.join().unwrap(), second.join().unwrap()]
        });
        assert_eq!(
            outcomes
                .iter()
                .filter(|output| output.status.success())
                .count(),
            1
        );
        assert_eq!(
            fs::read_to_string(f.global().join("MEMORY.md")).unwrap(),
            "after\n"
        );
        assert!(!f.global().join(".write-lock").exists());
    }
}
