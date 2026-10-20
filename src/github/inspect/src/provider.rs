use std::cell::RefCell;
use std::collections::HashMap;
use std::fs::File;
use std::path::Path;

use sha2::{Digest as _, Sha256};
use std::process::Output;
use std::time::{Duration, Instant};

use crate::{CachedLog, CheckRef, Error, Result, SnapshotRef};

/// Query boundary for live GitHub access and deterministic, hand-authored tests.
pub trait Provider {
    fn repository(&self) -> Result<crate::Repository> {
        Err(Error::InvalidReference(
            "supply --repo HOST/OWNER/REPO or a full URL".into(),
        ))
    }
    fn gh(&self, args: &[&str]) -> Result<Output>;
    fn gh_log(&self, args: &[&str], destination: &mut std::fs::File) -> Result<Output> {
        use std::io::Write as _;
        let mut output = self.gh(args)?;
        destination.write_all(&output.stdout)?;
        output.stdout.clear();
        Ok(output)
    }
    fn cached_log_file(
        &self,
        _reference: &CheckRef,
        _attempt: u64,
    ) -> Option<(std::fs::File, u64)> {
        None
    }
    fn cache_log_file(
        &self,
        _reference: &CheckRef,
        _attempt: u64,
        _file: &mut std::fs::File,
        _time: u64,
    ) -> bool {
        false
    }
    fn local_head_for(&self, _url: &str) -> Result<Option<String>> {
        self.local_head()
    }
    fn local_head(&self) -> Result<Option<String>>;
    fn saved_snapshot(&self, _reference: &SnapshotRef) -> Option<CachedLog> {
        None
    }
    fn read_snapshot(&self, reference: &SnapshotRef) -> Result<CachedLog> {
        self.saved_snapshot(reference).ok_or_else(|| {
            Error::InvalidResponse(
                "Saved observation is unavailable for this credential/cache location".into(),
            )
        })
    }
    fn save_snapshot(&self, _reference: &SnapshotRef, _bytes: &[u8], _observed_at: u64) -> bool {
        false
    }
}

/// Process boundary of [`GitHub`]: each `git` and `gh` invocation in its checkout.
///
/// [`Executables`] runs the installed tools with bounded output. Applications
/// substitute in-process replies to test everything above the process boundary.
pub trait Transport: std::fmt::Debug {
    fn git(&self, checkout: &Path, args: &[&str]) -> std::result::Result<Output, TransportError>;
    /// `timeout` is the remaining operation budget.
    fn gh(
        &self,
        checkout: &Path,
        args: &[&str],
        timeout: Duration,
    ) -> std::result::Result<Output, TransportError>;
    /// Stream stdout into `destination`; the returned stdout is empty.
    fn gh_log(
        &self,
        checkout: &Path,
        args: &[&str],
        timeout: Duration,
        destination: File,
    ) -> std::result::Result<Output, TransportError>;
}

pub use github_client::Error as TransportError;

/// The installed `git` and `gh`, through [`github_client::Client`].
#[derive(Debug, Clone, Copy, Default)]
pub struct Executables;

impl Transport for Executables {
    fn git(&self, checkout: &Path, args: &[&str]) -> std::result::Result<Output, TransportError> {
        github_client::Client.git(checkout, args)
    }
    fn gh(
        &self,
        checkout: &Path,
        args: &[&str],
        timeout: Duration,
    ) -> std::result::Result<Output, TransportError> {
        github_client::Client.gh_with_timeout(checkout, args, timeout)
    }
    fn gh_log(
        &self,
        checkout: &Path,
        args: &[&str],
        timeout: Duration,
        destination: File,
    ) -> std::result::Result<Output, TransportError> {
        github_client::Client.gh_log_to_file(checkout, args, timeout, destination)
    }
}

#[derive(Debug)]
pub struct GitHub<'a> {
    directory: &'a Path,
    transport: &'a dyn Transport,
    started: Instant,
    cache: Option<crate::cache::Cache>,
    snapshots: Option<crate::cache::Cache>,
    credentials: RefCell<HashMap<String, Option<Vec<u8>>>>,
}

impl<'a> GitHub<'a> {
    pub fn new(directory: &'a Path) -> Self {
        Self::with_transport(directory, &Executables)
    }

    /// Query through `transport` instead of the installed executables.
    pub fn with_transport(directory: &'a Path, transport: &'a dyn Transport) -> Self {
        Self {
            directory,
            transport,
            started: Instant::now(),
            cache: None,
            snapshots: None,
            credentials: RefCell::new(HashMap::new()),
        }
    }

    fn repositories(&self) -> Result<Vec<github_client::GitHubRepository>> {
        Ok(github_client::repositories_via(|args| {
            self.transport.git(self.directory, args)
        })?)
    }
    /// The host adapter chooses cache location; status and job metadata stay fresh.
    #[must_use]
    pub fn with_cache(mut self, directory: &Path) -> Self {
        self.cache = Some(crate::cache::Cache::new(directory.join("logs")));
        self.snapshots = Some(crate::cache::Cache::new(directory.join("snapshots")).with_ttl(300));
        self
    }

