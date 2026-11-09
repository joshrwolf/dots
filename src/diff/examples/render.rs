//! Prints one frame of the viewer as text, and optionally as a PNG, without
//! a terminal: `cargo run --example render --features dev -- PATH`.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;
use dots_diff::Look;
use dots_diff::dev::{Frame, frame};

#[derive(Debug, Parser)]
struct Args {
    /// A path inside the repository
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
    /// Mark moved code
    #[arg(long)]
    moves: bool,
    #[command(flatten)]
    look: Look,
}

fn main() -> ExitCode {
    let args = Args::parse();
    let drawn = frame(&Frame {
        path: args.path,
        worktree: args.worktree,
        file: args.file,
        width: args.width,
        height: args.height,
        ansi: args.ansi,
        look: args.look,
        scroll: args.scroll,
        png: args.png,
        switcher: args.switcher,
        collapse: args.collapse,
        review: args.review,
        select: args.select,
        moves: args.moves,
    });
    match drawn {
        Ok(text) => {
            print!("{text}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("render: {e:#}");
            ExitCode::FAILURE
        }
    }
}
