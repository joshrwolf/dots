//! XDG base directories for dots' machine-local stores.
use std::path::PathBuf;

use anyhow::{Context as _, Result};

/// The native host home; relative values are not safe process configuration.
pub(crate) fn home() -> Result<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .context("absolute HOME is required")
}

/// The absolute directory named by `variable`, else `$HOME/<fallback>`.
pub(crate) fn base(variable: &str, fallback: &str) -> Result<PathBuf> {
    std::env::var_os(variable)
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(fallback)))
        .with_context(|| format!("HOME or absolute {variable} is required"))
}
