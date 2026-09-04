use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{Result, bail};
use herdr_find::{Sources, groups, open};
use herdrkit::picker::Picker;
use herdrkit::runtime::{self, FailureSurface};
use herdrkit::{Invocation, InvocationKind};

fn main() -> ExitCode {
    runtime::finish("find", FailureSurface::Popup, run())
}

fn run() -> Result<()> {
    let invocation = Invocation::load()?;
    if !matches!(
        invocation.kind(),
        InvocationKind::PaneEntrypoint { id } if id == "picker"
    ) {
        bail!(
            "find must be launched by the `picker` pane entrypoint, got {:?}",
            invocation.kind()
        );
    }
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default();

    let destinations = Sources::collect(invocation.client())?.destinations(&home);
    if destinations.is_empty() {
        bail!("nothing to jump to");
    }

    let chosen = Picker::new(destinations)
        .groups(groups())
        .prompt("\u{26a1}  ")
        .match_paths()
        .run()?;

    match chosen {
        Some(destination) => open(invocation.client(), &destination),
        None => Ok(()),
    }
}
