//! Process lifecycle of the shipped binary: `dots github watch follow` ends
//! with its reader, and `dots task events --follow` streams other processes'
//! commits, optionally for one task's subtree.
#[cfg(test)]
mod tests {
    use std::io::{BufRead as _, BufReader};
    use std::path::PathBuf;
    use std::process::{Child, Command, ExitStatus, Stdio};
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{Duration, Instant};

    use nix::sys::signal::{Signal, kill};
    use nix::unistd::Pid;

    fn state() -> PathBuf {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let directory = std::env::temp_dir().join(format!(
            "dots-follow-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&directory).unwrap();
        directory
    }

    /// `follow` over an empty store, once it has printed `ready`. Nothing is
    /// ever due, so it never needs `gh`.
    fn ready() -> (Child, BufReader<std::process::ChildStdout>) {
        let mut child = Command::new(env!("CARGO_BIN_EXE_dots"))
            .args(["github", "watch", "follow"])
            .env("PATH", "")
            .env("XDG_STATE_HOME", state())
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        let mut stdout = BufReader::new(child.stdout.take().unwrap());
        let mut line = String::new();
        stdout.read_line(&mut line).unwrap();
        assert_eq!(line, "ready\n");
        (child, stdout)
    }

    /// Well within the 60-second pause `follow` takes over an empty store.
    fn exit_within(child: &mut Child, limit: Duration) -> ExitStatus {
        let deadline = Instant::now() + limit;
        loop {
            if let Some(status) = child.try_wait().unwrap() {
                return status;
            }
            if Instant::now() > deadline {
                let _ = child.kill();
                panic!("follow outlived its reader");
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    #[test]
    fn follow_exits_when_its_output_closes() {
        let (mut child, stdout) = ready();
        drop(stdout);
        let status = exit_within(&mut child, Duration::from_secs(10));
        assert_eq!(status.code(), Some(0), "{status}");
    }

    #[test]
    fn follow_exits_zero_when_signalled() {
        for signal in [Signal::SIGTERM, Signal::SIGINT, Signal::SIGHUP] {
            let (mut child, _stdout) = ready();
            kill(Pid::from_raw(i32::try_from(child.id()).unwrap()), signal).unwrap();
            let status = exit_within(&mut child, Duration::from_secs(10));
            assert_eq!(status.code(), Some(0), "{signal}: {status}");
        }
    }
}

#[cfg(test)]
mod task_events {
    use std::io::{BufRead as _, BufReader};
    use std::path::PathBuf;
    use std::process::{Child, Command, Stdio};
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    use nix::sys::signal::{Signal, kill};
    use nix::unistd::Pid;
    use serde_json::Value;

    /// An isolated home with one initialized project in a scratch checkout.
    struct Workspace {
        root: PathBuf,
    }

    impl Workspace {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let root = std::env::temp_dir().join(format!(
                "dots-task-events-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir_all(root.join("checkout")).unwrap();
            let workspace = Self { root };
            let git = Command::new("git")
                .args(["init", "-q"])
                .current_dir(workspace.root.join("checkout"))
                .status()
                .unwrap();
            assert!(git.success());
            workspace.ok(&["project", "init", "fixture", "--fresh"]);
            workspace
        }

        fn dots(&self) -> Command {
            let mut command = Command::new(env!("CARGO_BIN_EXE_dots"));
            command
                .current_dir(self.root.join("checkout"))
                .env("HOME", self.root.join("home"))
                .env("XDG_DATA_HOME", self.root.join("data"))
                .env("XDG_STATE_HOME", self.root.join("state"))
                .env_remove("PI_SESSION_ID")
                .env_remove("CLAUDE_CODE_SESSION_ID")
                .env_remove("CODEX_SESSION_ID");
            command
        }

        fn ok(&self, args: &[&str]) -> Value {
            let output = self.dots().arg("--json").args(args).output().unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            serde_json::from_slice(&output.stdout).unwrap()
        }

        /// Add a task from another process; the seq of its `created` event.
        fn add(&self, title: &str) -> i64 {
            self.add_task(title, &[]).1
        }

        /// Add a task from another process; its ID and the seq of its `created` event.
        fn add_task(&self, title: &str, args: &[&str]) -> (String, i64) {
            let added = self.ok(&[&["task", "add", title], args].concat());
            (
                added["task"]["id"].as_str().unwrap().to_owned(),
                added["task"]["last_event"]["seq"].as_i64().unwrap(),
            )
        }

        /// Move a task from another process; the seq of its `updated` event.
        fn reparent(&self, id: &str, parent: &[&str]) -> i64 {
            self.ok(&[&["task", "update", id], parent].concat())["task"]["last_event"]["seq"]
                .as_i64()
                .unwrap()
        }

        fn follow(&self, args: &[&str]) -> Following {
            let mut child = self
                .dots()
                .args(["task", "events", "--follow"])
                .args(args)
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::inherit())
                .spawn()
                .unwrap();
            let stdout = BufReader::new(child.stdout.take().unwrap());
            let (send, lines) = mpsc::channel();
            std::thread::spawn(move || {
                for line in stdout.lines() {
                    if send.send(line.unwrap()).is_err() {
                        return;
                    }
                }
            });
            Following { child, lines }
        }
    }

    impl Drop for Workspace {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    struct Following {
        child: Child,
        lines: mpsc::Receiver<String>,
    }

    impl Following {
        /// The next line, which must arrive within about a second of its commit.
        fn line(&self) -> String {
            self.lines
                .recv_timeout(Duration::from_secs(5))
                .expect("follow printed nothing new")
        }

        fn seq(&self) -> i64 {
            let event: Value = serde_json::from_str(&self.line()).unwrap();
            event["seq"].as_i64().unwrap()
        }

        fn quiet(&self) {
            if let Ok(line) = self.lines.recv_timeout(Duration::from_millis(750)) {
                panic!("unexpected line {line}");
            }
        }

        /// SIGTERM is a reader's normal way to stop following.
        fn stop(mut self) {
            kill(
                Pid::from_raw(i32::try_from(self.child.id()).unwrap()),
                Signal::SIGTERM,
            )
            .unwrap();
            let deadline = Instant::now() + Duration::from_secs(10);
            loop {
                if let Some(status) = self.child.try_wait().unwrap() {
                    assert_eq!(status.code(), Some(0), "{status}");
                    return;
                }
                assert!(Instant::now() < deadline, "follow outlived SIGTERM");
                std::thread::sleep(Duration::from_millis(20));
            }
        }
    }

    impl Drop for Following {
        fn drop(&mut self) {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }

    #[test]
    fn follow_prints_the_backlog_then_other_processes_commits_once_each() {
        let workspace = Workspace::new();
        let first = workspace.add("first");
        let following = workspace.follow(&["--json"]);
        assert_eq!(following.seq(), first);
        following.quiet();

        let second = workspace.add("second");
        let committed = Instant::now();
        assert_eq!(following.seq(), second);
        assert!(
            committed.elapsed() < Duration::from_secs(2),
            "took {:?}",
            committed.elapsed()
        );
        let third = workspace.add("third");
        let fourth = workspace.add("fourth");
        assert_eq!([following.seq(), following.seq()], [third, fourth]);
        following.quiet();
        following.stop();
    }

    #[test]
    fn follow_resumes_after_the_last_seen_seq_without_gap_or_repeat() {
        let workspace = Workspace::new();
        let first = workspace.add("first");
        let following = workspace.follow(&["--json"]);
        assert_eq!(following.seq(), first);
        drop(following);

        let second = workspace.add("second");
        let third = workspace.add("third");
        let resumed = workspace.follow(&["--json", "--since", &first.to_string()]);
        assert_eq!([resumed.seq(), resumed.seq()], [second, third]);
        resumed.quiet();
        resumed.stop();
    }

    #[test]
    fn follow_under_a_task_streams_its_current_subtree() {
        let workspace = Workspace::new();
        let (root, root_created) = workspace.add_task("root", &[]);
        let (stray, _) = workspace.add_task("stray", &[]);
        let following = workspace.follow(&["--json", "--under", &root]);
        assert_eq!(following.seq(), root_created);
        following.quiet();

        let (child, child_created) = workspace.add_task("child", &["--parent", &root]);
        assert_eq!(following.seq(), child_created);
        workspace.add("elsewhere");
        let moved_in = workspace.reparent(&stray, &["--parent", &child]);
        assert_eq!(following.seq(), moved_in);
        workspace.reparent(&child, &["--parent", ""]);
        let (_, direct) = workspace.add_task("direct", &["--parent", &root]);
        assert_eq!(
            following.seq(),
            direct,
            "a moved-out subtree no longer counts"
        );
        following.quiet();
        following.stop();

        let resumed =
            workspace.follow(&["--json", "--under", &root, "--since", &moved_in.to_string()]);
        assert_eq!(resumed.seq(), direct);
        resumed.quiet();
        resumed.stop();
    }

    #[test]
    fn follow_under_an_unknown_task_fails_with_an_error_line() {
        let workspace = Workspace::new();
        let mut following = workspace.follow(&["--json", "--under", "t-00"]);
        let line: Value = serde_json::from_str(&following.line()).unwrap();
        assert!(line["error"].as_str().unwrap().contains("t-00"), "{line}");
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(status) = following.child.try_wait().unwrap() {
                assert_eq!(status.code(), Some(1), "{status}");
                return;
            }
            assert!(Instant::now() < deadline, "follow outlived its failure");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    #[test]
    fn follow_prints_readable_lines_without_json() {
        let workspace = Workspace::new();
        let first = workspace.add("first");
        let following = workspace.follow(&[]);
        let line = following.line();
        assert!(line.contains(&format!(" #{first} t-")), "{line}");
        assert!(line.ends_with(" created"), "{line}");
        following.stop();
    }

    #[test]
    fn follow_exits_when_its_output_closes() {
        let workspace = Workspace::new();
        let mut child = workspace
            .dots()
            .args(["--json", "task", "events", "--follow"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        drop(child.stdout.take());
        let deadline = Instant::now() + Duration::from_secs(10);
        let status = loop {
            if let Some(status) = child.try_wait().unwrap() {
                break status;
            }
            if Instant::now() > deadline {
                let _ = child.kill();
                panic!("follow outlived its reader");
            }
            std::thread::sleep(Duration::from_millis(20));
        };
        assert_eq!(status.code(), Some(0), "{status}");
    }
}
