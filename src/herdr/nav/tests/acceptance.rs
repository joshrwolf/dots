//! Drives the `herdr` on `PATH` (stock 0.9.1+) in an isolated temporary session.
mod support;

use std::fs::{self, File};
use std::io::{Read as _, Write as _};
use std::os::fd::AsRawFd as _;
use std::os::unix::process::CommandExt as _;
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use anyhow::{Context as _, Result, ensure};
use herdrkit::{PaneId, TerminalId, WorkspaceId};
use nix::fcntl::{FcntlArg, OFlag, fcntl};
use nix::pty::{Winsize, openpty};
use serde::Deserialize;
use serde_json::json;
use support::{Process, Stock, eventually};

#[derive(Deserialize)]
struct Created {
    root_pane: Destination,
}
#[derive(Deserialize)]
#[expect(
    clippy::struct_field_names,
    reason = "subset of Herdr's wire identity fields"
)]
struct Destination {
    pane_id: PaneId,
    terminal_id: TerminalId,
    workspace_id: WorkspaceId,
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "integration assertions include expected/actual values; Result propagates setup failures"
)]
fn stock_server_jumplist() -> Result<()> {
    let stock = Stock::start()?;
    let destinations = ["A", "B", "C", "D"].map(|label| {
        let result = stock.rpc(
            "workspace.create",
            &json!({
            "cwd":stock.root, "label":label, "focus":true}),
            "workspace_created",
        )?;
        Ok::<_, anyhow::Error>(serde_json::from_value::<Created>(result)?.root_pane)
    });
    let [a, b, c, d] = destinations;
    let (a, b, c, d) = (a?, b?, c?, d?);
    stock.client.pane_focus(&a.pane_id)?;
    stock.link()?;
    stock.wait_current(&a.terminal_id)?;
    stock.visit(&b.pane_id, &b.terminal_id)?;
    stock.visit(&c.pane_id, &c.terminal_id)?;
    assert_eq!(
        stock.history()?.entries,
        [&a, &b, &c].map(|p| p.terminal_id.clone())
    );
    stock.action("back")?;
    assert_eq!(stock.focus()?, b.pane_id);
    stock.action("forward")?;
    assert_eq!(stock.focus()?, c.pane_id);
    stock.action("back")?;
    stock.visit(&d.pane_id, &d.terminal_id)?;
    assert_eq!(
        stock.history()?.entries,
        [&a, &c, &b, &d].map(|p| p.terminal_id.clone())
    );

    let actions = [stock.action_command(), stock.action_command()];
    let threads = actions.map(|mut command| {
        command.env("HERDR_PLUGIN_ACTION_ID", "back");
        thread::spawn(|| support::run(command))
    });
    for thread in threads {
        thread.join().unwrap()?;
    }
    assert_eq!(stock.focus()?, c.pane_id);
    let history = stock.history()?;
    for _ in 0..3 {
        let mut command = stock.action_command();
        command.env("HERDR_PLUGIN_EVENT", "pane.focused").env(
            "HERDR_PLUGIN_EVENT_JSON",
            json!({
                "event":"pane_focused","data":{"pane_id":d.pane_id}})
            .to_string(),
        );
        support::run(command)?;
    }
    assert_eq!(stock.history()?, history);

    stock.rpc(
        "pane.move",
        &json!({"pane_id":b.pane_id,"focus":false,
        "destination":{"type":"new_tab","workspace_id":a.workspace_id}}),
        "pane_move",
    )?;
    let moved = stock
        .client
        .navigation_snapshot()?
        .panes
        .into_iter()
        .find(|pane| pane.terminal_id == b.terminal_id)
        .context("moved pane missing")?;
    assert_ne!(moved.pane_id, b.pane_id);
    stock.action("forward")?;
    assert_eq!(stock.focus()?, moved.pane_id);
    stock.rpc("pane.close", &json!({"pane_id":d.pane_id}), "ok")?;
    eventually(Duration::from_secs(8), || {
        Ok((!stock.history()?.entries.contains(&d.terminal_id)).then_some(()))
    })?;
    stock.action("forward")?;
    assert_eq!(stock.focus()?, moved.pane_id);
    surfaces(&stock)?;
    let saved = stock.history()?;
    stock.restart_worker()?;
    assert_eq!(stock.history()?, saved);
    workspace_toggle(&stock)?;
    native_keys(&stock)?;
    Ok(())
}

