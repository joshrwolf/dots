//! Backend-neutral freshness, caching and write scheduling.
use std::collections::BTreeSet;
use std::collections::btree_map::Entry;
use std::fmt;
use std::path::Path;
use std::time::{Duration, SystemTime};

use dots_documents::mutation_lock::MutationLock;

use crate::cache::Cache;
use crate::state::{Bucket, Directory, Known, Offline, age};
use crate::{
    Backend, Blob, Error, FlushAttempt, Hash, MachineId, Outcome, Pending, RefName, Result,
    Stranded, Update, Version, Watch, invalid,
};

/// A per-machine token bucket for deferred flushes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Budget {
    capacity: u32,
    refill: Duration,
}

impl Budget {
    /// Allow bursts of `capacity` flushes, regaining one every `refill`.
    pub fn new(capacity: u32, refill: Duration) -> Result<Self> {
        if capacity == 0 || refill.is_zero() {
            return Err(invalid("budget", "capacity and refill must be positive"));
        }
        Ok(Self { capacity, refill })
    }

    /// Take one token, or report when the next one becomes available.
    fn take(self, bucket: &mut Option<Bucket>, now: SystemTime) -> Option<SystemTime> {
        let bucket = bucket.get_or_insert(Bucket {
            tokens: self.capacity,
            at: now,
        });
        let refills = age(now, bucket.at).as_nanos() / self.refill.as_nanos();
        if refills > 0 {
            let tokens = (u128::from(bucket.tokens) + refills).min(u128::from(self.capacity));
            bucket.tokens = u32::try_from(tokens).unwrap_or(self.capacity);
            bucket.at = if bucket.tokens == self.capacity {
                now
            } else {
                // Fewer refills than the capacity, so the count fits.
                bucket.at + self.refill * u32::try_from(refills).unwrap_or(u32::MAX)
            };
        }
        if bucket.tokens == 0 {
            return Some(bucket.at + self.refill);
        }
        bucket.tokens -= 1;
        None
    }
}

impl Default for Budget {
    /// Two flushes per minute, keeping a few machines under GitHub's
    /// recommended six pushes per minute per repository.
    fn default() -> Self {
        Self {
            capacity: 2,
            refill: Duration::from_secs(30),
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Config {
    pub budget: Budget,
}

/// How current the listed versions are.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Freshness {
    /// Every ref version was confirmed at `as_of`.
    Current { as_of: SystemTime },
    /// The backend is unreachable; versions date from the last listing, if any.
    Stale {
        since: Option<SystemTime>,
        reason: String,
    },
}

/// A ref's value as read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Value {
    pub version: Version,
    pub root: Hash,
    /// When offline: the last time this value was known current.
    pub stale_since: Option<SystemTime>,
}

/// What a flush did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Flush {
    /// Nothing was pending.
    Idle,
    /// Another process is flushing this state directory.
    Busy,
    /// The push budget is spent until `retry_at`.
    Throttled {
        retry_at: SystemTime,
    },
    Sent(FlushReport),
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FlushReport {
    pub applied: Vec<(RefName, Version)>,
    /// Pre-empted by another writer; see [`Status::stranded`].
    pub stranded: Vec<RefName>,
    /// Still pending because the outcome is unknown.
    pub retained: Vec<RefName>,
    pub error: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Status {
    pub pending: Vec<Pending>,
    pub stranded: Vec<Stranded>,
    pub last_flush: Option<FlushAttempt>,
    /// When ref versions were last listed.
    pub listed_at: Option<SystemTime>,
    /// Since when listing has failed, if it does.
    pub offline_since: Option<SystemTime>,
}

/// One machine's view of a backend: blob cache, shared freshness record,
/// outbox of deferred updates and machine identity, all in one state
/// directory that any number of processes may open.
///
/// Time-dependent operations take `now` explicitly. Nothing runs in the
/// background.
pub struct Substrate {
    backend: Box<dyn Backend>,
    directory: Directory,
    cache: Cache,
    machine: MachineId,
    config: Config,
    watch: Option<Box<dyn Watch>>,
    /// Whether this process listed after attaching its watch.
    watch_listed: bool,
}

impl fmt::Debug for Substrate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Substrate")
            .field("directory", &self.directory)
            .field("machine", &self.machine)
            .field("config", &self.config)
            .field("watch", &self.watch.is_some())
            .finish_non_exhaustive()
    }
}

