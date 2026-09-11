//! Narrow GitHub pull-request resolution for review workflows.
//!
//! This crate deliberately has no search or list operation. A caller must name
//! a pull request explicitly, or ask GitHub to resolve the current branch.

use std::ffi::OsString;
use std::os::unix::process::CommandExt as _;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};

use nix::sys::signal::{Signal, killpg};
use nix::unistd::Pid;
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

const COMMAND_TIMEOUT: Duration = Duration::from_secs(30);
const COMMAND_POLL_INTERVAL: Duration = Duration::from_millis(20);
const MAX_COMMAND_OUTPUT_BYTES: usize = 2 * 1024 * 1024;
const PULL_REQUEST_FIELDS: &str = "number,title,url,baseRefName,headRefName,headRefOid";
const REPOSITORY_FIELDS: &str = "url";

/// An explicit pull request, or the pull request associated with the current branch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    CurrentBranch,
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

/// Canonical identity and review metadata returned by GitHub.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PullRequest {
    pub host: String,
    pub owner: String,
    pub repository: String,
    pub number: u64,
    pub title: String,
    pub url: String,
    pub repository_url: String,
    pub base_ref: String,
    pub head_ref: String,
    pub head_oid: String,
}

/// Exact commit pair to present in a local pull-request review.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Comparison {
    pub base_oid: String,
    pub head_oid: String,
}

/// A stateless client for explicit pull-request operations.
#[derive(Debug, Clone, Copy, Default)]
pub struct Client;

impl Client {
    /// Resolves the checkout repository and one pull request without listing or
    /// searching other pull requests, rejecting targets from another repository.
    pub fn resolve(self, checkout: &Path, target: &Target) -> Result<PullRequest> {
        resolve_with(&SystemRunner, checkout, target)
    }

    /// Fetches the current base branch and pull-request head, verifies the
    /// immutable head, and computes the exact merge-base/head pair.
    pub fn prepare_comparison(
        self,
        checkout: &Path,
        pull_request: &PullRequest,
    ) -> Result<Comparison> {
        prepare_with(&SystemRunner, checkout, pull_request)
    }

