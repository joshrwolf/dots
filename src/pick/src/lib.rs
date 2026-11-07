//! A fuzzy picker for terminal views: grouped, scoped, typed rows that are
//! matched and drawn independently.
//!
//! It runs in this process and hands typed values to the host's actions
//! without serializing rows. Live rows, previews, and actions run on one
//! managed async worker thread; terminal state and selection remain on the
//! calling thread.
//!
//! Owning the loop is what buys the three properties everything here is built
//! on:
//!
//! - **Match text is independent of displayed text.** A [`Cell`] can be drawn
//!   without being searched, and an entry can contribute search terms it never
//!   draws.
//! - **Rows can be inert.** A group's divider is a row that renders, never
//!   matches, and that the cursor steps over, so `Enter` cannot land on a
//!   non-choice.
//! - **It is a pure function** of items, query, cursor and width, which is to
//!   say it is a table test rather than something verified by screen-scraping
//!   a pane and counting character offsets by hand.

mod background;
mod columns;
mod entry;
mod error;
mod group;
mod keys;
mod live;
mod model;
mod palette;
mod pane;
mod picker;
mod preview;
mod run;
mod shortcut;
pub mod spec;
mod text;

pub use entry::{Cell, Entry};
pub use error::Error;
pub use group::{Column, Group, Width};
pub use keys::{Command, command};
pub use live::Fetch;
pub use model::{Model, Outcome};
pub use palette::{LoadError, Palette};
pub use pane::{Pane, PaneLayout, Response, Task};
pub use picker::{Closed, Picker};
pub use run::Page as View;
pub use shortcut::{Act, Acting, Action, Actions, Choice, Chosen, Shortcut};
