//! A terminal diff viewer for every worktree of a repository: each
//! worktree's branch against its merge base with the default branch, as one
//! scrolling stream of files.

mod app;
mod cards;
mod ctl;
mod deliver;
mod diff;
mod editor;
mod graphics;
mod highlight;
mod input;
mod keys;
mod lru;
mod picker;
mod plan;
mod render;
mod repo;
mod review;
mod run;
mod stats;
mod switcher;
mod theme;
mod timings;
mod ui;
mod watch;
mod wrap;

use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::Result;
use clap::{Args, Parser, Subcommand, ValueEnum};

#[derive(Debug, Parser)]
#[command(
    name = "dots-diff",
    version,
    about = "Review the changes in every worktree of a repository"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Cmd>,
    /// A path inside the repository; its worktree opens first
    #[arg(default_value = ".")]
    path: PathBuf,
    /// Scroll to this file once loaded, by path within the worktree
    #[arg(long, value_name = "PATH")]
    file: Option<String>,
    /// Send reviews to the Pi session in this Rex pane while the first
    /// worktree is shown
    #[arg(long, value_name = "BLOCK")]
    bind: Option<String>,
    /// Draw file headers as kitty graphics images
    #[arg(long)]
    graphics: bool,
    /// Print timings on exit, to standard error or FILE
    #[arg(long, value_name = "FILE", num_args = 0..=1, require_equals = true)]
    #[allow(clippy::option_option)]
    timings: Option<Option<PathBuf>>,
    #[command(flatten)]
    look: Look,
}

