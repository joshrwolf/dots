//! A terminal diff viewer for every worktree of a repository: each
//! worktree's branch against its merge base with the default branch, as one
//! scrolling stream of files.

mod app;
mod ctl;
mod diff;
mod graphics;
mod highlight;
mod input;
mod plan;
mod render;
mod repo;
mod run;
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
        #[command(flatten)]
        look: Look,
    },
    /// Steer a running viewer
    Ctl {
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
    },
}

#[derive(Debug, Subcommand)]
enum CtlCmd {
    /// Show the worktree containing PATH
    Worktree { path: PathBuf },
    /// Scroll to a file
    File { path: String },
    /// Reload now
    Refresh,
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
            })?;
            print!("{frame}");
            Ok(())
        }
        Some(Cmd::Ctl { command }) => ctl::send(&match command {
            CtlCmd::Worktree { path } => ctl::Command::Worktree(std::path::absolute(path)?),
            CtlCmd::File { path } => ctl::Command::File(path),
            CtlCmd::Refresh => ctl::Command::Refresh,
        }),
        Some(Cmd::Bench {
            path,
            worktree,
            width,
            height,
            summaries,
        }) => {
            let report = render::bench(&render::Bench {
                path,
                worktrees: worktree,
                width,
                height,
                summaries,
            })?;
            print!("{report}");
            Ok(())
        }
    }
}
