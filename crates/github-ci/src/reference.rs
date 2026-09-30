//! Exact Actions job identity with optional observation provenance.
use crate::target::{Resource, id, invalid, sha};
use crate::{Error, Repository, Result, Subject};
use serde::{Deserialize, Serialize};
use std::{fmt, str::FromStr};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct CheckRef {
    pub(crate) repository: Repository,
    pub(crate) pr: Option<u64>,
    pub(crate) head: String,
    pub(crate) run: u64,
    pub(crate) attempt: Option<u64>,
    pub(crate) job: u64,
}
impl CheckRef {
    #[cfg(test)]
    pub(crate) fn from_check(pr_url: &str, head: &str, link: &str) -> Option<Self> {
        let (repository, Resource::Pr(pr)) = Resource::parse(pr_url).ok()? else {
            return None;
        };
        Self::from_subject(
            &Subject::PullRequest {
                repository,
                pr: crate::PullRequest {
                    url: pr_url.into(),
                    number: pr,
                    head_ref_oid: head.into(),
                    state: String::new(),
                },
            },
            link,
        )
    }
    pub(crate) fn from_subject(subject: &Subject, link: &str) -> Option<Self> {
        let (
            repository,
            Resource::Job {
                id: job,
                run: Some(run),
            },
        ) = Resource::parse(link).ok()?
        else {
            return None;
        };
        if &repository != subject.repository() {
            return None;
        }
        Some(Self {
            repository,
            pr: subject.pr().map(|pr| pr.number),
            head: sha(subject.head()).ok()?,
            run,
            attempt: match subject {
                Subject::Run { attempt, .. } => Some(*attempt),
                _ => None,
            },
            job,
        })
    }
    pub(crate) fn endpoint(&self) -> String {
        self.repository
            .endpoint(&format!("actions/jobs/{}", self.job))
    }
    pub fn pull_request_url(&self) -> Option<String> {
        self.pr
            .map(|pr| format!("{}/pull/{pr}", self.repository.url()))
    }
    pub fn job_url(&self) -> String {
        format!(
            "{}/actions/runs/{}/job/{}",
            self.repository.url(),
            self.run,
            self.job
        )
    }
    pub fn command(&self) -> String {
        format!("dots github ci logs {self}")
    }
}
impl FromStr for CheckRef {
    type Err = Error;
    fn from_str(value: &str) -> Result<Self> {
        let parts: Vec<_> = value.split(':').collect();
        match parts.as_slice() {
            ["job", repo, pr, head, run, attempt, job] => Ok(Self {
                repository: repo.parse()?,
                pr: optional_id(pr)?,
                head: sha(head)?,
                run: id(run)?,
                attempt: optional_id(attempt)?,
                job: id(job)?,
            }),
            _ => Err(invalid()),
        }
    }
}
pub(crate) fn optional_id(value: &str) -> Result<Option<u64>> {
    if value == "-" {
        Ok(None)
    } else {
        id(value).map(Some)
    }
}
impl fmt::Display for CheckRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "job:{}:{}:{}:{}:{}:{}",
            self.repository,
            self.pr.map_or_else(|| "-".into(), |id| id.to_string()),
            self.head,
            self.run,
            self.attempt.map_or_else(|| "-".into(), |id| id.to_string()),
            self.job
        )
    }
}
impl From<CheckRef> for String {
    fn from(value: CheckRef) -> Self {
        value.to_string()
    }
}
impl TryFrom<String> for CheckRef {
    type Error = Error;
    fn try_from(value: String) -> Result<Self> {
        value.parse()
    }
}
