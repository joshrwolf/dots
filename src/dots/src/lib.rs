//! Agent-facing command parsing and presentation over shared domain operations.
mod agent;
mod attention;
mod attention_view;
mod command;
mod github_reviews;
mod goal_brief;
mod goal_graph;
mod goals_view;
mod json;
mod memory;
mod outputs;
mod project;
mod render;
mod review;
mod skills;
mod stream;
mod substrate;
mod tasks;
mod terminal_text;
mod watch;
mod xdg;

use std::ffi::OsString;
use std::io::{self, Write};
use std::path::PathBuf;

use anyhow::{Context as _, Result};
use clap::{CommandFactory as _, Parser as _};
use github_inspect::{Executables, GitHub, Transport};

use command::{CiCommand, Cli, Command, GitHubCommand, InspectArgs, SkillCommand};

/// Host context of one invocation, resolved by the binary and supplied by tests.
#[derive(Debug)]
pub struct Environment<'host> {
    /// Default for `--directory`; `None` resolves the process working directory.
    pub directory: Option<PathBuf>,
    /// GitHub evidence cache root; `None` disables caching.
    pub cache: Option<PathBuf>,
    /// Every `gh` and `git` query of the GitHub commands.
    pub github: &'host dyn Transport,
}

impl Environment<'static> {
    /// The process working directory, XDG cache, and installed executables.
    #[must_use]
    pub fn process() -> Self {
        Self {
            directory: None,
            cache: cache_directory(),
            github: &Executables,
        }
    }
}

impl Environment<'_> {
    fn directory(&self, selected: Option<PathBuf>) -> Result<PathBuf> {
        selected
            .or_else(|| self.directory.clone())
            .map_or_else(std::env::current_dir, Ok)
            .context("resolve working directory")
    }
}

/// A failed invocation and whether its successfully parsed arguments requested JSON.
#[derive(Debug)]
pub struct RunError {
    pub error: anyhow::Error,
    pub json: bool,
}

/// Run a process invocation, retaining the parsed output mode for error presentation.
pub fn run(
    args: impl IntoIterator<Item = OsString>,
    out: &mut dyn Write,
) -> std::result::Result<(), RunError> {
    run_with_environment(&Environment::process(), args, out)
}

/// Run one invocation, writing its output to `out`. Most commands write once at
/// the end; `dots github watch follow` streams.
pub fn run_in(
    environment: &Environment<'_>,
    args: impl IntoIterator<Item = OsString>,
    out: &mut dyn Write,
) -> Result<()> {
    run_with_environment(environment, args, out).map_err(|failure| failure.error)
}

fn run_with_environment(
    environment: &Environment<'_>,
    args: impl IntoIterator<Item = OsString>,
    out: &mut dyn Write,
) -> std::result::Result<(), RunError> {
    let arguments = std::iter::once(OsString::from("dots")).chain(args);
    let cli = match Cli::try_parse_from(arguments) {
        Ok(cli) => cli,
        Err(error)
            if matches!(
                error.kind(),
                clap::error::ErrorKind::DisplayHelp | clap::error::ErrorKind::DisplayVersion
            ) =>
        {
            return write_output(out, &error.to_string())
                .map_err(|error| RunError { error, json: false });
        }
        Err(error) => {
            return Err(RunError {
                error: error.into(),
                json: false,
            });
        }
    };
    let json = cli.json;
    let output = dispatch(environment, cli, out).map_err(|error| RunError { error, json })?;
    write_output(out, &output).map_err(|error| RunError { error, json })
}

fn write_output(out: &mut dyn Write, output: &str) -> Result<()> {
    match out.write_all(output.as_bytes()).and_then(|()| out.flush()) {
        // The reader went away; there is no one left to tell.
        Err(error) if error.kind() == io::ErrorKind::BrokenPipe => Ok(()),
        written => written.context("write output"),
    }
}

fn dispatch(environment: &Environment<'_>, cli: Cli, out: &mut dyn Write) -> Result<String> {
    let Some(command) = cli.command else {
        return Ok(Cli::command().render_long_help().to_string());
    };
    match command {
        Command::Output {
            project,
            owner,
            command,
        } => outputs::run(
            command,
            project.as_deref(),
            owner.as_deref(),
            &environment.directory(cli.directory)?,
            cli.json,
        ),
        Command::Project { command } => {
            project::run(command, &environment.directory(cli.directory)?)
        }
        Command::Memory { project, command } => {
            let directory = cli
                .directory
                .map_or_else(std::env::current_dir, Ok)
                .context("resolve working directory")?;
            memory::run(command, &directory, project.as_deref(), cli.json)
        }
        Command::Substrate { command } => substrate::run(command),
        Command::Task {
            project,
            owner,
            command,
        } => {
            let directory = environment.directory(cli.directory)?;
            tasks::run(tasks::Request {
                command,
                project,
                owner: owner.as_deref(),
                directory: &directory,
                json: cli.json,
                out,
            })
        }
        Command::Attention {
            project,
            owner,
            command,
        } => attention::run(attention::Invocation {
            command,
            project,
            owner: owner.as_deref(),
            directory: &environment.directory(cli.directory)?,
            json: cli.json,
        }),
        Command::Review { command } => {
            review::run(command, &environment.directory(cli.directory)?, cli.json)
        }
        Command::Skills {
            command: SkillCommand::Render { reference },
        } => Ok(if reference {
            skills::reference()
        } else {
            skills::skill()
        }),
        Command::Skills {
            command: SkillCommand::Generate { output },
        } => skills::generate(&output),
        Command::Skills {
            command: SkillCommand::Check { output },
        } => skills::check(&output),
        Command::Github {
            repo,
            no_cache,
            command,
        } => {
            let directory = environment.directory(cli.directory)?;
            let provider = || {
                let provider = GitHub::with_transport(&directory, environment.github);
                match &environment.cache {
                    Some(cache) if !no_cache => provider.with_cache(cache),
                    _ => provider,
                }
            };
            match command {
                GitHubCommand::Ci(args) => ci(&provider(), *args, repo.as_ref(), cli.json),
                GitHubCommand::Reviews(args) => {
                    github_reviews::run(&provider(), *args, repo.as_ref(), cli.json)
                }
                GitHubCommand::Watch { command } => {
                    anyhow::ensure!(
                        repo.is_none() && !no_cache,
                        "the watch list names PRs by URL and keeps its own cache; --repo and --no-cache do not apply"
                    );
                    watch(environment, &directory, command, cli.json, out)
                }
            }
        }
    }
}

