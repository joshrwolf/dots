//! Bounded `git`/`gh` execution and GitHub pull-request and repository identity
//! parsing for `dots github`.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus};
use std::time::Duration;

use serde::Serialize;

const COMMAND_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_COMMAND_OUTPUT_BYTES: usize = 2 * 1024 * 1024;

/// Validated canonical identity of a github.com PR, requiring no GitHub
/// request; serializes as its canonical URL.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct PullRequestUrl {
    url: String,
    #[serde(skip)]
    owner: String,
    #[serde(skip)]
    repository: String,
    #[serde(skip)]
    number: u64,
}

impl PullRequestUrl {
    /// Accepts any https github.com PR URL form `Target` accepts and
    /// canonicalizes it: lowercase owner and repository, no suffix.
    pub fn parse(value: &str) -> Result<Self> {
        if value.len() > 256 {
            return Err(Error::InvalidUrl { url: value.into() });
        }
        let identity = parse_pull_request_url(value)?;
        if identity.scheme != "https" || identity.host != "github.com" {
            return Err(Error::InvalidUrl { url: value.into() });
        }
        let owner = identity.owner.to_ascii_lowercase();
        let repository = identity.repository.to_ascii_lowercase();
        Ok(Self {
            url: format!(
                "https://github.com/{owner}/{repository}/pull/{}",
                identity.number
            ),
            owner,
            repository,
            number: identity.number,
        })
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.url
    }

    #[must_use]
    pub fn owner(&self) -> &str {
        &self.owner
    }

    #[must_use]
    pub fn repository(&self) -> &str {
        &self.repository
    }

    #[must_use]
    pub fn number(&self) -> u64 {
        self.number
    }
}

/// A pull request number or URL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    Number(u64),
    Url(String),
}

impl Target {
    /// Parses a pull request number, `#number`, or full pull request URL.
    pub fn parse(value: &str) -> Result<Self> {
        if value.starts_with("https://") || value.starts_with("http://") {
            parse_pull_request_url(value)?;
            return Ok(Self::Url(value.to_owned()));
        }
        let number = value
            .strip_prefix('#')
            .unwrap_or(value)
            .parse::<u64>()
            .map_err(|_| Error::InvalidUrl {
                url: value.to_owned(),
            })?;
        if number == 0 {
            return Err(Error::InvalidNumber);
        }
        Ok(Self::Number(number))
    }

    /// The repository named by a URL target, before any checkout is selected.
    pub fn repository(&self) -> Result<Option<GitHubRepository>> {
        validate_target(self).map(|identity| {
            identity.map(|identity| GitHubRepository::from(identity.repository_identity()))
        })
    }
}

/// A stateless client for bounded `git` and `gh` commands.
#[derive(Debug, Clone, Copy, Default)]
pub struct Client;

impl Client {
    /// Run a Git query with bounded output and process-group teardown.
    pub fn git(&self, checkout: &Path, args: &[&str]) -> Result<std::process::Output> {
        SystemRunner.output(
            checkout,
            "git",
            &args.iter().map(OsString::from).collect::<Vec<_>>(),
        )
    }

    /// Run a GitHub CLI query with a caller's remaining operation budget,
    /// capped at the normal deadline. Ambient `GH_REPO` overrides are removed.
    pub fn gh_with_timeout(
        &self,
        checkout: &Path,
        args: &[&str],
        timeout: Duration,
    ) -> Result<std::process::Output> {
        command_with_timeout(
            checkout,
            "gh",
            &args.iter().map(OsString::from).collect::<Vec<_>>(),
            timeout.min(COMMAND_TIMEOUT),
        )
    }

    /// Spool raw job logs to a caller-owned file, capped at 512 MiB.
    /// Metadata and stderr retain their normal bounds; no whole log body is buffered.
    /// This synchronous API must run outside a Tokio runtime. On timeout, discard
    /// the failed capture: one in-flight sink write may still complete afterward.
    pub fn gh_log_to_file(
        &self,
        checkout: &Path,
        args: &[&str],
        timeout: Duration,
        destination: std::fs::File,
    ) -> Result<std::process::Output> {
        command_with_destination(
            checkout,
            "gh",
            &args.iter().map(OsString::from).collect::<Vec<_>>(),
            timeout.min(COMMAND_TIMEOUT),
            512 * 1024 * 1024,
            Some(destination),
        )
    }
}

