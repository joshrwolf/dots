//! Steering a running viewer: one text line per connection over a per-user
//! Unix socket, answered with `ok` or an error line.

use std::io::{BufRead as _, BufReader, Write as _};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context as _, Result, bail};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Command {
    /// Show the worktree containing a path.
    Worktree(PathBuf),
    /// Scroll to a file, by absolute path or path within the worktree.
    File(String),
    Refresh,
}

impl Command {
    pub(crate) fn parse(line: &str) -> Result<Self> {
        let (verb, arg) = line.trim().split_once(' ').unwrap_or((line.trim(), ""));
        Ok(match (verb, arg.trim()) {
            ("worktree", path) if !path.is_empty() => Command::Worktree(PathBuf::from(path)),
            ("file", path) if !path.is_empty() => Command::File(path.to_owned()),
            ("refresh", "") => Command::Refresh,
            _ => bail!(
                "unknown command {line:?}; expected `worktree PATH`, `file PATH` or `refresh`"
            ),
        })
    }

    fn line(&self) -> String {
        match self {
            Command::Worktree(p) => format!("worktree {}", p.display()),
            Command::File(p) => format!("file {p}"),
            Command::Refresh => "refresh".to_owned(),
        }
    }
}

/// The control socket: in the runtime directory, or the user's state directory.
pub(crate) fn socket_path() -> PathBuf {
    if let Some(dir) = std::env::var_os("XDG_RUNTIME_DIR") {
        return PathBuf::from(dir).join("dots-diff.sock");
    }
    let state = std::env::var_os("XDG_STATE_HOME").map_or_else(
        || PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/state"),
        PathBuf::from,
    );
    state.join("dots").join("diff.sock")
}

/// Accepts commands on `path` and passes each to `deliver`. A socket left by
/// a viewer that exited is replaced; a live one is not.
pub(crate) fn serve(path: &Path, deliver: impl Fn(Command) + Send + 'static) -> Result<()> {
    if UnixStream::connect(path).is_ok() {
        bail!("another dots-diff is listening on {}", path.display());
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    }
    let _ = std::fs::remove_file(path);
    let listener =
        UnixListener::bind(path).with_context(|| format!("listening on {}", path.display()))?;
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
            let mut line = String::new();
            if BufReader::new(&stream).read_line(&mut line).is_err() {
                continue;
            }
            let reply = match Command::parse(&line) {
                Ok(command) => {
                    deliver(command);
                    "ok\n".to_owned()
                }
                Err(e) => format!("error: {e}\n"),
            };
            let _ = stream.write_all(reply.as_bytes());
        }
    });
    Ok(())
}

/// Removes the socket when the viewer exits.
#[derive(Debug)]
pub(crate) struct Cleanup(pub PathBuf);

impl Drop for Cleanup {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// Sends one command to the running viewer.
pub(crate) fn send(command: &Command) -> Result<()> {
    let path = socket_path();
    let mut stream = UnixStream::connect(&path)
        .with_context(|| format!("no dots-diff is listening on {}", path.display()))?;
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    stream.write_all(format!("{}\n", command.line()).as_bytes())?;
    let mut reply = String::new();
    BufReader::new(&stream).read_line(&mut reply)?;
    match reply.trim() {
        "ok" => Ok(()),
        other => bail!("{other}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_commands() {
        assert_eq!(Command::parse("refresh\n").expect("ok"), Command::Refresh);
        assert_eq!(
            Command::parse("file src/a b.rs").expect("ok"),
            Command::File("src/a b.rs".into())
        );
        assert_eq!(
            Command::parse("worktree /x").expect("ok"),
            Command::Worktree("/x".into())
        );
        assert!(Command::parse("file").is_err());
        assert!(Command::parse("quit").is_err());
    }
}
