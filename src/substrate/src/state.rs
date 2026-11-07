//! The per-machine record shared by every process using one state directory,
//! kept in one SQLite database in WAL mode.
//!
//! Every change runs in one immediate transaction, so concurrent processes
//! serialize on SQLite's own lock. `PRAGMA user_version` holds the schema
//! version; a database written by a newer binary is refused, never rewritten.
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::os::unix::fs::DirBuilderExt as _;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rusqlite::types::Type;
use rusqlite::{Connection, OptionalExtension as _, Row, Transaction, TransactionBehavior, params};

use crate::{Error, Hash, MachineId, RefName, Result, Version, io};

/// The schema this binary reads and writes.
pub const SCHEMA: i64 = 1;
const DATABASE: &str = "state.sqlite3";
/// How long a statement waits for another process's transaction.
const BUSY_TIMEOUT: Duration = Duration::from_secs(10);

const CREATE: &str = "
CREATE TABLE machine (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 0),
    id TEXT NOT NULL
);
-- The freshness record, the sync agent's health and the flush budget.
CREATE TABLE sync (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 0),
    listed_at INTEGER,
    pass_started_at INTEGER,
    pass_finished_at INTEGER,
    failing_since INTEGER,
    failures INTEGER NOT NULL DEFAULT 0,
    last_error TEXT,
    retry_at INTEGER,
    repair INTEGER NOT NULL DEFAULT 0,
    tokens INTEGER,
    bucket_at INTEGER,
    flush_at INTEGER,
    flush_error TEXT
);
INSERT INTO sync (singleton) VALUES (0);
CREATE TABLE listed (
    name TEXT PRIMARY KEY,
    version TEXT NOT NULL
) WITHOUT ROWID;
CREATE TABLE known (
    name TEXT PRIMARY KEY,
    version TEXT NOT NULL,
    root TEXT NOT NULL,
    at INTEGER NOT NULL
) WITHOUT ROWID;
CREATE TABLE outbox (
    name TEXT PRIMARY KEY,
    expected TEXT,
    root TEXT NOT NULL,
    since INTEGER NOT NULL,
    revision INTEGER NOT NULL
) WITHOUT ROWID;
CREATE TABLE outbox_blobs (
    name TEXT NOT NULL REFERENCES outbox (name) ON DELETE CASCADE,
    hash TEXT NOT NULL,
    PRIMARY KEY (name, hash)
) WITHOUT ROWID;
CREATE TABLE stranded (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    expected TEXT,
    actual TEXT,
    root TEXT NOT NULL,
    at INTEGER NOT NULL
);
CREATE TABLE stranded_blobs (
    stranded INTEGER NOT NULL REFERENCES stranded (id) ON DELETE CASCADE,
    hash TEXT NOT NULL,
    PRIMARY KEY (stranded, hash)
) WITHOUT ROWID;
CREATE TABLE counters (
    name TEXT PRIMARY KEY,
    value INTEGER NOT NULL
) WITHOUT ROWID;
";

/// A deferred update waiting for a flush.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pending {
    pub name: RefName,
    pub expected: Option<Version>,
    pub root: Hash,
    /// Blobs to upload with the update; a coalesced proposal replaces them.
    pub blobs: BTreeSet<Hash>,
    pub since: SystemTime,
    /// Advances whenever a proposal coalesces into this entry.
    pub(crate) revision: i64,
}

/// A deferred update another writer pre-empted. Its blobs stay cached.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stranded {
    pub name: RefName,
    pub expected: Option<Version>,
    /// What the ref held instead (`None`: absent).
    pub actual: Option<Version>,
    pub root: Hash,
    pub blobs: BTreeSet<Hash>,
    pub at: SystemTime,
}

/// The last flush that sent updates.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FlushAttempt {
    pub at: SystemTime,
    pub error: Option<String>,
}

/// A ref value this machine holds in its cache.
#[derive(Clone, Debug)]
pub(crate) struct Known {
    pub(crate) version: Version,
    pub(crate) root: Hash,
    /// When this value was confirmed current.
    pub(crate) at: SystemTime,
}