/// The GitHub repository identities named by configured Git remotes, over a caller's own `git` transport,
/// which receives each argument list and answers for the intended checkout.
pub fn repositories_via(
    git: impl Fn(&[&str]) -> Result<CommandOutput>,
) -> Result<Vec<GitHubRepository>> {
    repositories_with(&GitVia(git), Path::new(""))
}

struct GitVia<F>(F);

impl<F: Fn(&[&str]) -> Result<CommandOutput>> Runner for GitVia<F> {
    fn output(&self, _cwd: &Path, _program: &str, args: &[OsString]) -> Result<CommandOutput> {
        // Arguments originate as `&str` in `git_text`, so the round trip is lossless.
        let args = args
            .iter()
            .map(|argument| argument.to_string_lossy())
            .collect::<Vec<_>>();
        (self.0)(&args.iter().map(AsRef::as_ref).collect::<Vec<_>>())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("pull request number must be greater than zero")]
    InvalidNumber,

    #[error("pull request URL is invalid: {url:?}")]
    InvalidUrl { url: String },

    #[error("checkout repository URL is invalid: {url:?}")]
    InvalidRepositoryUrl { url: String },

    #[error("could not start {program} in {cwd}")]
    StartCommand {
        program: String,
        cwd: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("could not inspect {program} while it ran")]
    InspectCommand {
        program: String,
        #[source]
        source: std::io::Error,
    },

    #[error("{program} exceeded its {timeout:?} deadline")]
    CommandTimedOut { program: String, timeout: Duration },

    #[error("{program} produced more than {limit} bytes on {stream}")]
    CommandOutputTooLarge {
        program: String,
        stream: &'static str,
        limit: usize,
    },

    #[error("could not read {program} {stream}")]
    ReadCommandOutput {
        program: String,
        stream: &'static str,
        #[source]
        source: std::io::Error,
    },

    #[error("starting the command runtime")]
    CommandRuntime {
        #[source]
        source: std::io::Error,
    },

    #[error("{program} failed ({status}): {stderr}")]
    CommandFailed {
        program: String,
        status: ExitStatus,
        stderr: String,
    },

    #[error("{program} returned non-UTF-8 output")]
    NonUtf8Output { program: String },
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Clone, PartialEq, Eq)]
struct RepositoryIdentity {
    host: String,
    owner: String,
    repository: String,
}

/// A provider identity independent of any particular local clone.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct GitHubRepository {
    host: String,
    owner: String,
    repository: String,
}

impl GitHubRepository {
    #[must_use]
    pub fn display(&self) -> String {
        format!("{}/{}/{}", self.host, self.owner, self.repository)
    }

    #[must_use]
    pub fn same_repository(&self, other: &Self) -> bool {
        self.host.eq_ignore_ascii_case(&other.host)
            && self.owner.eq_ignore_ascii_case(&other.owner)
            && self.repository.eq_ignore_ascii_case(&other.repository)
    }
}

