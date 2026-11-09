//! The `dots-diff` command line.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand, ValueEnum};
use dots_diff::{Ctl, Look, Note, Options};

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

impl From<CtlCmd> for Ctl {
    fn from(command: CtlCmd) -> Self {
        match command {
            CtlCmd::Worktree { path } => Self::Worktree(path),
            CtlCmd::File { path } => Self::File(path),
            CtlCmd::Refresh => Self::Refresh,
            CtlCmd::List => Self::List,
            CtlCmd::Note {
                file, clear: true, ..
            } => Self::ClearNotes(file),
            CtlCmd::Note {
                file,
                line,
                end,
                side,
                text,
                ..
            } => Self::Note(Note {
                file,
                line,
                end,
                old: side == NoteSide::Old,
                text: text.unwrap_or_default(),
            }),
            CtlCmd::Bind { block } => Self::Bind(block),
            CtlCmd::Shot { path } => Self::Shot(path),
        }
    }
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
        Some(Cmd::Ctl { block, command }) => dots_diff::ctl(block.as_deref(), command.into()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("dots-diff: {e:#}");
            ExitCode::FAILURE
        }
    }
}
