//! Versioned knowledge with explicit identities, immutable snapshots and atomic publication.
//! Machine-local document mechanics live in dots-documents; Git owns knowledge history.
mod coordinator;
#[cfg(test)]
mod fixture;
mod git;
mod history;
mod identity;
mod incoming;
mod peer;
mod protocol;
pub use coordinator::{
    Choice, Conflict, Coordinator, Delivery, DeliveryReceipt, Integration, IntegrationKind,
    Resolution,
};
pub use incoming::{BundleReceipt, Candidate, CandidateRead, MAX_BUNDLE_BYTES};
pub use peer::{Endpoint, Peer, Tailscale};
pub use protocol::{Export, PeerRequest, PeerResponse, PeerStatus, serve_peer};
mod store;

pub use identity::{NodeId, ProjectId, Snapshot, StoreId};
pub use store::{
    Action, Change, DocumentRead, Hit, Identity, Manifest, Publication, Registration, Resource,
    Role, Scope, Search, SearchResult, Store, View,
};

use std::path::PathBuf;

pub const MAX_FILE_BYTES: usize = dots_documents::MAX_FILE_BYTES;
pub(crate) const MAX_DOCUMENTS: usize = 4096;
pub(crate) const MAX_CORPUS_BYTES: usize = 64 * 1024 * 1024;
pub(crate) const MAX_METADATA_BYTES: usize = 2 * 1024 * 1024;

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
    #[error("{operation}")]
    Process {
        operation: &'static str,
        #[source]
        source: dots_process::Error,
    },
    #[error("peer {operation} failed ({status:?}): {stderr}")]
    Peer {
        operation: &'static str,
        status: Option<i32>,
        stderr: String,
    },
    #[error("git {operation} failed ({status:?}): {stderr}")]
    Git {
        operation: &'static str,
        status: Option<i32>,
        stderr: String,
    },
    #[error("decode {operation}")]
    Json {
        operation: &'static str,
        #[source]
        source: serde_json::Error,
    },
    #[error("knowledge changed; expected {expected}, found {actual}; reread before publishing")]
    Conflict {
        expected: Snapshot,
        actual: Snapshot,
    },
    #[error("unknown knowledge project {0}")]
    UnknownProject(ProjectId),
    #[error(transparent)]
    Documents(#[from] dots_documents::Error),
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