impl Substrate {
    /// Open or create the state directory at `root`.
    pub fn open(root: &Path, backend: Box<dyn Backend>, config: Config) -> Result<Self> {
        let directory = Directory::open(root)?;
        let machine = directory.machine()?;
        Ok(Self {
            backend,
            cache: Cache::new(root.join("blobs")),
            directory,
            machine,
            config,
            watch: None,
            watch_listed: false,
        })
    }

    /// Use `watch` to satisfy freshness without listing while it is healthy.
    pub fn attach_watch(&mut self, watch: Box<dyn Watch>) {
        self.watch = Some(watch);
        self.watch_listed = false;
    }

    pub fn machine(&self) -> &MachineId {
        &self.machine
    }

    /// Make the listed versions no older than `max_age`, listing only when
    /// neither the shared record nor a healthy watch already vouches for them.
    /// Any pending hint forces a listing. A listing that failed within
    /// `max_age` is not retried, so an unreachable backend costs callers that
    /// tolerate stale versions at most one attempt per `max_age`.
    pub fn ensure_fresh(&mut self, max_age: Duration, now: SystemTime) -> Result<Freshness> {
        let hinted = self
            .watch
            .as_mut()
            .is_some_and(|watch| !watch.hints().is_empty());
        if !hinted {
            let (listed_at, offline) = self
                .directory
                .read(|state| (state.listed_at, state.offline.clone()))?;
            if let Some(at) = listed_at
                && age(now, at) <= max_age
            {
                return Ok(Freshness::Current { as_of: at });
            }
            if let Some(offline) = offline
                && age(now, offline.checked_at) <= max_age
            {
                return Ok(Freshness::Stale {
                    since: listed_at,
                    reason: offline.reason,
                });
            }
            if self.watch_listed
                && let Some(at) = self.watch.as_ref().and_then(|w| w.confirmed_as_of())
                && age(now, at) <= max_age
            {
                return Ok(Freshness::Current { as_of: at });
            }
        }
        match self.backend.list() {
            Ok(listed) => {
                self.directory.modify(|state| {
                    state.listed = listed;
                    state.listed_at = Some(now);
                    state.offline = None;
                    Ok(())
                })?;
                self.watch_listed = self.watch.is_some();
                Ok(Freshness::Current { as_of: now })
            }
            Err(Error::Unavailable(reason)) => {
                let since = self.directory.modify(|state| {
                    let since = state.offline.as_ref().map_or(now, |offline| offline.since);
                    state.offline = Some(Offline {
                        since,
                        checked_at: now,
                        reason: reason.clone(),
                    });
                    Ok(state.listed_at)
                })?;
                Ok(Freshness::Stale { since, reason })
            }
            Err(error) => Err(error),
        }
    }

    /// A ref's value at its listed version, fetching it if the cache lacks it.
    /// Before any listing, fetches the current value. Offline, returns the
    /// cached value marked stale.
    pub fn read(&self, name: &RefName, now: SystemTime) -> Result<Option<Value>> {
        let (listed_at, offline, listed, known) = self.directory.read(|state| {
            (
                state.listed_at,
                state.offline.is_some(),
                state.listed.get(name).cloned(),
                state.known.get(name).cloned(),
            )
        })?;
        if listed_at.is_some() {
            let Some(version) = listed else {
                return Ok(None);
            };
            if let Some(known) = &known
                && known.version == version
            {
                return Ok(Some(Value {
                    version,
                    root: known.root,
                    stale_since: offline
                        .then(|| listed_at.map_or(known.at, |listed| listed.max(known.at))),
                }));
            }
        }
        let stale = |reason: String| match &known {
            Some(known) => Ok(Some(Value {
                version: known.version.clone(),
                root: known.root,
                stale_since: Some(known.at),
            })),
            None => Err(Error::Unavailable(reason)),
        };
        if offline {
            return stale(format!("{name} is not cached and the backend is offline"));
        }
        match self.fetch(name, now) {
            Ok(None) if listed_at.is_some() => Err(Error::Backend(format!(
                "{name} was listed but the backend reports it absent"
            ))),
            Err(Error::Unavailable(reason)) => stale(reason),
            result => result,
        }
    }

    /// A cached blob, verified against its hash.
    pub fn blob(&self, hash: &Hash) -> Result<Blob> {
        self.cache.get(hash)
    }

