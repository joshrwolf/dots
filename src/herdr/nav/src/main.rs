use std::process::ExitCode;

use herdrkit::Invocation;
use herdrkit::runtime::{self, FailureSurface};

fn main() -> ExitCode {
    runtime::finish("nav", FailureSurface::Log, run())
}

fn run() -> anyhow::Result<()> {
    herdr_nav::run(&Invocation::load()?)
}
