//! Bounded one-shot commands.

use std::fs::File;
use std::io;
use std::process::{Command, Output, Stdio};
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncReadExt as _, AsyncWriteExt as _};
use tokio::process::ChildStdin;

use crate::Error;
use crate::group::Group;

/// Execute a command with one deadline and the same limit for each stream.
///
/// See [`Runner::output`].
pub fn output(command: Command, deadline: Duration, limit: usize) -> Result<Output, Error> {
    Runner::new(deadline, limit).output(command)
}

/// Bounds for one noninteractive command.
#[derive(Debug)]
pub struct Runner {
    deadline: Duration,
    stdout_limit: usize,
    stderr_limit: usize,
    stdout_sink: Option<File>,
    stdin: Option<Vec<u8>>,
    stdin_limit: usize,
}

impl Runner {
    /// Bound the whole run by `deadline` and each stream by `limit` bytes.
    pub fn new(deadline: Duration, limit: usize) -> Self {
        Self {
            deadline,
            stdout_limit: limit,
            stderr_limit: limit,
            stdout_sink: None,
            stdin: None,
            stdin_limit: 0,
        }
    }

    /// Deliver bounded input while concurrently draining both output streams.
    ///
    /// Rejects excess input before spawning. The pipe closes after delivery so
    /// the child observes EOF. A write failure, cancellation or timeout does not
    /// prove that the child did not consume input or perform a side effect.
    #[must_use]
    pub fn stdin(mut self, bytes: Vec<u8>, limit: usize) -> Self {
        self.stdin = Some(bytes);
        self.stdin_limit = limit;
        self
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

    /// Run `command` with the configured input (closed by default) and captured output.
    ///
    /// The caller supplies operation-specific context and interprets the exit
    /// status. A timeout never proves a side effect did not occur.
    ///
    /// # Panics
    ///
    /// When called from inside a Tokio runtime; use [`Runner::run`] there.
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
    /// the runtime's I/O and time drivers. Dropping the future before it
    /// completes kills the command's process group.
    pub async fn run(self, command: Command) -> Result<Output, Error> {
        self.validate()?;
        let stdin_mode = if self.stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        };
        let (mut group, stdout, stderr) = Group::spawn(command, stdin_mode)?;
        let stdin = group.take_stdin();
        let result = tokio::time::timeout(self.deadline, async {
            // Do not reap the leader while descendants may still hold the pipes.
            // Its reserved PID makes process-group cleanup safe on failure.
            let (stdout, stderr, ()) = tokio::try_join!(
                capture(stdout, "stdout", self.stdout_limit, self.stdout_sink),
                capture(stderr, "stderr", self.stderr_limit, None),
                deliver(stdin, self.stdin),
            )?;
            Ok(Output {
                status: group.wait().await?,
                stdout,
                stderr,
            })
        })
        .await
        .unwrap_or(Err(Error::Timeout(self.deadline)));
        if result.is_err() {
            group.kill();
            group.wait().await?;
        }
        result
    }

    fn validate(&self) -> Result<(), Error> {
        if self.deadline.is_zero()
            || self.stdout_limit == 0
            || self.stderr_limit == 0
            || (self.stdin.is_some() && self.stdin_limit == 0)
        {
            return Err(Error::InvalidLimits);
        }
        if self
            .stdin
            .as_ref()
            .is_some_and(|input| input.len() > self.stdin_limit)
        {
            return Err(Error::InputLimit {
                limit: self.stdin_limit,
            });
        }
        Ok(())
    }
}

async fn deliver(stdin: Option<ChildStdin>, input: Option<Vec<u8>>) -> Result<(), Error> {
    if let Some(input) = input {
        let mut pipe = stdin.expect("configured input has a piped child stdin");
        pipe.write_all(&input).await.map_err(Error::Stdin)?;
    } else {
        assert!(stdin.is_none(), "default subprocess stdin is not piped");
    }
    Ok(())
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
        // `read` reports at most the buffer's length.
        let retained = count.min(limit.saturating_sub(length));
        let chunk = &buffer[..retained];
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
            Runner::new(Duration::from_secs(1), 1).stdin(Vec::new(), 0),
        ] {
            assert!(matches!(
                runner.output(missing()),
                Err(Error::InvalidLimits)
            ));
        }
        assert!(matches!(
            Runner::new(Duration::from_secs(1), 1)
                .stdin(vec![0; 2], 1)
                .output(missing()),
            Err(Error::InputLimit { limit: 1 })
        ));
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
