use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context as _, Result, bail};
use herdr_nvim::{Listings, Target, groups, open};
use herdrkit::picker::Picker;
use herdrkit::runtime::{self, FailureSurface};
use herdrkit::{Invocation, InvocationKind};

fn main() -> ExitCode {
    let surface = if matches!(
        InvocationKind::from_env(),
        Ok(InvocationKind::PaneEntrypoint { ref id }) if id == "picker"
    ) {
        FailureSurface::Popup
    } else {
        FailureSurface::Log
    };
    runtime::finish("nvim", surface, run())
}

fn run() -> Result<()> {
    let invocation = Invocation::load()?;
    match invocation.kind() {
        InvocationKind::PaneEntrypoint { id } if id == "picker" => pick(&invocation),
        InvocationKind::Action { id } if id == "open" => open_target(&invocation),
        other => bail!(
            "nvim must be launched by the `picker` pane entrypoint or `open` action, got {other:?}"
        ),
    }
}

fn pick(invocation: &Invocation) -> Result<()> {
    let root = invocation.require_target_dir()?;

    let files = Listings::collect(root)?.index(root);
    if files.is_empty() {
        bail!("no files under {}", root.display());
    }

    let chosen = Picker::new(files)
        .groups(groups())
        .prompt("\u{f0f6}  ")
        .match_paths()
        .run()?;

    let Some(file) = chosen else {
        return Ok(());
    };
    hand_over(invocation, &Target::at(file.path))
}

/// The click arrives in `HERDR_PLUGIN_CLICKED_URL`.
fn open_target(invocation: &Invocation) -> Result<()> {
    let raw = invocation
        .clicked_url()
        .map(str::to_owned)
        .context("no path given")?;

    // A click happened in a pane, so that pane's directory is what a relative
    // path is relative to — unlike the picker, where the repo root is.
    let base = invocation
        .context()
        .focused_pane_cwd()
        .map(Path::to_path_buf)
        .or_else(|| invocation.target_dir().map(Path::to_path_buf));
    let home = std::env::var_os("HOME").map(PathBuf::from);

    let target = Target::parse(&raw, base.as_deref(), home.as_deref())?;
    hand_over(invocation, &target)
}

fn hand_over(invocation: &Invocation, target: &Target) -> Result<()> {
    let pane = open(
        invocation.client(),
        invocation.require_tab_id()?,
        invocation.pane_id(),
        target,
    )?;
    println!("{}:{} in {pane}", target.path.display(), target.line);
    Ok(())
}
