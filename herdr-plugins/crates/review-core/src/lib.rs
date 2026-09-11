//! Durable repository review contexts with immutable Git observations.
//!
//! One database belongs to the repository clone and is shared by linked
//! worktrees. A context owns lineage and discussion; runtime bindings project
//! an exact observation into one or more ephemeral Herdr views.

mod anchor;
mod model;
mod repository;
mod store;

pub use model::{
    AgentAssignment, AgentRequest, AgentRequestId, AgentRequestState, AnchorContext,
    AnchorLocation, AnchorResolution, AnchorStatus, BindingCursor, BindingPosition, BindingState,
    CapturedComparison, CapturedEndpoint, CheckoutKind, ComparisonSpec, ContextListQuery,
    ContextListing, CreateAgentRequest, CreateBoundThread, DiffEndpoint, DiffSide, DispatchAttempt,
    DispatchAttemptId, DispatchAttemptState, DispatchJob, DispatchOutcome, ExternalReference,
    Finding, FindingKind, FindingSeverity, Message, MessageId, MessageOrigin, NewFinding,
    NewMessage, NewThread, Observation, ObservationId, RequestRecovery, ReviewAnchor,
    ReviewContext, ReviewContextId, RuntimeBinding, RuntimeBindingId, Thread, ThreadId,
    ThreadStatus,
};
pub use repository::Repository;
pub use store::{Store, StoreSummary};

