//! Follow modes: commands that keep writing while their reader is alive.
use std::io::{self, Write};
use std::time::{Duration, Instant};

use anyhow::{Context as _, Result};

/// Write `text` and flush it; `false` once the reader has closed its end.
pub(crate) fn write(out: &mut dyn Write, text: &str) -> Result<bool> {
    match out.write_all(text.as_bytes()).and_then(|()| out.flush()) {
        Ok(()) => Ok(true),
        Err(error) if error.kind() == io::ErrorKind::BrokenPipe => Ok(false),
        Err(error) => Err(error).context("write follow output"),
    }
}

/// Sleep for `duration` unless stdout's reader closes first; `false` once it has.
///
/// Polling stdout for a hang-up notices a closed pipe without writing to it.
/// macOS reports `POLLHUP` only alongside a requested event, so this asks for
/// `POLLPRI`, which a pipe's write end never raises otherwise. Where stdout
/// reports something else or cannot be polled, this sleeps.
pub(crate) fn sleep_while_read(duration: Duration) -> bool {
    use nix::errno::Errno;
    use nix::poll::{PollFd, PollFlags, PollTimeout, poll};
    use std::os::fd::AsFd as _;

    let deadline = Instant::now() + duration;
    let stdout = io::stdout();
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return true;
        }
        let timeout = PollTimeout::try_from(left).unwrap_or(PollTimeout::MAX);
        let mut fds = [PollFd::new(stdout.as_fd(), PollFlags::POLLPRI)];
        match poll(&mut fds, timeout) {
            Ok(0) | Err(Errno::EINTR) => {}
            Ok(_) => {
                let events = fds.first().and_then(nix::poll::PollFd::revents);
                if events.is_some_and(|events| {
                    events.intersects(PollFlags::POLLHUP | PollFlags::POLLERR)
                }) {
                    return false;
                }
                std::thread::sleep(left);
            }
            Err(_) => std::thread::sleep(left),
        }
    }
}

/// Exit 0 on SIGTERM, SIGINT, or SIGHUP: a reader stopping `follow` is its
/// normal end. Call before starting threads, which inherit the blocked mask;
/// child processes start with an empty one.
pub(crate) fn exit_on_signals() -> Result<()> {
    use nix::sys::signal::{SigSet, Signal};

    let mut signals = SigSet::empty();
    for signal in [Signal::SIGTERM, Signal::SIGINT, Signal::SIGHUP] {
        signals.add(signal);
    }
    signals
        .thread_block()
        .context("block termination signals")?;
    std::thread::Builder::new()
        .name("signals".into())
        .spawn(move || {
            if signals.wait().is_ok() {
                std::process::exit(0);
            }
            // Cannot wait: let the signals take their default action here.
            let _ = signals.thread_unblock();
            loop {
                std::thread::park();
            }
        })
        .context("start the signal thread")?;
    Ok(())
}