/// The sync record: freshness, pass health and flush budget.
#[derive(Clone, Debug, Default)]
pub(crate) struct Record {
    pub(crate) listed_at: Option<SystemTime>,
    pub(crate) pass_started_at: Option<SystemTime>,
    pub(crate) pass_finished_at: Option<SystemTime>,
    /// The first failure of the current run of failed passes.
    pub(crate) failing_since: Option<SystemTime>,
    pub(crate) failures: u32,
    pub(crate) last_error: Option<String>,
    /// Scheduled passes wait until then.
    pub(crate) retry_at: Option<SystemTime>,
    /// A cached blob failed verification; the next pass refetches every ref.
    pub(crate) repair: bool,
    pub(crate) tokens: Option<u32>,
    pub(crate) bucket_at: Option<SystemTime>,
    pub(crate) flush: Option<FlushAttempt>,
}

/// Request and outcome counts since the state directory was created.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counters {
    pub passes: u64,
    pub failed_passes: u64,
    pub lists: u64,
    pub fetches: u64,
    /// Update batches sent to the backend, deferred or synchronous.
    pub pushes: u64,
    /// Deferred flushes the push budget postponed.
    pub throttled: u64,
    pub maintenance: u64,
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum Counter {
    Passes,
    FailedPasses,
    Lists,
    Fetches,
    Pushes,
    Throttled,
    Maintenance,
}

impl Counter {
    fn key(self) -> &'static str {
        match self {
            Self::Passes => "passes",
            Self::FailedPasses => "failed_passes",
            Self::Lists => "lists",
            Self::Fetches => "fetches",
            Self::Pushes => "pushes",
            Self::Throttled => "throttled",
            Self::Maintenance => "maintenance",
        }
    }
}

/// A state directory: the database, the blob cache directory and a lock
/// file for passes.
#[derive(Debug)]
pub(crate) struct Database {
    path: PathBuf,
    connection: Connection,
    /// `PRAGMA data_version` when last checked: it changes when another
    /// connection commits.
    seen_version: i64,
}

impl Database {
    /// Open or create the database under `root`.
    pub(crate) fn open(root: &Path) -> Result<Self> {
        for path in [root.to_owned(), root.join("blobs")] {
            fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(&path)
                .map_err(|source| io("create substrate state directory", &path, source))?;
        }
        let path = root.join(DATABASE);
        let failed = |source| database(&path, "open substrate state", source);
        let mut connection = Connection::open(&path).map_err(failed)?;
        connection.busy_timeout(BUSY_TIMEOUT).map_err(failed)?;
        connection
            .pragma_update(None, "foreign_keys", true)
            .map_err(failed)?;
        let mode: String = connection
            .query_row("PRAGMA journal_mode = WAL", [], |row| row.get(0))
            .map_err(failed)?;
        if !mode.eq_ignore_ascii_case("wal") {
            return Err(Error::State {
                path,
                reason: format!("journal mode is {mode}, not WAL"),
            });
        }
        if schema(&connection, &path)? == 0 {
            let transaction = connection
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(failed)?;
            // Another process may have created it while this one waited.
            if schema(&transaction, &path)? == 0 {
                transaction.execute_batch(CREATE).map_err(failed)?;
                transaction
                    .pragma_update(None, "user_version", SCHEMA)
                    .map_err(failed)?;
            }
            transaction.commit().map_err(failed)?;
        }
        let mut database = Self {
            path,
            connection,
            seen_version: 0,
        };
        database.check()?;
        database.seen_version = database.data_version()?;
        Ok(database)
    }

    /// Fail with [`Error::Schema`] if another binary changed the schema.
    pub(crate) fn check(&self) -> Result<()> {
        match schema(&self.connection, &self.path)? {
            SCHEMA => Ok(()),
            found => Err(Error::Schema {
                found,
                supported: SCHEMA,
            }),
        }
    }

    /// Whether another connection committed since the last call.
    pub(crate) fn changed_elsewhere(&mut self) -> Result<bool> {
        let version = self.data_version()?;
        let changed = version != self.seen_version;
        self.seen_version = version;
        Ok(changed)
    }

    fn data_version(&self) -> Result<i64> {
        self.connection
            .query_row("PRAGMA data_version", [], |row| row.get(0))
            .map_err(|source| self.error("read substrate state version", source))
    }

