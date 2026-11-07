//! The sync agent as its login service runs it, in disposable homes.
#[cfg(test)]
mod tests {
    use std::fs;
    use std::io::{BufRead as _, BufReader};
    use std::path::{Path, PathBuf};
    use std::process::{Child, Command, Stdio};
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::mpsc;
    use std::time::{Duration, Instant, SystemTime};

    use dots_substrate::git::GitBackend;
    use dots_substrate::{Blob, Config, Substrate, Update};
    use nix::sys::signal::{self, Signal};
    use nix::unistd::Pid;

    fn scratch(kind: &str) -> PathBuf {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "dots-sync-{kind}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(root.join("home")).unwrap();
        root
    }

    fn git(directory: &Path, args: &[&str]) -> String {
        let mut command = Command::new("git");
        command
            .env_clear()
            .env("PATH", std::env::var_os("PATH").expect("test PATH"))
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .current_dir(directory)
            .args(args);
        let output = dots_process::Runner::new(Duration::from_secs(10), 64 * 1024)
            .output(command)
            .unwrap();
        assert!(output.status.success(), "git {args:?}");
        String::from_utf8(output.stdout).unwrap()
    }

    /// One machine's home with a configured substrate on a `file://` remote.
    struct Home {
        root: PathBuf,
    }

    impl Home {
        fn new() -> Self {
            Self {
                root: scratch("home"),
            }
        }

        fn configured() -> Self {
            let home = Self::new();
            git(&home.root, &["init", "--bare", "-q", "remote.git"]);
            let url = home.url();
            let status = home
                .dots(&["substrate", "init", "--remote", &url])
                .status()
                .unwrap();
            assert!(status.success());
            home
        }

        fn url(&self) -> String {
            format!("file://{}", self.root.join("remote.git").display())
        }

        fn state(&self) -> PathBuf {
            self.root.join("data/dots/substrate")
        }

        fn dots(&self, args: &[&str]) -> Command {
            let mut command = Command::new(env!("CARGO_BIN_EXE_dots"));
            command
                .env_clear()
                .env("PATH", std::env::var_os("PATH").expect("test PATH"))
                .env("HOME", self.root.join("home"))
                .env("XDG_DATA_HOME", self.root.join("data"))
                .env("XDG_STATE_HOME", self.root.join("state"))
                .current_dir(&self.root)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .args(args);
            command
        }

        /// Start the agent; it never runs its own timer during the test.
        fn agent(&self) -> Agent {
            let mut child = self
                .dots(&["sync", "run", "--interval", "600"])
                .stderr(Stdio::piped())
                .spawn()
                .unwrap();
            let (sender, lines) = mpsc::channel();
            let stderr = child.stderr.take().unwrap();
            std::thread::spawn(move || {
                for line in BufReader::new(stderr).lines() {
                    let Ok(line) = line else { return };
                    // Shown with the test's output when it fails.
                    eprintln!("agent: {line}");
                    if sender.send(line).is_err() {
                        return;
                    }
                }
            });
            Agent { child, lines }
        }

        /// A second process on the same state, as a domain writer would be.
        fn writer(&self) -> Substrate {
            let backend = GitBackend::open(&self.state().join("git"), self.url()).unwrap();
            Substrate::open(&self.state(), Box::new(backend), Config::default()).unwrap()
        }

        fn remote_ref(&self, name: &str) -> Option<String> {
            let listed = git(
                &self.root,
                &[
                    "--git-dir",
                    "remote.git",
                    "for-each-ref",
                    "--format=%(objectname)",
                    name,
                ],
            );
            Some(listed.trim().to_owned()).filter(|oid| !oid.is_empty())
        }
    }

    impl Drop for Home {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    struct Agent {
        child: Child,
        lines: mpsc::Receiver<String>,
    }

    impl Agent {
        /// Wait for a stderr line containing `text`.
        fn expect(&self, text: &str) -> String {
            let deadline = Instant::now() + Duration::from_secs(20);
            loop {
                let left = deadline.saturating_duration_since(Instant::now());
                match self.lines.recv_timeout(left) {
                    Ok(line) if line.contains(text) => return line,
                    Ok(_) => {}
                    Err(error) => panic!("agent never logged {text:?}: {error}"),
                }
            }
        }

        fn signal(&self, signal: Signal) {
            let pid = Pid::from_raw(i32::try_from(self.child.id()).unwrap());
            signal::kill(pid, signal).unwrap();
        }

        fn exit(&mut self) -> std::process::ExitStatus {
            let deadline = Instant::now() + Duration::from_secs(20);
            loop {
                if let Some(status) = self.child.try_wait().unwrap() {
                    return status;
                }
                assert!(Instant::now() < deadline, "agent did not exit");
                std::thread::sleep(Duration::from_millis(20));
            }
        }
    }

    impl Drop for Agent {
        fn drop(&mut self) {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }

    fn wait_until(what: &str, mut done: impl FnMut() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(20);
        while !done() {
            assert!(Instant::now() < deadline, "timed out waiting for {what}");
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    #[test]
    fn the_agent_flushes_a_local_write_and_stops_on_sigterm() {
        let home = Home::configured();
        let mut agent = home.agent();
        agent.expect("running for machine");
        let writer = home.writer();
        wait_until("the first pass", || {
            writer.health().unwrap().pass_finished_at.is_some()
        });
        let passes = writer.status().unwrap().counters.passes;

        let blob = Blob::new(b"neutral".to_vec()).unwrap();
        writer
            .defer(
                Update {
                    name: "fixture".parse().unwrap(),
                    expected: None,
                    root: blob.hash(),
                    new_blobs: vec![blob],
                },
                SystemTime::now(),
            )
            .unwrap();
        // With a ten-minute timer, only the write's trigger flushes it.
        wait_until("the flush", || {
            home.remote_ref("refs/dots/fixture").is_some()
        });
        wait_until("an empty outbox", || {
            writer.status().unwrap().pending.is_empty()
        });
        assert_eq!(writer.status().unwrap().counters.passes, passes + 1);

        agent.signal(Signal::SIGTERM);
        assert!(agent.exit().success());
    }

    #[test]
    fn an_unconfigured_agent_idles_until_interrupted() {
        let home = Home::new();
        let mut agent = home.agent();
        agent.expect("no substrate is configured");
        agent.signal(Signal::SIGINT);
        assert!(agent.exit().success());
        assert!(!home.state().join("state.sqlite3").exists());
    }

    #[test]
    fn a_newer_state_schema_stops_the_agent_with_an_error() {
        let home = Home::configured();
        let once = home.dots(&["sync", "once"]).status().unwrap();
        assert!(once.success());
        let database = rusqlite::Connection::open(home.state().join("state.sqlite3")).unwrap();
        let mut agent = home.agent();
        agent.expect("running for machine");
        database.pragma_update(None, "user_version", 99).unwrap();
        // Any touch of the changes marker wakes the agent to recheck.
        fs::write(home.state().join("changes"), "wake\n").unwrap();
        agent.expect("schema");
        assert!(!agent.exit().success());

        let mut restarted = home.agent();
        restarted.expect("schema");
        assert!(!restarted.exit().success());
        drop(database);
    }
}
