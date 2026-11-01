//! An agent-curated list of GitHub pull requests with cached status.
//!
//! There is no daemon. The store holds each entry, its last observed status,
//! and when it is next due; any caller runs the same due-driven refresh.
//! Claiming leases due rows under a fresh token, so concurrent refreshers never
//! fetch one PR at the same time, and a result is recorded only for the claim
//! that produced it. Callers supply the storage path and the current time
//! explicitly.
mod fetch;
mod status;
mod store;

pub use fetch::Gh;
pub use github_client::{Error as TransportError, PullRequestUrl};
pub use status::{Ci, Review, State};
pub use store::{Entry, Error, Failure, Listing, NewEntry, Refresh, STORE_PATH, Store};
