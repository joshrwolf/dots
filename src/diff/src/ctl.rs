//! Steering running viewers: each listens on its own Unix socket in a
//! per-user directory and answers one JSON line per connection with
//! another. `status` reports the Rex pane a viewer runs in and the
//! worktree it shows, so a command reaches the right one of several.

use std::fmt::Write as _;
use std::io::{BufRead as _, BufReader, Write as _};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Context as _, Result, bail};
use serde::{Deserialize, Serialize};

/// A command for a running viewer, as `dots-diff ctl` takes it and its
/// socket receives it.
#[derive(Debug, Clone, PartialEq, Eq, clap::Subcommand, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Ctl {
    /// Show the worktree containing PATH
    Worktree { path: PathBuf },
    /// Scroll to a file; an absolute path in another worktree switches to it
    File { path: String },
    /// Reload now
    Refresh,
    /// Show the user a note: on lines of a file, a whole file, or the review
    Note(Note),
    /// Send reviews to the Pi session in this Rex pane
    Bind { block: String },
    /// Save the viewer's screen as a PNG picture
    Shot { path: PathBuf },
}

/// A note for the user, or the notes to clear.
#[derive(Debug, Clone, PartialEq, Eq, clap::Args, Serialize, Deserialize)]
pub struct Note {
    /// The file: absolute, or relative to the current directory or the
    /// worktree
    #[arg(long, value_name = "PATH")]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    /// First line, 1-based; without it the note is about the whole file
    #[arg(long, requires = "file")]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
    /// Last line of a range
    #[arg(long, requires = "line")]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end: Option<u32>,
    /// The side the line numbers count
    #[arg(long, value_enum, default_value_t = NoteSide::New)]
    #[serde(default)]
    pub side: NoteSide,
    /// Clear notes instead: those of --file, or all
    #[arg(long, conflicts_with_all = ["line", "text"])]
    #[serde(default)]
    pub clear: bool,
    /// What the note says
    #[arg(required_unless_present = "clear")]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

/// The side of a diff a note's line numbers count.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, clap::ValueEnum, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum NoteSide {
    #[default]
    New,
    Old,
}

/// A line a viewer's socket reads: `status`, which the socket answers
/// itself, or a command for the viewer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum Request {
    /// Report the viewer's pane and worktree.
    Status,
    Command(Ctl),
}

/// The line a viewer's socket answers with.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum Reply {
    Ok,
    Status(Status),
    Error(String),
}

/// What a viewer reports about itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Status {
    /// The Rex pane it runs in, from `REX_BLOCK`.
    pub(crate) block: Option<String>,
    pub(crate) worktree: PathBuf,
}

/// One JSON line.
fn line(value: &impl Serialize) -> String {
    let mut out = serde_json::to_string(value).expect("socket messages serialize");
    out.push('\n');
    out
}

/// Where viewers listen: the runtime directory, or the user's state
/// directory.
fn socket_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("XDG_RUNTIME_DIR") {
        return PathBuf::from(dir).join("dots-diff");
    }
    crate::state_home().join("dots").join("diff")
}

/// This viewer's socket.
pub(crate) fn socket_path() -> PathBuf {
    socket_dir().join(format!("{}.sock", std::process::id()))
}

/// Accepts commands on `path`, answers `status` from `status`, and passes
/// every other command to `deliver`.
pub(crate) fn serve(
    path: &Path,
    status: Arc<Mutex<Status>>,
    deliver: impl Fn(Ctl) + Send + 'static,
) -> Result<()> {
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
            let reply = match serde_json::from_str(&line) {
                Ok(Request::Status) => status.lock().map_or_else(
                    |_| Reply::Error("status unavailable".to_owned()),
                    |s| Reply::Status(s.clone()),
                ),
                Ok(Request::Command(command)) => {
                    deliver(command);
                    Reply::Ok
                }
                Err(e) => Reply::Error(format!("reading {:?}: {e}", line.trim_end())),
            };
            let _ = stream.write_all(self::line(&reply).as_bytes());
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

fn request(socket: &Path, request: &Request) -> Result<Reply> {
    let mut stream = UnixStream::connect(socket)
        .with_context(|| format!("no dots-diff is listening on {}", socket.display()))?;
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    stream.write_all(line(request).as_bytes())?;
    let mut reply = String::new();
    BufReader::new(&stream).read_line(&mut reply)?;
    serde_json::from_str(&reply).with_context(|| {
        format!(
            "reading the reply {:?} from {}",
            reply.trim_end(),
            socket.display()
        )
    })
}

/// A running viewer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Viewer {
    pub(crate) socket: PathBuf,
    pub(crate) status: Status,
}

