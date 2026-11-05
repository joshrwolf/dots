//! Process-lifecycle acceptance for the runner, using only its own fixture.
#![cfg(feature = "testing")]

use std::fs::File;
use std::io::Read as _;
use std::os::fd::OwnedFd;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use dots_process::{Error, Lines, Runner};
use nix::errno::Errno;
use nix::sys::signal::{self, Signal};
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
    let got = dots_process::output(fixture(&["emit", "out", "err", "0"]), LONG, 32).unwrap();
    assert!(got.status.success());
    assert_eq!(got.stdout, b"out");
    assert_eq!(got.stderr, b"err");
}

#[test]
fn bounded_input_and_both_outputs_make_progress_without_pipe_deadlock() {
    let input = vec![b'i'; 256 * 1024];
    let prefix = 128 * 1024;
    let got = Runner::new(LONG, prefix)
        .stdin(input.clone(), input.len())
        .stdout_limit(prefix + input.len())
        .output(fixture(&["stdin", "131072"]))
        .expect("input and both output streams must complete together");
    assert!(got.status.success());
    let mut expected = vec![b'o'; prefix];
    expected.extend(input);
    assert_eq!(got.stdout, expected);
    assert_eq!(got.stderr, vec![b'e'; prefix]);
}

#[test]
fn default_and_explicit_empty_input_deliver_eof() {
    for runner in [
        Runner::new(LONG, 16),
        Runner::new(LONG, 16).stdin(Vec::new(), 1),
    ] {
        let got = runner
            .output(fixture(&["stdin", "0"]))
            .expect("closed or empty input must terminate the reader");
        assert!(got.status.success());
        assert_eq!(got.stdout, b"");
        assert_eq!(got.stderr, b"");
    }
}

#[test]
fn early_input_closure_is_not_reported_as_successful_delivery() {
    let result = Runner::new(LONG, 1024)
        .stdin(vec![0; 2 * 1024 * 1024], 2 * 1024 * 1024)
        .output(fixture(&["emit", "", "", "0"]));
    assert!(
        matches!(result, Err(Error::Stdin(ref error))
        if error.kind() == std::io::ErrorKind::BrokenPipe),
        "{result:?}"
    );
}

#[test]
fn blocked_input_obeys_deadline_and_kills_the_process_group() {
    let scratch = Scratch::new("stdin-group").expect("create fixture directory");
    let (ready, lock) = (scratch.path("ready"), scratch.path("lock"));
    let result = Runner::new(DEADLINE, 1024)
        .stdin(vec![0; 2 * 1024 * 1024], 2 * 1024 * 1024)
        .output(fixture(&["group", &ready, &lock]));
    assert!(matches!(result, Err(Error::Timeout(_))), "{result:?}");
    assert_eq!(
        signal::kill(pid(&ready).expect("fixture became ready"), None),
        Err(Errno::ESRCH)
    );
    within(move || {
        File::open(lock)
            .expect("open descendant lock")
            .lock()
            .expect("lock released");
    })
    .expect("descendant survived the input deadline");
}

#[test]
fn output_limit_cancels_a_simultaneously_blocked_input() {
    let result = Runner::new(LONG, 1024)
        .stdin(vec![0; 2 * 1024 * 1024], 2 * 1024 * 1024)
        .output(fixture(&["write", "stdout", "65536", "hold"]));
    assert!(
        matches!(
            result,
            Err(Error::OutputLimit {
                stream: "stdout",
                ..
            })
        ),
        "{result:?}"
    );
}