    fn credential_key(&self, host: &str, endpoint: &str, attempt: u64) -> Option<String> {
        self.cache.as_ref()?;
        let mut credentials = self.credentials.borrow_mut();
        let digest = credentials
            .entry(host.to_owned())
            .or_insert_with(|| {
                // Local credential lookup only. Never expose or persist the token;
                // hash it immediately, and disable caching when it is unavailable.
                let remaining = Duration::from_secs(45).checked_sub(self.started.elapsed())?;
                if remaining.is_zero() {
                    return None;
                }
                let output = self
                    .transport
                    .gh(
                        self.directory,
                        &["auth", "token", "--hostname", host],
                        remaining.min(Duration::from_secs(1)),
                    )
                    .ok()?;
                if !output.status.success() {
                    return None;
                }
                let token = String::from_utf8(output.stdout).ok()?;
                let token = token.trim();
                if token.is_empty() {
                    return None;
                }
                Some(Sha256::digest(token.as_bytes()).to_vec())
            })
            .as_deref()?;
        Some(crate::cache::key(digest, host, endpoint, attempt))
    }
}

impl Provider for GitHub<'_> {
    fn repository(&self) -> Result<crate::Repository> {
        let repositories = self.repositories()?;
        if let [repository] = repositories.as_slice() {
            return repository.display().parse();
        }
        Err(Error::InvalidReference(
            "checkout repository is absent or ambiguous; supply --repo or a full URL".into(),
        ))
    }
    fn gh(&self, args: &[&str]) -> Result<Output> {
        let remaining = Duration::from_secs(45)
            .checked_sub(self.started.elapsed())
            .filter(|duration| !duration.is_zero())
            .ok_or_else(|| {
                Error::InvalidResponse(
                    "CI query exceeded its 45-second budget; retry or use gh directly".into(),
                )
            })?;
        Ok(self.transport.gh(self.directory, args, remaining)?)
    }

    fn gh_log(&self, args: &[&str], destination: &mut std::fs::File) -> Result<Output> {
        let remaining = Duration::from_secs(45)
            .checked_sub(self.started.elapsed())
            .filter(|duration| !duration.is_zero())
            .ok_or_else(|| {
                Error::InvalidResponse("CI query exceeded its 45-second budget".into())
            })?;
        Ok(self
            .transport
            .gh_log(self.directory, args, remaining, destination.try_clone()?)?)
    }

    fn cached_log_file(&self, reference: &CheckRef, attempt: u64) -> Option<(std::fs::File, u64)> {
        self.cache.as_ref()?.get_file(&self.credential_key(
            &reference.repository.host,
            &reference.endpoint(),
            attempt,
        )?)
    }
    fn cache_log_file(
        &self,
        reference: &CheckRef,
        attempt: u64,
        file: &mut std::fs::File,
        time: u64,
    ) -> bool {
        if let (Some(cache), Some(key)) = (
            &self.cache,
            self.credential_key(&reference.repository.host, &reference.endpoint(), attempt),
        ) {
            return cache.put_file(&key, file, time);
        }
        false
    }

    fn local_head_for(&self, url: &str) -> Result<Option<String>> {
        let target = crate::Repository::from_url(url)?.0;
        let checkout = self
            .transport
            .git(self.directory, &["rev-parse", "--is-inside-work-tree"])?;
        if !checkout.status.success() {
            return Ok(None);
        }
        let repositories = self.repositories()?;
        if repositories
            .iter()
            .any(|repo| repo.display().eq_ignore_ascii_case(&target.to_string()))
        {
            self.local_head()
        } else {
            Ok(None)
        }
    }

    fn saved_snapshot(&self, reference: &SnapshotRef) -> Option<CachedLog> {
        let key = self.credential_key(reference.host(), &reference.to_string(), 0)?;
        self.snapshots.as_ref()?.get(&key)
    }

    fn read_snapshot(&self, reference: &SnapshotRef) -> Result<CachedLog> {
        let cache = self.snapshots.as_ref().ok_or_else(|| {
            Error::InvalidResponse("Saved-observation cache is disabled or unconfigured".into())
        })?;
        let key = self.credential_key(reference.host(), &reference.to_string(), 0).ok_or_else(|| Error::InvalidResponse("Cannot access the active GitHub credential for the saved observation; check gh authentication and sandbox/keychain access. GitHub was not queried".into()))?;
        cache.read(&key)
    }

    fn save_snapshot(&self, reference: &SnapshotRef, bytes: &[u8], observed_at: u64) -> bool {
        let Some(key) = self.credential_key(reference.host(), &reference.to_string(), 0) else {
            return false;
        };
        let Some(cache) = &self.snapshots else {
            return false;
        };
        cache.put(&key, bytes, observed_at);
        cache.get(&key).is_some()
    }

    fn local_head(&self) -> Result<Option<String>> {
        let result = self.transport.git(self.directory, &["rev-parse", "HEAD"])?;
        if !result.status.success() {
            return Ok(None);
        }
        Ok(Some(String::from_utf8_lossy(&result.stdout).trim().into()))
    }
}

pub(crate) fn checked(provider: &impl Provider, args: &[&str]) -> Result<Vec<u8>> {
    let result = provider.gh(args)?;
    if !result.status.success() {
        return Err(Error::Query {
            operation: args.join(" "),
            message: String::from_utf8_lossy(&result.stderr).trim().into(),
        });
    }
    Ok(result.stdout)
}
