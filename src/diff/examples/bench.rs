//! Measures loading, diffing, highlighting and drawing in-process against
//! real worktrees, and the process's memory:
//! `cargo run --release --example bench --features dev -- PATH`.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;
use dots_diff::dev::{Bench, bench};

#[derive(Debug, Parser)]
struct Args {
    /// A path inside the repository
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
}

fn main() -> ExitCode {
    let args = Args::parse();
    let report = bench(&Bench {
        path: args.path,
        worktrees: args.worktree,
        width: args.width,
        height: args.height,
        summaries: args.summaries,
        cold: args.cold,
    });
    match report {
        Ok(text) => {
            print!("{text}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("bench: {e:#}");
            ExitCode::FAILURE
        }
    }
}
