//! Backend-neutral sync passes, local reads and write scheduling.
use std::collections::BTreeSet;
use std::fmt;
use std::fs::{File, OpenOptions, TryLockError};
use std::io::Write as _;
use std::os::unix::fs::OpenOptionsExt as _;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::{Duration, SystemTime};

use crate::cache::Cache;
use crate::state::{Counter, Counters, Database, Known, Record, State, age};
use crate::{
    Backend, Blob, Error, FlushAttempt, Hash, MachineId, Maintenance, Outcome, Pending, RefName,
    Result, Stranded, Update, Version, Watch, invalid, io,
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

    /// Refill the bucket for the time elapsed by `now`.
    fn refill(self, sync: &mut Record, now: SystemTime) {
        let (tokens, at) = match (sync.tokens, sync.bucket_at) {
            (Some(tokens), Some(at)) => (tokens, at),
            _ => (self.capacity, now),
        };
        let refills = age(now, at).as_nanos() / self.refill.as_nanos();
        let tokens = (u128::from(tokens) + refills).min(u128::from(self.capacity));
        // At most the capacity, which fits.
        let tokens = u32::try_from(tokens).unwrap_or(self.capacity);
        let at = if tokens == self.capacity {
            now
        } else {
            // Fewer refills than the capacity, so the count fits.
            at + self.refill * u32::try_from(refills).unwrap_or(u32::MAX)
        };
        sync.tokens = Some(tokens);
        sync.bucket_at = Some(at);
    }

    /// Take one token, or report when the next one becomes available.
    fn take(self, sync: &mut Record, now: SystemTime) -> Option<SystemTime> {
        self.refill(sync, now);
        match sync.tokens {
            Some(tokens) if tokens > 0 => {
                sync.tokens = Some(tokens - 1);
                None
            }
            _ => Some(sync.bucket_at.unwrap_or(now) + self.refill),
        }
    }

    /// Whether a token is available at `now`, without taking it.
    fn available(self, sync: &Record, now: SystemTime) -> bool {
        let mut sync = sync.clone();
        self.refill(&mut sync, now);
        sync.tokens.is_some_and(|tokens| tokens > 0)
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

/// How long scheduled passes wait after consecutive failures: doubling from
/// `base` up to `cap`, each delay drawn uniformly from its upper half.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Backoff {
    pub base: Duration,
    pub cap: Duration,
}

impl Backoff {
    fn delay(self, failures: u32) -> Result<Duration> {
        let doubled = self
            .base
            .saturating_mul(1 << failures.saturating_sub(1).min(16));
        jitter(doubled.min(self.cap))
    }
}

impl Default for Backoff {
    fn default() -> Self {
        Self {
            base: Duration::from_secs(5),
            cap: Duration::from_secs(300),
        }
    }
}

/// A duration drawn uniformly from `[limit / 2, limit]`.
pub(crate) fn jitter(limit: Duration) -> Result<Duration> {
    let half = limit / 2;
    let spread = u64::try_from(limit.saturating_sub(half).as_millis()).unwrap_or(u64::MAX);
    let extra = if spread == 0 {
        0
    } else {
        getrandom::u64()? % (spread + 1)
    };
    Ok(half + Duration::from_millis(extra))
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Config {
    pub budget: Budget,
    pub backoff: Backoff,
}

/// When scheduled passes run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Schedule {
    /// The polling period.
    pub interval: Duration,
    /// The full-resync period while a healthy watch vouches for changes.
    pub resync: Duration,
}

impl Default for Schedule {
    fn default() -> Self {
        Self {
            interval: Duration::from_secs(20),
            resync: Duration::from_secs(600),
        }
    }
}

/// Who asked for a pass.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Trigger {
    /// A timer, hint or local write: skipped while another process syncs or
    /// while failures back off.
    Scheduled,
    /// A caller that needs current state now: waits for a pass in progress
    /// and ignores backoff.
    Explicit,
}

/// What a sync pass request did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Pass {
    /// Another process is running a pass.
    Busy,
    /// Earlier failures back off scheduled passes until `retry_at`.
    Backoff {
        retry_at: SystemTime,
    },
    Done(PassReport),
}

/// One completed pass.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PassReport {
    /// Why the backend was unreachable; the pass stopped there.
    pub offline: Option<String>,
    /// Refs whose new values were fetched into the cache.
    pub fetched: Vec<RefName>,
    pub flush: Flush,
}

/// What a pass's flush did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Flush {
    /// Nothing was pending.
    Idle,
    /// The backend was unreachable before the flush.
    Skipped,
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

