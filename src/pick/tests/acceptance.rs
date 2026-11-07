//! Terminal acceptance for the picker on a pseudo-terminal, using only its own
//! fixture: no shell, no Rex.
#![cfg(feature = "testing")]

use std::fs::File;
use std::io::{Read as _, Write as _};
use std::os::fd::OwnedFd;
use std::path::PathBuf;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use nix::errno::Errno;
use nix::pty::{self, Winsize};
use nix::sys::signal;
use nix::sys::termios::{self, LocalFlags};
use nix::unistd::Pid;

/// Hang watchdog for observations, not a guess at how quickly they happen.
const WATCHDOG: Duration = Duration::from_secs(10);
const ENTER_ALTERNATE: &[u8] = b"\x1b[?1049h";
const LEAVE_ALTERNATE: &[u8] = b"\x1b[?1049l";
const ENABLE_PASTE: &[u8] = b"\x1b[?2004h";
const DISABLE_PASTE: &[u8] = b"\x1b[?2004l";
const PANIC: &[u8] = b"fixture fetch invariant";
const COOKED: LocalFlags = LocalFlags::ICANON.union(LocalFlags::ECHO);

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

struct Scratch(PathBuf);
impl Scratch {
    fn new(name: &str) -> std::io::Result<Self> {
        let path = std::env::temp_dir().join(format!("dots-pick-{name}-{}", std::process::id()));
        std::fs::create_dir(&path)?;
        Ok(Self(path))
    }
    fn path(&self, name: &str) -> String {
        self.0.join(name).to_string_lossy().into_owned()
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The fixture, killed and reaped if the test fails before it exits.
struct Fixture(Child);
impl Fixture {
    fn wait(&mut self) -> ExitStatus {
        let start = Instant::now();
        loop {
            if let Some(status) = self.0.try_wait().expect("checking the fixture") {
                return status;
            }
            assert!(start.elapsed() < WATCHDOG, "the picker did not exit");
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Everything written to the terminal, read on another thread so the fixture
/// never blocks on a full terminal buffer. Reading ends once no slave is open.
struct Screen {
    chunks: mpsc::Receiver<Vec<u8>>,
    seen: Vec<u8>,
}
impl Screen {
    fn read(master: OwnedFd) -> Self {
        let (sender, chunks) = mpsc::channel();
        std::thread::spawn(move || {
            let mut master = File::from(master);
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

    /// Reads until `needle` appears, or the reader ends, or the watchdog fires.
    fn until(&mut self, needle: &[u8]) -> bool {
        let deadline = Instant::now() + WATCHDOG;
        while find(&self.seen, needle).is_none() {
            let left = deadline.saturating_duration_since(Instant::now());
            match self.chunks.recv_timeout(left) {
                Ok(chunk) => self.seen.extend(chunk),
                Err(_) => return false,
            }
        }
        true
    }

    /// Reads to the end, once the last slave has closed.
    fn finish(&mut self) -> bool {
        let deadline = Instant::now() + WATCHDOG;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            match self.chunks.recv_timeout(left) {
                Ok(chunk) => self.seen.extend(chunk),
                Err(mpsc::RecvTimeoutError::Disconnected) => return true,
                Err(mpsc::RecvTimeoutError::Timeout) => return false,
            }
        }
    }

    fn text(&self) -> String {
        String::from_utf8_lossy(&self.seen)
            .escape_debug()
            .to_string()
    }
}

#[test]
fn a_panicking_job_restores_the_terminal_and_fails_the_picker() {
    let scratch = Scratch::new("panic").unwrap();
    let gate = scratch.path("gate");
    let size = Winsize {
        ws_row: 24,
        ws_col: 80,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    let pty::OpenptyResult { master, slave } = pty::openpty(&size, None).unwrap();
    assert!(
        termios::tcgetattr(&slave)
            .unwrap()
            .local_flags
            .contains(COOKED),
        "a new terminal starts cooked"
    );
    let stdio = || Stdio::from(slave.try_clone().unwrap());
    let mut fixture = Fixture(
        Command::new(env!("CARGO_BIN_EXE_dots-pick-panic-fixture"))
            .args(["pick", &gate])
            .stdin(stdio())
            .stdout(stdio())
            .stderr(stdio())
            .spawn()
            .unwrap(),
    );
    let mut screen = Screen::read(master);

    // Bracketed paste is enabled last, after raw mode and the alternate screen.
    assert!(
        screen.until(ENABLE_PASTE),
        "never took the terminal: {}",
        screen.text()
    );
    let running = termios::tcgetattr(&slave).unwrap().local_flags;
    assert!(
        !running.intersects(COOKED),
        "raw mode while running: {running:?}"
    );
    File::create(&gate).unwrap();

    let status = fixture.wait();
    assert_eq!(
        status.code(),
        Some(101),
        "the panic did not fail the picker"
    );
    let after = termios::tcgetattr(&slave).unwrap().local_flags;
    assert!(after.contains(COOKED), "raw mode left on: {after:?}");
    drop(slave);
    assert!(
        screen.finish(),
        "the terminal never closed: {}",
        screen.text()
    );
    let seen = &screen.seen;
    let enter = find(seen, ENTER_ALTERNATE).expect("entered the alternate screen");
    let leave = find(seen, LEAVE_ALTERNATE).expect("left the alternate screen");
    let panic = find(seen, PANIC).expect("reported the panic");
    // Ratatui's hook restores the screen before the panic is reported.
    assert!(enter < leave && leave < panic, "{}", screen.text());
    let enabled = find(seen, ENABLE_PASTE).expect("enabled bracketed paste");
    assert!(
        find(&seen[enabled..], DISABLE_PASTE).is_some(),
        "bracketed paste left on: {}",
        screen.text()
    );

    // Stopping the jobs killed the watch command's process group.
    let watch = std::fs::read_to_string(format!("{gate}.watch")).unwrap();
    let watch = Pid::from_raw(watch.parse().unwrap());
    let start = Instant::now();
    while signal::kill(watch, None) != Err(Errno::ESRCH) {
        assert!(start.elapsed() < WATCHDOG, "the watch command survived");
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// The action fixture on a fresh terminal, once it has drawn: the fixture,
/// the keyboard, the screen, and the slave side to drop once it exits.
fn act_fixture(args: &[&str]) -> (Fixture, File, Screen, OwnedFd) {
    let size = Winsize {
        ws_row: 24,
        ws_col: 80,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    let pty::OpenptyResult { master, slave } =
        pty::openpty(&size, None).expect("opening a terminal");
    let stdio = || Stdio::from(slave.try_clone().expect("sharing the terminal"));
    let fixture = Fixture(
        Command::new(env!("CARGO_BIN_EXE_dots-pick-act-fixture"))
            .args(args)
            .stdin(stdio())
            .stdout(stdio())
            .stderr(stdio())
            .spawn()
            .expect("starting the fixture"),
    );
    let keys = File::from(master.try_clone().expect("sharing the terminal"));
    let mut screen = Screen::read(master);
    assert!(
        screen.until(ENABLE_PASTE),
        "never took the terminal: {}",
        screen.text()
    );
    (fixture, keys, screen, slave)
}

#[test]
fn a_failed_action_stays_in_the_picker_and_a_successful_one_closes_it() {
    let (mut fixture, mut keys, mut screen, slave) = act_fixture(&[]);

    keys.write_all(b"\r").unwrap();
    assert!(
        screen.until(b"fixture action failed"),
        "the error was not shown: {}",
        screen.text()
    );
    assert!(
        fixture.0.try_wait().unwrap().is_none(),
        "the picker closed on a failed action"
    );

    keys.write_all(b"\r").unwrap();
    let status = fixture.wait();
    assert!(status.success(), "{status:?}: {}", screen.text());
    drop(slave);
    assert!(
        screen.finish(),
        "the terminal never closed: {}",
        screen.text()
    );
    let leave = find(&screen.seen, LEAVE_ALTERNATE).expect("left the alternate screen");
    let acted = find(&screen.seen, b"closed: Acted").expect("the picker returned after success");
    assert!(leave < acted, "{}", screen.text());
}

#[test]
fn a_detail_composer_keeps_filter_input_separate_and_discards_repeat_submission() {
    let (mut fixture, mut keys, mut screen, slave) = act_fixture(&["pane"]);
    assert!(screen.until(b"Original"), "{}", screen.text());
    keys.write_all(b"it\r\x1b[200~neutral reply\x1b[201~\r\r")
        .unwrap();
    assert!(screen.until(b"CONFIRMED"), "{}", screen.text());
    keys.write_all(b"\x1b").unwrap();
    let status = fixture.wait();
    assert!(status.success(), "{status:?}: {}", screen.text());
    assert!(
        termios::tcgetattr(&slave)
            .unwrap()
            .local_flags
            .contains(COOKED)
    );
    drop(slave);
    assert!(screen.finish(), "{}", screen.text());
    assert!(
        find(&screen.seen, b"query=it draft=neutral reply saved=1").is_some(),
        "{}",
        screen.text()
    );
}

#[test]
fn escape_closes_the_picker_as_cancelled() {
    let (mut fixture, mut keys, mut screen, slave) = act_fixture(&[]);

    keys.write_all(b"\x1b").unwrap();
    let status = fixture.wait();
    assert!(status.success(), "{status:?}: {}", screen.text());
    drop(slave);
    assert!(
        screen.finish(),
        "the terminal never closed: {}",
        screen.text()
    );
    assert!(
        find(&screen.seen, b"closed: Cancelled").is_some(),
        "{}",
        screen.text()
    );
}
