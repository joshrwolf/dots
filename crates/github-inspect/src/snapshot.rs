//! Explicit, short-lived observations scoped to a repository and target.
use crate::checks::Row;
use crate::{Error, Provider, Repository, Result, Subject};
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use std::{fmt, str::FromStr};

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(try_from = "String", into = "String")]
pub struct SnapshotRef {
    repository: Repository,
    path: String,
    digest: String,
}
impl SnapshotRef {
    pub(crate) fn host(&self) -> &str {
        &self.repository.host
    }
    pub fn url(&self) -> String {
        format!("{}/{}", self.repository.url(), self.path)
    }
    pub fn refresh_command(&self) -> String {
        match crate::target::Resource::parse(&self.url()).map(|(_, resource)| resource) {
            Ok(
                crate::target::Resource::ReviewThreads(pr)
                | crate::target::Resource::ReviewThread(pr, _),
            ) => format!(
                "dots github reviews --pr {}/pull/{pr}",
                self.repository.url()
            ),
            Ok(crate::target::Resource::Pr(_)) => format!("dots github ci --pr {}", self.url()),
            Ok(crate::target::Resource::Commit(sha)) => {
                format!("dots github ci --repo {} --sha {sha}", self.repository)
            }
            Ok(crate::target::Resource::Run { .. }) => {
                format!("dots github ci --run {}", self.url())
            }
            Ok(crate::target::Resource::Job { .. }) => {
                format!("dots github ci logs {}", self.url())
            }
            _ => format!("dots github ci output {}", self.url()),
        }
    }
    pub(crate) fn create(url: &str, bytes: &[u8]) -> Option<Self> {
        crate::target::Resource::parse(url).ok()?;
        let (repository, path) = Repository::from_url(url).ok()?;
        Some(Self {
            repository,
            path,
            digest: format!("{:x}", Sha256::digest(bytes)),
        })
    }
}
impl FromStr for SnapshotRef {
    type Err = Error;
    fn from_str(value: &str) -> Result<Self> {
        let parts: Vec<_> = value.split(':').collect();
        let ["snapshot", repository, path, digest] = parts.as_slice() else {
            return Err(crate::target::invalid());
        };
        let repository: Repository = repository.parse()?;
        crate::target::Resource::parse(&format!("{}/{path}", repository.url()))?;
        if path.contains(['?', '#']) {
            return Err(crate::target::invalid());
        }
        if digest.len() != 64 || !digest.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(crate::target::invalid());
        }
        Ok(Self {
            repository,
            path: (*path).into(),
            digest: (*digest).into(),
        })
    }
}
impl fmt::Display for SnapshotRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "snapshot:{}:{}:{}",
            self.repository, self.path, self.digest
        )
    }
}
impl From<SnapshotRef> for String {
    fn from(value: SnapshotRef) -> Self {
        value.to_string()
    }
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct Snapshot {
    pub subject: Subject,
    pub checks: Vec<Row>,
    pub observed_at: u64,
    pub warnings: Vec<String>,
}
pub(crate) fn save(provider: &impl Provider, snapshot: &Snapshot) -> Option<SnapshotRef> {
    let bytes = serde_json::to_vec(snapshot).ok()?;
    let reference = SnapshotRef::create(&snapshot.subject.url(), &bytes)?;
    provider
        .save_snapshot(&reference, &bytes, snapshot.observed_at)
        .then_some(reference)
}
pub(crate) fn load(provider: &impl Provider, reference: &SnapshotRef) -> Result<Snapshot> {
    let invalid = || {
        Error::InvalidResponse(format!(
            "Saved observation expired or invalid; refresh with {}",
            reference.refresh_command()
        ))
    };
    let cached = provider.read_snapshot(reference).map_err(|error| {
        Error::InvalidResponse(format!(
            "{}; refresh with {}",
            error.diagnostic(),
            reference.refresh_command()
        ))
    })?;
    if format!("{:x}", Sha256::digest(&cached.bytes)) != reference.digest {
        return Err(invalid());
    }
    let snapshot: Snapshot = serde_json::from_slice(&cached.bytes).map_err(|_| invalid())?;
    if snapshot.subject.url() != reference.url()
        || crate::cache::now()
            .checked_sub(snapshot.observed_at)
            .is_none_or(|age| age >= 300)
    {
        return Err(invalid());
    }
    Ok(snapshot)
}

impl TryFrom<String> for SnapshotRef {
    type Error = Error;
    fn try_from(value: String) -> Result<Self> {
        value.parse()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CachedLog, PullRequest, status::CheckBucket};
    use std::cell::RefCell;
    use std::process::Output;

