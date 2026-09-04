//! Open a file in the nvim already running in this tab, from a picker or a
//! clicked link.

mod files;
mod remote;
mod target;

pub use files::{File, Listings, Tier, groups};
pub use remote::open;
pub use target::Target;
