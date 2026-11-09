//! A terminal diff viewer for every worktree of a repository: each
//! worktree's branch against its base, as one scrolling stream of files.
//!
//! The program in `main.rs` parses the command line and calls [`run`] or
//! [`ctl`]. With the `dev` feature, `dev` holds the tools the crate's
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

/// A command for a running viewer.
#[derive(Debug, Clone)]
pub enum Ctl {
    /// Show the worktree containing this path.
    Worktree(PathBuf),
    /// Scroll to a file; an absolute path in another worktree switches to it.
    File(String),
    Refresh,
    /// List running viewers: their Rex pane and worktree.
    List,
    Note(Note),
    /// Clear the notes on this file, or all.
    ClearNotes(Option<String>),
    /// Send reviews to the Pi session in this Rex pane.
    Bind(String),
    /// Save the viewer's screen as a PNG picture here.
    Shot(PathBuf),
}

/// A note for the user: on lines of a file, a whole file, or the review.
#[derive(Debug, Clone)]
pub struct Note {
    /// Absolute, or relative to the current directory or the worktree.
    pub file: Option<String>,
    /// First line, 1-based; without it the note is about the whole file.
    pub line: Option<u32>,
    /// Last line of a range.
    pub end: Option<u32>,
    /// The line numbers count the old side.
    pub old: bool,
    pub text: String,
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

/// Sends `command` to the viewer in Rex pane `block`, else the one showing
/// the repository it names, else the only one running.
pub fn ctl(block: Option<&str>, command: Ctl) -> Result<()> {
    let viewers = ctl::viewers();
    let (command, path) = match command {
        Ctl::List => {
            for v in &viewers {
                println!(
                    "{}\t{}",
                    v.status.block.as_deref().unwrap_or("-"),
                    v.status.worktree.display()
                );
            }
            return Ok(());
        }
        Ctl::Worktree(path) => {
            let path = std::path::absolute(path)?;
            (ctl::Command::Worktree(path.clone()), Some(path))
        }
        Ctl::File(path) => {
            let (path, absolute) = absolute_if_exists(path)?;
            (ctl::Command::File(path), absolute)
        }
        Ctl::Refresh => (ctl::Command::Refresh, None),
        Ctl::Note(note) => {
            let (file, path) = match note.file {
                Some(f) => {
                    let (f, p) = absolute_if_exists(f)?;
                    (Some(f), p)
                }
                None => (None, None),
            };
            let spec = ctl::NoteSpec {
                file,
                line: note.line,
                end: note.end,
                old: note.old,
                text: note.text,
            };
            (ctl::Command::Note(spec), path)
        }
        Ctl::ClearNotes(file) => {
            let (file, path) = match file {
                Some(f) => {
                    let (f, p) = absolute_if_exists(f)?;
                    (Some(f), p)
                }
                None => (None, None),
            };
            (ctl::Command::ClearNotes(file), path)
        }
        Ctl::Bind(block) => (ctl::Command::Bind(block), None),
        Ctl::Shot(path) => (ctl::Command::Shot(std::path::absolute(path)?), None),
    };
    let viewer = ctl::pick(&viewers, block, path.as_deref(), repo::same_repository)?;
    ctl::send(viewer, &command)
}