#[test]
fn returns_a_nonzero_status_to_the_caller() {
    let got = dots_process::output(fixture(&["emit", "", "failed", "7"]), LONG, 32).unwrap();
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
        let result = dots_process::output(
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
    let result = dots_process::output(fixture(&["spew"]), LONG, 128 * 1024);
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
    let result = dots_process::output(fixture(&["group", &ready, &lock]), DEADLINE, 1024);
    assert!(matches!(result, Err(Error::Timeout(_))), "{result:?}");
    assert!(started.elapsed() < DEADLINE + WATCHDOG);
    // The leader was our child: if it were not reaped it would still be a zombie.
    assert_eq!(signal::kill(pid(&ready).unwrap(), None), Err(Errno::ESRCH));
    // The kernel releases the descendant's lock only when it has exited.
    within(move || File::open(lock).unwrap().lock().unwrap())
        .expect("descendant still holds the lock");
}

/// The PID a fixture records once it is ready, polled until it is complete.
async fn ready_pid(path: &str) -> Pid {
    loop {
        if let Ok(pid) = pid(path) {
            return pid;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

#[test]
fn cancelling_a_run_kills_its_process_group_and_reaps_the_leader() {
    let scratch = Scratch::new("cancel").unwrap();
    let (ready, lock) = (scratch.path("ready"), scratch.path("lock"));
    let command = fixture(&["group", &ready, &lock]);
    within(move || {
        block_on(async move {
            let run = Runner::new(LONG, 1024).run(command);
            let leader = tokio::select! {
                result = run => panic!("the run ended before it was cancelled: {result:?}"),
                leader = ready_pid(&ready) => leader,
            };
            // Losing the select dropped the run. A dropped child is reaped once
            // the runtime sees its exit; until then it would still be a zombie.
            while signal::kill(leader, None) != Err(Errno::ESRCH) {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .unwrap();
    })
    .expect("the cancelled leader was never reaped");
    // A descendant in the leader's group holds the lock until the group dies.
    within(move || File::open(lock).unwrap().lock().unwrap())
        .expect("descendant survived the cancelled run");
}

#[test]
fn detached_pipe_holder_cannot_extend_the_deadline() {
    let scratch = Scratch::new("detach").unwrap();
    let holder = scratch.path("holder");
    let started = Instant::now();
    let result = dots_process::output(fixture(&["detach", &holder]), DEADLINE, 1024);
    let elapsed = started.elapsed();
    // A holder in its own session is outside the group the runner may kill.
    let survived = signal::kill(pid(&holder).unwrap(), Signal::SIGKILL);
    assert!(matches!(result, Err(Error::Timeout(_))), "{result:?}");
    assert!(elapsed < DEADLINE + WATCHDOG);
    assert_eq!(survived, Ok(()));
}

#[test]
fn successful_leader_leaves_closed_pipe_descendants_alone() {
    let scratch = Scratch::new("orphan").unwrap();
    let gate = scratch.path("gate");
    nix::unistd::mkfifo(Path::new(&gate), nix::sys::stat::Mode::S_IRWXU).unwrap();
    let result = dots_process::output(fixture(&["orphan", &gate]), LONG, 1024);
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

fn block_on<T>(future: impl std::future::Future<Output = T>) -> std::io::Result<T> {
    Ok(tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(future))
}

#[test]
fn a_line_stream_delivers_each_line_then_how_the_command_ended() {
    let (seen, ended) = within(|| {
        block_on(async {
            let mut lines = Lines::spawn(fixture(&["lines", "3", "watch failed"]), 64).unwrap();
            let mut seen = Vec::new();
            while let Some(line) = lines.next_line().await.unwrap() {
                seen.push(line);
            }
            (seen, lines.wait().await.unwrap())
        })
        .unwrap()
    })
    .expect("stream did not end");
    assert_eq!(seen, ["line 1", "line 2", "line 3"]);
    assert_eq!(ended.status.code(), Some(3));
    assert_eq!(ended.stderr, "watch failed");
}

#[test]
fn a_line_stream_rejects_a_line_over_its_limit() {
    let result = within(|| {
        block_on(async {
            let mut lines =
                Lines::spawn(fixture(&["write", "stdout", "4096", "hold"]), 100).unwrap();
            lines.next_line().await
        })
        .unwrap()
    })
    .expect("oversized line did not end the read");
    assert!(
        matches!(
            result,
            Err(Error::OutputLimit {
                stream: "stdout",
                limit: 100
            })
        ),
        "{result:?}"
    );
}

#[test]
fn an_unfinished_wait_for_a_line_stream_kills_its_process_group_when_dropped() {
    let scratch = Scratch::new("lines-linger").unwrap();
    let lock = scratch.path("lock");
    let command = fixture(&["linger", &lock]);
    within(move || {
        block_on(async move {
            let mut lines = Lines::spawn(command, 64).unwrap();
            assert_eq!(lines.next_line().await.unwrap(), None);
            // The exited leader stays unreaped while a descendant holds stderr.
            let waited = tokio::time::timeout(Duration::from_millis(200), lines.wait()).await;
            assert!(waited.is_err(), "{waited:?}");
        })
        .unwrap();
    })
    .expect("stdout never closed");
    within(move || File::open(lock).unwrap().lock().unwrap())
        .expect("descendant survived the dropped wait");
}

#[test]
fn dropping_a_wait_releases_stderr_held_outside_the_group() {
    let scratch = Scratch::new("lines-escape").unwrap();
    let holder = scratch.path("holder");
    let command = fixture(&["escape", &holder]);
    within(move || {
        block_on(async move {
            let mut lines = Lines::spawn(command, 64).unwrap();
            assert_eq!(lines.next_line().await.unwrap(), None);
            let holder = ready_pid(&holder).await;
            // The group cannot reach the holder, so stderr stays open.
            let waited = tokio::time::timeout(Duration::from_millis(200), lines.wait()).await;
            assert!(waited.is_err(), "{waited:?}");
            // Only closing our end of stderr stops it: its next write fails.
            while signal::kill(holder, None) != Err(Errno::ESRCH) {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .unwrap();
    })
    .expect("the dropped wait kept reading stderr");
}

#[test]
fn dropping_a_line_stream_kills_its_process_group() {
    let scratch = Scratch::new("lines-group").unwrap();
    let (ready, lock) = (scratch.path("ready"), scratch.path("lock"));
    within(move || {
        block_on(async move {
            let lines = Lines::spawn(fixture(&["group", &ready, &lock]), 64).unwrap();
            while !Path::new(&ready).exists() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            drop(lines);
        })
        .unwrap();
    })
    .expect("fixture never became ready");
    // A descendant in the leader's group holds the lock until the group dies.
    within(move || File::open(scratch.path("lock")).unwrap().lock().unwrap())
        .expect("descendant survived the dropped stream");
}