/// Every viewer that answers. A socket nobody listens on was left by a
/// viewer that died and is removed.
pub(crate) fn viewers() -> Vec<Viewer> {
    let Ok(entries) = std::fs::read_dir(socket_dir()) else {
        return Vec::new();
    };
    let mut found = Vec::new();
    for entry in entries.flatten() {
        let socket = entry.path();
        if socket.extension().is_none_or(|e| e != "sock") {
            continue;
        }
        match request(&socket, &Request::Status) {
            Ok(Reply::Status(status)) => found.push(Viewer { socket, status }),
            Err(e)
                if e.downcast_ref::<std::io::Error>()
                    .or_else(|| e.root_cause().downcast_ref())
                    .is_some_and(|e| e.kind() == std::io::ErrorKind::ConnectionRefused) =>
            {
                let _ = std::fs::remove_file(&socket);
            }
            Ok(_) | Err(_) => {}
        }
    }
    found.sort_by(|a, b| a.socket.cmp(&b.socket));
    found
}

/// The viewer a command goes to: the one in Rex pane `block` when given;
/// otherwise the only viewer of the repository holding `path`, or the only
/// viewer at all.
pub(crate) fn pick<'a>(
    viewers: &'a [Viewer],
    block: Option<&str>,
    path: Option<&Path>,
    same_repo: impl Fn(&Path, &Path) -> bool,
) -> Result<&'a Viewer> {
    let listing = || {
        viewers.iter().fold(String::new(), |mut out, v| {
            let _ = write!(
                out,
                "\n  pane {} showing {}",
                v.status.block.as_deref().unwrap_or("-"),
                v.status.worktree.display()
            );
            out
        })
    };
    if let Some(block) = block {
        return viewers
            .iter()
            .find(|v| v.status.block.as_deref() == Some(block))
            .with_context(|| format!("no dots-diff runs in pane {block}; running:{}", listing()));
    }
    if let Some(path) = path {
        let matching: Vec<&Viewer> = viewers
            .iter()
            .filter(|v| same_repo(&v.status.worktree, path))
            .collect();
        match matching.as_slice() {
            [one] => return Ok(one),
            [] => {}
            _ => bail!(
                "several dots-diff viewers show this repository; pass --block:{}",
                listing()
            ),
        }
    }
    match viewers {
        [one] => Ok(one),
        [] => bail!("no dots-diff is running"),
        _ => bail!("several dots-diff viewers run; pass --block:{}", listing()),
    }
}

/// Sends one command to `viewer`.
pub(crate) fn send(viewer: &Viewer, command: Ctl) -> Result<()> {
    match request(&viewer.socket, &Request::Command(command))? {
        Reply::Ok => Ok(()),
        Reply::Error(e) => bail!("{e}"),
        Reply::Status(_) => bail!("the viewer answered a command with its status"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_and_replies_travel_as_json_lines() {
        let note = Ctl::Note(Note {
            file: Some("src/a b.rs".into()),
            line: Some(3),
            end: None,
            side: NoteSide::Old,
            clear: false,
            text: Some("Look here\nand \"here\"".into()),
        });
        for request in [
            Request::Status,
            Request::Command(Ctl::Refresh),
            Request::Command(Ctl::File {
                path: "src/a b.rs".into(),
            }),
            Request::Command(note),
        ] {
            let sent = line(&request);
            assert_eq!(sent.matches('\n').count(), 1, "{sent}");
            assert_eq!(
                serde_json::from_str::<Request>(&sent).expect("read"),
                request
            );
        }
        assert_eq!(line(&Request::Status), "\"status\"\n");
        for reply in [
            Reply::Ok,
            Reply::Error("nope".into()),
            Reply::Status(Status {
                block: None,
                worktree: "/r/a b".into(),
            }),
        ] {
            assert_eq!(
                serde_json::from_str::<Reply>(&line(&reply)).expect("read"),
                reply
            );
        }
        assert!(serde_json::from_str::<Request>("\"quit\"").is_err());
    }

    #[test]
    fn picks_by_pane_then_repository_then_the_only_viewer() {
        let viewer = |block: &str, worktree: &str| Viewer {
            socket: format!("/s/{block}.sock").into(),
            status: Status {
                block: Some(block.to_owned()),
                worktree: worktree.into(),
            },
        };
        // Worktrees of one repository share their first path component.
        let same_repo = |a: &Path, b: &Path| a.components().nth(1) == b.components().nth(1);
        let all = [viewer("a", "/one/main"), viewer("b", "/two/main")];
        let pick_block = |block, path: Option<&str>| {
            pick(&all, block, path.map(Path::new), same_repo).map(|v| v.socket.clone())
        };
        assert_eq!(
            pick_block(Some("b"), Some("/one/x")).expect("pane"),
            PathBuf::from("/s/b.sock")
        );
        assert!(pick_block(Some("c"), None).is_err());
        assert_eq!(
            pick_block(None, Some("/two/feature")).expect("repository"),
            PathBuf::from("/s/b.sock")
        );
        assert!(pick_block(None, Some("/three/x")).is_err());
        assert!(pick_block(None, None).is_err());
        let one = [viewer("a", "/one/main")];
        assert!(pick(&one, None, Some(Path::new("/three/x")), same_repo).is_ok());
        let twins = [viewer("a", "/one/main"), viewer("b", "/one/feature")];
        assert!(pick(&twins, None, Some(Path::new("/one/x")), same_repo).is_err());
        assert!(pick(&[], None, None, same_repo).is_err());
    }
}