#[derive(Debug, Args, Clone)]
struct Look {
    /// Fixed colours instead of the terminal's
    #[arg(long, value_enum)]
    theme: Option<Theme>,
    /// Layout instead of choosing by width
    #[arg(long, value_enum)]
    layout: Option<LayoutArg>,
    /// Cut long lines instead of wrapping them
    #[arg(long)]
    no_wrap: bool,
    /// Compare with HEAD (uncommitted changes) instead of the merge base
    #[arg(long)]
    uncommitted: bool,
    /// Hide the file sidebar
    #[arg(long)]
    no_sidebar: bool,
    /// Sidebar width in columns instead of a fifth of the terminal
    #[arg(long, value_name = "COLUMNS", value_parser = clap::value_parser!(u16).range(16..=64))]
    sidebar_width: Option<u16>,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum Theme {
    Light,
    Dark,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum LayoutArg {
    Unified,
    Split,
}

impl Look {
    fn settings(&self) -> app::Settings {
        app::Settings {
            layout: match self.layout {
                None => app::LayoutChoice::Auto,
                Some(LayoutArg::Unified) => app::LayoutChoice::Unified,
                Some(LayoutArg::Split) => app::LayoutChoice::Split,
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

#[derive(Debug, Subcommand)]
enum Cmd {
    /// Print one frame as text
    Render {
        #[arg(default_value = ".")]
        path: PathBuf,
        #[arg(long, default_value_t = 160)]
        width: u16,
        #[arg(long, default_value_t = 48)]
        height: u16,
        /// Show this worktree instead of the one containing PATH
        #[arg(long)]
        worktree: Option<PathBuf>,
        /// Scroll to this file
        #[arg(long)]
        file: Option<String>,
        /// Lines to scroll past the file's start
        #[arg(long, default_value_t = 0)]
        scroll: u32,
        /// Keep colours as ANSI escapes
        #[arg(long)]
        ansi: bool,
        /// Also draw the frame as a PNG picture
        #[arg(long, value_name = "FILE")]
        png: Option<PathBuf>,
        /// Show the worktree switcher, filtered by QUERY
        #[arg(long, value_name = "QUERY", num_args = 0..=1, default_missing_value = "")]
        switcher: Option<String>,
        /// Collapse this sidebar folder (repeatable)
        #[arg(long, value_name = "DIR")]
        collapse: Vec<String>,
        /// Mark files under this path prefix reviewed (repeatable)
        #[arg(long, value_name = "PREFIX")]
        review: Vec<String>,
        /// Focus the sidebar on this file or folder
        #[arg(long, value_name = "PATH")]
        select: Option<String>,
        #[command(flatten)]
        look: Look,
    },
    /// Steer a running viewer
    Ctl {
        /// The viewer in this Rex pane; by default the one showing the
        /// path's repository, or the only one running
        #[arg(long, global = true, value_name = "ID")]
        block: Option<String>,
        #[command(subcommand)]
        command: CtlCmd,
    },
    /// Measure loading, diffing, highlighting and drawing in-process
    Bench {
        #[arg(default_value = ".")]
        path: PathBuf,
        /// Worktrees to measure; the one containing PATH by default
        #[arg(long)]
        worktree: Vec<PathBuf>,
        #[arg(long, default_value_t = 200)]
        width: u16,
        #[arg(long, default_value_t = 60)]
        height: u16,
        /// Also time the rail's per-worktree stats for every worktree
        #[arg(long)]
        summaries: bool,
        /// With --summaries: ignore the on-disk line-count cache
        #[arg(long)]
        cold: bool,
        /// Compare full status with and without the fsmonitor (may start its
        /// daemon for the measured worktree)
        #[arg(long)]
        fsmonitor: bool,
    },
}

#[derive(Debug, Subcommand)]
enum CtlCmd {
    /// Show the worktree containing PATH
    Worktree { path: PathBuf },
    /// Scroll to a file; an absolute path in another worktree switches to it
    File { path: String },
    /// Reload now
    Refresh,
    /// List running viewers: their Rex pane and worktree
    List,
    /// Show the user a note: on lines of a file, a whole file, or the review
    Note {
        /// The file: absolute, or relative to the current directory or the
        /// worktree
        #[arg(long, value_name = "PATH")]
        file: Option<String>,
        /// First line, 1-based; without it the note is about the whole file
        #[arg(long, requires = "file")]
        line: Option<u32>,
        /// Last line of a range
        #[arg(long, requires = "line")]
        end: Option<u32>,
        /// The side the line numbers count
        #[arg(long, value_enum, default_value_t = NoteSide::New)]
        side: NoteSide,
        /// Clear notes instead: those of --file, or all
        #[arg(long, conflicts_with_all = ["line", "text"])]
        clear: bool,
        /// What the note says
        #[arg(required_unless_present = "clear")]
        text: Option<String>,
    },
    /// Send reviews to the Pi session in this Rex pane
    Bind { block: String },
    /// Save the viewer's screen as a PNG picture
    Shot { path: PathBuf },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum NoteSide {
    New,
    Old,
}

fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("dots-diff: {e:#}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli) -> Result<()> {
    match cli.command {
        None => run::run(run::Options {
            path: cli.path,
            file: cli.file,
            bind: cli.bind,
            theme: cli.look.theme(),
            graphics: cli.graphics,
            timings: cli.timings,
            settings: cli.look.settings(),
        }),
        Some(Cmd::Render {
            path,
            width,
            height,
            worktree,
            file,
            scroll,
            ansi,
            png,
            switcher,
            collapse,
            review,
            select,
            look,
        }) => {
            let frame = render::frame(&render::Frame {
                path,
                worktree,
                file,
                width,
                height,
                ansi,
                theme: look.theme().unwrap_or_else(theme::Detected::light),
                settings: look.settings(),
                scroll,
                png,
                switcher,
                collapse,
                review,
                select,
            })?;
            print!("{frame}");
            Ok(())
        }
        Some(Cmd::Ctl { block, command }) => steer(block.as_deref(), command),
        Some(Cmd::Bench {
            path,
            worktree,
            width,
            height,
            summaries,
            cold,
            fsmonitor,
        }) => {
            let report = render::bench(&render::Bench {
                path,
                worktrees: worktree,
                width,
                height,
                summaries,
                cold,
                fsmonitor,
            })?;
            print!("{report}");
            Ok(())
        }
    }
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

/// Sends a `ctl` command to the viewer it is meant for.
fn steer(block: Option<&str>, command: CtlCmd) -> Result<()> {
    let viewers = ctl::viewers();
    let (command, path) = match command {
        CtlCmd::List => {
            for v in &viewers {
                println!(
                    "{}\t{}",
                    v.status.block.as_deref().unwrap_or("-"),
                    v.status.worktree.display()
                );
            }
            return Ok(());
        }
        CtlCmd::Worktree { path } => {
            let path = std::path::absolute(path)?;
            (ctl::Command::Worktree(path.clone()), Some(path))
        }
        CtlCmd::File { path } => {
            let (path, absolute) = absolute_if_exists(path)?;
            (ctl::Command::File(path), absolute)
        }
        CtlCmd::Refresh => (ctl::Command::Refresh, None),
        CtlCmd::Note {
            file,
            line,
            end,
            side,
            clear,
            text,
        } => {
            let (file, path) = match file {
                Some(f) => {
                    let (f, p) = absolute_if_exists(f)?;
                    (Some(f), p)
                }
                None => (None, None),
            };
            if clear {
                (ctl::Command::ClearNotes(file), path)
            } else {
                let spec = ctl::NoteSpec {
                    file,
                    line,
                    end,
                    old: side == NoteSide::Old,
                    text: text.unwrap_or_default(),
                };
                (ctl::Command::Note(spec), path)
            }
        }
        CtlCmd::Bind { block } => (ctl::Command::Bind(block), None),
        CtlCmd::Shot { path } => (ctl::Command::Shot(std::path::absolute(path)?), None),
    };
    let viewer = ctl::pick(&viewers, block, path.as_deref(), repo::same_repository)?;
    ctl::send(viewer, &command)
}
