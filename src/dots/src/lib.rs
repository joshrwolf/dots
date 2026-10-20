//! Agent-facing command parsing and presentation over shared domain operations.
mod command;
mod herdr;
mod json;
mod memory;
mod render;
mod reviews;
mod skills;
mod tasks;

use std::ffi::OsString;

use anyhow::{Context as _, Result};
use clap::{CommandFactory as _, Parser as _};
use github_inspect::GitHub;

use command::{CiCommand, Cli, Command, GitHubCommand, InspectArgs, SkillCommand};

pub fn run(args: impl IntoIterator<Item = OsString>) -> Result<String> {
    let arguments = std::iter::once(OsString::from("dots")).chain(args);
    let cli = match Cli::try_parse_from(arguments) {
        Ok(cli) => cli,
        Err(error)
            if matches!(
                error.kind(),
                clap::error::ErrorKind::DisplayHelp | clap::error::ErrorKind::DisplayVersion
            ) =>
        {
            return Ok(error.to_string());
        }
        Err(error) => return Err(error.into()),
    };
    let Some(command) = cli.command else {
        return Ok(Cli::command().render_long_help().to_string());
    };
    match command {
        Command::Memory { command } => {
            anyhow::ensure!(
                cli.repo.is_none() && !cli.no_cache,
                "memory is local; --repo and --no-cache do not apply"
            );
            let directory = cli
                .directory
                .map_or_else(std::env::current_dir, Ok)
                .context("resolve working directory")?;
            memory::run(command, &directory, cli.json)
        }
        Command::Herdr { command } => {
            anyhow::ensure!(
                cli.repo.is_none() && cli.directory.is_none() && !cli.no_cache,
                "Herdr associations use --workspace and the invoking server; --repo, --directory and --no-cache do not apply"
            );
            herdr::run(command, cli.json)
        }
        Command::Task {
            project,
            owner,
            command,
        } => {
            anyhow::ensure!(
                cli.repo.is_none() && !cli.no_cache,
                "tasks are local to the project checkout; --repo and --no-cache do not apply"
            );
            let directory = cli
                .directory
                .map_or_else(std::env::current_dir, Ok)
                .context("resolve working directory")?;
            tasks::run(tasks::Request {
                command,
                project,
                owner: owner.as_deref(),
                directory: &directory,
                json: cli.json,
            })
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
        } => skills::publish(&output, false),
        Command::Skills {
            command: SkillCommand::Check { output },
        } => skills::publish(&output, true),
        Command::Github { command } => {
            let directory = cli
                .directory
                .map_or_else(std::env::current_dir, Ok)
                .context("resolve working directory")?;
            let mut provider = GitHub::new(&directory);
            if !cli.no_cache
                && let Some(cache) = cache_directory()
            {
                provider = provider.with_cache(&cache);
            }
            match command {
                GitHubCommand::Ci(args) => ci(&provider, *args, cli.repo.as_ref(), cli.json),
                GitHubCommand::Reviews(args) => {
                    reviews::run(&provider, *args, cli.repo.as_ref(), cli.json)
                }
            }
        }
    }
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

fn cache_directory() -> Option<std::path::PathBuf> {
    let root = std::env::var_os("XDG_CACHE_HOME")
        .map(std::path::PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| {
            std::env::var_os("HOME").map(|home| std::path::PathBuf::from(home).join(".cache"))
        })?;
    Some(root.join("dots/github"))
}
