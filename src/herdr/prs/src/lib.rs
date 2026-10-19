//! Durable explicit PR associations. Callers supply host context and storage paths.
//!
//! Branches and agent state deliberately have no role in association identity.
use std::path::Path;
use std::time::{Duration, Instant};

use rusqlite::{Connection, OptionalExtension as _, params};
use serde::Serialize;

pub use github_client::PullRequestUrl;

/// Path relative to the caller’s platform state directory.
pub const REGISTRY_PATH: &str = "dots/herdr-prs/registry.sqlite3";

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    #[error("invalid persistent workspace scope: {0}")]
    InvalidScope(&'static str),
    #[error("open or update PR association storage")]
    Storage(#[from] rusqlite::Error),
    #[error("read stored PR identity")]
    Identity(#[from] github_client::Error),
    #[error("unsupported PR registry schema {0}")]
    Schema(i64),
}

/// Installation/session endpoint namespace plus the host's persistent workspace ID.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Scope {
    endpoint: String,
    identity: String,
}

impl Scope {
    pub fn new(endpoint: String, mut identity: String) -> Result<Self, Error> {
        if endpoint.is_empty() || endpoint.len() > 4096 || endpoint.chars().any(char::is_control) {
            return Err(Error::InvalidScope("invalid endpoint namespace"));
        }
        if identity.len() != 36
            || !identity.bytes().enumerate().all(|(index, byte)| {
                if matches!(index, 8 | 13 | 18 | 23) {
                    byte == b'-'
                } else {
                    byte.is_ascii_hexdigit()
                }
            })
            || identity
                .bytes()
                .filter(|byte| *byte != b'-')
                .all(|byte| byte == b'0')
        {
            return Err(Error::InvalidScope("expected a non-nil workspace UUID"));
        }
        identity.make_ascii_lowercase();
        Ok(Self { endpoint, identity })
    }

    pub fn identity(&self) -> &str {
        &self.identity
    }
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    Agent,
    Manual,
    Review,
    Migration,
}

impl Source {
    fn as_str(self) -> &'static str {
        match self {
            Self::Agent => "agent",
            Self::Manual => "manual",
            Self::Review => "review",
            Self::Migration => "migration",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Association {
    pub url: PullRequestUrl,
    pub registered_at: i64,
    pub source: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Associations {
    pub revision: i64,
    pub pulls: Vec<Association>,
}

/// One connection per caller; SQLite serializes competing registrations.
#[derive(Debug)]
pub struct Registry {
    connection: Connection,
}

impl Registry {
    pub fn open(path: &Path) -> Result<Self, Error> {
        Self::initialize(Connection::open(path)?)
    }

    fn initialize(mut connection: Connection) -> Result<Self, Error> {
        // SQLite may bypass its busy handler when simultaneous first-open callers
        // promote the journal to WAL. Retry that promotion with a bounded deadline.
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
        let transaction =
            connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let version: i64 =
            transaction.pragma_query_value(None, "user_version", |row| row.get(0))?;
        match version {
            0 => transaction.execute_batch(
                "CREATE TABLE scopes(endpoint TEXT NOT NULL, identity TEXT NOT NULL, revision INTEGER NOT NULL DEFAULT 0, PRIMARY KEY(endpoint, identity));
                 CREATE TABLE associations(endpoint TEXT NOT NULL, identity TEXT NOT NULL, url TEXT NOT NULL, registered_at INTEGER NOT NULL, source TEXT NOT NULL, PRIMARY KEY(endpoint, identity, url));
                 PRAGMA user_version=1;"
            )?,
            1 => {},
            other => return Err(Error::Schema(other)),
        }
        transaction.commit()?;
        Ok(Self { connection })
    }

    pub fn register(
        &mut self,
        scope: &Scope,
        url: &PullRequestUrl,
        source: Source,
        now: i64,
    ) -> Result<bool, Error> {
        let tx = self
            .connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        tx.execute(
            "INSERT INTO scopes(endpoint,identity) VALUES(?1,?2) ON CONFLICT DO NOTHING",
            params![scope.endpoint, scope.identity],
        )?;
        let changed = tx.execute(
            "INSERT INTO associations VALUES(?1,?2,?3,?4,?5) ON CONFLICT DO NOTHING",
            params![
                scope.endpoint,
                scope.identity,
                url.as_str(),
                now,
                source.as_str()
            ],
        )? != 0;
        if changed {
            tx.execute(
                "UPDATE scopes SET revision=revision+1 WHERE endpoint=?1 AND identity=?2",
                params![scope.endpoint, scope.identity],
            )?;
        }
        tx.commit()?;
        Ok(changed)
    }

    pub fn unregister(&mut self, scope: &Scope, url: &PullRequestUrl) -> Result<bool, Error> {
        let tx = self
            .connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let changed = tx.execute(
            "DELETE FROM associations WHERE endpoint=?1 AND identity=?2 AND url=?3",
            params![scope.endpoint, scope.identity, url.as_str()],
        )? != 0;
        if changed {
            tx.execute(
                "UPDATE scopes SET revision=revision+1 WHERE endpoint=?1 AND identity=?2",
                params![scope.endpoint, scope.identity],
            )?;
        }
        tx.commit()?;
        Ok(changed)
    }

    /// Fence a bounded publication against concurrent association changes.
    /// The callback must not reenter this registry or perform unbounded work.
    pub fn while_current<T>(
        &mut self,
        scope: &Scope,
        revision: i64,
        publish: impl FnOnce() -> T,
    ) -> Result<Option<T>, Error> {
        let tx = self
            .connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let current: i64 = tx
            .query_row(
                "SELECT revision FROM scopes WHERE endpoint=?1 AND identity=?2",
                params![scope.endpoint, scope.identity],
                |row| row.get(0),
            )
            .optional()?
            .unwrap_or(0);
        if current != revision {
            return Ok(None);
        }
        let result = publish();
        tx.commit()?;
        Ok(Some(result))
    }

    pub fn list(&mut self, scope: &Scope) -> Result<Associations, Error> {
        let tx = self.connection.transaction()?;
        let revision = tx
            .query_row(
                "SELECT revision FROM scopes WHERE endpoint=?1 AND identity=?2",
                params![scope.endpoint, scope.identity],
                |row| row.get(0),
            )
            .optional()?
            .unwrap_or(0);
        let pulls = {
            let mut statement = tx.prepare("SELECT url,registered_at,source FROM associations WHERE endpoint=?1 AND identity=?2 ORDER BY registered_at DESC, rowid DESC")?;
            let rows = statement.query_map(params![scope.endpoint, scope.identity], |row| {
                Ok((row.get::<_, String>(0)?, row.get(1)?, row.get(2)?))
            })?;
            rows.map(|row| {
                let (url, registered_at, source) = row?;
                Ok(Association {
                    url: PullRequestUrl::parse(&url)?,
                    registered_at,
                    source,
                })
            })
            .collect::<Result<Vec<_>, Error>>()?
        };
        tx.commit()?;
        Ok(Associations { revision, pulls })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn scope(endpoint: &str) -> Scope {
        Scope::new(
            endpoint.into(),
            "bd2f97a9-bcc5-432c-8f39-7b44692bac2e".into(),
        )
        .unwrap()
    }
    #[test]
    fn registrations_are_idempotent_and_workspace_local() {
        let mut registry = Registry::initialize(Connection::open_in_memory().unwrap()).unwrap();
        let first =
            PullRequestUrl::parse("https://github.com/Example/Project/pull/1/files#diff").unwrap();
        let second = PullRequestUrl::parse("https://github.com/example/project/pull/2").unwrap();
        let a = scope("/session-a");
        let b = scope("/session-b");
        assert!(registry.register(&a, &first, Source::Agent, 10).unwrap());
        assert!(!registry.register(&a, &first, Source::Review, 20).unwrap());
        registry.register(&a, &second, Source::Agent, 20).unwrap();
        registry.register(&b, &first, Source::Review, 30).unwrap();
        let list = registry.list(&a).unwrap();
        assert_eq!(list.revision, 2);
        assert_eq!(list.pulls.len(), 2);
        assert_eq!(list.pulls.first().unwrap().url, second);
        assert!(registry.unregister(&a, &first).unwrap());
        assert!(!registry.unregister(&a, &first).unwrap());
        assert_eq!(registry.list(&a).unwrap().revision, 3);
        assert_eq!(registry.list(&b).unwrap().pulls.len(), 1);
    }
    #[test]
    fn short_handles_cannot_be_used_as_durable_identities() {
        assert!(Scope::new("/session".into(), "w1".into()).is_err());
        assert!(
            Scope::new(
                "/session".into(),
                "00000000-0000-0000-0000-000000000000".into()
            )
            .is_err()
        );
    }
    #[test]
    fn reopen_and_competing_writers_preserve_every_association() {
        let directory =
            std::env::temp_dir().join(format!("herdr-pr-registry-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("concurrent.sqlite3");
        let _ = std::fs::remove_file(&path);
        let workers: Vec<_> = (1..=8)
            .map(|number| {
                let path = path.clone();
                std::thread::spawn(move || {
                    let mut registry = Registry::open(&path).unwrap();
                    let url = PullRequestUrl::parse(&format!(
                        "https://github.com/example/project/pull/{number}"
                    ))
                    .unwrap();
                    registry
                        .register(&scope("/session"), &url, Source::Agent, number)
                        .unwrap();
                })
            })
            .collect();
        for worker in workers {
            worker.join().unwrap();
        }
        let mut registry = Registry::open(&path).unwrap();
        assert_eq!(registry.list(&scope("/session")).unwrap().pulls.len(), 8);
        let stale = registry.list(&scope("/session")).unwrap().revision;
        let url = PullRequestUrl::parse("https://github.com/example/project/pull/1").unwrap();
        registry.unregister(&scope("/session"), &url).unwrap();
        assert!(
            registry
                .while_current(&scope("/session"), stale, || panic!(
                    "stale publication ran"
                ))
                .unwrap()
                .is_none()
        );
        let reincarnated = Scope::new(
            "/session".into(),
            "bd2f97a9-bcc5-432c-8f39-7b44692bac2f".into(),
        )
        .unwrap();
        assert!(registry.list(&reincarnated).unwrap().pulls.is_empty());
        drop(registry);
        std::fs::remove_dir_all(directory).unwrap();
    }
}
