//! Read-only GitHub inspection shared by local commands and integration adapters.
//!
//! No Herdr invocation, agent lifecycle, output formatting, or persistent service
//! belongs here. References pin jobs; querying a newer PR never redirects logs.

mod cache;
mod checks;
mod evidence;
mod inspect;
mod logview;
mod output;
mod provider;
mod reference;
pub mod reviews;
mod snapshot;
pub mod status;
mod subjects;
mod target;

pub use cache::CachedLog;
pub use evidence::{Evidence, LogMatch, LogOptions, LogPage, LogSource, Step, job_logs};
pub use inspect::{
    Check, InspectOptions, Inspection, ObservationSource, PullRequest, Summary, inspect,
    inspect_snapshot, inspect_snapshot_filtered, inspect_with_options,
};
pub use output::{
    Annotation, CheckOutput, OutputOptions, OutputRef, check_output, check_output_target,
};
pub use provider::{GitHub, Provider};
pub use reference::CheckRef;
pub use snapshot::SnapshotRef;
pub use subjects::inspect_target;
pub use target::{Repository, Subject, Target};

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    #[error("could not read or store GitHub evidence")]
    LogIo(#[from] std::io::Error),
    #[error("{operation} failed: {message}")]
    Query { operation: String, message: String },
    #[error("could not execute GitHub query")]
    Command(#[from] github_client::Error),
    #[error("could not decode GitHub response")]
    Decode(#[from] serde_json::Error),
    #[error("invalid GitHub reference: {0}")]
    InvalidReference(String),
    #[error("{0}")]
    InvalidResponse(String),
}

pub type Result<T> = std::result::Result<T, Error>;

impl Error {
    /// Preserve the cause when a query failure is returned as partial evidence.
    pub fn diagnostic(&self) -> String {
        use std::error::Error as _;
        let mut text = self.to_string();
        let mut source = self.source();
        while let Some(error) = source {
            text.push_str(": ");
            text.push_str(&error.to_string());
            source = error.source();
        }
        text
    }
}

#[cfg(test)]
mod tests;
