//! A terminal diff viewer for every worktree of a repository: each
//! worktree's branch against its base, as one scrolling stream of files.
//!
//! The program in `main.rs` parses the command line and calls [`run`],
//! [`list`] or [`ctl`]. With the `dev` feature, `dev` holds the tools the crate's
//! examples run from the checkout.

mod agents;
mod app;
mod attribution;
mod cards;
mod ctl;
mod deliver;
#[cfg(feature = "dev")]
pub mod dev;
mod diff;
mod editor;
mod graphics;
mod highlight;
mod input;
mod keys;
mod lru;
mod moves;
mod picker;
mod plan;
mod repo;
mod review;
mod run;
mod session;
mod shot;
mod stats;
mod switcher;
mod terminal;
mod theme;
mod ui;
mod watch;
mod wrap;

use std::path::PathBuf;

use anyhow::Result;
pub use ctl::{Ctl, Note, NoteSide};

/// The user's state directory: `$XDG_STATE_HOME`, or `~/.local/state`.
fn state_home() -> PathBuf {
    std::env::var_os("XDG_STATE_HOME").map_or_else(
        || PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/state"),
        PathBuf::from,
    )
}

/// How the viewer shows changes.
#[derive(Debug, Clone, Default, clap::Args)]
pub struct Look {
    /// Fixed colours instead of the terminal's
    #[arg(long, value_enum)]
    pub theme: Option<Theme>,
    /// Layout instead of choosing by width
    #[arg(long, value_enum)]
    pub layout: Option<Layout>,
    /// Cut long lines instead of wrapping them
    #[arg(long)]
    pub no_wrap: bool,
    /// Compare with HEAD (uncommitted changes) instead of the base
    #[arg(long)]
    pub uncommitted: bool,
    /// Hide the file sidebar
    #[arg(long)]
    pub no_sidebar: bool,
    /// Sidebar width in columns instead of a fifth of the terminal
    #[arg(long, value_name = "COLUMNS", value_parser = clap::value_parser!(u16).range(16..=64))]
    pub sidebar_width: Option<u16>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum Theme {
    Light,
    Dark,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum Layout {
    Unified,
    Split,
}

impl Look {
    fn settings(&self) -> app::Settings {
        app::Settings {
            layout: match self.layout {
                None => app::LayoutChoice::Auto,
                Some(Layout::Unified) => app::LayoutChoice::Unified,
                Some(Layout::Split) => app::LayoutChoice::Split,
            },
            wrap: !self.no_wrap,
            mode: if self.uncommitted {
                repo::Mode::Head
            } else {
                repo::Mode::Base
            },
            sidebar: !self.no_sidebar,
            sidebar_width: self.sidebar_width,
            ..app::Settings::default()
        }
    }

    fn theme(&self) -> Option<theme::Detected> {
        self.theme.map(|t| match t {
            Theme::Light => theme::Detected::light(),
            Theme::Dark => theme::Detected::dark(),
        })
    }
}

/// What the viewer opens.
#[derive(Debug, Clone)]
pub struct Options {
    /// A path inside the repository; its worktree opens first.
    pub path: PathBuf,
    /// A file to scroll to once loaded, by path within the worktree.
    pub file: Option<String>,
    /// The Rex pane whose Pi session receives reviews of the first worktree.
    pub bind: Option<String>,
    pub look: Look,
}

/// Shows the viewer in the terminal until the user quits.
pub fn run(options: Options) -> Result<()> {
    run::run(&run::Options {
        path: options.path,
        file: options.file,
        bind: options.bind,
        theme: options.look.theme(),
        settings: options.look.settings(),
    })
}

/// A file named from inside a worktree travels as an absolute path, so it
/// reaches that worktree; otherwise it is taken within the viewer's.
fn absolute_if_exists(path: String) -> Result<(String, Option<PathBuf>)> {
    let absolute = std::path::absolute(&path)?;
    Ok(if absolute.exists() {
        (absolute.to_string_lossy().into_owned(), Some(absolute))
    } else {
        (path, None)
    })
}

/// Prints the running viewers: their Rex pane and worktree.
pub fn list() {
    for v in &ctl::viewers() {
        println!(
            "{}\t{}",
            v.status.block.as_deref().unwrap_or("-"),
            v.status.worktree.display()
        );
    }
}

/// Sends `command` to the viewer in Rex pane `block`, else the one showing
/// the repository it names, else the only one running. Paths that exist
/// travel absolute.
pub fn ctl(block: Option<&str>, command: Ctl) -> Result<()> {
    let (command, path) = match command {
        Ctl::Worktree { path } => {
            let path = std::path::absolute(path)?;
            (Ctl::Worktree { path: path.clone() }, Some(path))
        }
        Ctl::File { path } => {
            let (path, absolute) = absolute_if_exists(path)?;
            (Ctl::File { path }, absolute)
        }
        Ctl::Note(mut note) => {
            let path = match note.file.take() {
                Some(f) => {
                    let (f, p) = absolute_if_exists(f)?;
                    note.file = Some(f);
                    p
                }
                None => None,
            };
            (Ctl::Note(note), path)
        }
        Ctl::Shot { path } => (
            Ctl::Shot {
                path: std::path::absolute(path)?,
            },
            None,
        ),
        command @ (Ctl::Refresh | Ctl::Bind { .. }) => (command, None),
    };
    let viewers = ctl::viewers();
    let viewer = ctl::pick(&viewers, block, path.as_deref(), repo::same_repository)?;
    ctl::send(viewer, command)
}
