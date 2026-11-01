//! Process lifecycle of the shipped binary: `dots github watch follow` ends
//! with its reader.
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
