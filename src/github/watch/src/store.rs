//! The SQLite store: entries, their cached status, claims, and the schedule.
use std::path::Path;
use std::time::{Duration, Instant};

use github_client::PullRequestUrl;
use rusqlite::{Connection, OptionalExtension as _, Row, Transaction, TransactionBehavior, params};
use serde::Serialize;

use crate::fetch::{self, Gh};
use crate::status::{Ci, Review, State, Status};

/// Path relative to the caller's platform state directory.
pub const STORE_PATH: &str = "dots/github/watch.sqlite3";

/// How long a claim keeps other refreshers away from a row being fetched;
/// well beyond one request's timeout.
const LEASE_SECS: i64 = 120;
/// When a row is due for a claim: its next check, but not before an active
/// lease on it expires.
const DUE_AT: &str = "MAX(next_check_at, COALESCE(lease_until, next_check_at))";
/// How long a merged or closed PR stays listed after its closure was observed.
const RETAIN_CLOSED_SECS: i64 = 24 * 60 * 60;
/// Rows fetched per GraphQL request.
const CHUNK: i64 = 50;
const MAX_ERROR_BYTES: usize = 1024;
const MAX_BODY_BYTES: usize = 16 * 1024;
/// Ends a stored description that was cut at [`MAX_BODY_BYTES`].
const BODY_TRUNCATED: &str = "\n\n[Description truncated; open the PR for the rest.]";

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("open or update the PR watch store")]
    Storage(#[from] rusqlite::Error),
    #[error("unsupported PR watch store schema {0}; use a matching binary or reset the store")]
    Schema(i64),
    #[error("invalid {0}")]
    Invalid(&'static str),
    #[error("watched PR {url} has an unreadable {field}")]
    Corrupt { url: String, field: &'static str },
}

/// A watched PR and its cached status.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Entry {
    pub url: PullRequestUrl,
    pub owner: String,
    pub repo: String,
    pub number: i64,
    pub added_at: i64,
    pub added_by: Option<String>,
    /// Absolute path of the checkout the PR was added from.
    pub checkout: Option<String>,
    /// The Rex session the work belongs to, such as `session:<id>`.
    pub rex_session: Option<String>,
    pub title: Option<String>,
    pub state: State,
    pub review: Review,
    pub ci: Ci,
    pub head_ref: Option<String>,
    /// PR description with LF line endings, at most 16 KiB plus a truncation
    /// note; empty when the PR has none, `None` before the first check.
    pub body: Option<String>,
    /// Time of the last successful check; the cached status is from then.
    pub checked_at: Option<i64>,
    /// Last refresh error, cleared by the next successful check.
    pub error: Option<String>,
    /// `None` once the PR is merged or closed: it is never checked again.
    pub next_check_at: Option<i64>,
}

/// Every entry, most recently added first, and how many are due now.
#[derive(Debug, Clone, Serialize)]
pub struct Listing {
    pub entries: Vec<Entry>,
    pub due: u64,
}

/// What one refresh fetched.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Refresh {
    pub refreshed: u64,
    pub failed: u64,
    pub failures: Vec<Failure>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Failure {
    pub url: String,
    pub error: String,
}

/// An entry to add; empty values leave a previous value in place.
#[derive(Debug, Clone, Copy)]
pub struct NewEntry<'a> {
    pub url: &'a PullRequestUrl,
    pub added_by: Option<&'a str>,
    pub checkout: Option<&'a str>,
    pub rex_session: Option<&'a str>,
}

/// How long after a failed check its row is due again; the failure keeps the
/// previously cached status.
const RETRY_SECS: i64 = 300;

/// When a row is next due after a successful check observed `state` and `ci`;
/// `None` means never check again.
fn next_check(state: State, ci: Ci, now: i64) -> Option<i64> {
    match state {
        State::Merged | State::Closed => None,
        State::Unknown => Some(now + 60),
        State::Open | State::Draft if ci == Ci::Pending => Some(now + 60),
        State::Open | State::Draft => Some(now + 600),
    }
}

/// A claimed row: everything the fetch needs, and the token that fences its
/// result.
#[derive(Debug, Clone)]
pub(crate) struct Claimed {
    pub url: String,
    pub lease: i64,
    pub owner: String,
    pub repo: String,
    pub number: i64,
}

/// One connection per caller; SQLite serializes competing writers.
#[derive(Debug)]
pub struct Store {
    connection: Connection,
}

impl Store {
    pub fn open(path: &Path) -> Result<Self, Error> {
        Self::initialize(Connection::open(path)?)
    }

