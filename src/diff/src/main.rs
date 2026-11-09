//! The `dots-diff` command line.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use dots_diff::{Ctl, Look, Options};

#[derive(Debug, Parser)]
#[command(
    name = "dots-diff",
    version,
    about = "Review the changes in every worktree of a repository",
    long_about = "Review the changes in every worktree of a repository.\n\n\
        The switcher (w) lists worktrees whose Pi worker asks a question \
        first, then those waiting for review: changes not looked at since \
        they last changed, with no Pi worker still at them. Each row names \
        the Pi worker or interactive Pi session working there and the \
        worker's status, read from Pi's worker registry \
        ($PI_CODING_AGENT_DIR, else ~/.pi/agent) and its inbox registrations."
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
    #[command(flatten)]
    look: Look,
}

#[derive(Debug, Subcommand)]
enum Cmd {
    /// Steer a running viewer
    Ctl {
        /// The viewer in this Rex pane; by default the one showing the
        /// path's repository, or the only one running
        #[arg(long, global = true, value_name = "ID")]
        block: Option<String>,
        #[command(subcommand)]
        command: CtlCmd,
    },
}

#[derive(Debug, Subcommand)]
enum CtlCmd {
    /// List running viewers: their Rex pane and worktree
    List,
    #[command(flatten)]
    Viewer(Ctl),
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let result = match cli.command {
        None => dots_diff::run(Options {
            path: cli.path,
            file: cli.file,
            bind: cli.bind,
            look: cli.look,
        }),
        Some(Cmd::Ctl {
            command: CtlCmd::List,
            ..
        }) => {
            dots_diff::list();
            Ok(())
        }
        Some(Cmd::Ctl {
            block,
            command: CtlCmd::Viewer(command),
        }) => dots_diff::ctl(block.as_deref(), command),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("dots-diff: {e:#}");
            ExitCode::FAILURE
        }
    }
}
