//! Steering running viewers: each listens on its own Unix socket in a
//! per-user directory and answers one text line per connection with `ok`
//! or an error line. `status` reports the Rex pane a viewer runs in and the
//! worktree it shows, so a command reaches the right one of several.

use std::fmt::Write as _;
use std::io::{BufRead as _, BufReader, Write as _};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Context as _, Result, bail};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Command {
    /// Show the worktree containing a path.
    Worktree(PathBuf),
    /// Scroll to a file, by absolute path or path within the worktree. An
    /// absolute path in another worktree switches to it.
    File(String),
    Refresh,
    /// Show an agent's note to the user.
    Note(NoteSpec),
    /// Clear the notes of one file, or all.
    ClearNotes(Option<String>),
    /// Send reviews of the shown worktree to the Pi session in this Rex pane.
    Bind(String),
    /// Save the screen as a PNG picture.
    Shot(PathBuf),
}

/// Where an agent's note goes and what it says. Lines are 1-based; with
/// no file it is about the whole review, with no line the whole file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct NoteSpec {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) file: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) line: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) end: Option<u32>,
    #[serde(default)]
    pub(crate) old: bool,
    pub(crate) text: String,
}

impl Command {
    pub(crate) fn parse(line: &str) -> Result<Self> {
        let (verb, arg) = line.trim().split_once(' ').unwrap_or((line.trim(), ""));
        Ok(match (verb, arg.trim()) {
            ("worktree", path) if !path.is_empty() => Command::Worktree(PathBuf::from(path)),
            ("file", path) if !path.is_empty() => Command::File(path.to_owned()),
            ("refresh", "") => Command::Refresh,
            ("note", json) if !json.is_empty() => Command::Note(
                serde_json::from_str(json).with_context(|| format!("reading note {json:?}"))?,
            ),
            ("clear-notes", path) => {
                Command::ClearNotes((!path.is_empty()).then(|| path.to_owned()))
            }
            ("bind", block) if !block.is_empty() => Command::Bind(block.to_owned()),
            ("shot", path) if !path.is_empty() => Command::Shot(PathBuf::from(path)),
            _ => bail!(
                "unknown command {line:?}; expected `worktree PATH`, `file PATH`, `refresh`, `status`, `note JSON`, `clear-notes [PATH]`, `bind BLOCK` or `shot PATH`"
            ),
        })
    }

    fn line(&self) -> String {
        match self {
            Command::Worktree(p) => format!("worktree {}", p.display()),
            Command::File(p) => format!("file {p}"),
            Command::Refresh => "refresh".to_owned(),
            Command::Note(n) => format!(
                "note {}",
                serde_json::to_string(n).unwrap_or_else(|_| "{}".to_owned())
            ),
            Command::ClearNotes(None) => "clear-notes".to_owned(),
            Command::ClearNotes(Some(p)) => format!("clear-notes {p}"),
            Command::Bind(b) => format!("bind {b}"),
            Command::Shot(p) => format!("shot {}", p.display()),
        }
    }
}

/// A line a viewer's socket accepts: `status`, which the socket answers
/// itself, or a command for the viewer.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Request {
    /// Report the viewer's pane and worktree.
    Status,
    Command(Command),
}

impl Request {
    fn parse(line: &str) -> Result<Self> {
        if line.trim() == "status" {
            return Ok(Request::Status);
        }
        Command::parse(line).map(Request::Command)
    }

    fn line(&self) -> String {
        match self {
            Request::Status => "status".to_owned(),
            Request::Command(c) => c.line(),
        }
    }
}

/// What a viewer reports about itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Status {
    /// The Rex pane it runs in, from `REX_BLOCK`.
    pub(crate) block: Option<String>,
    pub(crate) worktree: PathBuf,
}

impl Status {
    fn line(&self) -> String {
        format!(
            "ok\t{}\t{}",
            self.block.as_deref().unwrap_or("-"),
            self.worktree.display()
        )
    }

    fn parse(line: &str) -> Option<Self> {
        let mut parts = line.trim_end_matches('\n').splitn(3, '\t');
        let (Some("ok"), Some(block), Some(worktree)) = (parts.next(), parts.next(), parts.next())
        else {
            return None;
        };
        Some(Self {
            block: (block != "-").then(|| block.to_owned()),
            worktree: PathBuf::from(worktree),
        })
    }
}

/// Where viewers listen: the runtime directory, or the user's state
/// directory.
fn socket_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("XDG_RUNTIME_DIR") {
        return PathBuf::from(dir).join("dots-diff");
    }
    let state = std::env::var_os("XDG_STATE_HOME").map_or_else(
        || PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/state"),
        PathBuf::from,
    );
    state.join("dots").join("diff")
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
    deliver: impl Fn(Command) + Send + 'static,
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
            let reply = match Request::parse(&line) {
                Ok(Request::Status) => status
                    .lock()
                    .map_or_else(|_| "error: status unavailable".to_owned(), |s| s.line()),
                Ok(Request::Command(command)) => {
                    deliver(command);
                    "ok".to_owned()
                }
                Err(e) => format!("error: {e}"),
            };
            let _ = stream.write_all(format!("{reply}\n").as_bytes());
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

fn request(socket: &Path, request: &Request) -> Result<String> {
    let mut stream = UnixStream::connect(socket)
        .with_context(|| format!("no dots-diff is listening on {}", socket.display()))?;
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    stream.write_all(format!("{}\n", request.line()).as_bytes())?;
    let mut reply = String::new();
    BufReader::new(&stream).read_line(&mut reply)?;
    Ok(reply)
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
            Ok(reply) => {
                if let Some(status) = Status::parse(&reply) {
                    found.push(Viewer { socket, status });
                }
            }
            Err(e)
                if e.downcast_ref::<std::io::Error>()
                    .or_else(|| e.root_cause().downcast_ref())
                    .is_some_and(|e| e.kind() == std::io::ErrorKind::ConnectionRefused) =>
            {
                let _ = std::fs::remove_file(&socket);
            }
            Err(_) => {}
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
pub(crate) fn send(viewer: &Viewer, command: &Command) -> Result<()> {
    let reply = request(&viewer.socket, &Request::Command(command.clone()))?;
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
        assert_eq!(Request::parse("status\n").expect("ok"), Request::Status);
        assert_eq!(
            Request::parse("refresh").expect("ok"),
            Request::Command(Command::Refresh)
        );
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
        let note = Command::Note(NoteSpec {
            file: Some("src/a.rs".into()),
            line: Some(3),
            end: None,
            old: false,
            text: "Look here\nand \"here\"".into(),
        });
        assert_eq!(Command::parse(&note.line()).expect("note"), note);
        for c in [
            Command::ClearNotes(None),
            Command::ClearNotes(Some("a b.rs".into())),
            Command::Bind("block:1".into()),
        ] {
            assert_eq!(Command::parse(&c.line()).expect("round trip"), c);
        }
        assert!(Command::parse("note {").is_err());
    }

    #[test]
    fn status_round_trips_with_and_without_a_pane() {
        for block in [Some("b-1".to_owned()), None] {
            let status = Status {
                block,
                worktree: "/r/a b".into(),
            };
            assert_eq!(Status::parse(&format!("{}\n", status.line())), Some(status));
        }
        assert_eq!(Status::parse("error: nope"), None);
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