    /// Run `read` in one read transaction, a consistent snapshot.
    pub(crate) fn read<T>(&mut self, read: impl FnOnce(&State<'_>) -> Result<T>) -> Result<T> {
        let path = self.path.clone();
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Deferred)
            .map_err(|source| database(&path, "read substrate state", source))?;
        let value = read(&State {
            transaction: &transaction,
            path: &path,
        })?;
        transaction
            .finish()
            .map_err(|source| database(&path, "read substrate state", source))?;
        Ok(value)
    }

    /// Run `change` in one immediate transaction, committed only on success.
    pub(crate) fn modify<T>(&mut self, change: impl FnOnce(&State<'_>) -> Result<T>) -> Result<T> {
        let path = self.path.clone();
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|source| database(&path, "change substrate state", source))?;
        let value = change(&State {
            transaction: &transaction,
            path: &path,
        })?;
        transaction
            .commit()
            .map_err(|source| database(&path, "commit substrate state", source))?;
        // This connection's own commits do not count as changes elsewhere.
        self.seen_version = self.data_version()?;
        Ok(value)
    }

    fn error(&self, operation: &'static str, source: rusqlite::Error) -> Error {
        database(&self.path, operation, source)
    }
}

/// Queries and changes inside one transaction.
pub(crate) struct State<'a> {
    transaction: &'a Transaction<'a>,
    path: &'a Path,
}

