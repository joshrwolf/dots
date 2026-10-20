//! Bounded one-shot subprocesses. Not for interactive terminals or services.
//!
//! Stdin is closed and both pipes are drained concurrently with bounded storage.
//! A timeout, oversized response, or I/O failure kills the child's process group
//! and reaps the direct child. A successful leader is never signalled: once it
//! is reaped, its group ID can be reused. Descendants that close both pipes or
//! leave the process group are not supervised.

use std::fs::File;
use std::io;
use std::os::unix::process::CommandExt as _;
use std::process::{Command, Output, Stdio};
use std::time::Duration;

use nix::sys::signal::{Signal, killpg};
use nix::unistd::Pid;
use tokio::io::{AsyncRead, AsyncReadExt as _};

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    #[error("subprocess deadline and output limits must be positive")]
    InvalidLimits,
    #[error("starting subprocess runtime")]
    Runtime(#[source] io::Error),
    #[error("starting subprocess")]
    Spawn(#[source] io::Error),
    #[error("reading subprocess {stream}")]
    Read {
        stream: &'static str,
        #[source]
        source: io::Error,
    },
    #[error("writing subprocess stdout to its destination")]
    Sink(#[source] io::Error),
    #[error("waiting for subprocess")]
    Wait(#[source] io::Error),
    #[error("subprocess exceeded its {0:?} deadline")]
    Timeout(Duration),
    #[error("subprocess {stream} exceeded {limit} bytes")]
    OutputLimit { stream: &'static str, limit: usize },
}

/// Execute a command with one deadline and the same limit for each stream.
///
/// See [`Runner::output`].
pub fn output(command: Command, deadline: Duration, limit: usize) -> Result<Output, Error> {
    Runner::new(deadline, limit).output(command)
}

/// Bounds for one synchronous, noninteractive command.
#[derive(Debug)]
pub struct Runner {
    deadline: Duration,
    stdout_limit: usize,
    stderr_limit: usize,
    stdout_sink: Option<File>,
}

impl Runner {
    /// Bound the whole run by `deadline` and each stream by `limit` bytes.
    pub fn new(deadline: Duration, limit: usize) -> Self {
        Self {
            deadline,
            stdout_limit: limit,
            stderr_limit: limit,
            stdout_sink: None,
        }
    }

    /// Give stdout its own byte budget; stderr keeps the original limit.
    #[must_use]
    pub fn stdout_limit(mut self, limit: usize) -> Self {
        self.stdout_limit = limit;
        self
    }

    /// Stream stdout into `sink` instead of retaining it; the returned stdout
    /// is empty. On failure, discard the sink's contents: it may hold a prefix,
    /// and one in-flight write may still complete after a timeout.
    #[must_use]
    pub fn stdout_to(mut self, sink: File) -> Self {
        self.stdout_sink = Some(sink);
        self
    }

    /// Run `command` with stdin closed and stdout/stderr captured.
    ///
    /// Do not call from inside a Tokio runtime. The caller supplies
    /// operation-specific context and interprets the exit status. A timeout
    /// never proves a side effect did not occur.
    pub fn output(self, command: Command) -> Result<Output, Error> {
        self.validate()?;
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(Error::Runtime)?;
        let result = runtime.block_on(self.run(command));
        // An OS write already in progress cannot be cancelled. Do not let runtime
        // shutdown turn a timeout into an unbounded wait for that sink syscall.
        runtime.shutdown_background();
        result
    }

    /// [`Runner::output`] for callers already inside a Tokio runtime; it needs
    /// the runtime's I/O and time drivers.
    pub async fn run(self, mut command: Command) -> Result<Output, Error> {
        self.validate()?;
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
        let result = tokio::time::timeout(self.deadline, async {
            let missing = |stream| Error::Read {
                stream,
                source: io::Error::other("pipe was not captured"),
            };
            let stdout = child.stdout.take().ok_or_else(|| missing("stdout"))?;
            let stderr = child.stderr.take().ok_or_else(|| missing("stderr"))?;
            // Do not reap the leader while descendants may still hold the pipes.
            // Its reserved PID makes process-group cleanup safe on failure.
            let (stdout, stderr) = tokio::try_join!(
                capture(stdout, "stdout", self.stdout_limit, self.stdout_sink),
                capture(stderr, "stderr", self.stderr_limit, None),
            )?;
            Ok(Output {
                status: child.wait().await.map_err(Error::Wait)?,
                stdout,
                stderr,
            })
        })
        .await
        .unwrap_or(Err(Error::Timeout(self.deadline)));
        // Never signal a group after its leader has been reaped: its PID can be
        // reused. On failure this child still owns the PID; on success do nothing.
        if result.is_err() && child.id().is_some() {
            if let Some(group) = group {
                let _ = killpg(group, Signal::SIGKILL);
            }
            let _ = child.start_kill();
            child.wait().await.map_err(Error::Wait)?;
        }
        result
    }

    fn validate(&self) -> Result<(), Error> {
        if self.deadline.is_zero() || self.stdout_limit == 0 || self.stderr_limit == 0 {
            return Err(Error::InvalidLimits);
        }
        Ok(())
    }
}

async fn capture(
    mut pipe: impl AsyncRead + Unpin,
    stream: &'static str,
    limit: usize,
    mut sink: Option<File>,
) -> Result<Vec<u8>, Error> {
    use std::io::Write as _;
    let mut length = 0;
    let mut bytes = Vec::new();
    let mut buffer = vec![0_u8; 16 * 1024];
    loop {
        let count = pipe
            .read(&mut buffer)
            .await
            .map_err(|source| Error::Read { stream, source })?;
        if count == 0 {
            return Ok(bytes);
        }
        let retained = count.min(limit.saturating_sub(length));
        let chunk = buffer.get(..retained).unwrap_or_default();
        if let Some(mut file) = sink.take() {
            let chunk = chunk.to_vec();
            // One bounded chunk in flight; never block pipe progress or the
            // deadline on a synchronous sink.
            sink = Some(
                tokio::task::spawn_blocking(move || {
                    file.write_all(&chunk)?;
                    Ok::<_, io::Error>(file)
                })
                .await
                .map_err(|error| Error::Sink(io::Error::other(error)))?
                .map_err(Error::Sink)?,
            );
        } else {
            bytes.extend_from_slice(chunk);
        }
        length += retained;
        if retained < count {
            return Err(Error::OutputLimit { stream, limit });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block_on<T>(future: impl Future<Output = T>) -> T {
        tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap()
            .block_on(future)
    }

    #[test]
    fn rejects_empty_bounds_before_spawning() {
        let missing = || Command::new("/nonexistent/never-started");
        for runner in [
            Runner::new(Duration::ZERO, 1),
            Runner::new(Duration::from_secs(1), 0),
            Runner::new(Duration::from_secs(1), 1).stdout_limit(0),
        ] {
            assert!(matches!(
                runner.output(missing()),
                Err(Error::InvalidLimits)
            ));
        }
        assert!(matches!(
            output(missing(), Duration::from_secs(1), 1),
            Err(Error::Spawn(_))
        ));
    }

    #[test]
    fn capture_keeps_exactly_the_limit_and_rejects_more() {
        let bytes = vec![b'x'; 40 * 1024];
        assert_eq!(
            block_on(capture(bytes.as_slice(), "stdout", bytes.len(), None)).unwrap(),
            bytes
        );
        let result = block_on(capture(bytes.as_slice(), "stderr", bytes.len() - 1, None));
        assert!(
            matches!(result, Err(Error::OutputLimit { stream: "stderr", limit }) if limit == bytes.len() - 1),
            "{result:?}"
        );
    }

    #[test]
    fn capture_streams_to_a_sink_without_retaining_a_second_copy() {
        let path = std::env::temp_dir().join(format!("dots-process-sink-{}", std::process::id()));
        let result = block_on(capture(
            b"complete".as_slice(),
            "stdout",
            8,
            Some(File::create(&path).unwrap()),
        ))
        .unwrap();
        assert_eq!(result, b"");
        assert_eq!(std::fs::read(&path).unwrap(), b"complete");
        let result = block_on(capture(
            b"overflow".as_slice(),
            "stdout",
            4,
            Some(File::create(&path).unwrap()),
        ));
        assert!(matches!(result, Err(Error::OutputLimit { .. })));
        assert_eq!(std::fs::read(&path).unwrap(), b"over");
        std::fs::remove_file(path).unwrap();
    }
}