/// `dots github watch`: resolve the store, sessions, clock, and process
/// lifetime for the domain.
fn watch(
    environment: &Environment<'_>,
    directory: &std::path::Path,
    command: command::WatchCommand,
    json: bool,
    out: &mut dyn Write,
) -> Result<String> {
    if matches!(command, command::WatchCommand::Follow) {
        stream::exit_on_signals()?;
    }
    let store = xdg::base("XDG_STATE_HOME", ".local/state")?.join(github_watch::STORE_PATH);
    let session = agent::session();
    let rex_session = agent::rex_session();
    watch::run(&mut watch::Request {
        command,
        store: &store,
        gh: &watch::Gh {
            transport: environment.github,
            directory,
        },
        directory,
        session: session.as_deref(),
        rex_session: rex_session.as_deref(),
        clock: &|| time::OffsetDateTime::now_utc().unix_timestamp(),
        json,
        out,
        sleep: &mut stream::sleep_while_read,
    })
}

fn ci(
    provider: &GitHub<'_>,
    args: command::CiArgs,
    repository: Option<&github_inspect::Repository>,
    json: bool,
) -> Result<String> {
    match args.command {
        None => inspect(provider, &args.inspect, repository, json),
        Some(CiCommand::Inspect(args)) => inspect(provider, &args, repository, json),
        Some(CiCommand::Output(args)) => {
            let result = github_inspect::check_output_target(
                provider,
                &args.reference,
                repository,
                github_inspect::OutputOptions {
                    snapshot: args.snapshot.as_ref(),
                    start: args.start,
                    lines: usize::from(args.lines.unwrap_or(if args.search.is_some() {
                        20
                    } else {
                        40
                    })),
                    search: args.search.as_deref(),
                    annotations: args.annotations,
                    annotation_page: args.annotation_page as usize,
                },
            )?;
            if json {
                Ok(json::render(serde_json::json!({"evidence":result})))
            } else {
                Ok(render::check_output(&result))
            }
        }
        Some(CiCommand::Logs(args)) => {
            let lines =
                usize::from(
                    args.lines
                        .unwrap_or(if args.search.is_some() { 20 } else { 100 }),
                );
            let result = github_inspect::job_logs(
                provider,
                &args.reference,
                repository,
                github_inspect::LogOptions {
                    snapshot: args.snapshot.as_ref(),
                    start: args.start,
                    lines,
                    search: args.search.as_deref(),
                    test: args.test.as_deref(),
                },
            )?;
            if json {
                Ok(json::render(serde_json::json!({"evidence":result})))
            } else {
                Ok(render::evidence(&result))
            }
        }
    }
}

fn inspect(
    provider: &GitHub<'_>,
    args: &InspectArgs,
    repository: Option<&github_inspect::Repository>,
    json: bool,
) -> Result<String> {
    let limit = usize::from(args.limit);
    let options = github_inspect::InspectOptions {
        include_evidence: !args.no_logs,
        offset: args.offset,
        limit,
        show_all: args.all,
        name: args.name.as_deref(),
    };
    let result = if let Some(reference) = &args.snapshot {
        github_inspect::inspect_snapshot_filtered(provider, reference, options)?
    } else {
        let target = if let Some(value) = &args.run {
            github_inspect::Target::Run {
                value,
                attempt: args.attempt,
            }
        } else if let Some(value) = &args.git_ref {
            github_inspect::Target::Ref(value)
        } else if let Some(value) = &args.sha {
            github_inspect::Target::Commit(value)
        } else {
            github_inspect::Target::PullRequest(args.pr.as_deref())
        };
        github_inspect::inspect_target(provider, target, repository, options)?
    };
    if json {
        Ok(json::render(serde_json::to_value(&result)?))
    } else {
        Ok(render::inspection(&result))
    }
}

fn cache_directory() -> Option<PathBuf> {
    let root = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")))?;
    Some(root.join("dots/github"))
}

#[cfg(test)]
mod tests {
    use super::*;

    struct ClosedPipe;

    impl Write for ClosedPipe {
        fn write(&mut self, _: &[u8]) -> io::Result<usize> {
            Err(io::ErrorKind::BrokenPipe.into())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn help_and_output_ignore_closed_readers() {
        for args in [vec!["--help"], vec!["skills", "render"]] {
            run_in(
                &Environment::process(),
                args.into_iter().map(OsString::from),
                &mut ClosedPipe,
            )
            .expect("closed reader is not a command failure");
        }
    }
}
