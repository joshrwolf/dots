//! The installed program on a pseudo-terminal, against a scratch repository:
//! the terminal's answers choose image or text headers, `ctl` reaches the
//! running viewer, and quitting gives the terminal back.

use std::fs::File;
use std::io::{Read as _, Write as _};
use std::os::fd::OwnedFd;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Output, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use nix::pty::{self, Winsize};

/// Hang watchdog for observations, not a guess at how quickly they happen.
const WATCHDOG: Duration = Duration::from_secs(20);
const LEAVE_ALTERNATE: &[u8] = b"\x1b[?1049l";
/// The device-attributes request that ends the viewer's startup queries.
const END_OF_QUERIES: &[u8] = b"\x1b[c";
const DEVICE_ATTRIBUTES: &[u8] = b"\x1b[?62;22c";
const GRAPHICS_OK: &[u8] = b"\x1b_Gi=31;OK\x1b\\";
/// An image transmission, as opposed to the graphics query.
const TRANSMIT: &[u8] = b"\x1b_Ga=T,";
/// The character image header cells hold.
const PLACEHOLDER: &str = "\u{10EEEE}";

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

/// A scratch repository and the directories the viewer reads and writes,
/// removed when the test ends.
struct Scratch(PathBuf);

impl Scratch {
    /// A repository whose `feature` branch changes `notes.txt` since `main`.
    fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!("dd-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for dir in ["repo", "home", "state", "run", "pi"] {
            std::fs::create_dir_all(root.join(dir)).expect("creating the scratch tree");
        }
        // The viewer reports canonical paths; macOS's temporary directory
        // is behind a symbolic link.
        let scratch = Self(root.canonicalize().expect("resolving the scratch tree"));
        scratch.git(&["init", "-q", "-b", "main"]);
        std::fs::write(scratch.repo().join("notes.txt"), "one\n").expect("writing");
        scratch.git(&["add", "."]);
        scratch.git(&["commit", "-q", "-m", "one"]);
        scratch.git(&["switch", "-q", "-c", "feature"]);
        std::fs::write(scratch.repo().join("notes.txt"), "one\ntwo\n").expect("writing");
        scratch.git(&["commit", "-q", "-am", "two"]);
        scratch
    }

    fn repo(&self) -> PathBuf {
        self.0.join("repo")
    }

    fn git(&self, args: &[&str]) {
        let mut git = Command::new("git");
        git.args(["-c", "user.name=t", "-c", "user.email=t@t"])
            .args(["-c", "commit.gpgsign=false"])
            .args(args)
            .current_dir(self.repo());
        let out = self.isolate(&mut git).output().expect("running git");
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    /// Runs `command` with nothing from the user's own setup.
    fn isolate<'c>(&self, command: &'c mut Command) -> &'c mut Command {
        command
            .env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .env("TERM", "xterm-256color")
            .env("HOME", self.0.join("home"))
            .env("XDG_STATE_HOME", self.0.join("state"))
            .env("XDG_CACHE_HOME", self.0.join("state"))
            .env("XDG_RUNTIME_DIR", self.0.join("run"))
            .env("PI_CODING_AGENT_DIR", self.0.join("pi"))
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
    }

    fn ctl(&self, args: &[&str]) -> Output {
        let mut ctl = Command::new(env!("CARGO_BIN_EXE_dots-diff"));
        ctl.arg("ctl").args(args).current_dir(self.repo());
        self.isolate(&mut ctl).output().expect("running ctl")
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The viewer, killed and reaped if the test fails before it exits.
struct Viewer(Child);

impl Viewer {
    fn wait(&mut self) -> ExitStatus {
        let start = Instant::now();
        loop {
            if let Some(status) = self.0.try_wait().expect("checking the viewer") {
                return status;
            }
            assert!(start.elapsed() < WATCHDOG, "the viewer did not exit");
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

impl Drop for Viewer {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Everything written to the terminal, read on another thread so the viewer
/// never blocks on a full terminal buffer.
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

    /// Reads until `needle` appears after `from`, or the watchdog fires.
    fn until_after(&mut self, from: usize, needle: &[u8]) -> bool {
        let deadline = Instant::now() + WATCHDOG;
        while find(&self.seen[from.min(self.seen.len())..], needle).is_none() {
            let left = deadline.saturating_duration_since(Instant::now());
            match self.chunks.recv_timeout(left) {
                Ok(chunk) => self.seen.extend(chunk),
                Err(_) => return false,
            }
        }
        true
    }

    fn until(&mut self, needle: &[u8]) -> bool {
        self.until_after(0, needle)
    }

    fn text(&self) -> String {
        String::from_utf8_lossy(&self.seen)
            .escape_debug()
            .to_string()
    }
}

/// Starts the viewer on `repo` in a fresh 120x30 terminal with 9x19-pixel
/// cells, and answers its startup queries with `answer`.
fn start(repo: &Path, scratch: &Scratch, answer: &[u8]) -> (Viewer, File, Screen, OwnedFd) {
    let size = Winsize {
        ws_row: 30,
        ws_col: 120,
        ws_xpixel: 120 * 9,
        ws_ypixel: 30 * 19,
    };
    let pty::OpenptyResult { master, slave } =
        pty::openpty(&size, None).expect("opening a terminal");
    let stdio = || Stdio::from(slave.try_clone().expect("sharing the terminal"));
    let mut command = Command::new(env!("CARGO_BIN_EXE_dots-diff"));
    command.arg(repo).current_dir(repo);
    scratch
        .isolate(&mut command)
        .stdin(stdio())
        .stdout(stdio())
        .stderr(stdio());
    let viewer = Viewer(command.spawn().expect("starting the viewer"));
    let master = File::from(master);
    let mut keyboard = master.try_clone().expect("sharing the terminal");
    let mut screen = Screen::read(master);
    assert!(
        screen.until(END_OF_QUERIES),
        "never asked the terminal: {}",
        screen.text()
    );
    keyboard.write_all(answer).expect("answering");
    (viewer, keyboard, screen, slave)
}

/// Quits with `q` and checks the terminal is given back.
fn quit(mut viewer: Viewer, keyboard: &mut File, screen: &mut Screen) {
    let before = screen.seen.len();
    keyboard.write_all(b"q").expect("typing");
    assert!(viewer.wait().success(), "the viewer failed");
    assert!(
        screen.until_after(before, LEAVE_ALTERNATE),
        "the viewer kept the alternate screen: {}",
        screen.text()
    );
}

#[test]
fn a_terminal_with_kitty_graphics_gets_image_headers_and_ctl_reaches_the_viewer() {
    let scratch = Scratch::new("graphics");
    let mut answer = GRAPHICS_OK.to_vec();
    answer.extend_from_slice(DEVICE_ATTRIBUTES);
    let (viewer, mut keyboard, mut screen, _slave) = start(&scratch.repo(), &scratch, &answer);
    assert!(
        screen.until(TRANSMIT) && screen.until(PLACEHOLDER.as_bytes()),
        "no image header: {}",
        screen.text()
    );

    // The viewer lists itself, and a note sent to it reaches the screen.
    let deadline = Instant::now() + WATCHDOG;
    let listed = loop {
        let out = scratch.ctl(&["list"]);
        let listed = String::from_utf8_lossy(&out.stdout).into_owned();
        if listed.contains("repo") || Instant::now() > deadline {
            break listed;
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    assert_eq!(
        listed.trim_end(),
        format!("-\t{}", scratch.repo().display())
    );
    let before = screen.seen.len();
    let note = scratch.ctl(&["note", "--file", "notes.txt", "--line", "2", "reads well"]);
    assert!(
        note.status.success(),
        "ctl note: {}",
        String::from_utf8_lossy(&note.stderr)
    );
    assert!(
        screen.until_after(before, b"reads well"),
        "the note never showed: {}",
        screen.text()
    );

    quit(viewer, &mut keyboard, &mut screen);
}

#[test]
fn a_terminal_without_kitty_graphics_gets_text_headers() {
    let scratch = Scratch::new("text");
    let (viewer, mut keyboard, mut screen, _slave) =
        start(&scratch.repo(), &scratch, DEVICE_ATTRIBUTES);
    assert!(
        screen.until(b"notes.txt"),
        "no file header: {}",
        screen.text()
    );
    quit(viewer, &mut keyboard, &mut screen);
    assert!(
        find(&screen.seen, TRANSMIT).is_none()
            && find(&screen.seen, PLACEHOLDER.as_bytes()).is_none(),
        "images in a terminal without graphics: {}",
        screen.text()
    );
    // Quitting removed the viewer's control socket.
    assert_eq!(scratch.ctl(&["list"]).stdout, b"");
}
