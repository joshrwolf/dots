//! Ownership of a command's process group, tied to its leader's lifetime.
//!
//! Tokio's `kill_on_drop` signals only the direct child, so a cancelled run
//! would leave its descendants running. A [`Group`] instead signals the whole
//! group whenever it is stopped or dropped before its leader has been reaped.
//! Until then the leader's PID, which is also the group ID, stays reserved, so
//! the signal cannot reach an unrelated process. Once the leader is reaped the
//! group is never signalled again. A dropped leader is reaped by Tokio when
//! its runtime next observes a child exit.

use std::os::unix::process::CommandExt as _;
use std::process::{Command, ExitStatus, Stdio};

use nix::sys::signal::{self, Signal};
use nix::unistd::Pid;
use tokio::process::{Child, ChildStderr, ChildStdout};

use crate::Error;

/// A running command that leads its own process group.
#[derive(Debug)]
pub(crate) struct Group {
    leader: Child,
    id: Pid,
}

impl Group {
    /// Starts `command` in a new process group, with stdin closed and both
    /// output streams piped. Call from inside a Tokio runtime with I/O enabled.
    pub(crate) fn spawn(mut command: Command) -> Result<(Self, ChildStdout, ChildStderr), Error> {
        command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .process_group(0);
        let mut leader = tokio::process::Command::from(command)
            .spawn()
            .map_err(Error::Spawn)?;
        let pid = leader
            .id()
            .expect("a child that was just spawned has not been reaped");
        let id = Pid::from_raw(i32::try_from(pid).expect("a Unix PID fits in pid_t"));
        let stdout = leader.stdout.take().expect("stdout was piped");
        let stderr = leader.stderr.take().expect("stderr was piped");
        Ok((Self { leader, id }, stdout, stderr))
    }

    /// Waits for the leader to exit and reaps it. After this the group is
    /// never signalled.
    pub(crate) async fn wait(&mut self) -> Result<ExitStatus, Error> {
        self.leader.wait().await.map_err(Error::Wait)
    }

    /// Kills every process still in the group, if the leader is unreaped.
    pub(crate) fn kill(&mut self) {
        // `id` is `None` once Tokio has reaped the leader.
        if self.leader.id().is_some() {
            let _ = signal::killpg(self.id, Signal::SIGKILL);
            // The leader may have moved to another group.
            let _ = self.leader.start_kill();
        }
    }
}

impl Drop for Group {
    fn drop(&mut self) {
        self.kill();
    }
}
