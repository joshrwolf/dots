//! Handing a file to the nvim already running in a tab.
//!
//! nvim needs no configuration for this: every instance already listens on a
//! socket under `$TMPDIR/nvim.$USER`, named after its process id.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context as _, Result, bail};
use herdrkit::api::Direction;
use herdrkit::{Client, PaneId, TabId};

use herdrkit::apps::is_nvim;

use crate::Target;

/// Opens `target` in whichever pane of `tab` is running nvim, and moves focus
/// there from `from` when they are different panes.
pub fn open(
    client: &Client,
    tab: &TabId,
    from: Option<&PaneId>,
    target: &Target,
) -> Result<PaneId> {
    let panes = client.pane_list().context("listing panes")?;
    let candidates: Vec<&PaneId> = panes
        .iter()
        .filter(|pane| pane.tab_id == *tab)
        .map(|pane| &pane.pane_id)
        .collect();
    if candidates.is_empty() {
        bail!("tab {tab} has no panes");
    }

    let Some((pane, socket)) = candidates
        .iter()
        .find_map(|pane| socket_for_pane(client, pane).map(|socket| (*pane, socket)))
    else {
        // The failure that most looks like this one is a socket error while
        // asking what runs in each pane, and `socket_for_pane` has already
        // reported any of those on stderr, so the two are told apart in the
        // plugin log.
        // A sandboxed nvim cannot create its socket at all, and then it is
        // running but unreachable, so the message names both possibilities.
        bail!("no nvim in this tab is listening on a socket");
    };

    send(&socket, target)?;
    // Focus is best effort: the file is open either way, and failing here
    // would report an open that succeeded as a failure.
    if let Some(from) = from.filter(|from| *from != pane) {
        focus(client, from, pane);
    }
    Ok(pane.clone())
}

/// herdr reports a pane's foreground process, which is nvim's TUI, while the
/// socket is named after the `--embed` server nvim spawns as its child. So the
/// mapping runs pane -> TUI pid -> child pid -> socket file. Asking each nvim
/// who it belongs to instead would mean starting an nvim per candidate socket.
fn socket_for_pane(client: &Client, pane: &PaneId) -> Option<PathBuf> {
    let info = match client.pane_process_info(pane) {
        Ok(info) => info,
        Err(error) => {
            eprintln!("nvim: could not read what is running in {pane}: {error:#}");
            return None;
        }
    };
    let tui = info.find(is_nvim)?.pid;

    let root = run_root();
    // The TUI pid is tried first because nvim served the socket from that
    // process before it split the server out.
    std::iter::once(tui)
        .chain(children_of(tui))
        .find_map(|pid| socket_named(&root, pid))
}

fn run_root() -> PathBuf {
    let tmp = std::env::var_os("TMPDIR").map_or_else(|| PathBuf::from("/tmp"), PathBuf::from);
    let user = std::env::var("USER").unwrap_or_else(|_| "nobody".to_owned());
    tmp.join(format!("nvim.{user}"))
}

fn children_of(pid: u32) -> Vec<u32> {
    let Ok(output) = Command::new("pgrep")
        .arg("-P")
        .arg(pid.to_string())
        .output()
    else {
        return Vec::new();
    };
    String::from_utf8_lossy(&output.stdout)
        .split_whitespace()
        .filter_map(|field| field.parse().ok())
        .collect()
}

/// nvim puts its socket in a randomly named directory under the run root,
/// so the leaf name is known but its parent is not and has to be scanned.
fn socket_named(root: &Path, pid: u32) -> Option<PathBuf> {
    let name = format!("nvim.{pid}.0");
    let entries = std::fs::read_dir(root).ok()?;
    entries
        .flatten()
        .map(|entry| entry.path().join(&name))
        .find(|path| is_socket(path))
}

fn is_socket(path: &Path) -> bool {
    use std::os::unix::fs::FileTypeExt as _;
    std::fs::metadata(path).is_ok_and(|meta| meta.file_type().is_socket())
}

fn send(socket: &Path, target: &Target) -> Result<()> {
    let nvim = std::env::var_os("HERDR_NVIM_BIN").unwrap_or_else(|| "nvim".into());
    send_with(&nvim, socket, target)
}