    fn initialize(mut connection: Connection) -> Result<Self, Error> {
        // Simultaneous first-open callers can bypass the busy handler while
        // promoting the journal to WAL; retry that promotion briefly.
        connection.busy_timeout(Duration::from_millis(50))?;
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            match connection.pragma_update(None, "journal_mode", "WAL") {
                Ok(()) => break,
                Err(rusqlite::Error::SqliteFailure(error, _))
                    if matches!(
                        error.code,
                        rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked
                    ) && Instant::now() < deadline =>
                {
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(error) => return Err(error.into()),
            }
        }
        connection.busy_timeout(Duration::from_secs(5))?;
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let version: i64 = tx.pragma_query_value(None, "user_version", |row| row.get(0))?;
        match version {
            0 => tx.execute_batch(
                "CREATE TABLE prs(
                    url TEXT PRIMARY KEY, owner TEXT NOT NULL, repo TEXT NOT NULL, number INTEGER NOT NULL,
                    added_at INTEGER NOT NULL, added_by TEXT, checkout TEXT,
                    title TEXT, state TEXT NOT NULL, review TEXT NOT NULL, ci TEXT NOT NULL, head_ref TEXT,
                    checked_at INTEGER, error TEXT, next_check_at INTEGER, body TEXT,
                    lease_until INTEGER, lease INTEGER, rex_session TEXT);
                 CREATE INDEX prs_due ON prs(next_check_at);
                 CREATE TABLE leases(last INTEGER NOT NULL);
                 INSERT INTO leases VALUES(0);
                 PRAGMA user_version=4;",
            )?,
            4 => {}
            other => return Err(Error::Schema(other)),
        }
        tx.commit()?;
        Ok(Self { connection })
    }

    fn write(&mut self) -> Result<Transaction<'_>, Error> {
        Ok(self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?)
    }

    /// Add a PR, or update its checkout and sessions; either way it becomes
    /// due now, once any claim in flight on it ends.
    pub fn add(&mut self, entry: &NewEntry<'_>, now: i64) -> Result<Entry, Error> {
        let number =
            i64::try_from(entry.url.number()).map_err(|_| Error::Invalid("pull request number"))?;
        let added_by = present(entry.added_by);
        if added_by.is_some_and(|name| name.len() > 128 || name.chars().any(char::is_control)) {
            return Err(Error::Invalid("session name"));
        }
        let rex_session = present(entry.rex_session);
        if rex_session.is_some_and(|id| id.len() > 128 || id.chars().any(char::is_control)) {
            return Err(Error::Invalid("Rex session"));
        }
        let checkout = present(entry.checkout);
        if checkout.is_some_and(|path| {
            !Path::new(path).is_absolute()
                || path.len() > 4096
                || path.chars().any(char::is_control)
        }) {
            return Err(Error::Invalid("checkout: must be an absolute path"));
        }
        let tx = self.write()?;
        tx.execute(
            "INSERT INTO prs(url, owner, repo, number, added_at, added_by, checkout, rex_session, state, review, ci, next_check_at)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'unknown', 'unknown', 'unknown', ?5)
             ON CONFLICT(url) DO UPDATE SET
                added_by = COALESCE(excluded.added_by, added_by),
                checkout = COALESCE(excluded.checkout, checkout),
                rex_session = COALESCE(excluded.rex_session, rex_session),
                next_check_at = excluded.next_check_at",
            params![
                entry.url.as_str(),
                entry.url.owner(),
                entry.url.repository(),
                number,
                now,
                added_by,
                checkout,
                rex_session
            ],
        )?;
        let added = load(&tx, entry.url.as_str())?
            .expect("the upserted row is visible inside its own write transaction");
        tx.commit()?;
        Ok(added)
    }

    /// Remove a PR; reports whether it was listed.
    pub fn remove(&mut self, url: &PullRequestUrl) -> Result<bool, Error> {
        Ok(self
            .connection
            .execute("DELETE FROM prs WHERE url = ?1", [url.as_str()])?
            > 0)
    }

    pub fn get(&mut self, url: &PullRequestUrl) -> Result<Option<Entry>, Error> {
        let tx = self.connection.transaction()?;
        load(&tx, url.as_str())
    }

    /// Every entry, after dropping PRs whose closure was observed over a day ago.
    pub fn list(&mut self, now: i64) -> Result<Listing, Error> {
        let tx = self.write()?;
        tx.execute(
            "DELETE FROM prs WHERE state IN ('merged', 'closed') AND checked_at < ?1",
            [now - RETAIN_CLOSED_SECS],
        )?;
        let entries = {
            let mut statement = tx.prepare(&format!(
                "SELECT {COLUMNS} FROM prs ORDER BY added_at DESC, rowid DESC"
            ))?;
            let rows = statement.query_map([], row)?;
            rows.map(|entry| entry?)
                .collect::<Result<Vec<_>, Error>>()?
        };
        let due: i64 = tx.query_row(
            &format!("SELECT COUNT(*) FROM prs WHERE {DUE_AT} <= ?1"),
            [now],
            |row| row.get(0),
        )?;
        tx.commit()?;
        Ok(Listing {
            entries,
            due: u64::try_from(due).expect("SQLite COUNT(*) is never negative"),
        })
    }

    /// The earliest time any PR is next due, or `None` when none ever will be.
    pub fn next_due(&mut self) -> Result<Option<i64>, Error> {
        Ok(self
            .connection
            .query_row(&format!("SELECT MIN({DUE_AT}) FROM prs"), [], |row| {
                row.get(0)
            })?)
    }

    /// Make every PR that is not merged or closed due now, once any claim in
    /// flight on it ends; returns how many.
    pub fn make_due(&mut self, now: i64) -> Result<u64, Error> {
        let changed = self.connection.execute(
            "UPDATE prs SET next_check_at = ?1 WHERE state NOT IN ('merged', 'closed')",
            [now],
        )?;
        Ok(u64::try_from(changed).expect("a changed-row count fits in u64"))
    }

    /// Fetch every due PR (or only `only`, if due) and record the outcome.
    ///
    /// GitHub failures are recorded on their rows and reported, not returned
    /// as errors; only storage failures are. A result whose claim was
    /// superseded is dropped and counted in neither total.
    pub fn refresh(
        &mut self,
        gh: &dyn Gh,
        now: i64,
        only: Option<&PullRequestUrl>,
    ) -> Result<Refresh, Error> {
        let mut outcome = Refresh::default();
        // Every claimed row stays leased past `now`, so this ends.
        loop {
            let claimed = self.claim(now, only)?;
            if claimed.is_empty() {
                return Ok(outcome);
            }
            let results = fetch::fetch(gh, &claimed);
            self.record(&claimed, results, now, &mut outcome)?;
        }
    }

    /// Atomically lease up to one chunk of due rows to this caller under a
    /// token no earlier claim used.
    fn claim(&mut self, now: i64, only: Option<&PullRequestUrl>) -> Result<Vec<Claimed>, Error> {
        let tx = self.write()?;
        let lease: i64 = tx.query_row(
            "UPDATE leases SET last = last + 1 RETURNING last",
            [],
            |row| row.get(0),
        )?;
        let claimed = {
            let mut statement = tx.prepare(&format!(
                "SELECT url, owner, repo, number FROM prs
                 WHERE {DUE_AT} <= ?1 AND (?2 IS NULL OR url = ?2)
                 ORDER BY next_check_at, url LIMIT ?3"
            ))?;
            let rows = statement.query_map(
                params![now, only.map(PullRequestUrl::as_str), CHUNK],
                |row| {
                    Ok(Claimed {
                        url: row.get(0)?,
                        lease,
                        owner: row.get(1)?,
                        repo: row.get(2)?,
                        number: row.get(3)?,
                    })
                },
            )?;
            rows.collect::<Result<Vec<_>, _>>()?
        };
        if claimed.is_empty() {
            // Rolls back the unused token.
            return Ok(claimed);
        }
        for row in &claimed {
            tx.execute(
                "UPDATE prs SET lease_until = ?2, lease = ?3 WHERE url = ?1",
                params![row.url, now + LEASE_SECS, lease],
            )?;
        }
        tx.commit()?;
        Ok(claimed)
    }

    /// Apply each result to its row and release the lease, unless a later
    /// claim took the row (or it was removed) in the meantime.
    ///
    /// # Panics
    ///
    /// When `results` does not hold exactly one result per claimed row; no
    /// row is touched then.
    fn record(
        &mut self,
        claimed: &[Claimed],
        results: Vec<Result<Status, String>>,
        now: i64,
        outcome: &mut Refresh,
    ) -> Result<(), Error> {
        assert_eq!(
            claimed.len(),
            results.len(),
            "fetch returns exactly one result per claimed row"
        );
        let tx = self.write()?;
        for (row, result) in claimed.iter().zip(results) {
            match result {
                Ok(status) => {
                    let applied = tx.execute(
                        "UPDATE prs SET title = ?3, state = ?4, review = ?5, ci = ?6, head_ref = ?7,
                            checked_at = ?8, error = NULL, next_check_at = ?9, body = ?10,
                            lease_until = NULL
                         WHERE url = ?1 AND lease = ?2",
                        params![
                            row.url,
                            row.lease,
                            status.title,
                            status.state.as_str(),
                            status.review.as_str(),
                            status.ci.as_str(),
                            status.head_ref,
                            now,
                            next_check(status.state, status.ci, now),
                            stored_body(&status.body),
                        ],
                    )?;
                    if applied > 0 {
                        outcome.refreshed += 1;
                    }
                }
                Err(error) => {
                    let error = bounded(&error);
                    let applied = tx.execute(
                        "UPDATE prs SET error = ?3, next_check_at = ?4, lease_until = NULL
                         WHERE url = ?1 AND lease = ?2",
                        params![row.url, row.lease, error, now + RETRY_SECS],
                    )?;
                    if applied == 0 {
                        continue;
                    }
                    outcome.failed += 1;
                    outcome.failures.push(Failure {
                        url: row.url.clone(),
                        error,
                    });
                }
            }
        }
        tx.commit()?;
        Ok(())
    }
}

