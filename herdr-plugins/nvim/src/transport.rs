//! Native Neovim RPC transport. The only executable Lua is a fixed entrypoint;
//! every request field is JSON data passed through luaeval's `_A` argument.

use std::ffi::OsStr;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

use anyhow::{Context as _, Result, ensure};

use crate::protocol::{MAX_REQUEST_BYTES, Reply, Request};

pub(crate) fn call(executable: &OsStr, socket: &Path, request: &Request) -> Result<Reply> {
    let json = serde_json::to_string(request)?;
    ensure!(
        json.len() <= MAX_REQUEST_BYTES,
        "editor request exceeds 16 KiB"
    );
    let expression = format!(
        "luaeval(\"require('dots.herdr').request(_A)\", {})",
        serde_json::to_string(&json)?
    );
    let mut command = Command::new(executable);
    command.args([
        OsStr::new("--server"),
        socket.as_os_str(),
        OsStr::new("--remote-expr"),
        OsStr::new(&expression),
    ]);
    let output = herdrkit::process::output(command, Duration::from_secs(3), 64 * 1024).context(
        "calling Neovim editor bridge; delivery may have occurred, do not automatically retry",
    )?;
    ensure!(
        output.status.success(),
        "Neovim bridge exited {}: {}",
        output.status,
        String::from_utf8_lossy(&output.stderr).trim()
    );
    serde_json::from_slice(&output.stdout)
        .context("decoding editor acknowledgement; inspect editor before retrying")
}
