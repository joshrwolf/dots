//! Bounded one-shot subprocesses. Not for interactive terminals or services.
//!
//! Both pipes are drained concurrently with bounded storage. A timeout, oversized
//! response, or I/O failure kills the process group and reaps the direct child.

use std::io;
use std::process::{Command, Output, Stdio};
use std::time::Duration;

use nix::sys::signal::{Signal, killpg};
use nix::unistd::Pid;
use thiserror::Error;
use tokio::io::{AsyncRead, AsyncReadExt as _};

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum Error {
    #[error("subprocess deadline and output limit must be positive")]
    InvalidLimits,
    #[error("starting subprocess runtime")]
    Runtime(#[source] io::Error),
    #[error("starting subprocess")]
    Spawn(#[source] io::Error),
    #[error("subprocess I/O failed")]
    Io(#[from] io::Error),
    #[error("subprocess exceeded its {0:?} deadline")]
    Timeout(Duration),
    #[error("subprocess {stream} exceeded {limit} bytes")]
    OutputLimit { stream: &'static str, limit: usize },
}

/// Execute a synchronous, noninteractive command with a per-stream output limit.
///
/// Stdin is closed and stdout/stderr are captured. Do not call from inside a
/// Tokio runtime. The caller supplies operation-specific error context and
/// interprets the exit status. A timeout never proves a side effect did not occur.
/// This is not a supervisor for detached descendants that close both pipes.
pub fn output(command: Command, deadline: Duration, limit: usize) -> Result<Output, Error> {
    if deadline.is_zero() || limit == 0 {
        return Err(Error::InvalidLimits);
    }
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(Error::Runtime)?
        .block_on(run(command, deadline, limit))
}

async fn capture(
    mut pipe: impl AsyncRead + Unpin,
    stream: &'static str,
    limit: usize,
) -> Result<Vec<u8>, Error> {
    let mut bytes = Vec::new();
    let mut chunk = [0; 4096];
    loop {
        let n = pipe.read(&mut chunk).await?;
        if n == 0 {
            return Ok(bytes);
        }
        if n > limit.saturating_sub(bytes.len()) {
            return Err(Error::OutputLimit { stream, limit });
        }
        bytes.extend_from_slice(chunk.get(..n).unwrap_or_default());
    }
}

async fn run(mut command: Command, deadline: Duration, limit: usize) -> Result<Output, Error> {
    use std::os::unix::process::CommandExt as _;

    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0);
    let mut child = tokio::process::Command::from(command)
        .kill_on_drop(true)
        .spawn()
        .map_err(Error::Spawn)?;
    let group = child
        .id()
        .and_then(|id| i32::try_from(id).ok())
        .map(Pid::from_raw);
    let result = tokio::time::timeout(deadline, async {
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| io::Error::other("stdout was not captured"))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| io::Error::other("stderr was not captured"))?;
        // Do not reap the leader while descendants may still hold the pipes.
        // Its reserved PID makes process-group cleanup safe on capture failure.
        let (stdout, stderr) = tokio::try_join!(
            capture(stdout, "stdout", limit),
            capture(stderr, "stderr", limit),
        )?;
        Ok(Output {
            status: child.wait().await?,
            stdout,
            stderr,
        })
    })
    .await
    .unwrap_or(Err(Error::Timeout(deadline)));
    // Never signal a group after its leader has been reaped: its PID can be
    // reused. On failure this child still owns the PID; on success we do nothing.
    if result.is_err() && child.id().is_some() {
        if let Some(group) = group {
            let _ = killpg(group, Signal::SIGKILL);
        }
        let _ = child.start_kill();
        child.wait().await?;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shell(script: &str) -> Command {
        let mut command = Command::new("/bin/sh");
        command.args(["-c", script]);
        command
    }

    #[test]
    fn captures_both_pipes_and_nonzero_status() {
        let got = output(
            shell("printf out; printf err >&2; exit 7"),
            Duration::from_secs(2),
            32,
        )
        .unwrap();
        assert_eq!(got.stdout, b"out");
        assert_eq!(got.stderr, b"err");
        assert_eq!(got.status.code(), Some(7));
    }

    #[test]
    fn bounds_output_without_deadlocking_on_full_pipes() {
        let result = output(
            shell("while :; do printf 'xxxxxxxxxxxxxxxx'; printf 'xxxxxxxxxxxxxxxx' >&2; done"),
            Duration::from_secs(2),
            128,
        );
        assert!(
            matches!(result, Err(Error::OutputLimit { .. })),
            "{result:?}"
        );
    }

    #[test]
    fn successful_leader_does_not_signal_a_reaped_process_group() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("herdrkit-process-{}-{nonce}", std::process::id()));
        std::fs::create_dir(&root).unwrap();
        let marker = root.join("finished");
        let mut command =
            shell("(sleep 0.05; printf survived > \"$1\") </dev/null >/dev/null 2>&1 & exit 0");
        command.arg("process-test").arg(&marker);
        let result = output(command, Duration::from_secs(2), 128);
        let started = std::time::Instant::now();
        while std::fs::read(&marker).ok().as_deref() != Some(b"survived")
            && started.elapsed() < Duration::from_secs(2)
        {
            std::thread::sleep(Duration::from_millis(10));
        }
        let contents = std::fs::read(&marker);
        std::fs::remove_dir_all(&root).unwrap();
        assert!(result.unwrap().status.success());
        assert_eq!(contents.unwrap(), b"survived");
    }

    #[test]
    fn times_out_when_the_leader_closes_both_pipes_but_keeps_running() {
        let result = output(
            shell("exec 1>&- 2>&-; sleep 20"),
            Duration::from_millis(100),
            128,
        );
        assert!(matches!(result, Err(Error::Timeout(_))), "{result:?}");
    }

    #[test]
    fn times_out_even_when_descendant_holds_pipes() {
        let result = output(shell("sleep 20 & exit 0"), Duration::from_millis(100), 128);
        assert!(matches!(result, Err(Error::Timeout(_))), "{result:?}");
    }
}