    /// Compare-and-swap now. An unknown backend outcome is resolved by listing:
    /// [`Outcome::Unknown`] remains only when the ref still holds the expected
    /// version or the backend is unreachable; re-sending the same update is safe.
    pub fn update(&self, update: &Update, now: SystemTime) -> Result<Outcome> {
        self.store(&update.new_blobs)?;
        let outcome = match self.send(std::slice::from_ref(update))?.pop() {
            Some(Outcome::Unknown) => self.resolve(update, now)?,
            Some(outcome) => outcome,
            None => return Err(Error::Backend("update returned no outcome".into())),
        };
        self.directory.modify(|state| {
            match &outcome {
                Outcome::Applied(version) => {
                    state.known.insert(
                        update.name.clone(),
                        Known {
                            version: version.clone(),
                            root: update.root,
                            at: now,
                        },
                    );
                    state.observe(&update.name, Some(version));
                }
                Outcome::Conflict { actual } => state.observe(&update.name, actual.as_ref()),
                Outcome::Unknown => {}
            }
            Ok(())
        })?;
        Ok(outcome)
    }

    /// Record an update for the next flush. A pending update of the same ref
    /// absorbs it: the new root replaces the old one, blobs accumulate and the
    /// original expectation stays, since a single writer's base cannot move.
    pub fn defer(&self, update: Update, now: SystemTime) -> Result<()> {
        self.store(&update.new_blobs)?;
        let blobs = update.new_blobs.iter().map(Blob::hash);
        self.directory.modify(|state| {
            match state.outbox.entry(update.name.clone()) {
                Entry::Occupied(mut entry) => {
                    let pending = entry.get_mut();
                    pending.root = update.root;
                    pending.blobs.extend(blobs);
                }
                Entry::Vacant(entry) => {
                    entry.insert(Pending {
                        name: update.name,
                        expected: update.expected,
                        root: update.root,
                        blobs: blobs.collect(),
                        since: now,
                    });
                }
            }
            Ok(())
        })
    }

