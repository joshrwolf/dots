use herdrkit::Invocation;
use herdrkit::runtime::{self, FailureSurface};
use std::process::ExitCode;

fn main() -> ExitCode {
    runtime::finish(
        "find",
        FailureSurface::Popup,
        Invocation::load()
            .map_err(anyhow::Error::from)
            .and_then(|invocation| herdr_find::run(&invocation)),
    )
}