const COLUMNS: &str = "url, owner, repo, number, added_at, added_by, checkout, title, state, review, ci, head_ref, checked_at, error, next_check_at, body, rex_session";

fn load(tx: &Transaction<'_>, url: &str) -> Result<Option<Entry>, Error> {
    tx.query_row(
        &format!("SELECT {COLUMNS} FROM prs WHERE url = ?1"),
        [url],
        row,
    )
    .optional()?
    .transpose()
}

/// Read one row; the inner result reports unreadable stored values.
fn row(row: &Row<'_>) -> rusqlite::Result<Result<Entry, Error>> {
    let url: String = row.get(0)?;
    let state = State::parse(&row.get::<_, String>(8)?);
    let review = Review::parse(&row.get::<_, String>(9)?);
    let ci = Ci::parse(&row.get::<_, String>(10)?);
    let field = match (PullRequestUrl::parse(&url), state, review, ci) {
        (Ok(parsed_url), Some(state), Some(review), Some(ci)) => {
            return entry(row, parsed_url, state, review, ci).map(Ok);
        }
        (Err(_), ..) => "url",
        (_, None, ..) => "state",
        (_, _, None, _) => "review",
        (.., None) => "ci",
    };
    Ok(Err(Error::Corrupt { url, field }))
}

fn entry(
    row: &Row<'_>,
    parsed_url: PullRequestUrl,
    state: State,
    review: Review,
    ci: Ci,
) -> rusqlite::Result<Entry> {
    Ok(Entry {
        url: parsed_url,
        owner: row.get(1)?,
        repo: row.get(2)?,
        number: row.get(3)?,
        added_at: row.get(4)?,
        added_by: row.get(5)?,
        checkout: row.get(6)?,
        rex_session: row.get(16)?,
        title: row.get(7)?,
        state,
        review,
        ci,
        head_ref: row.get(11)?,
        checked_at: row.get(12)?,
        error: row.get(13)?,
        next_check_at: row.get(14)?,
        body: row.get(15)?,
    })
}

fn present(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

/// At most [`MAX_ERROR_BYTES`], cut between grapheme clusters.
fn bounded(text: &str) -> String {
    let text = text.trim();
    match cut(text, MAX_ERROR_BYTES) {
        Some(head) => format!("{head}…"),
        None => text.to_owned(),
    }
}

/// A description with LF line endings and at most [`MAX_BODY_BYTES`] of it.
fn stored_body(body: &str) -> String {
    let body = body.replace("\r\n", "\n");
    let body = body.trim_end();
    match cut(body, MAX_BODY_BYTES) {
        Some(head) => format!("{}{BODY_TRUNCATED}", head.trim_end()),
        None => body.to_owned(),
    }
}

/// The longest prefix of at most `limit` bytes ending between grapheme
/// clusters, or `None` when `text` already fits.
fn cut(text: &str, limit: usize) -> Option<&str> {
    use unicode_segmentation::UnicodeSegmentation as _;
    if text.len() <= limit {
        return None;
    }
    let end = text
        .grapheme_indices(true)
        .map(|(start, cluster)| start + cluster.len())
        .take_while(|end| *end <= limit)
        .last()
        .unwrap_or_default();
    Some(text.split_at(end).0)
}

#[cfg(test)]
mod tests;