impl From<RepositoryIdentity> for GitHubRepository {
    fn from(value: RepositoryIdentity) -> Self {
        Self {
            host: value.host,
            owner: value.owner,
            repository: value.repository,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct UrlIdentity {
    scheme: String,
    host: String,
    owner: String,
    repository: String,
    number: u64,
}

impl UrlIdentity {
    fn repository_identity(&self) -> RepositoryIdentity {
        RepositoryIdentity {
            host: self.host.clone(),
            owner: self.owner.clone(),
            repository: self.repository.clone(),
        }
    }
}

trait Runner {
    fn output(&self, cwd: &Path, program: &str, args: &[OsString]) -> Result<CommandOutput>;
}

#[derive(Debug, Clone, Copy)]
struct SystemRunner;

impl Runner for SystemRunner {
    fn output(&self, cwd: &Path, program: &str, args: &[OsString]) -> Result<CommandOutput> {
        command_with_timeout(cwd, program, args, COMMAND_TIMEOUT)
    }
}

type CommandOutput = std::process::Output;

fn repositories_with(runner: &impl Runner, checkout: &Path) -> Result<Vec<GitHubRepository>> {
    let remotes = git_text(runner, checkout, &["remote"])?;
    let mut repositories = Vec::new();
    for remote in remotes.lines().filter(|remote| !remote.is_empty()) {
        let urls = git_text(runner, checkout, &["remote", "get-url", "--all", remote])?;
        for url in urls.lines().filter(|url| !url.is_empty()) {
            let Some(repository) = parse_remote_url(url) else {
                continue;
            };
            if !repositories
                .iter()
                .any(|known: &GitHubRepository| known.same_repository(&repository))
            {
                repositories.push(repository);
            }
        }
    }
    Ok(repositories)
}

fn validate_target(target: &Target) -> Result<Option<UrlIdentity>> {
    match target {
        Target::Number(0) => Err(Error::InvalidNumber),
        Target::Number(_) => Ok(None),
        Target::Url(url) => parse_pull_request_url(url).map(Some),
    }
}

fn parse_pull_request_url(url: &str) -> Result<UrlIdentity> {
    let (scheme, remainder) = url.split_once("://").ok_or_else(|| Error::InvalidUrl {
        url: url.to_owned(),
    })?;
    if !matches!(scheme, "https" | "http") || remainder.chars().any(char::is_whitespace) {
        return Err(Error::InvalidUrl {
            url: url.to_owned(),
        });
    }
    let path = remainder
        .split_once(['?', '#'])
        .map_or(remainder, |(path, _)| path);
    let mut parts = path.split('/');
    let host = parts.next().unwrap_or_default();
    let owner = parts.next().unwrap_or_default();
    let repository = parts.next().unwrap_or_default();
    let pull = parts.next().unwrap_or_default();
    let number = parts.next().unwrap_or_default();
    let suffix = parts.next().filter(|part| !part.is_empty());
    if host.is_empty()
        || owner.is_empty()
        || repository.is_empty()
        || pull != "pull"
        || suffix.is_some_and(|part| !matches!(part, "files" | "commits" | "checks"))
        || parts.any(|part| !part.is_empty())
        || !safe_url_component(host, true)
        || !safe_url_component(owner, false)
        || !safe_url_component(repository, false)
    {
        return Err(Error::InvalidUrl {
            url: url.to_owned(),
        });
    }
    let number = number
        .parse::<u64>()
        .ok()
        .filter(|number| *number > 0)
        .ok_or_else(|| Error::InvalidUrl {
            url: url.to_owned(),
        })?;
    Ok(UrlIdentity {
        scheme: scheme.to_owned(),
        host: host.to_ascii_lowercase(),
        owner: owner.to_owned(),
        repository: repository.to_owned(),
        number,
    })
}

fn parse_repository_url(url: &str) -> Result<RepositoryIdentity> {
    let (scheme, remainder) = url
        .split_once("://")
        .ok_or_else(|| Error::InvalidRepositoryUrl {
            url: url.to_owned(),
        })?;
    if !matches!(scheme, "https" | "http")
        || remainder.contains(['?', '#', '@'])
        || remainder.chars().any(char::is_whitespace)
    {
        return Err(Error::InvalidRepositoryUrl {
            url: url.to_owned(),
        });
    }
    let mut parts = remainder.split('/');
    let host = parts.next().unwrap_or_default();
    let owner = parts.next().unwrap_or_default();
    let raw_repository = parts.next().unwrap_or_default();
    let repository = raw_repository
        .strip_suffix(".git")
        .unwrap_or(raw_repository);
    if host.is_empty()
        || owner.is_empty()
        || repository.is_empty()
        || parts.any(|part| !part.is_empty())
        || !safe_url_component(host, true)
        || !safe_url_component(owner, false)
        || !safe_url_component(repository, false)
    {
        return Err(Error::InvalidRepositoryUrl {
            url: url.to_owned(),
        });
    }
    Ok(RepositoryIdentity {
        host: host.to_ascii_lowercase(),
        owner: owner.to_owned(),
        repository: repository.to_owned(),
    })
}

fn parse_remote_url(url: &str) -> Option<GitHubRepository> {
    let identity = if url.starts_with("http://") || url.starts_with("https://") {
        parse_repository_url(url).ok()?
    } else if let Some(remainder) = url.strip_prefix("ssh://") {
        let remainder = remainder
            .rsplit_once('@')
            .map_or(remainder, |(_, tail)| tail);
        parse_remote_path(remainder, '/')?
    } else {
        let (_, remainder) = url.split_once('@')?;
        parse_remote_path(remainder, ':')?
    };
    Some(identity.into())
}

fn parse_remote_path(value: &str, separator: char) -> Option<RepositoryIdentity> {
    let (host, path) = value.split_once(separator)?;
    let mut parts = path.trim_start_matches('/').split('/');
    let owner = parts.next()?;
    let raw_repository = parts.next()?;
    if host.is_empty() || owner.is_empty() || raw_repository.is_empty() || parts.next().is_some() {
        return None;
    }
    let repository = raw_repository
        .strip_suffix(".git")
        .unwrap_or(raw_repository);
    if !safe_url_component(host, true)
        || !safe_url_component(owner, false)
        || !safe_url_component(repository, false)
    {
        return None;
    }
    Some(RepositoryIdentity {
        host: host.to_ascii_lowercase(),
        owner: owner.to_owned(),
        repository: repository.to_owned(),
    })
}

fn safe_url_component(value: &str, host: bool) -> bool {
    value.bytes().all(|byte| {
        byte.is_ascii_alphanumeric()
            || matches!(byte, b'-' | b'.')
            || (!host && byte == b'_')
            || (host && byte == b':')
    })
}

fn git_text(runner: &impl Runner, checkout: &Path, args: &[&str]) -> Result<String> {
    let args = args.iter().map(OsString::from).collect::<Vec<_>>();
    let bytes = successful_output(runner, checkout, "git", &args)?;
    String::from_utf8(bytes)
        .map_err(|_| Error::NonUtf8Output {
            program: "git".to_owned(),
        })
        .map(|value| value.trim().to_owned())
}

fn successful_output(
    runner: &impl Runner,
    cwd: &Path,
    program: &str,
    args: &[OsString],
) -> Result<Vec<u8>> {
    let output = runner.output(cwd, program, args)?;
    if output.status.success() {
        return Ok(output.stdout);
    }
    Err(Error::CommandFailed {
        program: command_description(program, args),
        status: output.status,
        stderr: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
    })
}

fn command_description(program: &str, args: &[OsString]) -> String {
    let mut description = program.to_owned();
    for argument in args {
        description.push(' ');
        description.push_str(&argument.to_string_lossy());
    }
    description
}

fn command_with_timeout(
    cwd: &Path,
    program: &str,
    args: &[OsString],
    timeout: Duration,
) -> Result<CommandOutput> {
    command_with_limits(cwd, program, args, timeout, MAX_COMMAND_OUTPUT_BYTES)
}

fn command_with_limits(
    cwd: &Path,
    program: &str,
    args: &[OsString],
    timeout: Duration,
    stdout_limit: usize,
) -> Result<CommandOutput> {
    command_with_destination(cwd, program, args, timeout, stdout_limit, None)
}

fn command_with_destination(
    cwd: &Path,
    program: &str,
    args: &[OsString],
    timeout: Duration,
    stdout_limit: usize,
    destination: Option<std::fs::File>,
) -> Result<CommandOutput> {
    let mut runner =
        dots_process::Runner::new(timeout, MAX_COMMAND_OUTPUT_BYTES).stdout_limit(stdout_limit);
    if let Some(destination) = destination {
        runner = runner.stdout_to(destination);
    }
    runner
        .output(configured_command(cwd, program, args))
        .map_err(|error| command_error(error, cwd, program, args))
}

fn command_error(
    error: dots_process::Error,
    cwd: &Path,
    program: &str,
    args: &[OsString],
) -> Error {
    use dots_process::Error as Process;
    let program = command_description(program, args);
    match error {
        Process::Spawn(source) => Error::StartCommand {
            program,
            cwd: cwd.to_path_buf(),
            source,
        },
        Process::Timeout(timeout) => Error::CommandTimedOut { program, timeout },
        Process::OutputLimit { stream, limit } => Error::CommandOutputTooLarge {
            program,
            stream,
            limit,
        },
        Process::Read { stream, source } => Error::ReadCommandOutput {
            program,
            stream,
            source,
        },
        Process::Sink(source) => Error::ReadCommandOutput {
            program,
            stream: "stdout",
            source,
        },
        Process::Runtime(source) => Error::CommandRuntime { source },
        Process::Wait(source) => Error::InspectCommand { program, source },
        error @ (Process::InvalidLimits | Process::InputLimit { .. } | Process::Stdin(_)) => {
            Error::InspectCommand {
                program,
                source: std::io::Error::other(error),
            }
        }
    }
}

fn configured_command(cwd: &Path, program: &str, args: &[OsString]) -> Command {
    let mut command = Command::new(program);
    command
        .args(args)
        .current_dir(cwd)
        .env_remove("GH_REPO")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GH_HTTP_TIMEOUT", "10");
    command
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unexpected_input_errors_keep_their_process_error_identity() {
        let errors = [
            dots_process::Error::InputLimit { limit: 16 },
            dots_process::Error::Stdin(std::io::Error::from(std::io::ErrorKind::BrokenPipe)),
        ];
        for error in errors {
            let mapped = command_error(error, Path::new("/neutral"), "git", &[]);
            let Error::InspectCommand { source, .. } = mapped else {
                panic!("input failure must remain an operational command error");
            };
            let original = source
                .get_ref()
                .and_then(|error| error.downcast_ref::<dots_process::Error>())
                .expect("retain original process error as the source");
            assert!(matches!(
                original,
                dots_process::Error::InputLimit { limit: 16 } | dots_process::Error::Stdin(_)
            ));
        }
    }

    #[test]
    fn registered_pr_identity_is_canonical_and_supported() {
        let canonical =
            PullRequestUrl::parse("https://GitHub.com/Example/Project/pull/007/files#diff")
                .unwrap();
        assert_eq!(
            canonical.as_str(),
            "https://github.com/example/project/pull/7"
        );
        assert_eq!(
            (
                canonical.owner(),
                canonical.repository(),
                canonical.number()
            ),
            ("example", "project", 7)
        );
        for invalid in [
            "7",
            "http://github.com/example/project/pull/7",
            "https://forge.example/example/project/pull/7",
            "https://github.com/example/project/pull/0",
        ] {
            assert!(PullRequestUrl::parse(invalid).is_err(), "{invalid}");
        }
    }

    #[test]
    fn target_exposes_a_repository_before_checkout_resolution() {
        let target = Target::parse("https://github.com/Example/Project/pull/42/files").unwrap();
        assert_eq!(
            target.repository().unwrap().unwrap().display(),
            "github.com/Example/Project"
        );
        assert!(
            Target::parse("#42")
                .unwrap()
                .repository()
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn repository_inventory_reads_every_remote_url() {
        #[cfg(unix)]
        use std::os::unix::process::ExitStatusExt as _;

        let repositories = repositories_via(|args| {
            let stdout = match args {
                ["remote"] => "origin\nupstream\n",
                ["remote", "get-url", "--all", "origin"] => "git@github.com:fork/project.git\n",
                ["remote", "get-url", "--all", "upstream"] => {
                    "https://github.com/example/project.git\n"
                }
                _ => panic!("unexpected git arguments: {args:?}"),
            };
            Ok(CommandOutput {
                status: ExitStatus::from_raw(0),
                stdout: stdout.as_bytes().to_vec(),
                stderr: Vec::new(),
            })
        })
        .unwrap();
        assert_eq!(
            repositories
                .iter()
                .map(GitHubRepository::display)
                .collect::<Vec<_>>(),
            ["github.com/fork/project", "github.com/example/project"]
        );
    }

    #[test]
    fn all_supported_remote_shapes_are_normalized() {
        for remote in [
            "https://github.com/example/project.git",
            "git@github.com:example/project.git",
            "ssh://git@github.com/example/project.git",
        ] {
            assert_eq!(
                parse_remote_url(remote).unwrap().display(),
                "github.com/example/project"
            );
        }
        assert!(parse_remote_url("/local/repository").is_none());
    }
}
