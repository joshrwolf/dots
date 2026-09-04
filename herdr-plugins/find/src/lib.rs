//! One list of everywhere you can go in a herdr session, and the dispatch that
//! gets you there.
//!
//! Sources are open workspaces, live agents, git worktrees herdr knows about,
//! and zoxide's directories. Adding a source is a variant of [`Destination`],
//! a block in [`Sources::destinations`], a group in [`groups`], and an arm in
//! [`open`]. Nothing else changes.

mod destination;
mod dispatch;
mod index;

pub use destination::{Destination, DestinationKind, FIXED_CELLS, groups};
pub use dispatch::open;
pub use index::Sources;
