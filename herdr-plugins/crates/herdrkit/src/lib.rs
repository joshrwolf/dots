//! Shared foundation for herdr plugins: a typed API client, change streams,
//! normalized invocation context, retained or expiring metadata, services, and
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
//! | background service | per server | [`service`], [`events`], [`Tokens`] |
//!
//! Hooks ensure services are ready; services own bounded background work.
//! Provider scheduling belongs in plugins. Herdr renders published metadata.

pub mod api;
pub mod apps;
pub mod events;
pub mod picker;
pub mod runtime;
pub mod service;

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

pub use env::{ExecutionContext, Invocation, InvocationContext, InvocationKind};
pub use error::{Error, Result};
pub use id::{PaneId, TabId, WorkspaceId};
pub use metadata::MetadataReporter;
pub use socket::Client;
pub use theme::{Indicators, Theme};
pub use tokens::{TokenError, Tokens};