    /// Returns every GitHub repository identity named by configured Git remotes.
    /// Unsupported local remotes are ignored.
    pub fn repositories(self, checkout: &Path) -> Result<Vec<GitHubRepository>> {
        repositories_with(&SystemRunner, checkout)
    }
}

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    #[error("pull request number must be greater than zero")]
    InvalidNumber,

    #[error("pull request URL is invalid: {url:?}")]
    InvalidUrl { url: String },

    #[error("checkout repository URL is invalid: {url:?}")]
    InvalidRepositoryUrl { url: String },

    #[error("GitHub returned invalid pull request metadata: {field}")]
    InvalidMetadata { field: &'static str },

    #[error("GitHub resolved {actual}, not the requested pull request {expected}")]
    UnexpectedPullRequest { expected: String, actual: String },

    #[error("pull request repository {pull_request} does not match checkout repository {checkout}")]
    RepositoryMismatch {
        checkout: String,
        pull_request: String,
    },

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

    #[error("the {program} {stream} reader stopped unexpectedly")]
    CommandReaderStopped {
        program: String,
        stream: &'static str,
    },

    #[error("{program} failed ({status}): {stderr}")]
    CommandFailed {
        program: String,
        status: ExitStatus,
        stderr: String,
    },

    #[error("{program} returned non-UTF-8 output")]
    NonUtf8Output { program: String },

    #[error("could not decode the pull request returned by GitHub")]
    DecodePullRequest {
        #[source]
        source: serde_json::Error,
    },

    #[error("could not decode the repository returned by GitHub")]
    DecodeRepository {
        #[source]
        source: serde_json::Error,
    },

    #[error(
        "the fetched {revision} changed from {expected} to {actual}; resolve the pull request again"
    )]
    RevisionChanged {
        revision: &'static str,
        expected: String,
        actual: String,
    },
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PullRequestWire {
    number: u64,
    title: String,
    url: String,
    base_ref_name: String,
    head_ref_name: String,
    head_ref_oid: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RepositoryWire {
    url: String,
}

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
    pub fn display(&self) -> String {
        format!("{}/{}/{}", self.host, self.owner, self.repository)
    }

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

impl RepositoryIdentity {
    fn display(&self) -> String {
        format!("{}/{}/{}", self.host, self.owner, self.repository)
    }

    fn same_repository(&self, other: &Self) -> bool {
        self.host.eq_ignore_ascii_case(&other.host)
            && self.owner.eq_ignore_ascii_case(&other.owner)
            && self.repository.eq_ignore_ascii_case(&other.repository)
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
    fn display(&self) -> String {
        format!(
            "{}/{}/{}#{}",
            self.host, self.owner, self.repository, self.number
        )
    }

    fn same_pull_request(&self, other: &Self) -> bool {
        self.number == other.number
            && self.host.eq_ignore_ascii_case(&other.host)
            && self.owner.eq_ignore_ascii_case(&other.owner)
            && self.repository.eq_ignore_ascii_case(&other.repository)
    }

    fn repository_identity(&self) -> RepositoryIdentity {
        RepositoryIdentity {
            host: self.host.clone(),
            owner: self.owner.clone(),
            repository: self.repository.clone(),
        }
    }

    fn canonical_url(&self) -> String {
        format!(
            "{}://{}/{}/{}/pull/{}",
            self.scheme,
            self.host.to_ascii_lowercase(),
            self.owner.to_ascii_lowercase(),
            self.repository.to_ascii_lowercase(),
            self.number
        )
    }
}

impl TryFrom<PullRequestWire> for PullRequest {
    type Error = Error;

    fn try_from(value: PullRequestWire) -> Result<Self> {
        let identity = parse_pull_request_url(&value.url)?;
        if value.number == 0 || value.number != identity.number {
            return Err(Error::InvalidMetadata {
                field: "pull request number",
            });
        }
        validate_nonempty("title", &value.title)?;
        validate_ref_name("base ref", &value.base_ref_name)?;
        validate_ref_name("head ref", &value.head_ref_name)?;
        validate_oid("head OID", &value.head_ref_oid)?;
        let repository_url = format!(
            "{}://{}/{}/{}.git",
            identity.scheme, identity.host, identity.owner, identity.repository
        );
        let url = identity.canonical_url();
        Ok(Self {
            host: identity.host,
            owner: identity.owner,
            repository: identity.repository,
            number: value.number,
            title: value.title,
            url,
            repository_url,
            base_ref: value.base_ref_name,
            head_ref: value.head_ref_name,
            head_oid: value.head_ref_oid,
        })
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

#[derive(Debug)]
struct CommandOutput {
    status: ExitStatus,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

fn resolve_with(runner: &impl Runner, checkout: &Path, target: &Target) -> Result<PullRequest> {
    let expected = validate_target(target)?;
    let checkout_repository = if let Some(expected) = &expected {
        let requested = GitHubRepository::from(expected.repository_identity());
        let configured = repositories_with(runner, checkout)?;
        if !configured
            .iter()
            .any(|repository| repository.same_repository(&requested))
        {
            let checkout = if configured.is_empty() {
                "no configured GitHub remotes".to_owned()
            } else {
                configured
                    .iter()
                    .map(GitHubRepository::display)
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            return Err(Error::RepositoryMismatch {
                checkout,
                pull_request: requested.display(),
            });
        }
        None
    } else {
        Some(resolve_checkout_repository(runner, checkout)?)
    };
    let mut args = vec![OsString::from("pr"), OsString::from("view")];
    match target {
        Target::CurrentBranch => {}
        Target::Number(number) => args.push(number.to_string().into()),
        Target::Url(url) => {
            let Some(identity) = expected.as_ref() else {
                return Err(Error::InvalidUrl { url: url.clone() });
            };
            args.push(identity.canonical_url().into());
        }
    }
    args.extend([
        OsString::from("--json"),
        OsString::from(PULL_REQUEST_FIELDS),
    ]);
    let output = successful_output(runner, checkout, "gh", &args)?;
    let wire = serde_json::from_slice::<PullRequestWire>(&output)
        .map_err(|source| Error::DecodePullRequest { source })?;
    let pull_request = PullRequest::try_from(wire)?;
    if let Target::Number(expected) = target
        && *expected != pull_request.number
    {
        return Err(Error::UnexpectedPullRequest {
            expected: format!("#{expected}"),
            actual: format!("#{}", pull_request.number),
        });
    }
    if let Some(expected) = expected {
        let actual = UrlIdentity {
            scheme: pull_request
                .repository_url
                .split_once("://")
                .map_or("https", |(scheme, _)| scheme)
                .to_owned(),
            host: pull_request.host.clone(),
            owner: pull_request.owner.clone(),
            repository: pull_request.repository.clone(),
            number: pull_request.number,
        };
        if !expected.same_pull_request(&actual) {
            return Err(Error::UnexpectedPullRequest {
                expected: expected.display(),
                actual: actual.display(),
            });
        }
    }
    if let Some(checkout_repository) = checkout_repository {
        ensure_repository_matches(
            &checkout_repository,
            &RepositoryIdentity {
                host: pull_request.host.clone(),
                owner: pull_request.owner.clone(),
                repository: pull_request.repository.clone(),
            },
        )?;
    }
    Ok(pull_request)
}

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

fn resolve_checkout_repository(
    runner: &impl Runner,
    checkout: &Path,
) -> Result<RepositoryIdentity> {
    let args = [
        OsString::from("repo"),
        OsString::from("view"),
        OsString::from("--json"),
        OsString::from(REPOSITORY_FIELDS),
    ];
    let output = successful_output(runner, checkout, "gh", &args)?;
    let wire = serde_json::from_slice::<RepositoryWire>(&output)
        .map_err(|source| Error::DecodeRepository { source })?;
    parse_repository_url(&wire.url)
}

fn ensure_repository_matches(
    checkout: &RepositoryIdentity,
    pull_request: &RepositoryIdentity,
) -> Result<()> {
    if checkout.same_repository(pull_request) {
        Ok(())
    } else {
        Err(Error::RepositoryMismatch {
            checkout: checkout.display(),
            pull_request: pull_request.display(),
        })
    }
}

fn prepare_with(
    runner: &impl Runner,
    checkout: &Path,
    pull_request: &PullRequest,
) -> Result<Comparison> {
    validate_pull_request(pull_request)?;
    let namespace = private_ref_namespace(pull_request);
    let base_local = format!("{namespace}/base");
    let head_local = format!("{namespace}/head");
    let base_remote = format!("refs/heads/{}", pull_request.base_ref);
    let head_remote = format!("refs/pull/{}/head", pull_request.number);
    let args = vec![
        OsString::from("fetch"),
        OsString::from("--no-tags"),
        OsString::from("--force"),
        OsString::from(&pull_request.repository_url),
        OsString::from(format!("+{base_remote}:{base_local}")),
        OsString::from(format!("+{head_remote}:{head_local}")),
    ];
    successful_output(runner, checkout, "git", &args)?;

    let fetched_base = git_text(
        runner,
        checkout,
        &["rev-parse", &format!("{base_local}^{{commit}}")],
    )?;
    let fetched_head = git_text(
        runner,
        checkout,
        &["rev-parse", &format!("{head_local}^{{commit}}")],
    )?;
    verify_revision("head", &pull_request.head_oid, &fetched_head)?;
    let base_oid = git_text(
        runner,
        checkout,
        &["merge-base", &fetched_base, &fetched_head],
    )?;
    validate_oid("merge-base OID", &base_oid)?;
    Ok(Comparison {
        base_oid,
        head_oid: fetched_head,
    })
}

fn validate_target(target: &Target) -> Result<Option<UrlIdentity>> {
    match target {
        Target::Number(0) => Err(Error::InvalidNumber),
        Target::CurrentBranch | Target::Number(_) => Ok(None),
        Target::Url(url) => parse_pull_request_url(url).map(Some),
    }
}

fn validate_pull_request(pull_request: &PullRequest) -> Result<()> {
    let identity = parse_pull_request_url(&pull_request.url)?;
    if identity.number != pull_request.number
        || !identity.host.eq_ignore_ascii_case(&pull_request.host)
        || !identity.owner.eq_ignore_ascii_case(&pull_request.owner)
        || !identity
            .repository
            .eq_ignore_ascii_case(&pull_request.repository)
    {
        return Err(Error::InvalidMetadata {
            field: "pull request identity",
        });
    }
    let expected_repository_url = format!(
        "{}://{}/{}/{}.git",
        identity.scheme, identity.host, identity.owner, identity.repository
    );
    if pull_request.repository_url != expected_repository_url {
        return Err(Error::InvalidMetadata {
            field: "repository URL",
        });
    }
    validate_nonempty("title", &pull_request.title)?;
    validate_ref_name("base ref", &pull_request.base_ref)?;
    validate_ref_name("head ref", &pull_request.head_ref)?;
    validate_oid("head OID", &pull_request.head_oid)
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

fn validate_nonempty(field: &'static str, value: &str) -> Result<()> {
    if value.trim().is_empty() {
        Err(Error::InvalidMetadata { field })
    } else {
        Ok(())
    }
}

fn validate_ref_name(field: &'static str, value: &str) -> Result<()> {
    let invalid = value.is_empty()
        || value.starts_with('.')
        || value.starts_with('/')
        || value.ends_with('.')
        || value.ends_with('/')
        || value.contains("..")
        || value.contains("@{")
        || value.contains("//")
        || value.bytes().any(|byte| {
            byte <= b' ' || matches!(byte, b'~' | b'^' | b':' | b'?' | b'*' | b'[' | b'\\')
        });
    if invalid {
        Err(Error::InvalidMetadata { field })
    } else {
        Ok(())
    }
}

fn validate_oid(field: &'static str, value: &str) -> Result<()> {
    if matches!(value.len(), 40 | 64) && value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err(Error::InvalidMetadata { field })
    }
}

fn private_ref_namespace(pull_request: &PullRequest) -> String {
    let identity = format!(
        "{}/{}/{}#{}",
        pull_request.host, pull_request.owner, pull_request.repository, pull_request.number
    );
    let digest = Sha256::digest(identity.as_bytes());
    format!("refs/herdr/review/{digest:x}")
}

fn verify_revision(revision: &'static str, expected: &str, actual: &str) -> Result<()> {
    if expected.eq_ignore_ascii_case(actual) {
        Ok(())
    } else {
        Err(Error::RevisionChanged {
            revision,
            expected: expected.to_owned(),
            actual: actual.to_owned(),
        })
    }
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
    let started = Instant::now();
    let mut command = Command::new(program);
    command
        .args(args)
        .current_dir(cwd)
        .env_remove("GH_REPO")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GH_HTTP_TIMEOUT", "10")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0);
    let mut child = command.spawn().map_err(|source| Error::StartCommand {
        program: command_description(program, args),
        cwd: cwd.to_path_buf(),
        source,
    })?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| Error::ReadCommandOutput {
            program: program.to_owned(),
            stream: "stdout",
            source: std::io::Error::other("stdout pipe was not captured"),
        })?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| Error::ReadCommandOutput {
            program: program.to_owned(),
            stream: "stderr",
            source: std::io::Error::other("stderr pipe was not captured"),
        })?;
    let stdout = read_in_background(stdout);
    let stderr = read_in_background(stderr);
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() < timeout => thread::sleep(COMMAND_POLL_INTERVAL),
            Ok(None) => {
                terminate_process_group(&mut child);
                return Err(Error::CommandTimedOut {
                    program: command_description(program, args),
                    timeout,
                });
            }
            Err(source) => {
                terminate_process_group(&mut child);
                return Err(Error::InspectCommand {
                    program: command_description(program, args),
                    source,
                });
            }
        }
    };
    kill_process_group(child.id());
    let stdout = receive_output(&stdout, program, "stdout", started, timeout)?;
    let stderr = receive_output(&stderr, program, "stderr", started, timeout)?;
    if stdout.truncated {
        return Err(Error::CommandOutputTooLarge {
            program: command_description(program, args),
            stream: "stdout",
            limit: MAX_COMMAND_OUTPUT_BYTES,
        });
    }
    if stderr.truncated {
        return Err(Error::CommandOutputTooLarge {
            program: command_description(program, args),
            stream: "stderr",
            limit: MAX_COMMAND_OUTPUT_BYTES,
        });
    }
    Ok(CommandOutput {
        status,
        stdout: stdout.bytes,
        stderr: stderr.bytes,
    })
}

fn read_in_background(
    reader: impl std::io::Read + Send + 'static,
) -> Receiver<std::io::Result<Captured>> {
    let (sender, receiver) = mpsc::sync_channel(1);
    thread::spawn(move || {
        let _ = sender.send(read_bounded(reader));
    });
    receiver
}

fn receive_output(
    receiver: &Receiver<std::io::Result<Captured>>,
    program: &str,
    stream: &'static str,
    started: Instant,
    timeout: Duration,
) -> Result<Captured> {
    receiver
        .recv_timeout(timeout.saturating_sub(started.elapsed()))
        .map_err(|error| match error {
            RecvTimeoutError::Timeout => Error::CommandTimedOut {
                program: program.to_owned(),
                timeout,
            },
            RecvTimeoutError::Disconnected => Error::CommandReaderStopped {
                program: program.to_owned(),
                stream,
            },
        })?
        .map_err(|source| Error::ReadCommandOutput {
            program: program.to_owned(),
            stream,
            source,
        })
}

fn terminate_process_group(child: &mut std::process::Child) {
    kill_process_group(child.id());
    let _ = child.kill();
    let _ = child.wait();
}

fn kill_process_group(id: u32) {
    if let Ok(id) = i32::try_from(id) {
        let _ = killpg(Pid::from_raw(id), Signal::SIGKILL);
    }
}

#[derive(Debug)]
struct Captured {
    bytes: Vec<u8>,
    truncated: bool,
}

fn read_bounded(mut reader: impl std::io::Read) -> std::io::Result<Captured> {
    let mut bytes = Vec::new();
    let mut truncated = false;
    let mut buffer = [0_u8; 16 * 1024];
    loop {
        let count = reader.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        let remaining = MAX_COMMAND_OUTPUT_BYTES.saturating_sub(bytes.len());
        let retained = count.min(remaining);
        bytes.extend_from_slice(buffer.get(..retained).unwrap_or_default());
        truncated |= retained < count;
    }
    Ok(Captured { bytes, truncated })
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::sync::Mutex;

    #[cfg(unix)]
    use std::os::unix::process::ExitStatusExt as _;

    use super::*;

    const BASE: &str = "1111111111111111111111111111111111111111";
    const HEAD: &str = "2222222222222222222222222222222222222222";
    const MERGE_BASE: &str = "3333333333333333333333333333333333333333";

    #[derive(Debug)]
    struct FakeRunner {
        outputs: Mutex<VecDeque<CommandOutput>>,
        calls: Mutex<Vec<(PathBuf, String, Vec<String>)>>,
    }

    impl FakeRunner {
        fn new(outputs: impl IntoIterator<Item = CommandOutput>) -> Self {
            Self {
                outputs: Mutex::new(outputs.into_iter().collect()),
                calls: Mutex::new(Vec::new()),
            }
        }

        fn calls(&self) -> Vec<(PathBuf, String, Vec<String>)> {
            self.calls.lock().unwrap().clone()
        }
    }

    impl Runner for FakeRunner {
        fn output(&self, cwd: &Path, program: &str, args: &[OsString]) -> Result<CommandOutput> {
            self.calls.lock().unwrap().push((
                cwd.to_path_buf(),
                program.to_owned(),
                args.iter()
                    .map(|argument| argument.to_string_lossy().into_owned())
                    .collect(),
            ));
            self.outputs
                .lock()
                .unwrap()
                .pop_front()
                .ok_or(Error::InvalidMetadata {
                    field: "fake output",
                })
        }
    }

    fn output(stdout: impl Into<Vec<u8>>) -> CommandOutput {
        CommandOutput {
            status: ExitStatus::from_raw(0),
            stdout: stdout.into(),
            stderr: Vec::new(),
        }
    }

    fn pull_request_json(number: u64, url: &str) -> String {
        format!(
            r#"{{"number":{number},"title":"Make reviews local-first","url":"{url}","baseRefName":"main","headRefName":"review-flow","headRefOid":"{HEAD}"}}"#
        )
    }

    fn repository_json(url: &str) -> String {
        format!(r#"{{"url":"{url}"}}"#)
    }

    fn pull_request() -> PullRequest {
        PullRequest {
            host: "github.com".to_owned(),
            owner: "example".to_owned(),
            repository: "project".to_owned(),
            number: 42,
            title: "Make reviews local-first".to_owned(),
            url: "https://github.com/example/project/pull/42".to_owned(),
            repository_url: "https://github.com/example/project.git".to_owned(),
            base_ref: "main".to_owned(),
            head_ref: "review-flow".to_owned(),
            head_oid: HEAD.to_owned(),
        }
    }

    #[test]
    fn current_branch_resolution_uses_explicit_repository_and_pull_request_queries() {
        let runner = FakeRunner::new([
            output(repository_json("https://github.com/example/project")),
            output(pull_request_json(
                42,
                "https://github.com/example/project/pull/42",
            )),
        ]);
        let resolved = resolve_with(&runner, Path::new("/repo"), &Target::CurrentBranch).unwrap();
        assert_eq!(resolved, pull_request());
        assert_eq!(
            runner.calls(),
            [
                (
                    PathBuf::from("/repo"),
                    "gh".to_owned(),
                    vec![
                        "repo".to_owned(),
                        "view".to_owned(),
                        "--json".to_owned(),
                        REPOSITORY_FIELDS.to_owned(),
                    ],
                ),
                (
                    PathBuf::from("/repo"),
                    "gh".to_owned(),
                    vec![
                        "pr".to_owned(),
                        "view".to_owned(),
                        "--json".to_owned(),
                        PULL_REQUEST_FIELDS.to_owned(),
                    ],
                ),
            ]
        );
    }

    #[test]
    fn explicit_number_is_passed_as_one_argv_element() {
        let runner = FakeRunner::new([
            output(repository_json("https://github.com/example/project")),
            output(pull_request_json(
                42,
                "https://github.com/example/project/pull/42",
            )),
        ]);
        resolve_with(&runner, Path::new("/repo"), &Target::Number(42)).unwrap();
        let calls = runner.calls();
        let (_, program, args) = calls.get(1).unwrap();
        assert_eq!(program, "gh");
        assert_eq!(args.get(2).map(String::as_str), Some("42"));
        assert!(calls.iter().all(|(_, _, args)| {
            !args
                .iter()
                .any(|argument| argument == "list" || argument == "search")
        }));
    }

    #[test]
    fn explicit_number_must_match_the_returned_pull_request() {
        let runner = FakeRunner::new([
            output(repository_json("https://github.com/example/project")),
            output(pull_request_json(
                43,
                "https://github.com/example/project/pull/43",
            )),
        ]);
        let error = resolve_with(&runner, Path::new("/repo"), &Target::Number(42)).unwrap_err();
        assert!(matches!(error, Error::UnexpectedPullRequest { .. }));
    }

    #[test]
    fn current_branch_must_resolve_in_the_checkout_repository() {
        let runner = FakeRunner::new([
            output(repository_json("https://github.com/example/project")),
            output(pull_request_json(
                42,
                "https://github.com/other/project/pull/42",
            )),
        ]);
        let error = resolve_with(&runner, Path::new("/repo"), &Target::CurrentBranch).unwrap_err();
        assert!(matches!(error, Error::RepositoryMismatch { .. }));
    }

    #[test]
    fn explicit_url_must_resolve_to_the_same_pull_request() {
        let runner = FakeRunner::new([
            output("origin\n"),
            output("git@github.com:example/project.git\n"),
            output(pull_request_json(
                43,
                "https://github.com/example/project/pull/43",
            )),
        ]);
        let error = resolve_with(
            &runner,
            Path::new("/repo"),
            &Target::Url("https://github.com/example/project/pull/42".to_owned()),
        )
        .unwrap_err();
        assert!(matches!(error, Error::UnexpectedPullRequest { .. }));
    }

    #[test]
    fn copied_pull_request_tabs_are_normalized_before_calling_github() {
        let runner = FakeRunner::new([
            output("origin\n"),
            output("ssh://git@github.internal/owner/project.git\n"),
            output(pull_request_json(
                42,
                "http://github.internal/Owner/Project/pull/42",
            )),
        ]);
        let resolved = resolve_with(
            &runner,
            Path::new("/repo"),
            &Target::Url(
                "http://github.internal/Owner/Project/pull/42/files?diff=split#discussion"
                    .to_owned(),
            ),
        )
        .unwrap();

        assert_eq!(resolved.url, "http://github.internal/owner/project/pull/42");
        let calls = runner.calls();
        let (_, _, args) = calls.get(2).unwrap();
        assert_eq!(
            args.get(2).map(String::as_str),
            Some("http://github.internal/owner/project/pull/42")
        );
    }

    #[test]
    fn foreign_repository_url_is_rejected_before_the_pull_request_query() {
        let runner = FakeRunner::new([
            output("origin\n"),
            output("https://github.com/example/project.git\n"),
        ]);
        let error = resolve_with(
            &runner,
            Path::new("/repo"),
            &Target::Url("https://github.com/other/project/pull/42".to_owned()),
        )
        .unwrap_err();
        assert!(matches!(error, Error::RepositoryMismatch { .. }));
        assert_eq!(
            runner.calls(),
            [
                (
                    PathBuf::from("/repo"),
                    "git".to_owned(),
                    vec!["remote".to_owned()],
                ),
                (
                    PathBuf::from("/repo"),
                    "git".to_owned(),
                    vec![
                        "remote".to_owned(),
                        "get-url".to_owned(),
                        "--all".to_owned(),
                        "origin".to_owned(),
                    ],
                ),
            ]
        );
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

    #[test]
    fn repository_inventory_reads_every_remote_url() {
        let runner = FakeRunner::new([
            output("origin\nupstream\n"),
            output("git@github.com:fork/project.git\n"),
            output("https://github.com/example/project.git\n"),
        ]);
        let repositories = repositories_with(&runner, Path::new("/repo")).unwrap();
        assert_eq!(
            repositories
                .iter()
                .map(GitHubRepository::display)
                .collect::<Vec<_>>(),
            ["github.com/fork/project", "github.com/example/project"]
        );
    }

    #[test]
    fn preparation_fetches_only_the_named_base_and_pull_refs() {
        let runner = FakeRunner::new([
            output(Vec::new()),
            output(format!("{BASE}\n")),
            output(format!("{HEAD}\n")),
            output(format!("{MERGE_BASE}\n")),
        ]);
        let comparison = prepare_with(&runner, Path::new("/repo"), &pull_request()).unwrap();
        assert_eq!(
            comparison,
            Comparison {
                base_oid: MERGE_BASE.to_owned(),
                head_oid: HEAD.to_owned(),
            }
        );
        let calls = runner.calls();
        let (_, program, fetch) = calls.first().unwrap();
        assert_eq!(program, "git");
        assert_eq!(fetch.first().map(String::as_str), Some("fetch"));
        assert_eq!(fetch.get(1).map(String::as_str), Some("--no-tags"));
        assert_eq!(fetch.get(2).map(String::as_str), Some("--force"));
        assert_eq!(
            fetch.get(3).map(String::as_str),
            Some("https://github.com/example/project.git")
        );
        assert!(
            fetch
                .iter()
                .any(|argument| argument.starts_with("+refs/heads/main:"))
        );
        assert!(
            fetch
                .iter()
                .any(|argument| argument.starts_with("+refs/pull/42/head:"))
        );
        assert_eq!(fetch.len(), 6);
        assert!(calls.iter().all(|(_, program, args)| {
            program != "gh"
                && !args
                    .iter()
                    .any(|argument| argument == "list" || argument == "search")
        }));
    }

    #[test]
    fn preparation_rejects_a_pull_request_that_advanced_during_fetch() {
        let runner = FakeRunner::new([
            output(Vec::new()),
            output(format!("{BASE}\n")),
            output("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\n"),
        ]);
        let error = prepare_with(&runner, Path::new("/repo"), &pull_request()).unwrap_err();
        assert!(matches!(
            error,
            Error::RevisionChanged {
                revision: "head",
                ..
            }
        ));
    }

    #[test]
    fn invalid_explicit_targets_do_not_run_commands() {
        let runner = FakeRunner::new([]);
        assert!(resolve_with(&runner, Path::new("/repo"), &Target::Number(0)).is_err());
        assert!(
            resolve_with(
                &runner,
                Path::new("/repo"),
                &Target::Url("https://github.com/example/project/issues/42".to_owned()),
            )
            .is_err()
        );
        assert!(runner.calls().is_empty());
    }

    #[test]
    fn command_deadline_kills_and_reaps_the_child() {
        let started = Instant::now();
        let error = command_with_timeout(
            Path::new("/tmp"),
            "sh",
            &[OsString::from("-c"), OsString::from("sleep 2 & wait")],
            Duration::from_millis(20),
        )
        .unwrap_err();
        assert!(matches!(error, Error::CommandTimedOut { .. }));
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn successful_parent_does_not_leave_pipe_readers_waiting_on_descendants() {
        let started = Instant::now();
        let result = command_with_timeout(
            Path::new("/tmp"),
            "sh",
            &[OsString::from("-c"), OsString::from("sleep 2 & exit 0")],
            Duration::from_millis(200),
        )
        .unwrap();
        assert!(result.status.success());
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn command_output_is_bounded_without_blocking_the_child() {
        let error = command_with_timeout(
            Path::new("/tmp"),
            "sh",
            &[
                OsString::from("-c"),
                OsString::from("yes x | head -c 2200000"),
            ],
            Duration::from_secs(5),
        )
        .unwrap_err();
        assert!(matches!(
            error,
            Error::CommandOutputTooLarge {
                stream: "stdout",
                ..
            }
        ));
    }
}
