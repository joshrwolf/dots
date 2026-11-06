//! Hosts a picker view in a Rex terminal. A Lua action describes the view.
//! Enter sends the chosen row's id to the view's accept action and a shortcut
//! sends it to its own, with the typed line for a shortcut that takes it. The
//! action runs through `rex do` while the picker stays open: success closes
//! it, and a failure shows Lua's message in it. The view's watch command says
//! when to read it again. Closing it without choosing runs the `--dismiss`
//! action, if any, while it still shows. Rex semantics stay in Lua: this
//! program only runs the commands a view names and `rex do`.

use std::io::{self, BufRead as _, Write as _};
use std::process::{Command, ExitCode, Output};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context as _, Result, bail};
use clap::Parser;
use dots_pick::spec::{self, Groups, Prepared, Row, View};
use dots_pick::{Act, Actions, Choice, Closed, Fetch, Palette};

/// Config actions get 30 seconds. `rex do` waits that long for one; the
/// process deadline leaves it room to report a timeout itself.
const ACTION_TIMEOUT: &str = "30s";
const ACTION_DEADLINE: Duration = Duration::from_secs(35);
const VIEW_LIMIT: usize = 16 * 1024 * 1024;

#[derive(Debug, Parser)]
#[command(
    name = "dots-pick",
    version,
    about = "Run a picker view described by a Rex action"
)]
struct Cli {
    /// The Lua action that returns the view, such as `go_view`
    source: String,
    /// Arguments for the source action, as a JSON object
    #[arg(long, value_name = "JSON", default_value = "{}")]
    args: String,
    /// The Lua action to run when the picker closes without choosing
    #[arg(long, value_name = "ACTION")]
    dismiss: Option<String>,
}

fn main() -> ExitCode {
    let args = Cli::parse();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            // The picker could not be drawn or dismissed, and the view closes
            // when this exits, which would take the message with it. Hold it
            // until the user has read it.
            eprintln!("dots-pick: {error:#}");
            eprint!("Press Enter to close.");
            let _ = io::stderr().flush();
            let _ = io::stdin().lock().read_line(&mut String::new());
            ExitCode::FAILURE
        }
    }
}

fn run(args: &Cli) -> Result<()> {
    let json = output(&args.source, &args.args)?;
    let view =
        View::parse(&json).with_context(|| format!("reading the view from {}", args.source))?;
    let Prepared {
        mut picker,
        shortcuts,
        accept,
        watch,
        groups,
    } = view
        .prepare(Palette::load()?)
        .with_context(|| format!("preparing the view from {}", args.source))?;
    if let Some(watch) = watch {
        picker = picker.live(watch, fetch(args, groups));
    }
    let act: Act<Row, String> = Box::new(move |choice| {
        let (action, args) = match choice {
            Choice::Accept(row) => (accept.clone(), spec::arguments(Some(row), None)),
            Choice::Shortcut(action, chosen) => {
                (action.clone(), spec::arguments(chosen.entry, chosen.line))
            }
        };
        Box::pin(async move {
            let result = runner().run(rex_do(&action, &args.to_string())).await;
            acted(&action, result).map(|()| dots_pick::Action::Close)
        })
    });
    let closed = picker.run_actions(&Actions { shortcuts, act })?;
    if let (Closed::Cancelled, Some(dismiss)) = (closed, &args.dismiss) {
        output(dismiss, "{}")?;
    }
    Ok(())
}

/// Reads the view again while the picker runs, keeping its groups.
fn fetch(args: &Cli, groups: Groups) -> Fetch<Row> {
    let (source, source_args) = (args.source.clone(), args.args.clone());
    let groups = Arc::new(groups);
    Arc::new(move || {
        let (source, source_args, groups) =
            (source.clone(), source_args.clone(), Arc::clone(&groups));
        Box::pin(async move {
            let result = runner().run(rex_do(&source, &source_args)).await;
            let json = printed(&source, result).map_err(|error| format!("{error:#}"))?;
            let view = View::parse(&json).map_err(|error| format!("{error:#}"))?;
            groups.rows(view.rows).map_err(|error| format!("{error:#}"))
        })
    })
}

/// Runs a Lua action and returns what it printed as JSON.
fn output(action: &str, args: &str) -> Result<String> {
    printed(action, runner().output(rex_do(action, args)))
}

fn rex_do(action: &str, args: &str) -> Command {
    let mut command = Command::new("rex");
    command.args([
        "--timeout",
        ACTION_TIMEOUT,
        "do",
        action,
        "--json",
        "--args",
        args,
    ]);
    command
}

fn runner() -> dots_process::Runner {
    dots_process::Runner::new(ACTION_DEADLINE, 64 * 1024).stdout_limit(VIEW_LIMIT)
}

fn printed(action: &str, result: Result<Output, dots_process::Error>) -> Result<String> {
    let output = result.with_context(|| format!("running rex do {action}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stderr = stderr.trim();
        if stderr.is_empty() {
            bail!("rex do {action} failed ({})", output.status);
        }
        bail!("rex do {action} failed ({}): {stderr}", output.status);
    }
    String::from_utf8(output.stdout)
        .with_context(|| format!("rex do {action} printed invalid UTF-8"))
}

/// Whether an action succeeded, or what to tell the user: the Lua error
/// alone, without the `Error: ` that `rex` puts before it.
fn acted(action: &str, result: Result<Output, dots_process::Error>) -> Result<(), String> {
    let output = result.map_err(|error| {
        format!(
            "{:#}",
            anyhow::Error::new(error).context(format!("running rex do {action}"))
        )
    })?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stderr = stderr.trim();
    let message = stderr
        .strip_prefix("Error:")
        .map_or(stderr, str::trim_start);
    if message.is_empty() {
        return Err(format!("rex do {action} failed ({})", output.status));
    }
    Err(message.to_owned())
}

#[cfg(test)]
mod tests {
    use std::os::unix::process::ExitStatusExt as _;
    use std::process::ExitStatus;

    use super::*;

    fn acted_with(code: i32, stderr: &str) -> Result<(), String> {
        let output = Output {
            status: ExitStatus::from_raw(code << 8),
            stdout: Vec::new(),
            stderr: stderr.as_bytes().to_vec(),
        };
        acted("go_start_pr", Ok(output))
    }

    #[test]
    fn a_successful_action_closes_the_picker() {
        assert_eq!(acted_with(0, ""), Ok(()));
    }

    #[test]
    fn a_failed_action_says_what_lua_said() {
        let message =
            "pr:3 is not a GitHub PR URL (such as https://github.com/owner/repo/pull/123)";
        assert_eq!(
            acted_with(1, &format!("Error: {message}\n")),
            Err(message.to_owned())
        );
        assert_eq!(
            acted_with(1, "dots/go.lua:33: attempt to index a nil value"),
            Err("dots/go.lua:33: attempt to index a nil value".to_owned()),
            "anything else stays as it is"
        );
    }

    #[test]
    fn a_silent_failure_names_the_action() {
        assert_eq!(
            acted_with(1, "Error: \n"),
            Err("rex do go_start_pr failed (exit status: 1)".to_owned())
        );
        let timeout = acted("go_to", Err(dots_process::Error::Timeout(ACTION_DEADLINE)));
        assert_eq!(
            timeout,
            Err("running rex do go_to: subprocess exceeded its 35s deadline".to_owned())
        );
    }
}