fn workspace_toggle(stock: &Stock) -> Result<()> {
    let create = |label| -> Result<Destination> {
        let result = stock.rpc(
            "workspace.create",
            &json!({"cwd":stock.root,"label":label,"focus":true}),
            "workspace_created",
        )?;
        let pane = serde_json::from_value::<Created>(result)?.root_pane;
        stock.wait_current(&pane.terminal_id)?;
        Ok(pane)
    };
    let a = create("Toggle A")?;
    let result = stock.rpc(
        "tab.create",
        &json!({"workspace_id":a.workspace_id,"cwd":stock.root,"focus":true}),
        "tab_created",
    )?;
    let tab_a = serde_json::from_value::<Created>(result)?.root_pane;
    stock.wait_current(&tab_a.terminal_id)?;
    let b = create("Toggle B")?;
    let result = stock.rpc(
        "tab.create",
        &json!({"workspace_id":b.workspace_id,"cwd":stock.root,"focus":true}),
        "tab_created",
    )?;
    let tab_b = serde_json::from_value::<Created>(result)?.root_pane;
    stock.wait_current(&tab_b.terminal_id)?;
    for expected in [&tab_a, &tab_b, &tab_a, &tab_b] {
        stock.action("previous-workspace")?;
        ensure!(
            stock.focus()? == expected.pane_id,
            "workspace toggle did not preserve selected tab"
        );
    }
    // Pane traversal also changes the workspace pair, but same-workspace visits do not.
    stock.visit(&b.pane_id, &b.terminal_id)?;
    stock.action("previous-workspace")?;
    ensure!(
        stock.focus()? == tab_a.pane_id,
        "same-workspace tab visit displaced previous workspace"
    );
    stock.action("previous-workspace")?;
    ensure!(
        stock.focus()? == b.pane_id,
        "workspace did not retain newly selected tab"
    );
    stock.client.workspace_close(&a.workspace_id)?;
    stock.action("previous-workspace")?;
    ensure!(
        stock.focus()? == b.pane_id,
        "closed previous workspace should be a no-op"
    );
    Ok(())
}

fn surfaces(stock: &Stock) -> Result<()> {
    let root = stock.root.join("surfaces");
    fs::create_dir(&root)?;
    let mut manifest = String::from(
        "id = \"nav-surfaces\"\nname = \"Surfaces\"\nversion = \"0.1.0\"\nmin_herdr_version = \"0.9.1\"\n",
    );
    for placement in ["popup", "overlay", "split"] {
        use std::fmt::Write as _;
        writeln!(
            manifest,
            "\n[[panes]]\nid = \"{placement}\"\ntitle = \"Fixture\"\nplacement = \"{placement}\"\ncommand = [\"/bin/sleep\", \"90\"]"
        )?;
    }
    fs::write(root.join("herdr-plugin.toml"), manifest)?;
    stock.rpc("plugin.link", &json!({"path":root}), "plugin_linked")?;
    for placement in ["popup", "overlay", "split"] {
        let before = stock.history()?;
        let panes = stock.client.navigation_snapshot()?.panes;
        stock.rpc(
            "plugin.pane.open",
            &json!({"plugin_id":"nav-surfaces","entrypoint":placement,"focus":true}),
            if placement == "popup" {
                "ok"
            } else {
                "plugin_pane_opened"
            },
        )?;
        if placement == "popup" {
            thread::sleep(Duration::from_millis(1200));
            ensure!(stock.history()? == before, "popup changed history");
            stock.rpc("popup.close", &json!({}), "ok")?;
        } else {
            let added = stock
                .client
                .navigation_snapshot()?
                .panes
                .into_iter()
                .find(|pane| !panes.iter().any(|old| old.terminal_id == pane.terminal_id))
                .context("plugin surface missing")?;
            stock.wait_current(&added.terminal_id)?;
            stock.rpc("pane.close", &json!({"pane_id":added.pane_id}), "ok")?;
            eventually(Duration::from_secs(8), || {
                Ok((!stock.history()?.entries.contains(&added.terminal_id)).then_some(()))
            })?;
        }
    }
    Ok(())
}

// A separate test process can acquire a controlling PTY before exec without
// unsafe pre_exec code in a multithreaded parent.
#[test]
fn native_client_child() -> Result<()> {
    let Some(binary) = std::env::var_os("NAV_TEST_PTY_CHILD") else {
        return Ok(());
    };
    nix::unistd::setsid()?;
    rustix::process::ioctl_tiocsctty(std::io::stdin())?;
    Err(Command::new(binary)
        .env_remove("NAV_TEST_PTY_CHILD")
        .exec()
        .into())
}