impl State<'_> {
    fn fail(&self, operation: &'static str) -> impl Fn(rusqlite::Error) -> Error + '_ {
        move |source| database(self.path, operation, source)
    }

    /// The machine identity, allocated on first use.
    pub(crate) fn machine(&self) -> Result<MachineId> {
        let fail = self.fail("read machine identity");
        let current: Option<String> = self
            .transaction
            .query_row("SELECT id FROM machine", [], |row| row.get(0))
            .optional()
            .map_err(&fail)?;
        if let Some(id) = current {
            return id.parse();
        }
        let id = MachineId::allocate()?;
        self.transaction
            .execute(
                "INSERT INTO machine (singleton, id) VALUES (0, ?1)",
                [id.as_str()],
            )
            .map_err(&fail)?;
        Ok(id)
    }

    pub(crate) fn sync(&self) -> Result<Record> {
        self.transaction
            .query_row(
                "SELECT listed_at, pass_started_at, pass_finished_at, failing_since, failures,
                        last_error, retry_at, repair, tokens, bucket_at, flush_at, flush_error
                 FROM sync",
                [],
                |row| {
                    let flush_error: Option<String> = row.get(11)?;
                    Ok(Record {
                        listed_at: optional_time(row, 0)?,
                        pass_started_at: optional_time(row, 1)?,
                        pass_finished_at: optional_time(row, 2)?,
                        failing_since: optional_time(row, 3)?,
                        failures: row.get(4)?,
                        last_error: row.get(5)?,
                        retry_at: optional_time(row, 6)?,
                        repair: row.get(7)?,
                        tokens: row.get(8)?,
                        bucket_at: optional_time(row, 9)?,
                        flush: optional_time(row, 10)?.map(|at| FlushAttempt {
                            at,
                            error: flush_error,
                        }),
                    })
                },
            )
            .map_err(self.fail("read sync record"))
    }

    pub(crate) fn set_sync(&self, sync: &Record) -> Result<()> {
        self.transaction
            .execute(
                "UPDATE sync SET listed_at = ?1, pass_started_at = ?2, pass_finished_at = ?3,
                    failing_since = ?4, failures = ?5, last_error = ?6, retry_at = ?7,
                    repair = ?8, tokens = ?9, bucket_at = ?10, flush_at = ?11, flush_error = ?12",
                params![
                    sync.listed_at.map(millis).transpose()?,
                    sync.pass_started_at.map(millis).transpose()?,
                    sync.pass_finished_at.map(millis).transpose()?,
                    sync.failing_since.map(millis).transpose()?,
                    sync.failures,
                    sync.last_error,
                    sync.retry_at.map(millis).transpose()?,
                    sync.repair,
                    sync.tokens,
                    sync.bucket_at.map(millis).transpose()?,
                    sync.flush
                        .as_ref()
                        .map(|flush| millis(flush.at))
                        .transpose()?,
                    sync.flush.as_ref().and_then(|flush| flush.error.clone()),
                ],
            )
            .map_err(self.fail("write sync record"))?;
        Ok(())
    }

    /// Change the sync record in place.
    pub(crate) fn update_sync(&self, change: impl FnOnce(&mut Record)) -> Result<Record> {
        let mut sync = self.sync()?;
        change(&mut sync);
        self.set_sync(&sync)?;
        Ok(sync)
    }

    /// Listed refs whose names start with `prefix`, in name order.
    pub(crate) fn listed_names(&self, prefix: &str) -> Result<Vec<RefName>> {
        let fail = self.fail("read listed names");
        let mut statement = self
            .transaction
            .prepare("SELECT name FROM listed WHERE substr(name, 1, length(?1)) = ?1 ORDER BY name")
            .map_err(&fail)?;
        let names = statement
            .query_map([prefix], |row| name(row, 0))
            .map_err(&fail)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(&fail)?;
        Ok(names)
    }

    pub(crate) fn listed_version(&self, ref_name: &RefName) -> Result<Option<Version>> {
        self.transaction
            .query_row(
                "SELECT version FROM listed WHERE name = ?1",
                [ref_name.as_str()],
                |row| version(row, 0),
            )
            .optional()
            .map_err(self.fail("read listed version"))
    }

    /// Replace the listing with `listed`, confirmed at `now`.
    pub(crate) fn set_listed(
        &self,
        listed: &BTreeMap<RefName, Version>,
        now: SystemTime,
    ) -> Result<()> {
        let fail = self.fail("write listed versions");
        self.transaction
            .execute("DELETE FROM listed", [])
            .map_err(&fail)?;
        let mut insert = self
            .transaction
            .prepare("INSERT INTO listed (name, version) VALUES (?1, ?2)")
            .map_err(&fail)?;
        for (ref_name, version) in listed {
            insert
                .execute([ref_name.as_str(), version.as_str()])
                .map_err(&fail)?;
        }
        self.update_sync(|sync| sync.listed_at = Some(now))?;
        Ok(())
    }

    /// Record a version this machine observed as current, once it has listed.
    pub(crate) fn observe(&self, ref_name: &RefName, version: Option<&Version>) -> Result<()> {
        if self.sync()?.listed_at.is_none() {
            return Ok(());
        }
        let fail = self.fail("record observed version");
        match version {
            Some(version) => self
                .transaction
                .execute(
                    "INSERT INTO listed (name, version) VALUES (?1, ?2)
                     ON CONFLICT (name) DO UPDATE SET version = excluded.version",
                    [ref_name.as_str(), version.as_str()],
                )
                .map_err(&fail)?,
            None => self
                .transaction
                .execute("DELETE FROM listed WHERE name = ?1", [ref_name.as_str()])
                .map_err(&fail)?,
        };
        Ok(())
    }

    pub(crate) fn known(&self, ref_name: &RefName) -> Result<Option<Known>> {
        self.transaction
            .query_row(
                "SELECT version, root, at FROM known WHERE name = ?1",
                [ref_name.as_str()],
                |row| {
                    Ok(Known {
                        version: version(row, 0)?,
                        root: hash(row, 1)?,
                        at: time(row, 2)?,
                    })
                },
            )
            .optional()
            .map_err(self.fail("read known value"))
    }

    pub(crate) fn set_known(&self, ref_name: &RefName, known: &Known) -> Result<()> {
        self.transaction
            .execute(
                "INSERT INTO known (name, version, root, at) VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT (name) DO UPDATE
                 SET version = excluded.version, root = excluded.root, at = excluded.at",
                params![
                    ref_name.as_str(),
                    known.version.as_str(),
                    known.root.to_string(),
                    millis(known.at)?
                ],
            )
            .map_err(self.fail("write known value"))?;
        Ok(())
    }

    pub(crate) fn outbox(&self) -> Result<Vec<Pending>> {
        let fail = self.fail("read outbox");
        let mut statement = self
            .transaction
            .prepare("SELECT name, expected, root, since, revision FROM outbox ORDER BY name")
            .map_err(&fail)?;
        let rows = statement
            .query_map([], |row| {
                Ok(Pending {
                    name: name(row, 0)?,
                    expected: optional_version(row, 1)?,
                    root: hash(row, 2)?,
                    blobs: BTreeSet::new(),
                    since: time(row, 3)?,
                    revision: row.get(4)?,
                })
            })
            .map_err(&fail)?;
        let mut pending = rows.collect::<rusqlite::Result<Vec<_>>>().map_err(&fail)?;
        let mut blobs = self
            .transaction
            .prepare("SELECT hash FROM outbox_blobs WHERE name = ?1")
            .map_err(&fail)?;
        for entry in &mut pending {
            entry.blobs = blobs
                .query_map([entry.name.as_str()], |row| hash(row, 0))
                .map_err(&fail)?
                .collect::<rusqlite::Result<_>>()
                .map_err(&fail)?;
        }
        Ok(pending)
    }

    pub(crate) fn pending(&self, ref_name: &RefName) -> Result<Option<Pending>> {
        Ok(self
            .outbox()?
            .into_iter()
            .find(|pending| pending.name == *ref_name))
    }

    /// Insert or replace the outbox entry for `pending.name`.
    pub(crate) fn set_pending(&self, pending: &Pending) -> Result<()> {
        let fail = self.fail("write outbox");
        self.transaction
            .execute(
                "INSERT INTO outbox (name, expected, root, since, revision) VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT (name) DO UPDATE SET expected = excluded.expected,
                    root = excluded.root, since = excluded.since, revision = excluded.revision",
                params![
                    pending.name.as_str(),
                    pending.expected.as_ref().map(Version::as_str),
                    pending.root.to_string(),
                    millis(pending.since)?,
                    pending.revision,
                ],
            )
            .map_err(&fail)?;
        self.transaction
            .execute(
                "DELETE FROM outbox_blobs WHERE name = ?1",
                [pending.name.as_str()],
            )
            .map_err(&fail)?;
        let mut insert = self
            .transaction
            .prepare("INSERT INTO outbox_blobs (name, hash) VALUES (?1, ?2)")
            .map_err(&fail)?;
        for blob in &pending.blobs {
            insert
                .execute([pending.name.as_str(), &blob.to_string()])
                .map_err(&fail)?;
        }
        Ok(())
    }

    pub(crate) fn remove_pending(&self, ref_name: &RefName) -> Result<()> {
        self.transaction
            .execute("DELETE FROM outbox WHERE name = ?1", [ref_name.as_str()])
            .map_err(self.fail("remove outbox entry"))?;
        Ok(())
    }

    pub(crate) fn stranded(&self) -> Result<Vec<Stranded>> {
        let fail = self.fail("read stranded updates");
        let mut statement = self
            .transaction
            .prepare("SELECT id, name, expected, actual, root, at FROM stranded ORDER BY id")
            .map_err(&fail)?;
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    Stranded {
                        name: name(row, 1)?,
                        expected: optional_version(row, 2)?,
                        actual: optional_version(row, 3)?,
                        root: hash(row, 4)?,
                        blobs: BTreeSet::new(),
                        at: time(row, 5)?,
                    },
                ))
            })
            .map_err(&fail)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(&fail)?;
        let mut blobs = self
            .transaction
            .prepare("SELECT hash FROM stranded_blobs WHERE stranded = ?1")
            .map_err(&fail)?;
        rows.into_iter()
            .map(|(id, mut stranded)| {
                stranded.blobs = blobs
                    .query_map([id], |row| hash(row, 0))
                    .map_err(&fail)?
                    .collect::<rusqlite::Result<_>>()
                    .map_err(&fail)?;
                Ok(stranded)
            })
            .collect()
    }

    pub(crate) fn add_stranded(&self, stranded: &Stranded) -> Result<()> {
        let fail = self.fail("record stranded update");
        self.transaction
            .execute(
                "INSERT INTO stranded (name, expected, actual, root, at) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    stranded.name.as_str(),
                    stranded.expected.as_ref().map(Version::as_str),
                    stranded.actual.as_ref().map(Version::as_str),
                    stranded.root.to_string(),
                    millis(stranded.at)?,
                ],
            )
            .map_err(&fail)?;
        let id = self.transaction.last_insert_rowid();
        let mut insert = self
            .transaction
            .prepare("INSERT INTO stranded_blobs (stranded, hash) VALUES (?1, ?2)")
            .map_err(&fail)?;
        for blob in &stranded.blobs {
            insert
                .execute(params![id, blob.to_string()])
                .map_err(&fail)?;
        }
        Ok(())
    }

    pub(crate) fn count(&self, counter: Counter, by: u64) -> Result<()> {
        self.transaction
            .execute(
                "INSERT INTO counters (name, value) VALUES (?1, ?2)
                 ON CONFLICT (name) DO UPDATE SET value = value + excluded.value",
                params![counter.key(), i64::try_from(by).unwrap_or(i64::MAX)],
            )
            .map_err(self.fail("count substrate requests"))?;
        Ok(())
    }

    pub(crate) fn counters(&self) -> Result<Counters> {
        let fail = self.fail("read substrate counters");
        let mut statement = self
            .transaction
            .prepare("SELECT name, value FROM counters")
            .map_err(&fail)?;
        let rows = statement
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
            })
            .map_err(&fail)?;
        let mut counters = Counters::default();
        for row in rows {
            let (key, value) = row.map_err(&fail)?;
            let value = u64::try_from(value).unwrap_or_default();
            match key.as_str() {
                "passes" => counters.passes = value,
                "failed_passes" => counters.failed_passes = value,
                "lists" => counters.lists = value,
                "fetches" => counters.fetches = value,
                "pushes" => counters.pushes = value,
                "throttled" => counters.throttled = value,
                "maintenance" => counters.maintenance = value,
                _ => {}
            }
        }
        Ok(counters)
    }
}

