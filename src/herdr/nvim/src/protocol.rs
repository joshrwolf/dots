//! Editor wire contract. Rust owns these DTOs; real Neovim contract tests exercise
//! their serialization against `dots.herdr.protocol`, not a second Rust mock.
//! Both halves ship together. There are no compatibility aliases or fallbacks.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

pub const VERSION: u32 = 1;
pub const MAX_REQUEST_BYTES: usize = 16 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Context {
    pub registration: String,
    pub server: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Location {
    pub path: PathBuf,
    /// One-based line and byte column, not Unicode character offsets.
    pub line: u32,
    pub column: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "method", rename_all = "snake_case", deny_unknown_fields)]
pub enum Operation {
    Hello,
    Owner {
        control: OwnerControl,
    },
    Open {
        target: Location,
        /// Same-host Unix epoch milliseconds. Reject before preparation and
        /// presentation if this caller-owned deadline has elapsed.
        not_after_ms: u64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        lease: Option<LeaseId>,
    },
}

/// Coordination fence, not authentication against same-user RPC clients.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LeaseId {
    pub owner: String,
    pub generation: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum OwnerControl {
    Inspect,
    Renew {
        lease: LeaseId,
        not_after_ms: u64,
    },
    Release {
        lease: LeaseId,
        not_after_ms: u64,
    },
    Recover {
        expected: LeaseId,
        owner: String,
        not_after_ms: u64,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OwnerPhase {
    Active,
    Orphaned,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Ownership {
    pub lease: LeaseId,
    pub phase: OwnerPhase,
    pub remaining_ms: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub protocol: u32,
    pub context: Context,
    pub operation: Operation,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Identity {
    pub protocol: u32,
    pub context: Context,
    pub pid: u32,
}

/// Stable machine-readable errors. Human-readable messages are not an API.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    InvalidRequest,
    ProtocolMismatch,
    NotAttached,
    IdentityMismatch,
    HostUnavailable,
    ListenerUnavailable,
    InvalidTarget,
    InvalidPosition,
    DirtyBuffer,
    UnsupportedWindow,
    EditorChanged,
    PresentationFailed,
    SwapConflict,
    Expired,
    NotManaged,
    OwnerRequired,
    OwnerMismatch,
    OwnerExpired,
    OwnerActive,
    Internal,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Failure {
    pub code: ErrorCode,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum Reply {
    Ownership {
        identity: Identity,
        // Explicit null means unmanaged; a missing field is malformed.
        #[serde(deserialize_with = "Option::deserialize")]
        ownership: Option<Ownership>,
    },
    Ready {
        identity: Identity,
    },
    Opened {
        identity: Identity,
        target: Location,
    },
    Rejected {
        error: Failure,
    },
    Partial {
        error: Failure,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn malformed_or_unrecognized_replies_fail_closed() {
        for raw in [
            r"{}",
            r#"{"status":"opened"}"#,
            r#"{"status":"ownership","identity":{"protocol":1,"context":{"registration":"0123456789abcdef0123456789abcdef","server":"/tmp/example.sock"},"pid":1}}"#,
            r#"{"status":"ready","identity":{"pid":1}}"#,
            r#"{"status":"rejected","error":{"code":"made_up","message":"no"}}"#,
            r#"{"status":"rejected","error":{"code":"invalid_request","message":"no"},"extra":true}"#,
        ] {
            assert!(serde_json::from_str::<Reply>(raw).is_err(), "{raw}");
        }
    }
}