fn native_keys(stock: &Stock) -> Result<()> {
    // Start with exactly one tab so previous_tab has an unambiguous target.
    let created: Created = serde_json::from_value(stock.rpc(
        "workspace.create",
        &json!({"cwd":stock.root,"label":"Native keys","focus":true}),
        "workspace_created",
    )?)?;
    stock.wait_current(&created.root_pane.terminal_id)?;
    let pair = openpty(
        Some(&Winsize {
            ws_row: 30,
            ws_col: 100,
            ws_xpixel: 0,
            ws_ypixel: 0,
        }),
        None,
    )?;
    let mut master = File::from(pair.master);
    let slave = File::from(pair.slave);
    fcntl(master.as_raw_fd(), FcntlArg::F_SETFL(OFlag::O_NONBLOCK))?;
    let mut command = support::command(&stock.root, &std::env::current_exe()?);
    command
        .args(["--exact", "native_client_child", "--nocapture"])
        .env("NAV_TEST_PTY_CHILD", &stock.herdr)
        .stdin(slave.try_clone()?)
        .stdout(slave.try_clone()?)
        .stderr(slave);
    let mut child = Process(command.spawn()?);
    let reader = master.try_clone()?;
    let stop = Arc::new(AtomicBool::new(false));
    let finished = Arc::clone(&stop);
    let log = File::create(stock.root.join("native-client.log"))?;
    let drain = thread::spawn(move || drain_pty(reader, log, &finished));
    let result = (|| {
        thread::sleep(Duration::from_secs(2));
        ensure!(
            child.0.try_wait()?.is_none(),
            "native client exited before input"
        );
        let (original, original_terminal) = native_pane_keys(stock, &mut master)?;
        native_workspace_toggle(stock, &mut master, &original, &original_terminal)?;
        Ok(())
    })();
    drop(child);
    stop.store(true, Ordering::Relaxed);
    drain
        .join()
        .map_err(|_| anyhow::anyhow!("PTY reader panicked"))?;
    if result.is_err() {
        eprintln!(
            "native client output: {}",
            fs::read_to_string(stock.root.join("native-client.log"))?
        );
    }
    result
}

fn native_pane_keys(stock: &Stock, master: &mut File) -> Result<(PaneId, TerminalId)> {
    let original = stock.focus()?;
    master.write_all(b"\x13t\r")?;
    let new = eventually(Duration::from_secs(8), || {
        let pane = stock.focus()?;
        Ok((pane != original).then_some(pane))
    })
    .context("native new-tab key")?;
    let terminal = stock
        .client
        .navigation_snapshot()?
        .panes
        .into_iter()
        .find(|pane| pane.pane_id == new)
        .context("native new tab missing")?
        .terminal_id;
    stock.wait_current(&terminal)?;
    let hooks_before = stock.focus_hooks()?;
    master.write_all(b"\x13h")?;
    eventually(Duration::from_secs(8), || {
        Ok((stock.focus()? == original).then_some(()))
    })
    .context("native previous-tab key")?;
    eventually(Duration::from_secs(8), || {
        Ok(stock
            .focus_hooks()?
            .iter()
            .any(|id| !hooks_before.contains(id))
            .then_some(()))
    })
    .context("native tab navigation must emit a pane.focused hook")?;
    let original_terminal = stock
        .client
        .navigation_snapshot()?
        .panes
        .into_iter()
        .find(|pane| pane.pane_id == original)
        .context("original pane missing")?
        .terminal_id;
    stock.wait_current(&original_terminal)?;
    master.write_all(b"\x13o")?;
    eventually(Duration::from_secs(8), || {
        Ok((stock.focus()? == new).then_some(()))
    })
    .context("native Back key")?;
    master.write_all(b"\x13I")?;
    eventually(Duration::from_secs(8), || {
        Ok((stock.focus()? == original).then_some(()))
    })
    .context("native Forward key")?;
    Ok((original, original_terminal))
}

fn native_workspace_toggle(
    stock: &Stock,
    master: &mut File,
    original: &PaneId,
    original_terminal: &TerminalId,
) -> Result<()> {
    // Visit another workspace, return, and switch tabs locally. The Cmd+O
    // chord must still alternate workspaces, preserving this tab selection.
    let current = stock
        .client
        .navigation_snapshot()?
        .focused_workspace_id
        .context("missing workspace")?;
    let other = stock
        .client
        .navigation_snapshot()?
        .workspaces
        .into_iter()
        .find(|workspace| workspace.workspace_id != current)
        .context("missing alternate workspace")?
        .workspace_id;
    stock.client.workspace_focus(&other)?;
    let other_pane = stock.focus()?;
    let terminal = stock
        .client
        .navigation_snapshot()?
        .panes
        .into_iter()
        .find(|pane| pane.pane_id == other_pane)
        .context("missing alternate pane")?
        .terminal_id;
    stock.wait_current(&terminal)?;
    stock.visit(original, original_terminal)?;
    for expected in [&other_pane, original, &other_pane, original] {
        master.write_all(b"\x13O")?;
        eventually(Duration::from_secs(8), || {
            Ok((stock.focus()? == *expected).then_some(()))
        })
        .context("native previous-workspace key")?;
    }
    Ok(())
}

fn drain_pty(mut reader: File, mut log: File, finished: &AtomicBool) {
    let mut buffer = [0; 4096];
    let mut retained: usize = 0;
    while !finished.load(Ordering::Relaxed) {
        match reader.read(&mut buffer) {
            Ok(0) => break,
            Ok(n) => {
                let keep = n.min(65536_usize.saturating_sub(retained));
                let _ = log.write_all(buffer.get(..keep).unwrap_or_default());
                retained += keep;
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(10));
            }
            Err(_) => break,
        }
    }
}