/// A ref's value from the local copy.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Value {
    pub version: Version,
    pub root: Hash,
    /// When this value was last confirmed current.
    pub as_of: SystemTime,
}

/// The current run of failed passes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Failing {
    pub since: SystemTime,
    pub failures: u32,
    pub reason: String,
    /// Scheduled passes wait until then.
    pub retry_at: Option<SystemTime>,
}

/// How the local copy was last synchronized.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Health {
    /// When ref versions were last listed.
    pub listed_at: Option<SystemTime>,
    pub pass_started_at: Option<SystemTime>,
    pub pass_finished_at: Option<SystemTime>,
    pub failing: Option<Failing>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Status {
    pub pending: Vec<Pending>,
    pub stranded: Vec<Stranded>,
    pub last_flush: Option<FlushAttempt>,
    pub health: Health,
    pub counters: Counters,
}

struct Watching {
    watch: Option<Box<dyn Watch>>,
    /// Whether a pass listed after the watch was attached.
    listed: bool,
    /// Whether a hint arrived since the last pass.
    hinted: bool,
}

/// One machine's view of a backend: blob cache, shared sync record, outbox of
/// deferred updates and machine identity, all in one state directory that any
/// number of processes may open.
///
/// Reads use only the local copy. A sync pass refreshes it: list, fetch
/// changed refs, flush the outbox. Time-dependent operations take `now`
/// explicitly; nothing runs in the background.
pub struct Substrate {
    backend: Box<dyn Backend>,
    root: PathBuf,
    database: Mutex<Database>,
    cache: Cache,
    machine: MachineId,
    config: Config,
    watching: Mutex<Watching>,
}

impl fmt::Debug for Substrate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Substrate")
            .field("root", &self.root)
            .field("machine", &self.machine)
            .field("config", &self.config)
            .finish_non_exhaustive()
    }
}

const PASS_LOCK: &str = "sync.lock";
const MAINTENANCE_LOCK: &str = "maintenance.lock";
/// Touched whenever the local copy or its health changes, for watchers such as Pi.
const CHANGES: &str = "changes";

