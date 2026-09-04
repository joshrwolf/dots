//! Consistent process-level error reporting for plugin binaries.

use std::fmt::Display;
use std::process::ExitCode;

use crate::popup;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailureSurface {
    /// Write to stderr, which herdr retains in the plugin log.
    Log,
    /// Replace an interactive popup with the error and wait for dismissal.
    Popup,
}

/// Converts a plugin result into an exit code and one consistently formatted
/// error report, including an alternate `Display` error chain when available.
pub fn finish<E: Display>(
    plugin: &str,
    surface: FailureSurface,
    result: Result<(), E>,
) -> ExitCode {
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            let message = format!("{plugin}: {error:#}");
            match surface {
                FailureSurface::Log => eprintln!("{message}"),
                FailureSurface::Popup => popup::report(&message),
            }
            ExitCode::FAILURE
        }
    }
}
