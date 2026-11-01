//! A long-running command read one line at a time, for watchers that run until
//! their owner stops them.
//!
//! Unlike [`crate::Runner`], nothing bounds how long the command runs; its owner
//! does. Each line is bounded, stdin is closed, and the last bytes of stderr are
//! kept to explain an early exit. Dropping the stream, or the future of
//! [`Lines::wait`], kills the command's process group unless its leader has
//! been reaped, and closes both pipes.

use std::io;
use std::process::{Command, ExitStatus};

use tokio::io::{AsyncBufReadExt as _, AsyncReadExt as _, BufReader};
use tokio::process::{ChildStderr, ChildStdout};
use tokio::task::JoinHandle;

use crate::Error;
use crate::group::Group;

/// Bytes of stderr kept for [`Ended`].
const STDERR_TAIL: usize = 4 * 1024;

/// The command's stdout, line by line.
#[derive(Debug)]
pub struct Lines {
    group: Group,
    stdout: BufReader<ChildStdout>,
    stderr: JoinHandle<io::Result<Vec<u8>>>,
    line: Vec<u8>,
    limit: usize,
}

/// How a line stream's command ended.
#[derive(Debug)]
pub struct Ended {
    pub status: ExitStatus,
    /// The end of what the command wrote to stderr.
    pub stderr: String,
}

impl Lines {
    /// Starts `command`, rejecting any line longer than `limit` bytes. Call from
    /// inside a Tokio runtime with I/O enabled.
    pub fn spawn(command: Command, limit: usize) -> Result<Self, Error> {
        if limit == 0 {
            return Err(Error::InvalidLimits);
        }
        let (group, stdout, stderr) = Group::spawn(command)?;
        Ok(Self {
            group,
            stdout: BufReader::new(stdout),
            // Drains stderr so the command never blocks on it.
            stderr: tokio::spawn(tail(stderr)),
            line: Vec::new(),
            limit,
        })
    }

    /// The next line without its newline, or `None` once stdout closes.
    ///
    /// Cancel safe: a partially read line is kept for the next call.
    pub async fn next_line(&mut self) -> Result<Option<String>, Error> {
        let read = |source| Error::Read {
            stream: "stdout",
            source,
        };
        loop {
            let available = self.stdout.fill_buf().await.map_err(read)?;
            if available.is_empty() {
                if self.line.is_empty() {
                    return Ok(None);
                }
                return Ok(Some(self.take_line()));
            }
            let (chunk, complete) = match available.iter().position(|&byte| byte == b'\n') {
                Some(end) => (&available[..end], Some(end + 1)),
                None => (available, None),
            };
            if self.line.len() + chunk.len() > self.limit {
                return Err(Error::OutputLimit {
                    stream: "stdout",
                    limit: self.limit,
                });
            }
            self.line.extend_from_slice(chunk);
            let consumed = complete.unwrap_or(chunk.len());
            self.stdout.consume(consumed);
            if complete.is_some() {
                return Ok(Some(self.take_line()));
            }
        }
    }

    /// Waits, after stdout has closed, for the command to close stderr and
    /// exit.
    ///
    /// The leader is reaped only after stderr closes, as [`crate::Runner`]
    /// does, so the tail includes everything the command wrote. Like the
    /// stream itself, this is bounded only by its owner: a descendant still
    /// holding stderr delays it until that descendant exits or the future is
    /// dropped. Dropping it kills a descendant in the group and closes stderr;
    /// one that left the group is not otherwise supervised.
    pub async fn wait(mut self) -> Result<Ended, Error> {
        let read = |source| Error::Read {
            stream: "stderr",
            source,
        };
        let tail = match (&mut self.stderr).await {
            Ok(tail) => tail.map_err(read)?,
            Err(error) => match error.try_into_panic() {
                Ok(panic) => std::panic::resume_unwind(panic),
                // Only a runtime shutting down cancels the drain.
                Err(cancelled) => return Err(read(io::Error::other(cancelled))),
            },
        };
        let status = self.group.wait().await?;
        Ok(Ended {
            status,
            stderr: String::from_utf8_lossy(&tail).trim().to_owned(),
        })
    }

    fn take_line(&mut self) -> String {
        let line = String::from_utf8_lossy(&self.line).into_owned();
        self.line.clear();
        line
    }
}

impl Drop for Lines {
    // Stops draining stderr, closing the pipe even when a descendant outside
    // the group still holds it. The `Group` field then kills the group.
    fn drop(&mut self) {
        self.stderr.abort();
    }
}

/// The last [`STDERR_TAIL`] bytes of `stderr`, read to its end.
async fn tail(mut stderr: ChildStderr) -> io::Result<Vec<u8>> {
    let mut kept = Vec::new();
    let mut buffer = [0_u8; 1024];
    loop {
        let count = stderr.read(&mut buffer).await?;
        if count == 0 {
            return Ok(kept);
        }
        kept.extend_from_slice(&buffer[..count]);
        let excess = kept.len().saturating_sub(STDERR_TAIL);
        kept.drain(..excess);
    }
}
