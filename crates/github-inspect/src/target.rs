//! Validated repository identities and explicit CI subjects.
use crate::{Error, Provider, PullRequest, Result};
use serde::{Deserialize, Serialize};
use std::{fmt, str::FromStr};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Repository {
    pub(crate) host: String,
    pub(crate) owner: String,
    pub(crate) repo: String,
}
impl Repository {
    pub fn url(&self) -> String {
        format!("https://{self}")
    }
    pub(crate) fn endpoint(&self, path: &str) -> String {
        format!("repos/{}/{}/{path}", self.owner, self.repo)
    }
    pub(crate) fn from_url(url: &str) -> Result<(Self, String)> {
        let url = url.split(['?', '#']).next().ok_or_else(invalid)?;
        let parts: Vec<_> = url
            .strip_prefix("https://")
            .ok_or_else(invalid)?
            .trim_end_matches('/')
            .split('/')
            .collect();
        let [host, owner, repo, rest @ ..] = parts.as_slice() else {
            return Err(invalid());
        };
        let repository = format!("{host}/{owner}/{repo}").parse()?;
        Ok((repository, rest.join("/")))
    }
}
impl FromStr for Repository {
    type Err = Error;
    fn from_str(value: &str) -> Result<Self> {
        let parts: Vec<_> = value.split('/').collect();
        let (host, owner, repo) = match parts.as_slice() {
            [owner, repo] => ("github.com", *owner, *repo),
            [host, owner, repo] => (*host, *owner, *repo),
            _ => return Err(invalid()),
        };
        for part in [host, owner, repo] {
            if part.is_empty()
                || part.starts_with('-')
                || matches!(part, "." | "..")
                || !part
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b".-_".contains(&b))
            {
                return Err(invalid());
            }
        }
        Ok(Self {
            host: host.to_ascii_lowercase(),
            owner: owner.to_ascii_lowercase(),
            repo: repo.to_ascii_lowercase(),
        })
    }
}
impl TryFrom<String> for Repository {
    type Error = Error;
    fn try_from(value: String) -> Result<Self> {
        value.parse()
    }
}
impl From<Repository> for String {
    fn from(value: Repository) -> Self {
        value.to_string()
    }
}
impl fmt::Display for Repository {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}/{}", self.host, self.owner, self.repo)
    }
}

