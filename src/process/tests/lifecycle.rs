//! Process-lifecycle acceptance for the runner, using only its own fixture.
#![cfg(feature = "testing")]

use std::fs::File;
use std::io::Read as _;
use std::os::fd::OwnedFd;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use dots_process::{Error, Runner, output};
use nix::errno::Errno;
use nix::sys::signal::{Signal, kill};
use nix::unistd::Pid;

/// Deadline for runs that must time out. Fixtures signal readiness well before it.
const DEADLINE: Duration = Duration::from_secs(1);
/// Deadline for runs that must finish first; also bounds an accidental hang.
const LONG: Duration = Duration::from_secs(10);
/// Hang watchdog for observations, not a guess at how quickly they happen.
const WATCHDOG: Duration = Duration::from_secs(5);

fn fixture(args: &[&str]) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_dots-process-fixture"));
    command.args(args);
    command
}

struct Scratch(PathBuf);
impl Scratch {
    fn new(name: &str) -> std::io::Result<Self> {
        let path = std::env::temp_dir().join(format!("dots-process-{name}-{}", std::process::id()));
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

/// Run `observe` on another thread and fail instead of hanging the suite.
fn within<T: Send + 'static>(
    observe: impl FnOnce() -> T + Send + 'static,
) -> Result<T, mpsc::RecvTimeoutError> {
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = sender.send(observe());
    });
    receiver.recv_timeout(WATCHDOG)
}

fn pid(path: &str) -> Result<Pid, Box<dyn std::error::Error>> {
    // Missing means the fixture was not ready before the deadline.
    Ok(Pid::from_raw(std::fs::read_to_string(path)?.parse()?))
}

#[test]
fn captures_both_streams_of_a_successful_command() {
    let got = output(fixture(&["emit", "out", "err", "0"]), LONG, 32).unwrap();
    assert!(got.status.success());
    assert_eq!(got.stdout, b"out");
    assert_eq!(got.stderr, b"err");
}

#[test]
fn returns_a_nonzero_status_to_the_caller() {
    let got = output(fixture(&["emit", "", "failed", "7"]), LONG, 32).unwrap();
    assert_eq!(got.status.code(), Some(7));
    assert_eq!(got.stderr, b"failed");
}

#[test]
fn stdout_budget_is_separate_from_stderr() {
    let got = Runner::new(LONG, 16)
        .stdout_limit(64 * 1024)
        .output(fixture(&["write", "stdout", "40000", "exit"]))
        .unwrap();
    assert!(got.status.success());
    assert_eq!(got.stdout.len(), 40_000);
}

#[test]
fn oversized_output_stops_a_slow_producer_before_its_deadline() {
    for stream in ["stdout", "stderr"] {
        let started = Instant::now();
        let result = output(
            fixture(&["write", stream, "65536", "hold"]),
            LONG,
            32 * 1024,
        );
        assert!(
            matches!(result, Err(Error::OutputLimit { stream: actual, limit: 32_768 }) if actual == stream),
            "{stream}: {result:?}"
        );
        assert!(started.elapsed() < WATCHDOG);
    }
}

#[test]
fn bounds_output_without_deadlocking_on_full_pipes() {
    let result = output(fixture(&["spew"]), LONG, 128 * 1024);
    assert!(
        matches!(result, Err(Error::OutputLimit { .. })),
        "{result:?}"
    );
}

#[test]
fn deadline_kills_the_process_group_and_reaps_the_leader() {
    let scratch = Scratch::new("group").unwrap();
    let (ready, lock) = (scratch.path("ready"), scratch.path("lock"));
    let started = Instant::now();
    let result = output(fixture(&["group", &ready, &lock]), DEADLINE, 1024);
    assert!(matches!(result, Err(Error::Timeout(_))), "{result:?}");
    assert!(started.elapsed() < DEADLINE + WATCHDOG);
    // The leader was our child: if it were not reaped it would still be a zombie.
    assert_eq!(kill(pid(&ready).unwrap(), None), Err(Errno::ESRCH));
    // The kernel releases the descendant's lock only when it has exited.
    within(move || File::open(lock).unwrap().lock().unwrap())
        .expect("descendant still holds the lock");
}

#[test]
fn detached_pipe_holder_cannot_extend_the_deadline() {
    let scratch = Scratch::new("detach").unwrap();
    let holder = scratch.path("holder");
    let started = Instant::now();
    let result = output(fixture(&["detach", &holder]), DEADLINE, 1024);
    let elapsed = started.elapsed();
    // A holder in its own session is outside the group the runner may kill.
    let survived = kill(pid(&holder).unwrap(), Signal::SIGKILL);
    assert!(matches!(result, Err(Error::Timeout(_))), "{result:?}");
    assert!(elapsed < DEADLINE + WATCHDOG);
    assert_eq!(survived, Ok(()));
}

#[test]
fn successful_leader_leaves_closed_pipe_descendants_alone() {
    let scratch = Scratch::new("orphan").unwrap();
    let gate = scratch.path("gate");
    nix::unistd::mkfifo(Path::new(&gate), nix::sys::stat::Mode::S_IRWXU).unwrap();
    let result = output(fixture(&["orphan", &gate]), LONG, 1024);
    assert!(result.unwrap().status.success());
    // The descendant cannot finish before this reader opens the FIFO, so it
    // reports only if it survived the runner's return.
    let observed = within(move || {
        let mut bytes = Vec::new();
        File::open(gate).unwrap().read_to_end(&mut bytes).unwrap();
        bytes
    })
    .expect("descendant did not survive");
    assert_eq!(observed, b"survived");
}

#[test]
fn slow_destination_does_not_block_the_deadline_or_runtime_shutdown() {
    let (mut reader, writer) = std::io::pipe().unwrap();
    let sink = File::from(OwnedFd::from(writer));
    // The sink stays blocked until the runner returns.
    let result = within(move || {
        Runner::new(DEADLINE, 1024)
            .stdout_limit(3 * 1024 * 1024)
            .stdout_to(sink)
            .output(fixture(&["write", "stdout", "2097152", "exit"]))
    })
    .expect("runner waited for the blocked sink");
    assert!(matches!(result, Err(Error::Timeout(_))), "{result:?}");
    let written = within(move || {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).unwrap();
        bytes.len()
    })
    .expect("sink was not closed");
    assert!(
        written < 2 * 1024 * 1024,
        "must not continue writing the whole output after timeout"
    );
}