use std::io;
use std::path::PathBuf;
use std::process::ExitStatus;

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    #[error("could not run Git while inspecting {checkout}")]
    RunGit {
        checkout: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("Git did not finish while inspecting {checkout}")]
    GitTimedOut { checkout: PathBuf },
    #[error("Git output exceeded the {limit}-byte limit while inspecting {checkout}")]
    GitOutputTooLarge { checkout: PathBuf, limit: usize },
    #[error("Git could not inspect {checkout} ({status}): {stderr}")]
    GitFailed {
        checkout: PathBuf,
        status: ExitStatus,
        stderr: String,
    },
    #[error("Git returned non-UTF-8 data while inspecting {checkout}")]
    NonUtf8GitPath { checkout: PathBuf },
    #[error("Git returned an empty {field} while inspecting {checkout}")]
    EmptyGitPath {
        checkout: PathBuf,
        field: &'static str,
    },
    #[error("could not canonicalize {field} {path}")]
    Canonicalize {
        field: &'static str,
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("review state path is not representable as UTF-8: {path:?}")]
    NonUtf8StatePath { path: PathBuf },
    #[error("could not create review state directory {path}")]
    CreateStateDirectory {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("could not copy Git index {live_index} to private capture index {target}")]
    CaptureIndex {
        live_index: PathBuf,
        target: PathBuf,
        #[source]
        source_error: io::Error,
    },
    #[error("could not access checkout identity token {path}")]
    CheckoutTokenIo {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error(
        "checkout identity token {path} must contain exactly 32 lowercase hexadecimal characters"
    )]
    InvalidCheckoutToken { path: PathBuf },
    #[error("could not open review database {path}")]
    OpenDatabase {
        path: PathBuf,
        #[source]
        source: rusqlite::Error,
    },
    #[error("could not configure review database {path}")]
    ConfigureDatabase {
        path: PathBuf,
        #[source]
        source: rusqlite::Error,
    },
    #[error("could not initialize review database {path}")]
    InitializeDatabase {
        path: PathBuf,
        #[source]
        source: rusqlite::Error,
    },
    #[error("review database {path} does not have the schema expected by this build: {reason}")]
    IncompatibleDatabase { path: PathBuf, reason: String },
    #[error("could not register checkout {checkout} in review database {database}")]
    RegisterCheckout {
        checkout: PathBuf,
        database: PathBuf,
        #[source]
        source: rusqlite::Error,
    },
    #[error("could not {operation} in review database {path}")]
    DatabaseOperation {
        operation: &'static str,
        path: PathBuf,
        #[source]
        source: rusqlite::Error,
    },
    #[error("review context {context} does not exist")]
    ReviewContextNotFound { context: String },
    #[error("review observation {observation} does not exist")]
    ObservationNotFound { observation: String },
    #[error("observation {observation} does not belong to context {context}")]
    ObservationNotInContext {
        observation: String,
        context: String,
    },
    #[error("runtime binding {binding} does not exist")]
    RuntimeBindingNotFound { binding: String },
    #[error("runtime binding {binding} belongs to a different checkout")]
    BindingCheckoutMismatch { binding: String },
    #[error("runtime binding {binding} advanced; reload review context before saving findings")]
    StaleObservation { binding: String },
    #[error(
        "multiple review comparisons are bound to workspace {workspace}; select a binding explicitly"
    )]
    AmbiguousRuntimeBinding { workspace: String },
    #[error("there are no new reviewer messages to send")]
    NoPendingMessages,
    #[error("agent reply does not belong to this request, binding, or thread")]
    InvalidAgentReply,
    #[error("this request already has a different reply in that thread")]
    ConflictingAgentReply,
    #[error("could not encode a review request projection")]
    EncodeRequestProjection(#[source] serde_json::Error),
    #[error("could not encode a review binding page")]
    EncodeBindingPage(#[source] serde_json::Error),
    #[error("review state changed while loading; restart from the first page")]
    StaleCursor,
    #[error("could not decode a review request snapshot")]
    DecodeRequestSnapshot(#[source] serde_json::Error),
    #[error(
        "new review messages and finding metadata exceed the request budget; send fewer threads or shorter messages"
    )]
    RequestTooLarge,
    #[error("{field} exceeds the {limit}-byte serialized input limit")]
    InputTooLarge { field: &'static str, limit: usize },
    #[error("review thread {thread} does not exist in this binding")]
    ThreadNotFound { thread: String },
    #[error("review message {message} does not exist")]
    MessageNotFound { message: String },
    #[error("agent request {request} does not exist in this binding")]
    AgentRequestNotFound { request: String },
    #[error("dispatch attempt {attempt} does not exist in this binding")]
    DispatchAttemptNotFound { attempt: String },
    #[error("cannot {action} {entity} while it is {state}")]
    InvalidTransition {
        entity: &'static str,
        action: &'static str,
        state: String,
    },
    #[error("{field} must not be empty")]
    EmptyField { field: &'static str },
    #[error("{kind} id must be 32 lowercase hexadecimal characters, got {value:?}")]
    InvalidIdentifier { kind: &'static str, value: String },
    #[error("external references on one context must be unique")]
    DuplicateExternalReference,
    #[error("line range must start at 1 or later and end at or after its start")]
    InvalidLineRange,
    #[error("anchor selected lines must contain exactly {expected} line(s), got {actual}")]
    AnchorSpanMismatch { expected: u64, actual: usize },
    #[error(
        "the saved file does not contain the selected anchor at {path}:{start_line}-{end_line}"
    )]
    AnchorSourceMismatch {
        path: String,
        start_line: u32,
        end_line: u32,
    },
    #[error("review anchor source {path} is unavailable in {endpoint}")]
    AnchorSourceUnavailable { path: String, endpoint: String },
    #[error("review anchor path must be repository-relative without parent traversal: {path:?}")]
    InvalidAnchorPath { path: String },
    #[error("review anchor source {path} is larger than the {limit}-byte limit")]
    AnchorSourceTooLarge { path: String, limit: usize },
    #[error("review anchor source {path} is not UTF-8")]
    NonUtf8AnchorSource { path: String },
    #[error("could not encode review anchor context")]
    EncodeAnchorContext(#[source] serde_json::Error),
    #[error("could not encode a review finding")]
    EncodeFinding(#[source] serde_json::Error),
    #[error("an agent request must contain at least one thread")]
    EmptyThreadBatch,
    #[error("an agent request cannot contain the same thread more than once")]
    DuplicateThreadInBatch,
    #[error("review thread {thread} is not open")]
    ThreadNotOpen { thread: String },
    #[error("comparison does not identify a supported pair of distinct endpoints")]
    InvalidComparison,
    #[error("captured comparison has identical base and target content")]
    EmptyComparison,
    #[error("commit oid must be a full lowercase SHA-1 or SHA-256 object id")]
    InvalidCommitOid,
    #[error("dispatch lease must be between 1 and 300 seconds")]
    InvalidDispatchLease,
    #[error("an agent assignment must include an expected agent name or kind")]
    MissingExpectedAgentIdentity,
    #[error("dispatch attempt {attempt} is not claimed by {claimant}")]
    DispatchClaimLost { attempt: String, claimant: String },
    #[error("review database {path} contains invalid {field}: {value}")]
    InvalidStoredValue {
        path: PathBuf,
        field: &'static str,
        value: String,
    },
    #[error("review database {path} returned a negative {field} count: {count}")]
    InvalidCount {
        path: PathBuf,
        field: &'static str,
        count: i64,
    },
    #[error("{field} must be between 1 and {maximum}, got {value}")]
    InvalidListLimit {
        field: &'static str,
        value: u32,
        maximum: u32,
    },
}

pub type Result<T> = std::result::Result<T, Error>;
