use std::ffi::OsString;
use std::io;
use std::path::PathBuf;

pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Every way talking to herdr can fail.
///
/// No variant's message restates its `source`: `anyhow`'s `{:#}` walks the
/// chain, so a message that includes its own source prints it twice.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    #[error("choose an explicit Herdr session outside Herdr (--session <name>)")]
    MissingConnectionTarget,

    #[error("invalid Herdr connection target: {reason}")]
    InvalidConnectionTarget { reason: &'static str },

    #[error("Herdr session {name:?} was not found; inspect `herdr session list`")]
    SessionNotFound { name: String },

    #[error(
        "Herdr session {name:?} is stopped; start it explicitly with `herdr session attach <name>`"
    )]
    SessionStopped { name: String },

    #[error("Herdr returned ambiguous session identity {name:?}")]
    AmbiguousSession { name: String },

    #[error("could not run `herdr session list --json`")]
    SessionDiscoveryIo(#[source] io::Error),

    #[error("`herdr session list --json` failed with exit code {code:?}")]
    SessionDiscoveryExit { code: Option<i32> },

    #[error("`herdr session list --json` exceeded its three-second deadline")]
    SessionDiscoveryTimeout,

    #[error("`herdr session list --json` exceeded its output limit")]
    SessionDiscoveryTooLarge,

    #[error("could not decode `herdr session list --json`")]
    SessionDiscoveryDecode(#[source] serde_json::Error),

    #[error("HERDR_WORKTREE_TIMEOUT_SECS must be an integer from 1 through 86400, got {value:?}")]
    InvalidWorktreeTimeout { value: String },
    #[error("required environment variable {name} is unset or empty")]
    MissingEnv { name: &'static str },

    #[error("environment variable {name} is not valid Unicode: {value:?}")]
    NonUnicodeEnv { name: &'static str, value: OsString },

    #[error("{field} is not representable in herdr's JSON protocol: {path:?}")]
    NonUnicodePath { field: &'static str, path: PathBuf },

    #[error("{field} is too large for herdr's millisecond protocol field: {duration:?}")]
    DurationOverflow {
        field: &'static str,
        duration: std::time::Duration,
    },

    #[error("metadata TTL {duration:?} is shorter than the protocol's one-millisecond minimum")]
    MetadataTtlTooShort { duration: std::time::Duration },

    #[error("metadata TTL {duration:?} exceeds the protocol's 24-hour maximum")]
    MetadataTtlTooLong { duration: std::time::Duration },

    #[error("{field} {duration:?} must be greater than {minimum:?} and at most {maximum:?}")]
    DurationOutOfRange {
        field: &'static str,
        duration: std::time::Duration,
        minimum: std::time::Duration,
        maximum: std::time::Duration,
    },

    #[error("could not reach the herdr server at {path}")]
    Connect {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    #[error("could not identify the herdr server socket at {path}")]
    InspectEndpoint {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    #[error("could not encode the {method} request")]
    Encode {
        method: String,
        #[source]
        source: serde_json::Error,
    },

    #[error("{method} failed on the socket")]
    Io {
        method: String,
        #[source]
        source: io::Error,
    },

    #[error("herdr did not answer {method} within {}ms", timeout.as_millis())]
    Timeout {
        method: String,
        timeout: std::time::Duration,
    },

    #[error("the herdr server closed the connection without answering {method}")]
    NoResponse { method: String },

    #[error("the herdr server closed the connection midway through a {method} frame")]
    TruncatedFrame { method: String },

    #[error("herdr's {method} frame exceeded the {maximum}-byte transport limit")]
    FrameTooLarge { method: String, maximum: usize },

    #[error("herdr's answer to {method} was neither a result nor an error")]
    Malformed { method: String },

    #[error("could not parse herdr's answer to {method}")]
    Decode {
        method: String,
        #[source]
        source: serde_json::Error,
    },

    /// The server echoes the request id. A mismatch means the response belongs
    /// to someone else, so the payload cannot be trusted even if it parses.
    #[error("herdr answered request {expected} with a response for {actual}")]
    IdMismatch { expected: String, actual: String },

    #[error("herdr rejected {method}: {code}: {message}")]
    Rejected {
        method: String,
        code: String,
        message: String,
    },

    #[error("herdr answered {method} with a `{actual}` result, expected `{expected}`")]
    WrongResult {
        method: String,
        expected: &'static str,
        actual: String,
    },

    #[error("HERDR_PLUGIN_CONTEXT_JSON is not valid JSON")]
    InvalidPluginContext(#[source] serde_json::Error),

    #[error("HERDR_PLUGIN_EVENT_JSON is not valid JSON")]
    InvalidPluginEvent(#[source] serde_json::Error),

    #[error("herdr supplied conflicting plugin invocation identities: {fields}")]
    ConflictingInvocation { fields: String },

    #[error("herdr supplied no {field} for this plugin invocation")]
    MissingInvocationField { field: &'static str },

    /// Building the tokens is part of reporting them, so the two failures land
    /// in one type and a caller needs only one `?`.
    #[error("metadata tokens herdr would not accept")]
    Token(#[from] crate::tokens::TokenError),
}