fn schema(connection: &Connection, path: &Path) -> Result<i64> {
    connection
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .map_err(|source| database(path, "read substrate schema version", source))
}

fn database(path: &Path, operation: &'static str, source: rusqlite::Error) -> Error {
    Error::Database {
        operation,
        path: path.to_owned(),
        source,
    }
}

/// Milliseconds since the Unix epoch.
pub(crate) fn millis(time: SystemTime) -> Result<i64> {
    time.duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|since| i64::try_from(since.as_millis()).ok())
        .ok_or_else(|| crate::invalid("time", "is outside the representable range"))
}

fn conversion(
    index: usize,
    kind: Type,
    error: impl std::error::Error + Send + Sync + 'static,
) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(index, kind, Box::new(error))
}

fn time(row: &Row<'_>, index: usize) -> rusqlite::Result<SystemTime> {
    let millis: i64 = row.get(index)?;
    u64::try_from(millis)
        .ok()
        .and_then(|millis| UNIX_EPOCH.checked_add(Duration::from_millis(millis)))
        .ok_or_else(|| conversion(index, Type::Integer, crate::invalid("time", "out of range")))
}

fn optional_time(row: &Row<'_>, index: usize) -> rusqlite::Result<Option<SystemTime>> {
    match row.get_ref(index)? {
        rusqlite::types::ValueRef::Null => Ok(None),
        _ => time(row, index).map(Some),
    }
}

fn name(row: &Row<'_>, index: usize) -> rusqlite::Result<RefName> {
    row.get::<_, String>(index)?
        .parse()
        .map_err(|error| conversion(index, Type::Text, error))
}

fn version(row: &Row<'_>, index: usize) -> rusqlite::Result<Version> {
    Version::new(row.get::<_, String>(index)?).map_err(|error| conversion(index, Type::Text, error))
}

fn optional_version(row: &Row<'_>, index: usize) -> rusqlite::Result<Option<Version>> {
    row.get::<_, Option<String>>(index)?
        .map(Version::new)
        .transpose()
        .map_err(|error| conversion(index, Type::Text, error))
}

fn hash(row: &Row<'_>, index: usize) -> rusqlite::Result<Hash> {
    row.get::<_, String>(index)?
        .parse()
        .map_err(|error| conversion(index, Type::Text, error))
}

/// Time elapsed from `then` to `now`; zero if the clock moved backwards.
pub(crate) fn age(now: SystemTime, then: SystemTime) -> Duration {
    now.duration_since(then).unwrap_or_default()
}

#[cfg(test)]
mod tests;
