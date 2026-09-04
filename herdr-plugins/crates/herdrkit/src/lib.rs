//! Shared foundation for herdr plugins: a typed API client, change streams,
//! normalized invocation context, expiring metadata, runtime reporting, and
//! the picker.
//!
//! A plugin depending on this crate should contain only its own domain logic.
//!
//! Three shapes of plugin, and the module each turns on:
//!
//! | shape | runs | uses |
//! |---|---|---|
//! | one-shot action | per keypress | [`api`] via [`Client`] |
//! | picker | per invocation | [`picker`] |
//! | resident watcher | per session | [`events`] plus [`Tokens`] |
//!
//! The third is the one that needs saying. A watcher subscribes once, blocks
//! on [`events::Events::changes`], re-reads state when something moves, and
//! pushes [`Tokens`] into herdr's own rendering with a TTL. That replaces the
//! shape it supersedes — a `[[ui.tab_bar_right]]` command re-run on a timer,
//! writing one shared line whether or not anything changed.

pub mod api;
pub mod apps;
pub mod events;
pub mod picker;
pub mod runtime;

mod columns;
mod env;
mod error;
mod id;
mod metadata;
mod popup;
mod socket;
mod theme;
mod tokens;

#[cfg(any(test, feature = "testing"))]
pub mod testing;

pub use env::{Invocation, InvocationContext, InvocationKind};
pub use error::{Error, Result};
pub use id::{PaneId, TabId, WorkspaceId};
pub use metadata::MetadataReporter;
pub use socket::Client;
pub use theme::{Indicators, Theme};
pub use tokens::{TokenError, Tokens};