impl Substrate {
    /// Open or create the state directory at `root`.
    pub fn open(root: &Path, backend: Box<dyn Backend>, config: Config) -> Result<Self> {
        let mut database = Database::open(root)?;
        #[expect(
            clippy::redundant_closure_for_method_calls,
            reason = "the method path fixes the state's lifetime"
        )]
        let machine = database.modify(|state| state.machine())?;
        Ok(Self {
            backend,
            root: root.to_owned(),
            database: Mutex::new(database),
            cache: Cache::new(root.join("blobs")),
            machine,
            config,
            watching: Mutex::new(Watching {
                watch: None,
                listed: false,
                hinted: false,
            }),
        })
    }

    /// Schedule passes on `watch`'s hints while it vouches for its coverage.
    pub fn attach_watch(&self, watch: Box<dyn Watch>) {
        *self.watching() = Watching {
            watch: Some(watch),
            listed: false,
            hinted: false,
        };
    }

    pub fn machine(&self) -> &MachineId {
        &self.machine
    }

    /// The file touched whenever the local copy or its health changes.
    pub fn changes(&self) -> PathBuf {
        self.root.join(CHANGES)
    }

    /// Fail with [`Error::Schema`] if a newer binary changed the state schema.
    pub fn check_schema(&self) -> Result<()> {
        self.database().check()
    }

    fn database(&self) -> MutexGuard<'_, Database> {
        self.database.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn watching(&self) -> MutexGuard<'_, Watching> {
        self.watching.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn read_state<T>(&self, read: impl FnOnce(&State<'_>) -> Result<T>) -> Result<T> {
        self.database().read(read)
    }

    fn modify<T>(&self, change: impl FnOnce(&State<'_>) -> Result<T>) -> Result<T> {
        self.database().modify(change)
    }

    /// A ref's value in the local copy, as of the last sync pass or local
    /// update. Never contacts the backend. `None` when the last listing did
    /// not include it; [`Error::Unsynced`] before this machine ever synced.
    pub fn read(&self, name: &RefName) -> Result<Option<Value>> {
        self.read_state(|state| {
            let known = state.known(name)?;
            let value = |known: Known, as_of| Value {
                version: known.version,
                root: known.root,
                as_of,
            };
            let Some(listed_at) = state.sync()?.listed_at else {
                return known
                    .map(|known| {
                        let at = known.at;
                        value(known, at)
                    })
                    .map(Some)
                    .ok_or(Error::Unsynced);
            };
            let Some(version) = state.listed_version(name)? else {
                return Ok(None);
            };
            match known {
                Some(known) if known.version == version => {
                    let as_of = known.at.max(listed_at);
                    Ok(Some(value(known, as_of)))
                }
                // A newer version is listed but not fetched yet.
                Some(known) => {
                    let at = known.at;
                    Ok(Some(value(known, at)))
                }
                None => Err(Error::NotCached(name.clone())),
            }
        })
    }

    /// A cached blob, verified against its hash. A damaged blob is removed
    /// and the next pass refetches every ref.
    pub fn blob(&self, hash: &Hash) -> Result<Blob> {
        let blob = self.cache.get(hash);
        if let Err(Error::Integrity { .. }) = &blob {
            self.modify(|state| state.update_sync(|sync| sync.repair = true).map(drop))?;
        }
        blob
    }

    /// How the local copy was last synchronized.
    pub fn health(&self) -> Result<Health> {
        self.read_state(|state| Ok(health(&state.sync()?)))
    }

    /// Whether a scheduled pass is due at `now`: a hint arrived, another
    /// process deferred updates that the budget can flush, or the schedule's
    /// period passed since the last pass by any process.
    pub fn due(&self, schedule: Schedule, now: SystemTime) -> Result<bool> {
        let vouched = {
            let mut watching = self.watching();
            let hints = watching
                .watch
                .as_mut()
                .is_some_and(|watch| !watch.hints().is_empty());
            watching.hinted |= hints;
            if watching.hinted {
                return Ok(true);
            }
            watching.listed
                && watching
                    .watch
                    .as_ref()
                    .and_then(|watch| watch.confirmed_as_of())
                    .is_some_and(|at| age(now, at) <= schedule.interval)
        };
        let budget = self.config.budget;
        let mut database = self.database();
        let written = database.changed_elsewhere()?;
        database.read(|state| {
            let sync = state.sync()?;
            if written && !state.outbox()?.is_empty() && budget.available(&sync, now) {
                return Ok(true);
            }
            let period = if vouched {
                schedule.resync
            } else {
                schedule.interval
            };
            Ok(sync.pass_started_at.is_none_or(|at| age(now, at) >= period))
        })
    }

    /// Run one sync pass: list every ref, fetch changed values into the cache,
    /// flush the outbox within the budget, and record the result. One pass
    /// runs at a time per state directory; passes are idempotent.
    ///
    /// An unreachable backend is a completed pass with
    /// [`PassReport::offline`] set, and backs off later scheduled passes.
    pub fn sync(&self, trigger: Trigger, now: SystemTime) -> Result<Pass> {
        let lock = self.lock_file(PASS_LOCK)?;
        match trigger {
            Trigger::Scheduled => match lock.try_lock() {
                Ok(()) => {}
                Err(TryLockError::WouldBlock) => return Ok(Pass::Busy),
                Err(TryLockError::Error(source)) => {
                    return Err(io("lock sync pass", &self.root.join(PASS_LOCK), source));
                }
            },
            Trigger::Explicit => lock
                .lock()
                .map_err(|source| io("lock sync pass", &self.root.join(PASS_LOCK), source))?,
        }
        let before = self.modify(|state| {
            let sync = state.sync()?;
            if trigger == Trigger::Scheduled
                && let Some(retry_at) = sync.retry_at
                && now < retry_at
            {
                return Ok(Err(Pass::Backoff { retry_at }));
            }
            state.count(Counter::Passes, 1)?;
            state.update_sync(|sync| sync.pass_started_at = Some(now))?;
            Ok(Ok(sync))
        })?;
        let before = match before {
            Ok(before) => before,
            Err(pass) => return Ok(pass),
        };
        let result = self.pass(before.repair, now);
        let failure = match &result {
            Ok(report) => report.offline.clone(),
            Err(error) => Some(error.to_string()),
        };
        let backoff = match &failure {
            Some(_) => Some(
                self.config
                    .backoff
                    .delay(before.failures.saturating_add(1))?,
            ),
            None => None,
        };
        self.modify(|state| {
            if failure.is_some() {
                state.count(Counter::FailedPasses, 1)?;
            }
            state.update_sync(|sync| {
                sync.pass_finished_at = Some(now);
                if let (Some(reason), Some(delay)) = (&failure, backoff) {
                    sync.failing_since.get_or_insert(now);
                    sync.failures = sync.failures.saturating_add(1);
                    sync.last_error = Some(reason.clone());
                    sync.retry_at = Some(now + delay);
                } else {
                    sync.failing_since = None;
                    sync.failures = 0;
                    sync.last_error = None;
                    sync.retry_at = None;
                    if before.repair {
                        sync.repair = false;
                    }
                }
            })?;
            Ok(())
        })?;
        let flipped = before.failing_since.is_some() != failure.is_some();
        if flipped
            || result
                .as_ref()
                .is_ok_and(|report| !report.fetched.is_empty())
        {
            self.notify()?;
        }
        result.map(Pass::Done)
    }

    fn pass(&self, repair: bool, now: SystemTime) -> Result<PassReport> {
        if let Some(watch) = self.watching().watch.as_mut() {
            // This pass lists everything the hints could name.
            watch.hints();
        }
        self.watching().hinted = false;
        let offline = |offline: String, fetched| PassReport {
            offline: Some(offline),
            fetched,
            flush: Flush::Skipped,
        };
        let listed = match self.backend.list() {
            Ok(listed) => listed,
            Err(Error::Unavailable(reason)) => {
                self.modify(|state| state.count(Counter::Lists, 1))?;
                return Ok(offline(reason, Vec::new()));
            }
            Err(error) => return Err(error),
        };
        self.modify(|state| {
            state.count(Counter::Lists, 1)?;
            state.set_listed(&listed, now)
        })?;
        {
            let mut watching = self.watching();
            watching.listed = watching.watch.is_some();
        }
        let mut fetched = Vec::new();
        for (name, version) in &listed {
            let known = self.read_state(|state| state.known(name))?;
            let current = known
                .is_some_and(|known| known.version == *version && self.cache.contains(&known.root));
            if current && !repair {
                continue;
            }
            match self.fetch(name, now) {
                Ok(Some(_)) => fetched.push(name.clone()),
                // Gone since the listing; the next pass lists it again.
                Ok(None) => {}
                Err(Error::Unavailable(reason)) => return Ok(offline(reason, fetched)),
                Err(error) => return Err(error),
            }
        }
        let flush = self.flush(now)?;
        Ok(PassReport {
            offline: None,
            fetched,
            flush,
        })
    }

    /// Run the backend's local upkeep unless another process is doing so.
    pub fn maintain(&self) -> Result<Maintenance> {
        let lock = self.lock_file(MAINTENANCE_LOCK)?;
        match lock.try_lock() {
            Ok(()) => {}
            Err(TryLockError::WouldBlock) => return Ok(Maintenance::Skipped),
            Err(TryLockError::Error(source)) => {
                return Err(io(
                    "lock maintenance",
                    &self.root.join(MAINTENANCE_LOCK),
                    source,
                ));
            }
        }
        let maintenance = self.backend.maintain()?;
        if let Maintenance::Ran(_) = maintenance {
            self.modify(|state| state.count(Counter::Maintenance, 1))?;
        }
        Ok(maintenance)
    }

    fn lock_file(&self, name: &str) -> Result<File> {
        let path = self.root.join(name);
        OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .open(&path)
            .map_err(|source| io("open substrate lock", &path, source))
    }

    /// Touch [`Substrate::changes`], telling watchers to reread local state.
    pub fn notify(&self) -> Result<()> {
        let path = self.changes();
        OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&path)
            .and_then(|mut file| writeln!(file, "{:016x}", getrandom::u64().unwrap_or_default()))
            .map_err(|source| io("touch substrate changes", &path, source))
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
        self.modify(|state| match &outcome {
            Outcome::Applied(version) => {
                state.set_known(
                    &update.name,
                    &Known {
                        version: version.clone(),
                        root: update.root,
                        at: now,
                    },
                )?;
                state.observe(&update.name, Some(version))
            }
            Outcome::Conflict { actual } => state.observe(&update.name, actual.as_ref()),
            Outcome::Unknown => Ok(()),
        })?;
        if let Outcome::Applied(_) = outcome {
            self.notify()?;
        }
        Ok(outcome)
    }

    /// Record an update for the next flush and touch the changes marker. A
    /// pending update of the same ref absorbs it: the new root replaces the
    /// old one, blobs accumulate and the original expectation stays, since a
    /// single writer's base cannot move.
    pub fn defer(&self, update: Update, now: SystemTime) -> Result<()> {
        self.store(&update.new_blobs)?;
        let blobs = update.new_blobs.iter().map(Blob::hash);
        self.modify(|state| {
            let pending = match state.pending(&update.name)? {
                Some(mut pending) => {
                    pending.root = update.root;
                    pending.blobs.extend(blobs);
                    pending.revision += 1;
                    pending
                }
                None => Pending {
                    name: update.name,
                    expected: update.expected,
                    root: update.root,
                    blobs: blobs.collect(),
                    since: now,
                    revision: 0,
                },
            };
            state.set_pending(&pending)
        })?;
        // Wake the sync agent to flush it.
        self.notify()
    }

    /// Send every pending update in one backend batch if the budget allows.
    /// A pre-empted update becomes a stranded entry; it is never dropped.
    /// The caller holds the pass lock.
    fn flush(&self, now: SystemTime) -> Result<Flush> {
        let budget = self.config.budget;
        let batch = self.modify(|state| {
            let outbox = state.outbox()?;
            if outbox.is_empty() {
                return Ok(Err(Flush::Idle));
            }
            let mut sync = state.sync()?;
            if let Some(retry_at) = budget.take(&mut sync, now) {
                state.count(Counter::Throttled, 1)?;
                return Ok(Err(Flush::Throttled { retry_at }));
            }
            state.set_sync(&sync)?;
            Ok(Ok(outbox))
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
                self.modify(|state| {
                    state.update_sync(|sync| {
                        sync.flush = Some(FlushAttempt {
                            at: now,
                            error: Some(reason.clone()),
                        });
                    })
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
        let flush = self.modify(|state| {
            let mut report = FlushReport::default();
            for (sent, outcome) in batch.iter().zip(outcomes) {
                let name = &sent.name;
                match outcome {
                    Outcome::Applied(version) => {
                        state.set_known(
                            name,
                            &Known {
                                version: version.clone(),
                                root: sent.root,
                                at: now,
                            },
                        )?;
                        state.observe(name, Some(&version))?;
                        match state.pending(name)? {
                            Some(current) if current.revision == sent.revision => {
                                state.remove_pending(name)?;
                            }
                            Some(mut newer) => {
                                newer.expected = Some(version.clone());
                                newer.blobs.retain(|hash| !sent.blobs.contains(hash));
                                state.set_pending(&newer)?;
                            }
                            None => {}
                        }
                        report.applied.push((name.clone(), version));
                    }
                    Outcome::Conflict { actual } => {
                        state.observe(name, actual.as_ref())?;
                        let latest = state.pending(name)?.unwrap_or_else(|| sent.clone());
                        state.remove_pending(name)?;
                        state.add_stranded(&Stranded {
                            name: name.clone(),
                            expected: latest.expected,
                            actual,
                            root: latest.root,
                            blobs: latest.blobs,
                            at: now,
                        })?;
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
            state.update_sync(|sync| {
                sync.flush = Some(FlushAttempt {
                    at: now,
                    error: report.error.clone(),
                });
            })?;
            Ok(Flush::Sent(report))
        })?;
        if let Flush::Sent(report) = &flush
            && !report.applied.is_empty()
        {
            self.notify()?;
        }
        Ok(flush)
    }

    /// Pending and stranded updates, sync health and request counters.
    pub fn status(&self) -> Result<Status> {
        self.read_state(|state| {
            let sync = state.sync()?;
            Ok(Status {
                pending: state.outbox()?,
                stranded: state.stranded()?,
                last_flush: sync.flush.clone(),
                health: health(&sync),
                counters: state.counters()?,
            })
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
        let outcomes = self.backend.update(updates);
        self.modify(|state| state.count(Counter::Pushes, 1))?;
        let outcomes = outcomes?;
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
        let fetched = self.backend.fetch(name, &|hash| cache.contains(hash));
        self.modify(|state| state.count(Counter::Fetches, 1))?;
        let Some(fetched) = fetched? else {
            return Ok(None);
        };
        for (hash, content) in fetched.blobs {
            cache.put(&Blob::verified(hash, content)?)?;
        }
        if !cache.contains(&fetched.root) {
            return Err(Error::MissingBlob(fetched.root));
        }
        self.modify(|state| {
            state.set_known(
                name,
                &Known {
                    version: fetched.version.clone(),
                    root: fetched.root,
                    at: now,
                },
            )?;
            state.observe(name, Some(&fetched.version))
        })?;
        Ok(Some(Value {
            version: fetched.version,
            root: fetched.root,
            as_of: now,
        }))
    }

    /// Settle a lost outcome by comparing the ref's current value with the proposal.
    fn resolve(&self, update: &Update, now: SystemTime) -> Result<Outcome> {
        let listed = self.backend.list();
        self.modify(|state| state.count(Counter::Lists, 1))?;
        let listed = match listed {
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

fn health(sync: &Record) -> Health {
    Health {
        listed_at: sync.listed_at,
        pass_started_at: sync.pass_started_at,
        pass_finished_at: sync.pass_finished_at,
        failing: sync.failing_since.map(|since| Failing {
            since,
            failures: sync.failures,
            reason: sync.last_error.clone().unwrap_or_default(),
            retry_at: sync.retry_at,
        }),
    }
}
