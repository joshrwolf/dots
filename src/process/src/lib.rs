//! Bounded one-shot subprocesses, and line streams from long-running watchers.
//! Not for interactive terminals.
//!
//! Each command leads its own process group, with stdin closed and both pipes
//! drained concurrently with bounded storage. A timeout, oversized response,
//! I/O failure, or dropped run kills the whole group. A reaped leader is never
//! signalled: its group ID can then be reused. Descendants that close both
//! pipes or leave the process group are not supervised.

mod error;
mod group;
mod lines;
mod runner;

pub use error::Error;
pub use lines::{Ended, Lines};
pub use runner::{Runner, output};
