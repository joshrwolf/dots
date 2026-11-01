//! Explicit initialization and shared task/memory project resolution with real Git.
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::Value;

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "dots-project-cli-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(root.join("repo")).expect("create fixture repository");
        Self(root)
    }
    fn run(&self, directory: &str, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_dots"))
            .arg("--json")
            .args(args)
            .current_dir(self.0.join(directory))
            .env("HOME", self.0.join("home"))
            .env("XDG_DATA_HOME", self.0.join("data"))
            .env("XDG_STATE_HOME", self.0.join("state"))
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .expect("run dots fixture")
    }
    fn ok(&self, directory: &str, args: &[&str]) -> Value {
        let result = self.run(directory, args);
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        serde_json::from_slice(&result.stdout).expect("decode successful dots response")
    }
    fn git(&self, directory: &str, args: &[&str]) {
        let result = Command::new("git")
            .args(args)
            .current_dir(self.0.join(directory))
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .expect("run Git fixture");
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
    }
    fn repository(&self) {
        self.git("repo", &["init", "-q", "-b", "main"]);
        self.git(
            "repo",
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
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn id(value: &Value) -> &str {
    value
        .pointer("/project/id")
        .expect("project ID field")
        .as_str()
        .expect("string project ID")
}

#[test]
fn reads_do_not_create_a_catalog_and_old_data_requires_an_explicit_choice() {
    let f = Fixture::new();
    f.repository();
    for args in [vec!["project", "list"], vec!["task", "list"]] {
        assert!(!f.run("repo", &args).status.success());
    }
    let scope = f.ok("repo", &["memory", "scope"]);
    assert!(scope["project"].is_null());
    assert!(!f.0.join("data").exists());
    fs::create_dir_all(f.0.join("data/dots/memory")).unwrap();
    fs::write(f.0.join("data/dots/memory/source.md"), "retained source").unwrap();
    assert!(
        !f.run("repo", &["project", "init", "project"])
            .status
            .success()
    );
    assert!(!f.0.join("data/dots/workspace.sqlite3").exists());
    let initialized = f.ok("repo", &["project", "init", "project", "--fresh"]);
    assert_eq!(initialized["created"], true);
    assert_eq!(
        f.ok("repo", &["project", "init", "another name"])["created"],
        false
    );
    assert_eq!(
        id(&initialized),
        id(&f.ok("repo", &["project", "init", "project"]))
    );
    assert_eq!(
        fs::read_to_string(f.0.join("data/dots/memory/source.md")).unwrap(),
        "retained source"
    );
    let list = f.ok("repo", &["project", "list"]);
    assert_eq!(list["projects"].as_array().unwrap().len(), 1);
}

#[test]
fn worktrees_share_ids_while_clones_bare_repositories_and_submodules_do_not() {
    let f = Fixture::new();
    f.repository();
    let first = f.ok("repo", &["project", "init", "same-name"]);
    f.git("repo", &["worktree", "add", "-qb", "slice", "../linked"]);
    assert_eq!(id(&first), id(&f.ok("linked", &["project", "resolve"])));
    let task = f.ok("linked", &["task", "add", "work"]);
    assert_eq!(task["project"], id(&first));
    let scope = f.ok("repo", &["memory", "scope"]);
    assert!(scope["project"].as_str().unwrap().contains(id(&first)));
    assert_eq!(scope, f.ok("linked", &["memory", "scope"]));
    f.git("repo", &["clone", "-q", ".", "../clone"]);
    assert!(!f.run("clone", &["task", "list"]).status.success());
    let unbound = f.ok("clone", &["memory", "scope"]);
    assert!(unbound["project"].is_null());
    assert!(unbound["diagnostic"].as_str().unwrap().contains("unbound"));
    let clone = f.ok("clone", &["project", "init", "same-name"]);
    assert_ne!(id(&first), id(&clone));
    f.git("repo", &["init", "--bare", "-q", "../bare.git"]);
    let bare = f.ok("bare.git", &["project", "init", "same-name"]);
    assert_ne!(id(&first), id(&bare));
    f.git(
        "repo",
        &[
            "-c",
            "protocol.file.allow=always",
            "submodule",
            "add",
            "-q",
            "../clone",
            "sub",
        ],
    );
    let sub = f.ok("repo/sub", &["project", "init", "same-name"]);
    assert_ne!(id(&first), id(&sub));
    assert_ne!(id(&clone), id(&sub));
}

#[test]
fn explicit_move_preserves_identity_and_does_not_steal_another_binding() {
    let f = Fixture::new();
    f.repository();
    let first = f.ok("repo", &["project", "init", "project"]);
    let previous = first
        .pointer("/project/common_directories/0")
        .unwrap()
        .as_str()
        .unwrap();
    fs::rename(f.0.join("repo"), f.0.join("moved")).unwrap();
    assert!(!f.run("moved", &["project", "resolve"]).status.success());
    let moved = f.ok(
        "moved",
        &["project", "rebind", id(&first), "--from", previous],
    );
    assert_eq!(id(&first), id(&moved));
    let second = f.ok("moved", &["project", "register", "other"]);
    assert!(
        !f.run("moved", &["project", "bind", id(&second)])
            .status
            .success()
    );
    assert_eq!(id(&first), id(&f.ok("moved", &["project", "resolve"])));
}

#[test]
fn accidental_init_after_move_is_reversible_without_losing_work() {
    let f = Fixture::new();
    f.repository();
    let original = f.ok("repo", &["project", "init", "original"]);
    let previous = original
        .pointer("/project/common_directories/0")
        .unwrap()
        .as_str()
        .unwrap();
    fs::rename(f.0.join("repo"), f.0.join("moved")).unwrap();
    let wrong = f.ok("moved", &["project", "init", "accidental"]);
    let task = f.ok("moved", &["task", "add", "keep this"]);
    let common = wrong
        .pointer("/project/common_directories/0")
        .unwrap()
        .as_str()
        .unwrap();
    assert_eq!(
        f.ok(
            "moved",
            &["project", "unbind", id(&wrong), "--path", common]
        )["removed"],
        true
    );
    assert_eq!(
        f.ok(
            "moved",
            &["project", "unbind", id(&wrong), "--path", common]
        )["removed"],
        false
    );
    f.ok(
        "moved",
        &["project", "rebind", id(&original), "--from", previous],
    );
    assert_eq!(id(&original), id(&f.ok("moved", &["project", "resolve"])));
    let shown = f.ok(
        "moved",
        &[
            "task",
            "--project",
            id(&wrong),
            "show",
            task.pointer("/task/id").unwrap().as_str().unwrap(),
        ],
    );
    assert_eq!(shown.pointer("/task/title").unwrap(), "keep this");
}

#[test]
fn interrupted_staging_does_not_poison_catalog_initialization() {
    let f = Fixture::new();
    f.repository();
    let root = f.0.join("data/dots");
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join(".staged-00000000000000000000000000000000"), "").unwrap();
    f.ok("repo", &["project", "init", "project"]);
    assert!(root.join("workspace.sqlite3").is_file());
    assert_eq!(
        fs::read(root.join(".staged-00000000000000000000000000000000")).unwrap(),
        b""
    );
    f.ok("repo", &["task", "add", "usable catalog"]);
}

#[test]
fn empty_or_corrupt_catalogs_are_errors_not_implicit_initialization() {
    let f = Fixture::new();
    fs::create_dir_all(f.0.join("data/dots")).unwrap();
    let database = f.0.join("data/dots/workspace.sqlite3");
    for bytes in [b"".as_slice(), b"not sqlite".as_slice()] {
        fs::write(&database, bytes).unwrap();
        assert!(!f.run("repo", &["project", "list"]).status.success());
        assert_eq!(fs::read(&database).unwrap(), bytes);
    }
}
