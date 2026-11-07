//! Cross-machine storage of content-addressed blobs and compare-and-swap refs
//! over pluggable backends.
//!
//! Backends store refs and the blobs each ref carries. [`Substrate`] adds the
//! per-machine pieces every domain shares: a verified blob cache, a freshness
//! record shared by all processes, an outbox of deferred updates under a push
//! budget, and the machine identity.
mod backend;
mod cache;
#[cfg(any(test, feature = "testing"))]
pub mod conformance;
mod hash;
pub mod memory;
mod name;
mod state;
mod substrate;

pub use backend::{Backend, Fetched, Outcome, Update, Watch};
pub use hash::{Blob, Hash, MAX_BLOB_BYTES};
pub use name::{MachineId, RefName, Version};
pub use state::{FlushAttempt, Pending, Stranded};
pub use substrate::{Budget, Config, Flush, FlushReport, Freshness, Status, Substrate, Value};

use std::path::{Path, PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("invalid {what}: {reason}")]
    Invalid { what: &'static str, reason: String },
    #[error("blob of {size} bytes exceeds {MAX_BLOB_BYTES} bytes")]
    TooLarge { size: usize },
    #[error("blob content does not match {expected}")]
    Integrity { expected: Hash },
    #[error("blob {0} is not cached")]
    MissingBlob(Hash),
    /// The backend could not be reached; the operation's effect is unknown.
    #[error("backend unavailable: {0}")]
    Unavailable(String),
    /// The backend broke its contract or returned malformed data.
    #[error("backend: {0}")]
    Backend(String),
    #[error("substrate state {path}: {reason}")]
    State { path: PathBuf, reason: String },
    #[error("{operation}: {path}")]
    Io {
        operation: &'static str,
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("allocate machine identity")]
    Random(#[from] getrandom::Error),
    #[error("{operation}")]
    Process {
        operation: &'static str,
        #[source]
        source: dots_process::Error,
    },
    #[error("git {operation} failed ({status:?}): {stderr}")]
    Git {
        operation: &'static str,
        status: Option<i32>,
        stderr: String,
    },
    #[error(transparent)]
    Documents(#[from] dots_documents::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

fn invalid(what: &'static str, reason: impl Into<String>) -> Error {
    Error::Invalid {
        what,
        reason: reason.into(),
    }
}

fn io(operation: &'static str, path: &Path, source: std::io::Error) -> Error {
    Error::Io {
        operation,
        path: path.to_owned(),
        source,
    }
}