fn send_with(nvim: &OsStr, socket: &Path, target: &Target) -> Result<()> {
    // The path goes over as an argv element, so nothing about it needs
    // escaping. stdin must not be a terminal: given a tty, nvim ignores the
    // --remote arguments and starts a UI instead, which in a popup returns a
    // screenful of escape sequences and leaves the file unopened.
    let opened = Command::new(nvim)
        .arg("--server")
        .arg(socket)
        .arg("--remote-silent")
        .arg(&target.path)
        .stdin(Stdio::null())
        .status()
        .with_context(|| format!("running {}", Path::new(nvim).display()))?;
    if !opened.success() {
        bail!("nvim refused to open {}", target.path.display());
    }

    // --remote takes no +cmd, so the cursor is a second call. A positioning
    // failure is reported as a partial success: claiming `path:line` opened
    // would be false, while claiming the file did not open would also be
    // false.
    if target.line > 1 || target.column > 1 {
        let positioned = Command::new(nvim)
            .arg("--server")
            .arg(socket)
            .arg("--remote-expr")
            .arg(format!("cursor({}, {})", target.line, target.column))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .output()
            .with_context(|| {
                format!(
                    "opened {} but could not run nvim to position the cursor at {}:{}",
                    target.path.display(),
                    target.line,
                    target.column
                )
            })?;
        if !positioned.status.success() {
            let stderr = String::from_utf8_lossy(&positioned.stderr);
            bail!(
                "opened {} but nvim could not position the cursor at {}:{} ({}): {}",
                target.path.display(),
                target.line,
                target.column,
                positioned.status,
                stderr.trim()
            );
        }
    }
    Ok(())
}

/// Right and left before down and up: a file picker is nearly always opened
/// beside its editor rather than above it, so this finds the pane in one call.
fn focus(client: &Client, from: &PaneId, to: &PaneId) {
    for direction in [
        Direction::Right,
        Direction::Left,
        Direction::Down,
        Direction::Up,
    ] {
        let neighbor = match client.pane_neighbor(from, direction) {
            Ok(neighbor) => neighbor,
            Err(error) => {
                eprintln!("nvim: could not look {direction:?} from {from}: {error:#}");
                continue;
            }
        };
        if neighbor.as_ref() == Some(to) {
            if let Err(error) = client.pane_focus_direction(from, direction) {
                eprintln!("nvim: opened in {to} but could not focus it: {error:#}");
            }
            return;
        }
    }
    eprintln!("nvim: opened in {to}, which is not adjacent to {from}, so focus stayed");
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs::PermissionsExt as _;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    static NEXT_TEMP: AtomicUsize = AtomicUsize::new(0);

    fn command_that_rejects_cursor() -> PathBuf {
        let sequence = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "herdr-nvim-remote-test-{}-{sequence}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).unwrap();
        let script = dir.join("nvim");
        fs::write(
            &script,
            "#!/bin/sh\ncase \"$*\" in\n  *--remote-expr*) echo cursor-rejected >&2; exit 9;;\n  *) exit 0;;\nesac\n",
        )
        .unwrap();
        let mut permissions = fs::metadata(&script).unwrap().permissions();
        permissions.set_mode(0o700);
        fs::set_permissions(&script, permissions).unwrap();
        script
    }

    #[test]
    fn cursor_failure_is_reported_as_a_partial_open() {
        let nvim = command_that_rejects_cursor();
        let target = Target {
            path: PathBuf::from("/tmp/example.rs"),
            line: 42,
            column: 7,
        };

        let error = send_with(nvim.as_os_str(), Path::new("/tmp/nvim.sock"), &target).unwrap_err();
        let message = format!("{error:#}");
        assert!(message.contains("opened /tmp/example.rs"), "{message}");
        assert!(message.contains("42:7"), "{message}");
        assert!(message.contains("cursor-rejected"), "{message}");
        if let Some(dir) = nvim.parent() {
            let _ = fs::remove_dir_all(dir);
        }
    }

    #[test]
    fn no_cursor_call_is_needed_for_the_top_of_a_file() {
        let target = Target::at(PathBuf::from("/tmp/example.rs"));
        assert!(send_with(OsStr::new("/usr/bin/true"), Path::new("ignored"), &target).is_ok());
    }
}
