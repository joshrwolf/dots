//! The contract between the Rex configuration's Lua view builders and the
//! picker: every view they build from the hand-authored snapshots must parse
//! and prepare. Runs the real Lua in Rex's interpreter; tests/isolate.lua makes
//! any Rex API call fail, so a running server is never touched.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use dots_pick::Palette;
use dots_pick::spec::View;

type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;

/// An address no server listens on, so the script still runs when no Rex server
/// is up. A running server is reached regardless; isolation is in the Lua.
const NO_SERVER: &str = "unix:///nonexistent/dots-pick-contract.sock";

/// The `rex` CLI: `REX` if set, else the Mac app's current server copy.
fn rex() -> Result<PathBuf> {
    if let Some(path) = std::env::var_os("REX") {
        return Ok(PathBuf::from(path));
    }
    let home = std::env::var_os("HOME").ok_or("HOME is not set")?;
    let path = Path::new(&home).join(
        "Library/Application Support/com.superlogical.rex/RexServer/Versions/Current/Rex Server.app/Contents/MacOS/rex",
    );
    if path.exists() {
        Ok(path)
    } else {
        Err("the contract test needs the rex CLI; set REX to its path".into())
    }
}

fn views() -> Result<serde_json::Map<String, serde_json::Value>> {
    let script =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../home/rex/.config/rex/tests/contract.lua");
    let mut command = Command::new(rex()?);
    command
        .args(["--autostart=false", "do"])
        .arg(script)
        .arg("--json")
        .env("REX_SERVER", NO_SERVER);
    let output = dots_process::output(command, Duration::from_secs(20), 16 * 1024 * 1024)?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("the Lua view builders failed: {}", stderr.trim()).into());
    }
    match serde_json::from_slice(&output.stdout)? {
        serde_json::Value::Object(views) => Ok(views),
        other => Err(format!("expected views by name, got {other}").into()),
    }
}

/// An error with its causes, so a failure names the field that was wrong.
fn chain(name: &str, error: &dyn std::error::Error) -> String {
    let mut message = format!("{name}: {error}");
    let mut cause = error.source();
    while let Some(inner) = cause {
        let _ = write!(message, ": {inner}");
        cause = inner.source();
    }
    message
}

#[test]
fn every_configured_view_is_a_valid_picker() -> Result {
    let views = views()?;
    for name in ["go/busy", "go/empty", "attention/busy", "attention/empty"] {
        if !views.contains_key(name) {
            return Err(format!("the builders produced no {name} view").into());
        }
    }
    for (name, view) in views {
        let view = View::parse(&view.to_string()).map_err(|error| chain(&name, &error))?;
        // Every view's rows follow Rex or a watch command.
        if view.watch.is_none() {
            return Err(format!("{name} is not live").into());
        }
        view.prepare(Palette::default())
            .map_err(|error| chain(&name, &error))?
            .picker
            .build()
            .map_err(|error| chain(&name, &error))?;
    }
    Ok(())
}
