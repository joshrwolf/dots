//! Open a file in the nvim already running in this tab, from a picker or a
//! clicked link.

pub mod protocol;
pub mod recovery;

mod files;
mod remote;
mod target;
mod transport;

pub use files::{File, Listings, Tier, groups};
pub use remote::{Editors, RoutingError};
pub use target::Target;
