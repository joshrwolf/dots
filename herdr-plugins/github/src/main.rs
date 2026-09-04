use std::process::ExitCode;

use anyhow::{Context as _, Result, bail};
use herdr_github::ci::autofix::Config;
use herdrkit::runtime::{self, FailureSurface};
use herdrkit::{Invocation, InvocationKind};

fn main() -> ExitCode {
    let surface = if matches!(
        InvocationKind::from_env(),
        Ok(InvocationKind::PaneEntrypoint { ref id }) if id == "ci-brief"
    ) {
        FailureSurface::Popup
    } else {
        FailureSurface::Log
    };
    runtime::finish("github", surface, run())
}

fn run() -> Result<()> {
    let invocation = Invocation::load()?;
    let client = invocation.client();
    match command(invocation.kind())? {
        PluginCommand::Refresh => herdr_github::ci::refresh::run(
            client,
            invocation.require_workspace_id()?,
            invocation.require_target_dir()?,
        ),
        PluginCommand::Fix => herdr_github::ci::autofix::fix_now(
            client,
            invocation.require_target_dir()?,
            invocation.state_dir(),
            Config::from_env()?,
        ),
        PluginCommand::Arm => herdr_github::ci::autofix::arm(
            client,
            invocation.require_target_dir()?,
            invocation.state_dir(),
            Config::from_env()?,
        ),
        PluginCommand::Brief => herdr_github::ci::brief::show(invocation.require_target_dir()?),
        PluginCommand::OpenLink => herdr_github::open::open_url(
            invocation
                .clicked_url()
                .context("herdr supplied no GitHub URL")?,
        ),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PluginCommand {
    Refresh,
    Fix,
    Arm,
    Brief,
    OpenLink,
}

fn command(kind: &InvocationKind) -> Result<PluginCommand> {
    match kind {
        InvocationKind::Action { id } if id == "ci-refresh" => Ok(PluginCommand::Refresh),
        InvocationKind::Action { id } if id == "ci-fix" => Ok(PluginCommand::Fix),
        InvocationKind::PaneEntrypoint { id } if id == "ci-arm" => Ok(PluginCommand::Arm),
        InvocationKind::PaneEntrypoint { id } if id == "ci-brief" => Ok(PluginCommand::Brief),
        InvocationKind::Action { id } if id == "open-link" => Ok(PluginCommand::OpenLink),
        other => bail!(
            "github must be launched by the ci-refresh, ci-fix, ci-arm, ci-brief, or open-link manifest entry, got {other:?}"
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_entry_kinds_are_part_of_the_dispatch_contract() {
        assert_eq!(
            command(&InvocationKind::Action {
                id: "ci-refresh".to_owned()
            })
            .unwrap(),
            PluginCommand::Refresh
        );
        assert_eq!(
            command(&InvocationKind::PaneEntrypoint {
                id: "ci-arm".to_owned()
            })
            .unwrap(),
            PluginCommand::Arm
        );
        assert_eq!(
            command(&InvocationKind::PaneEntrypoint {
                id: "ci-brief".to_owned()
            })
            .unwrap(),
            PluginCommand::Brief
        );
        assert!(
            command(&InvocationKind::Action {
                id: "ci-arm".to_owned()
            })
            .is_err()
        );
        assert!(command(&InvocationKind::Startup).is_err());
    }
}
