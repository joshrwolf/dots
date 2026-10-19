use std::ffi::OsString;
use std::io::{self, BufReader};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context as _, Result, bail};
use herdrkit::runtime::{self, FailureSurface};
use herdrkit::{Invocation, InvocationKind};

fn main() -> ExitCode {
    match process_mode(std::env::args_os().skip(1)) {
        Ok(ProcessMode::Herdr) => runtime::finish("review", FailureSurface::Popup, run_herdr()),
        Ok(ProcessMode::Stdio { checkout }) => finish_stdio(run_stdio(&checkout)),
        Ok(ProcessMode::Agent { checkout }) => finish_stdio(herdr_review::serve_agent(
            &checkout,
            BufReader::new(io::stdin().lock()),
            io::stdout(),
        )),
        Err(error) => finish_stdio(Err(error)),
    }
}

fn run_herdr() -> Result<()> {
    let invocation = Invocation::load()?;
    match command(invocation.kind())? {
        PluginCommand::Open => herdr_review::open_review(&invocation),
    }
}

fn run_stdio(checkout: &Path) -> Result<()> {
    let stdin = io::stdin();
    let stdout = io::stdout();
    herdr_review::serve_stdio(checkout, BufReader::new(stdin.lock()), stdout)
        .context("serving the Neovim review bridge")
}

fn finish_stdio(result: Result<()>) -> ExitCode {
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("review bridge: {error:#}");
            ExitCode::FAILURE
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ProcessMode {
    Herdr,
    Stdio { checkout: PathBuf },
    Agent { checkout: PathBuf },
}

fn process_mode(args: impl IntoIterator<Item = OsString>) -> Result<ProcessMode> {
    let mut args = args.into_iter();
    let Some(mode) = args.next() else {
        return Ok(ProcessMode::Herdr);
    };
    if mode != "--stdio" && mode != "--agent" {
        bail!(
            "unsupported review process mode {}",
            Path::new(&mode).display()
        );
    }
    let checkout = args
        .next()
        .map(PathBuf::from)
        .context("review process mode requires an explicit checkout path")?;
    if let Some(extra) = args.next() {
        bail!(
            "unexpected argument after the checkout path: {}",
            Path::new(&extra).display()
        );
    }
    if mode == "--agent" {
        Ok(ProcessMode::Agent { checkout })
    } else {
        Ok(ProcessMode::Stdio { checkout })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PluginCommand {
    Open,
}

fn command(kind: &InvocationKind) -> Result<PluginCommand> {
    match kind {
        InvocationKind::PaneEntrypoint { id } if id == "open" => Ok(PluginCommand::Open),
        other => bail!("review must be launched by the `open` pane entrypoint, got {other:?}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_is_the_only_binary_entrypoint() {
        assert_eq!(
            command(&InvocationKind::PaneEntrypoint {
                id: "open".to_owned()
            })
            .unwrap(),
            PluginCommand::Open
        );
        assert!(
            command(&InvocationKind::Action {
                id: "open".to_owned()
            })
            .is_err()
        );
        assert!(command(&InvocationKind::Startup).is_err());
    }

    #[test]
    fn stdio_mode_requires_one_explicit_checkout() {
        assert_eq!(process_mode(Vec::new()).unwrap(), ProcessMode::Herdr);
        assert_eq!(
            process_mode([OsString::from("--stdio"), OsString::from("/checkout")]).unwrap(),
            ProcessMode::Stdio {
                checkout: PathBuf::from("/checkout")
            }
        );
        assert!(process_mode([OsString::from("--stdio")]).is_err());
        assert!(process_mode([OsString::from("status")]).is_err());
        assert!(process_mode([OsString::from("--deliver")]).is_err());
    }
}
