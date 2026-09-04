use std::process::ExitCode;

use anyhow::{Context as _, Result};
use herdr_nav::{NavigationDecision, chord, decide};
use herdrkit::api::Direction;
use herdrkit::runtime::{self, FailureSurface};
use herdrkit::{Invocation, InvocationKind};

fn main() -> ExitCode {
    runtime::finish("nav", FailureSurface::Log, run())
}

fn run() -> Result<()> {
    let invocation = Invocation::load()?;
    let InvocationKind::Action { id } = invocation.kind() else {
        anyhow::bail!(
            "nav must be launched by a navigation action, got {:?}",
            invocation.kind()
        );
    };
    let direction = id
        .parse::<Direction>()
        .with_context(|| format!("invalid navigation action {id:?}"))?;
    let client = invocation.client();
    // Without a pane there is nothing to inspect, nothing to send to, and
    // nothing to move focus from.
    let pane = invocation.require_pane_id()?;

    // Racy by construction: the foreground process can change between this
    // read and the send. herdr exposes no atomic route to close that.
    //
    // A failed read falls back to moving focus, because a chord that does
    // nothing reads as a broken keyboard — but it says so on stderr, which
    // herdr keeps in the plugin log. A silent fallback here would make every
    // key quietly stop reaching vim.
    let info = match client.pane_process_info(pane) {
        Ok(info) => Some(info),
        Err(error) => {
            eprintln!("nav: could not read what is running in {pane}: {error:#}");
            None
        }
    };
    match decide(direction, info.as_ref()) {
        NavigationDecision::Forward => client
            .pane_send_keys(pane, &[chord(direction)])
            .with_context(|| format!("sending {} to {pane}", chord(direction))),
        NavigationDecision::MoveFocus => client
            .pane_focus_direction(pane, direction)
            .with_context(|| format!("moving focus {} from {pane}", chord(direction))),
    }
}