    /// Send every pending update in one backend batch if the budget allows.
    /// A pre-empted update becomes a stranded entry; it is never dropped.
    pub fn flush(&self, now: SystemTime) -> Result<Flush> {
        let _flushing = match MutationLock::acquire(&self.directory.root().join("flush.lock")) {
            Ok(lock) => lock,
            Err(dots_documents::Error::Busy) => return Ok(Flush::Busy),
            Err(error) => return Err(error.into()),
        };
        let budget = self.config.budget;
        let batch = self.directory.modify(|state| {
            if state.outbox.is_empty() {
                return Ok(Err(Flush::Idle));
            }
            if let Some(retry_at) = budget.take(&mut state.bucket, now) {
                return Ok(Err(Flush::Throttled { retry_at }));
            }
            Ok(Ok(state.outbox.values().cloned().collect::<Vec<_>>()))
        })?;
        let batch = match batch {
            Ok(batch) => batch,
            Err(flush) => return Ok(flush),
        };
        let updates = batch
            .iter()
            .map(|pending| {
                Ok(Update {
                    name: pending.name.clone(),
                    expected: pending.expected.clone(),
                    root: pending.root,
                    new_blobs: pending
                        .blobs
                        .iter()
                        .map(|hash| self.cache.get(hash))
                        .collect::<Result<_>>()?,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let outcomes = match self.send(&updates) {
            Ok(outcomes) => outcomes,
            Err(error) => {
                let reason = error.to_string();
                self.directory.modify(|state| {
                    state.last_flush = Some(FlushAttempt {
                        at: now,
                        error: Some(reason.clone()),
                    });
                    Ok(())
                })?;
                let Error::Unavailable(_) = error else {
                    return Err(error);
                };
                return Ok(Flush::Sent(FlushReport {
                    retained: batch.into_iter().map(|pending| pending.name).collect(),
                    error: Some(reason),
                    ..FlushReport::default()
                }));
            }
        };
        let outcomes = updates
            .iter()
            .zip(outcomes)
            .map(|(update, outcome)| match outcome {
                Outcome::Unknown => self.resolve(update, now),
                outcome => Ok(outcome),
            })
            .collect::<Result<Vec<_>>>()?;
        self.settle(&batch, outcomes, now)
    }

    /// Apply a flushed batch's outcomes to the outbox. Entries coalesced while
    /// in flight rebase onto what landed, or strand on conflict.
    fn settle(&self, batch: &[Pending], outcomes: Vec<Outcome>, now: SystemTime) -> Result<Flush> {
        self.directory.modify(|state| {
            let mut report = FlushReport::default();
            for (sent, outcome) in batch.iter().zip(outcomes) {
                let name = &sent.name;
                match outcome {
                    Outcome::Applied(version) => {
                        state.known.insert(
                            name.clone(),
                            Known {
                                version: version.clone(),
                                root: sent.root,
                                at: now,
                            },
                        );
                        state.observe(name, Some(&version));
                        if state.outbox.get(name) == Some(sent) {
                            state.outbox.remove(name);
                        } else if let Some(newer) = state.outbox.get_mut(name) {
                            newer.expected = Some(version.clone());
                            newer.blobs.retain(|hash| !sent.blobs.contains(hash));
                        }
                        report.applied.push((name.clone(), version));
                    }
                    Outcome::Conflict { actual } => {
                        state.observe(name, actual.as_ref());
                        let latest = state.outbox.remove(name).unwrap_or_else(|| sent.clone());
                        state.stranded.push(Stranded {
                            name: name.clone(),
                            expected: latest.expected,
                            actual,
                            root: latest.root,
                            blobs: latest.blobs,
                            at: now,
                        });
                        report.stranded.push(name.clone());
                    }
                    Outcome::Unknown => report.retained.push(name.clone()),
                }
            }
            if !report.retained.is_empty() {
                report.error = Some(format!(
                    "{} update outcomes are unknown",
                    report.retained.len()
                ));
            }
            state.last_flush = Some(FlushAttempt {
                at: now,
                error: report.error.clone(),
            });
            Ok(Flush::Sent(report))
        })
    }

    /// Pending and stranded updates and the last flush result.
    pub fn status(&self) -> Result<Status> {
        self.directory.read(|state| Status {
            pending: state.outbox.values().cloned().collect(),
            stranded: state.stranded.clone(),
            last_flush: state.last_flush.clone(),
            listed_at: state.listed_at,
            offline_since: state.offline.as_ref().map(|offline| offline.since),
        })
    }

    /// Cache a blob no update carries yet, such as a domain's local candidate.
    /// It stays local until an update that carries it.
    pub fn put(&self, blob: &Blob) -> Result<()> {
        self.cache.put(blob)
    }

    fn store(&self, blobs: &[Blob]) -> Result<()> {
        blobs.iter().try_for_each(|blob| self.cache.put(blob))
    }

    fn send(&self, updates: &[Update]) -> Result<Vec<Outcome>> {
        let mut names = BTreeSet::new();
        if let Some(update) = updates.iter().find(|update| !names.insert(&update.name)) {
            return Err(invalid(
                "update batch",
                format!("{} appears twice", update.name),
            ));
        }
        let outcomes = self.backend.update(updates)?;
        if outcomes.len() != updates.len() {
            return Err(Error::Backend(format!(
                "{} outcomes for {} updates",
                outcomes.len(),
                updates.len()
            )));
        }
        Ok(outcomes)
    }

    /// Fetch a ref's current value into the cache and record it as known.
    fn fetch(&self, name: &RefName, now: SystemTime) -> Result<Option<Value>> {
        let cache = &self.cache;
        let Some(fetched) = self.backend.fetch(name, &|hash| cache.contains(hash))? else {
            return Ok(None);
        };
        for (hash, content) in fetched.blobs {
            cache.put(&Blob::verified(hash, content)?)?;
        }
        if !cache.contains(&fetched.root) {
            return Err(Error::MissingBlob(fetched.root));
        }
        self.directory.modify(|state| {
            state.known.insert(
                name.clone(),
                Known {
                    version: fetched.version.clone(),
                    root: fetched.root,
                    at: now,
                },
            );
            state.observe(name, Some(&fetched.version));
            Ok(())
        })?;
        Ok(Some(Value {
            version: fetched.version,
            root: fetched.root,
            stale_since: None,
        }))
    }

    /// Settle a lost outcome by comparing the ref's current value with the proposal.
    fn resolve(&self, update: &Update, now: SystemTime) -> Result<Outcome> {
        let listed = match self.backend.list() {
            Ok(listed) => listed,
            Err(Error::Unavailable(_)) => return Ok(Outcome::Unknown),
            Err(error) => return Err(error),
        };
        let current = listed.get(&update.name);
        if current == update.expected.as_ref() {
            return Ok(Outcome::Unknown);
        }
        if current.is_none() {
            return Ok(Outcome::Conflict { actual: None });
        }
        match self.fetch(&update.name, now) {
            Ok(Some(value)) if value.root == update.root => Ok(Outcome::Applied(value.version)),
            Ok(value) => Ok(Outcome::Conflict {
                actual: value.map(|value| value.version),
            }),
            Err(Error::Unavailable(_)) => Ok(Outcome::Unknown),
            Err(error) => Err(error),
        }
    }
}
