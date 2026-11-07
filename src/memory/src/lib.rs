//! Shared knowledge as immutable snapshots over the cross-machine substrate.
//!
//! A snapshot is a manifest blob naming its parents, the store, the shared
//! project registry and every document's content hash. The substrate ref
//! `memory` points at the latest manifest. Machine-local state (project
//! bindings and unpublished candidates) stays in the store's local directory.
mod convert;
mod identity;
mod integration;
mod manifest;
mod store;

pub use convert::{Conversion, Converted, convert};
pub use identity::{ProjectId, Snapshot, StoreId};
pub use integration::{Choice, Conflict, Integration, IntegrationKind, Resolution};
pub use manifest::{Entry, Manifest, Resource, Scope};
pub use store::{
    Action, CandidateRead, Change, DocumentRead, Freshness, Head, Hit, Landing, Pending,
    PendingPlan, Publication, Registration, Search, SearchResult, Store, View,
};

use std::path::PathBuf;

pub const MAX_FILE_BYTES: usize = dots_documents::MAX_FILE_BYTES;
pub(crate) const MAX_DOCUMENTS: usize = 4096;
pub(crate) const MAX_CORPUS_BYTES: usize = 64 * 1024 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{operation}: {path}")]
    Io {
        operation: &'static str,
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("{0}")]
    Invalid(String),
    #[error("allocate knowledge identity")]
    Random(#[from] getrandom::Error),
    #[error("decode {operation}")]
    Json {
        operation: &'static str,
        #[source]
        source: serde_json::Error,
    },
    /// The published snapshot moved away from the one a plan was made against.
    #[error("knowledge changed; expected {expected}, found {actual}; reread before publishing")]
    Conflict {
        expected: Snapshot,
        actual: Snapshot,
    },
    /// The same documents changed on both sides; the candidate stays retained.
    #[error(
        "knowledge changed since {expected}; {conflicts} documents diverge from {actual}; candidate {candidate} is retained; inspect memory status and resolve with memory reconcile"
    )]
    Diverged {
        expected: Snapshot,
        actual: Snapshot,
        candidate: Snapshot,
        conflicts: usize,
    },
    /// The substrate has no `memory` ref yet.
    #[error("no knowledge is published; initialize it with memory init")]
    Uninitialized,
    #[error("knowledge is already published at {0}; refusing to replace it")]
    AlreadyInitialized(Snapshot),
    #[error("remote unavailable: {0}")]
    Unavailable(String),
    #[error("unknown knowledge project {0}")]
    UnknownProject(ProjectId),
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
    Substrate(dots_substrate::Error),
    #[error(transparent)]
    Documents(#[from] dots_documents::Error),
}

impl From<dots_substrate::Error> for Error {
    fn from(error: dots_substrate::Error) -> Self {
        match error {
            dots_substrate::Error::Unavailable(reason) => Self::Unavailable(reason),
            error => Self::Substrate(error),
        }
    }
}

pub type Result<T> = std::result::Result<T, Error>;

fn invalid(message: impl Into<String>) -> Error {
    Error::Invalid(message.into())
}

fn io(operation: &'static str, path: &std::path::Path, source: std::io::Error) -> Error {
    Error::Io {
        operation,
        path: path.to_owned(),
        source,
    }
}