pub(crate) fn invalid() -> Error {
    Error::InvalidReference(
        "expected an explicit GitHub target with a valid repository and positive ID".into(),
    )
}
pub(crate) fn id(value: &str) -> Result<u64> {
    value
        .parse::<u64>()
        .ok()
        .filter(|id| *id > 0)
        .ok_or_else(invalid)
}
pub(crate) fn sha(value: &str) -> Result<String> {
    if matches!(value.len(), 40 | 64) && value.bytes().all(|b| b.is_ascii_hexdigit()) {
        Ok(value.to_ascii_lowercase())
    } else {
        Err(Error::InvalidReference(
            "SHA must be a full 40- or 64-character hexadecimal commit ID".into(),
        ))
    }
}
pub(crate) fn encode(value: &str) -> String {
    use std::fmt::Write as _;
    let mut out = String::new();
    for b in value.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
            out.push(char::from(b));
        } else {
            let _ = write!(out, "%{b:02X}");
        }
    }
    out
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Resource {
    Pr(u64),
    ReviewThreads(u64),
    ReviewThread(u64, String),
    Commit(String),
    Run { id: u64, attempt: Option<u64> },
    Job { id: u64, run: Option<u64> },
    Check(u64),
}
impl Resource {
    pub(crate) fn parse(url: &str) -> Result<(Repository, Self)> {
        let (repo, path) = Repository::from_url(url)?;
        let parts: Vec<_> = path.split('/').collect();
        let resource = match parts.as_slice() {
            ["pull", n] => Self::Pr(id(n)?),
            ["pull", n, "review-threads"] => Self::ReviewThreads(id(n)?),
            ["pull", n, "review-threads", thread] if crate::reviews::valid_node(thread) => {
                Self::ReviewThread(id(n)?, (*thread).into())
            }
            ["commit", head] => Self::Commit(sha(head)?),
            ["actions", "runs", n] => Self::Run {
                id: id(n)?,
                attempt: None,
            },
            ["actions", "runs", n, "attempts", a] => Self::Run {
                id: id(n)?,
                attempt: Some(id(a)?),
            },
            ["actions", "runs", r, "job", j] | ["runs", r, "jobs", j] => Self::Job {
                id: id(j)?,
                run: Some(id(r)?),
            },
            ["actions", "jobs", j] => Self::Job {
                id: id(j)?,
                run: None,
            },
            ["check-runs" | "runs", n] => Self::Check(id(n.split('?').next().unwrap_or(n))?),
            _ => return Err(invalid()),
        };
        Ok((repo, resource))
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Subject {
    PullRequest {
        repository: Repository,
        pr: PullRequest,
    },
    Commit {
        repository: Repository,
        sha: String,
        requested_ref: Option<String>,
    },
    Run {
        repository: Repository,
        id: u64,
        attempt: u64,
        sha: String,
        name: String,
        event: String,
        branch: Option<String>,
        status: String,
        conclusion: Option<String>,
    },
}
impl Subject {
    pub fn repository(&self) -> &Repository {
        match self {
            Self::PullRequest { repository, .. }
            | Self::Commit { repository, .. }
            | Self::Run { repository, .. } => repository,
        }
    }
    pub fn head(&self) -> &str {
        match self {
            Self::PullRequest { pr, .. } => &pr.head_ref_oid,
            Self::Commit { sha, .. } | Self::Run { sha, .. } => sha,
        }
    }
    pub fn pr(&self) -> Option<&PullRequest> {
        if let Self::PullRequest { pr, .. } = self {
            Some(pr)
        } else {
            None
        }
    }
    pub fn url(&self) -> String {
        match self {
            Self::PullRequest { repository, pr } => {
                format!("{}/pull/{}", repository.url(), pr.number)
            }
            Self::Commit {
                repository, sha, ..
            } => format!("{}/commit/{sha}", repository.url()),
            Self::Run {
                repository,
                id,
                attempt,
                ..
            } => format!("{}/actions/runs/{id}/attempts/{attempt}", repository.url()),
        }
    }
    pub fn command(&self) -> String {
        match self {
            Self::PullRequest { pr, .. } => format!("dots github ci --pr {}", pr.url),
            Self::Commit {
                repository, sha, ..
            } => format!("dots github ci --repo {repository} --sha {sha}"),
            Self::Run { .. } => format!("dots github ci --run {}", self.url()),
        }
    }
}

/// An unresolved user selection. Moving refs are resolved exactly once.
#[derive(Debug, Clone, Copy)]
pub enum Target<'a> {
    PullRequest(Option<&'a str>),
    Ref(&'a str),
    Commit(&'a str),
    Run {
        value: &'a str,
        attempt: Option<u64>,
    },
}

pub(crate) fn repository(
    provider: &impl Provider,
    explicit: Option<&Repository>,
    from_url: Option<Repository>,
) -> Result<Repository> {
    match (explicit, from_url) {
        (Some(explicit), Some(url)) if explicit != &url => Err(Error::InvalidReference(
            "URL repository conflicts with --repo".into(),
        )),
        (_, Some(url)) => Ok(url),
        (Some(explicit), None) => Ok(explicit.clone()),
        (None, None) => provider.repository(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn copied_urls_normalize_location_without_leaking_query_syntax() {
        for url in [
            "https://GitHub.com/Example/Project/actions/runs/20/job/30?pr=7#step:1:2",
            "https://github.com/example/project/runs/20/jobs/30",
        ] {
            let (repository, resource) = Resource::parse(url).unwrap();
            assert_eq!(repository.to_string(), "github.com/example/project");
            assert_eq!(
                resource,
                Resource::Job {
                    id: 30,
                    run: Some(20)
                }
            );
        }
        assert!(
            format!(
                "snapshot:github.com/example/project:pull/7?$(id):{}",
                "a".repeat(64)
            )
            .parse::<crate::SnapshotRef>()
            .is_err()
        );
        for repo in [
            "example/../project",
            "-host/example/project",
            "github.com/example/$(id)",
            "https://github.com/example/project",
        ] {
            assert!(repo.parse::<Repository>().is_err());
        }
    }
}