    #[derive(Default)]
    struct Memory(RefCell<Option<(String, Vec<u8>, u64)>>);
    impl Provider for Memory {
        fn gh(&self, _: &[&str]) -> Result<Output> {
            Err(Error::InvalidResponse(
                "snapshot must not query GitHub".into(),
            ))
        }
        fn local_head(&self) -> Result<Option<String>> {
            Ok(None)
        }
        fn saved_snapshot(&self, reference: &SnapshotRef) -> Option<CachedLog> {
            let value = self.0.borrow();
            let (key, bytes, fetched_at) = value.as_ref()?;
            (key == &reference.to_string()).then(|| CachedLog {
                bytes: bytes.clone(),
                fetched_at: *fetched_at,
            })
        }
        fn save_snapshot(&self, reference: &SnapshotRef, bytes: &[u8], observed_at: u64) -> bool {
            *self.0.borrow_mut() = Some((reference.to_string(), bytes.to_vec(), observed_at));
            true
        }
    }
    fn snapshot(observed_at: u64) -> Snapshot {
        Snapshot {
            subject: Subject::PullRequest {
                repository: "example/project".parse().unwrap(),
                pr: PullRequest {
                    url: "https://github.com/example/project/pull/7".into(),
                    number: 7,
                    state: "OPEN".into(),
                    head_ref_oid: "a".repeat(40),
                },
            },
            observed_at,
            warnings: vec!["Original warning".into()],
            checks: vec![Row {
                job: None,
                status: None,
                check_id: None,
                name: "tests".into(),
                workflow: "CI".into(),
                bucket: CheckBucket::Fail,
                link: "https://github.com/example/project/actions/runs/20/job/30".into(),
                required: Some(true),
            }],
        }
    }

    #[test]
    fn commit_and_run_snapshots_preserve_scope_and_never_query_github() {
        let repository: Repository = "example/project".parse().unwrap();
        for subject in [
            Subject::Commit {
                repository: repository.clone(),
                sha: "a".repeat(40),
                requested_ref: Some("main".into()),
            },
            Subject::Run {
                repository,
                id: 20,
                attempt: 2,
                sha: "a".repeat(40),
                name: "CI".into(),
                event: "schedule".into(),
                branch: Some("main".into()),
                status: "completed".into(),
                conclusion: Some("failure".into()),
            },
        ] {
            let provider = Memory::default();
            let mut observation = snapshot(crate::cache::now());
            let command = subject.command();
            observation.subject = subject;
            observation.checks.first_mut().unwrap().required = None;
            let reference = save(&provider, &observation).unwrap();
            assert_eq!(reference.refresh_command(), command);
            let result = crate::inspect_snapshot(&provider, &reference, 0, 1).unwrap();
            assert_eq!(result.subject.as_ref().unwrap().command(), command);
            assert!(result.subject.as_ref().unwrap().pr().is_none());
            assert!(result.checks.first().unwrap().evidence.is_none());
            assert!(
                result
                    .all_checks_command
                    .unwrap()
                    .contains("--snapshot snapshot:")
            );
        }
    }

    #[test]
    fn snapshot_pages_preserve_original_observation_and_never_fetch_evidence() {
        let provider = Memory::default();
        let observed = crate::cache::now().saturating_sub(10);
        let reference = save(&provider, &snapshot(observed)).unwrap();
        let result = crate::inspect_snapshot(&provider, &reference, 0, 1).unwrap();
        assert_eq!(result.observed_at, observed);
        assert_eq!(result.summary.failed, 1);
        assert!(matches!(result.source, crate::ObservationSource::Snapshot));
        assert!(result.checks.first().unwrap().evidence.is_none());
        assert!(result.warnings.contains(&"Original warning".into()));
        assert!(
            result
                .checks
                .first()
                .unwrap()
                .next_command
                .as_ref()
                .unwrap()
                .contains("logs job:")
        );
    }

    #[test]
    fn expired_missing_and_corrupt_snapshots_offer_fresh_query_without_network() {
        let provider = Memory::default();
        let reference = save(
            &provider,
            &snapshot(crate::cache::now().saturating_sub(300)),
        )
        .unwrap();
        assert!(
            load(&provider, &reference)
                .unwrap_err()
                .to_string()
                .contains("--pr https://github.com/example/project/pull/7")
        );
        let reference = save(&provider, &snapshot(crate::cache::now())).unwrap();
        provider.0.borrow_mut().as_mut().unwrap().1.push(b'x');
        assert!(load(&provider, &reference).is_err());
        *provider.0.borrow_mut() = None;
        assert!(load(&provider, &reference).is_err());
    }

    #[test]
    fn references_reject_paths_and_shell_input() {
        for reference in [
            "snapshot:github.com/example/../project:pull/7:aaaaaaaa",
            "snapshot:$(id)/example/project:pull/7:aaaaaaaa",
            "snapshot:github.com/example/project:pull/0:aaaaaaaa",
        ] {
            assert!(reference.parse::<SnapshotRef>().is_err());
        }
        let reference =
            SnapshotRef::create("https://github.com/example/project/pull/7", b"sample").unwrap();
        assert_eq!(
            reference
                .to_string()
                .parse::<SnapshotRef>()
                .unwrap()
                .to_string(),
            reference.to_string()
        );
    }
}
