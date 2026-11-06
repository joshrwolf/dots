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
                .env_remove("DOTS_AGENT_OWNER")
                .env_remove("PI_SESSION_FILE")
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

/// `dots attention view` on a pseudo-terminal over a disposable catalog: real
/// keys, mouse reports and a resize; answers persist and closing answers
/// nothing. No Rex server, agent or provider is involved.
#[cfg(test)]
mod attention_view {
    use std::fs::File;
    use std::io::{Read as _, Write as _};
    use std::os::fd::OwnedFd;
    use std::path::{Path, PathBuf};
    use std::process::{Child, Command, Stdio};
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    use dots_tasks::attention::{Choice, RequestContent, RequestKind, Scope, Source, StateFilter};
    use dots_tasks::{Actor, Owner, Project, RequestState, Store};
    use nix::pty::{self, Winsize};
    use nix::sys::signal::{Signal, kill};
    use nix::sys::termios::{self, LocalFlags};
    use nix::unistd::Pid;

    const WATCHDOG: Duration = Duration::from_secs(15);
    const COORDINATOR: &str = "pi-coordinator";

    struct Home(PathBuf);
    impl Home {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "dots-attention-view-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir_all(path.join("data/dots")).unwrap();
            Self(path)
        }
        fn catalog(&self) -> PathBuf {
            self.0.join("data").join(dots_tasks::STORE_PATH)
        }
    }
    impl Drop for Home {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    struct Viewer(Child);
    impl Drop for Viewer {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    struct Screen {
        chunks: mpsc::Receiver<Vec<u8>>,
        seen: Vec<u8>,
    }
    impl Screen {
        fn read(master: File) -> Self {
            let (sender, chunks) = mpsc::channel();
            std::thread::spawn(move || {
                let mut master = master;
                let mut buffer = [0_u8; 4096];
                while let Ok(count @ 1..) = master.read(&mut buffer) {
                    if sender.send(buffer[..count].to_vec()).is_err() {
                        return;
                    }
                }
            });
            Self {
                chunks,
                seen: Vec::new(),
            }
        }

        /// Reads until `needle` appears after `from`, returning where it ends.
        /// Ratatui moves the cursor over unchanged cells, spaces included, so
        /// a needle is one word.
        fn until(&mut self, from: usize, needle: &str) -> usize {
            let deadline = Instant::now() + WATCHDOG;
            loop {
                if let Some(at) = self.seen[from.min(self.seen.len())..]
                    .windows(needle.len())
                    .position(|window| window == needle.as_bytes())
                {
                    return from + at + needle.len();
                }
                let left = deadline.saturating_duration_since(Instant::now());
                match self.chunks.recv_timeout(left) {
                    Ok(chunk) => self.seen.extend(chunk),
                    Err(error) => panic!(
                        "{needle:?} never appeared ({error}): {}",
                        String::from_utf8_lossy(&self.seen[from.min(self.seen.len())..])
                            .escape_debug()
                    ),
                }
            }
        }
    }

    fn actor(name: &str) -> Actor<'_> {
        Actor {
            name: Some(Owner::try_new(name).unwrap()),
            now: 1,
        }
    }

    fn content(kind: RequestKind, title: &str, choices: &[(&str, &str)]) -> RequestContent {
        RequestContent {
            kind,
            title: title.into(),
            question: format!("{title} now?"),
            context: None,
            recommendation: None,
            choices: choices
                .iter()
                .map(|(id, label)| Choice {
                    id: (*id).into(),
                    label: (*label).into(),
                    detail: None,
                })
                .collect(),
            tasks: Vec::new(),
        }
    }

    /// A project with one coordinated goal and two open requests.
    fn fixture(catalog: &Path) -> (Project, String, String) {
        let mut store = Store::create(catalog).unwrap();
        let project = store.register_project("fixture").unwrap();
        let goal = store
            .create_goal(
                &project,
                "Ship the refactor",
                "briefs/goal.md",
                actor(COORDINATOR),
            )
            .unwrap();
        let goal = goal.root.id;
        store
            .coordinate(
                &project,
                &goal,
                Owner::try_new(COORDINATOR).unwrap(),
                false,
                false,
                1,
            )
            .unwrap();
        let spike = store
            .add(
                &project,
                &dots_tasks::NewTask {
                    title: "Old spike".into(),
                    body: None,
                    parent: Some(goal.clone()),
                    blocked_by: Vec::new(),
                },
                actor(COORDINATOR),
            )
            .unwrap();
        store
            .set_status(
                &project,
                &spike.id,
                dots_tasks::Status::Dropped,
                Some("superseded by the plan"),
                actor(COORDINATOR),
                false,
            )
            .unwrap();
        let names = store
            .raise_request(
                &project,
                &goal,
                "names",
                &content(
                    RequestKind::Decision,
                    "Pick names",
                    &[("update", "Update consumers"), ("keep", "Keep aliases")],
                ),
                actor(COORDINATOR),
            )
            .unwrap()
            .request
            .id;
        let signin = store
            .raise_request(
                &project,
                &goal,
                "signin",
                &content(RequestKind::Action, "Sign in", &[]),
                actor(COORDINATOR),
            )
            .unwrap()
            .request
            .id;
        (project, names, signin)
    }

    fn send(keyboard: &mut File, bytes: &str) {
        keyboard.write_all(bytes.as_bytes()).unwrap();
        keyboard.flush().unwrap();
        // Lone escapes are told apart from sequences by timing.
        std::thread::sleep(Duration::from_millis(150));
    }

    fn resize(slave: &OwnedFd, columns: u16, rows: u16, viewer: &Viewer) {
        let status = Command::new("stty")
            .args(["cols", &columns.to_string(), "rows", &rows.to_string()])
            .stdin(Stdio::from(slave.try_clone().unwrap()))
            .status()
            .unwrap();
        assert!(status.success(), "stty {status}");
        let pid = Pid::from_raw(i32::try_from(viewer.0.id()).unwrap());
        kill(pid, Signal::SIGWINCH).unwrap();
    }

    /// An SGR left click, 1-based.
    fn click(keyboard: &mut File, column: u16, row: u16) {
        send(
            keyboard,
            &format!("\x1b[<0;{column};{row}M\x1b[<0;{column};{row}m"),
        );
    }

    #[test]
    fn answers_persist_from_a_real_terminal_and_closing_answers_nothing() {
        let home = Home::new();
        let (project, names, signin) = fixture(&home.catalog());
        let size = Winsize {
            ws_row: 24,
            ws_col: 80,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        let pty::OpenptyResult { master, slave } = pty::openpty(&size, None).unwrap();
        let stdio = || Stdio::from(slave.try_clone().unwrap());
        let mut viewer = Viewer(
            Command::new(env!("CARGO_BIN_EXE_dots"))
                .args(["attention", "view", "--scope", "global"])
                .env_clear()
                .env("HOME", &home.0)
                .env("XDG_DATA_HOME", home.0.join("data"))
                .env("PATH", "/usr/bin:/bin")
                .env("TERM", "xterm-256color")
                .stdin(stdio())
                .stdout(stdio())
                .stderr(stdio())
                .spawn()
                .unwrap(),
        );
        let master = File::from(master);
        let mut keyboard = master.try_clone().unwrap();
        let mut screen = Screen::read(master);

        // The oldest question and its choices show at once on a narrow terminal.
        let at = screen.until(0, "names");
        let at = screen.until(at, "consumers");
        send(&mut keyboard, "\r");
        send(&mut keyboard, "\r");
        // The notice, not "Answer saved": the reload after a local answer
        // must not read as an answer from elsewhere.
        let at = screen.until(at, "“Pick");
        let mut store = Store::open(&home.catalog()).unwrap();
        let answered = store.request(&project, &names).unwrap();
        assert_eq!(answered.state, RequestState::Answered);
        let answer = answered.answer.unwrap();
        assert_eq!(answer.content.choice.as_deref(), Some("update"));
        assert_eq!(answer.content.source, Source::Ui);
        assert_eq!(answer.content.interpretation, None);
        assert_eq!(
            answer.content.provenance,
            "dots attention view --scope global"
        );
        assert_eq!(answer.by.as_deref(), Some("human"));

        // Wide enough, the inbox and the answer sit side by side.
        resize(&slave, 120, 40, &viewer);
        let at = screen.until(at, "Inbox");

        // The answered request stays selected; the user moves to the next one.
        // A question replaced while the user types keeps the draft with the
        // original, and repeated Return never answers the replacement.
        send(&mut keyboard, "\x1b[A");
        send(&mut keyboard, "\r");
        send(&mut keyboard, "\r");
        send(&mut keyboard, "half typed");
        let replacement = store
            .supersede_request(
                &project,
                &signin,
                &content(RequestKind::Action, "Sign in again", &[]),
                actor(COORDINATOR),
            )
            .unwrap()
            .request
            .id;
        let at = screen.until(at, "changed");
        for _ in 0..4 {
            send(&mut keyboard, "\r");
        }
        let at = screen.until(at, "unsent");
        send(&mut keyboard, "\x1b");

        // Mouse: the footer's Goals button, then Close in the drawer.
        click(&mut keyboard, 6, 39);
        let at = screen.until(at, "Goals");
        let at = screen.until(at, "refactor");
        screen.until(at, "Dropped");
        click(&mut keyboard, 15, 39);

        let deadline = Instant::now() + WATCHDOG;
        let status = loop {
            if let Some(status) = viewer.0.try_wait().unwrap() {
                break status;
            }
            assert!(Instant::now() < deadline, "the view did not close");
            std::thread::sleep(Duration::from_millis(20));
        };
        assert!(status.success(), "{status}");
        let flags = termios::tcgetattr(&slave).unwrap().local_flags;
        assert!(
            flags.contains(LocalFlags::ICANON | LocalFlags::ECHO),
            "{flags:?}"
        );
        screen.until(0, "\x1b[?1000l");

        let list = store.attention(&Scope::Global, StateFilter::All).unwrap();
        let requests = &list.goals[0].requests;
        let state = |id: &str| {
            requests
                .iter()
                .find(|request| request.id == id)
                .map(|request| (request.state, request.answer.is_some()))
        };
        assert_eq!(state(&names), Some((RequestState::Answered, true)));
        assert_eq!(state(&signin), Some((RequestState::Closed, false)));
        assert_eq!(state(&replacement), Some((RequestState::Open, false)));
    }
}
