//! Machines sharing one local `file://` Git remote, each with its own state.
#![allow(dead_code, reason = "each test binary uses part of the support module")]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use dots_documents::staged_directory::StagedDirectory;
use dots_memory::{Change, Head, Landing, Publication, Scope, Snapshot, Store};
use dots_substrate::git::GitBackend;
use dots_substrate::{Config, Substrate};

pub(crate) type TestResult = Result<(), Box<dyn std::error::Error>>;

/// Run git in a scratch directory with no user or system configuration.
pub(crate) fn git(directory: &Path, args: &[&str], input: Option<&[u8]>) -> Vec<u8> {
    let mut command = Command::new("git");
    command
        .env_clear()
        .env("PATH", std::env::var_os("PATH").expect("test PATH"))
        .env("HOME", directory)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_AUTHOR_NAME", "fixture")
        .env("GIT_AUTHOR_EMAIL", "fixture@localhost")
        .env("GIT_COMMITTER_NAME", "fixture")
        .env("GIT_COMMITTER_EMAIL", "fixture@localhost")
        .current_dir(directory)
        .args(args);
    let mut runner = dots_process::Runner::new(Duration::from_secs(30), 64 * 1024 * 1024);
    if let Some(input) = input {
        runner = runner.stdin(input.to_vec(), input.len() + 1);
    }
    let output = runner.output(command).expect("run git");
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    output.stdout
}

/// A bare SHA-1 repository reached over `file://`, standing in for GitHub.
pub(crate) struct Remote {
    scratch: StagedDirectory,
}

impl Remote {
    pub(crate) fn new() -> Self {
        let scratch = StagedDirectory::create(&std::env::temp_dir()).expect("create remote");
        git(
            scratch.path(),
            &[
                "init",
                "--bare",
                "--quiet",
                "--object-format=sha1",
                "--template=",
                "remote.git",
            ],
            None,
        );
        Self { scratch }
    }

    pub(crate) fn url(&self) -> String {
        format!(
            "file://{}",
            self.scratch.path().join("remote.git").display()
        )
    }

    /// A URL that never answers, for an unreachable remote.
    pub(crate) fn unreachable(&self) -> String {
        format!(
            "file://{}",
            self.scratch.path().join("absent.git").display()
        )
    }
}

/// One machine: its own substrate state, Git cache and local knowledge state.
pub(crate) struct Machine {
    scratch: StagedDirectory,
    pub(crate) store: Store,
    tick: u64,
}

impl Machine {
    pub(crate) fn new(url: &str) -> Self {
        let scratch = StagedDirectory::create(&std::env::temp_dir()).expect("create machine");
        let store = open(scratch.path(), url);
        Self {
            scratch,
            store,
            tick: 0,
        }
    }

    /// Reopen the same state against another URL, as when the network changes.
    pub(crate) fn connect(&mut self, url: &str) {
        self.store = open(self.scratch.path(), url);
    }

    pub(crate) fn root(&self) -> PathBuf {
        self.scratch.path().to_owned()
    }

    /// Each operation happens one second after the previous one.
    pub(crate) fn now(&mut self) -> SystemTime {
        self.tick += 1;
        UNIX_EPOCH + Duration::from_secs(1_900_000_000 + self.tick)
    }

    pub(crate) fn advance(&mut self, seconds: u64) {
        self.tick += seconds;
    }

    /// Run one sync pass now; returns why the remote was unreachable, if it was.
    pub(crate) fn sync(&mut self) -> Option<String> {
        let now = self.now();
        self.store.sync(now).expect("sync")
    }

    /// Sync, then read the head, as an explicit fresh read does.
    pub(crate) fn head(&mut self) -> Head {
        self.sync();
        self.store.head().expect("read head")
    }

    pub(crate) fn publish(
        &mut self,
        expected: &Snapshot,
        changes: &[Change],
    ) -> dots_memory::Result<Publication> {
        let now = self.now();
        self.store.publish(expected, changes, now)
    }

    pub(crate) fn published(&mut self, expected: &Snapshot, changes: &[Change]) -> Snapshot {
        match self.publish(expected, changes).expect("publish").landing {
            Landing::Published { snapshot } => snapshot,
            Landing::Pending { reason, .. } => panic!("publication stayed pending: {reason}"),
        }
    }

    pub(crate) fn content(&mut self, snapshot: Option<&Snapshot>, path: &str) -> Option<String> {
        let head = self.head();
        self.store
            .view(&head, snapshot)
            .expect("select snapshot")
            .content(&Scope::Global, Path::new(path))
            .expect("read document")
    }
}

fn open(root: &Path, url: &str) -> Store {
    let state = root.join("substrate");
    let backend = GitBackend::open(&state.join("git"), url).expect("open Git backend");
    let substrate =
        Substrate::open(&state, Box::new(backend), Config::default()).expect("open substrate");
    Store::open(substrate, &root.join("memory")).expect("open store")
}

pub(crate) fn global(path: &str, content: &str) -> Change {
    Change {
        scope: Scope::Global,
        path: path.into(),
        action: dots_memory::Action::Write {
            content: content.into(),
        },
    }
}
