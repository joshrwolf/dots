//! Durable explicit PR associations. Callers supply host context and storage paths.
//!
//! Branches and agent state deliberately have no role in association identity.
//! Herdr workspace IDs are short and can be reused after their workspace closes,
//! so associations live only as long as their workspace: [`Registry::retain`]
//! discards scopes that are absent from a live inventory.
use std::collections::BTreeSet;
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
    #[error("invalid workspace scope: {0}")]
    InvalidScope(&'static str),
    #[error("open or update PR association storage")]
    Storage(#[from] rusqlite::Error),
    #[error("read stored PR identity")]
    Identity(#[from] github_client::Error),
    #[error("unsupported PR registry schema {0}")]
    Schema(i64),
}

/// Installation/session endpoint namespace plus the host's workspace ID.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Scope {
    endpoint: String,
    workspace: String,
}

impl Scope {
    pub fn new(endpoint: String, workspace: String) -> Result<Self, Error> {
        for (value, field) in [
            (&endpoint, "invalid endpoint namespace"),
            (&workspace, "invalid workspace ID"),
        ] {
            if value.is_empty() || value.len() > 4096 || value.chars().any(char::is_control) {
                return Err(Error::InvalidScope(field));
            }
        }
        Ok(Self {
            endpoint,
            workspace,
        })
    }

    /// Scope a workspace of the server listening at `endpoint`.
    pub fn at(endpoint: &Path, workspace: &str) -> Result<Self, Error> {
        let endpoint = endpoint
            .to_str()
            .ok_or(Error::InvalidScope("non-Unicode endpoint path"))?;
        Self::new(endpoint.to_owned(), workspace.to_owned())
    }

    pub fn workspace(&self) -> &str {
        &self.workspace
    }
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    Agent,
    Manual,
    Review,
}

impl Source {
    fn as_str(self) -> &'static str {
        match self {
            Self::Agent => "agent",
            Self::Manual => "manual",
            Self::Review => "review",
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
            // Version 1 keyed scopes by persistent workspace UUIDs that stock Herdr
            // does not expose; those rows cannot be mapped to workspace IDs.
            0 | 1 => transaction.execute_batch(
                "DROP TABLE IF EXISTS scopes;
                 DROP TABLE IF EXISTS associations;
                 CREATE TABLE scopes(endpoint TEXT NOT NULL, workspace TEXT NOT NULL, revision INTEGER NOT NULL DEFAULT 0, PRIMARY KEY(endpoint, workspace));
                 CREATE TABLE associations(endpoint TEXT NOT NULL, workspace TEXT NOT NULL, url TEXT NOT NULL, registered_at INTEGER NOT NULL, source TEXT NOT NULL, PRIMARY KEY(endpoint, workspace, url));
                 PRAGMA user_version=2;"
            )?,
            2 => {},
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
            "INSERT INTO scopes(endpoint,workspace) VALUES(?1,?2) ON CONFLICT DO NOTHING",
            params![scope.endpoint, scope.workspace],
        )?;
        let changed = tx.execute(
            "INSERT INTO associations VALUES(?1,?2,?3,?4,?5) ON CONFLICT DO NOTHING",
            params![
                scope.endpoint,
                scope.workspace,
                url.as_str(),
                now,
                source.as_str()
            ],
        )? != 0;
        if changed {
            tx.execute(
                "UPDATE scopes SET revision=revision+1 WHERE endpoint=?1 AND workspace=?2",
                params![scope.endpoint, scope.workspace],
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
            "DELETE FROM associations WHERE endpoint=?1 AND workspace=?2 AND url=?3",
            params![scope.endpoint, scope.workspace, url.as_str()],
        )? != 0;
        if changed {
            tx.execute(
                "UPDATE scopes SET revision=revision+1 WHERE endpoint=?1 AND workspace=?2",
                params![scope.endpoint, scope.workspace],
            )?;
        }
        tx.commit()?;
        Ok(changed)
    }

    /// Discard associations of `endpoint` workspaces that are absent from `live`.
    ///
    /// `live` runs while this registry holds its write lock, so a registration
    /// that validated its workspace earlier cannot be lost to an older inventory.
    /// It must not reenter this registry or perform unbounded work. Returns the
    /// discarded workspace IDs.
    pub fn retain<E: From<Error>>(
        &mut self,
        endpoint: &str,
        live: impl FnOnce() -> Result<BTreeSet<String>, E>,
    ) -> Result<Vec<String>, E> {
        let tx = self
            .connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(Error::from)?;
        let live = live()?;
        let stale = {
            let mut statement = tx
                .prepare("SELECT DISTINCT workspace FROM associations WHERE endpoint=?1")
                .map_err(Error::from)?;
            let rows = statement
                .query_map([endpoint], |row| row.get::<_, String>(0))
                .map_err(Error::from)?;
            let mut stale = Vec::new();
            for workspace in rows {
                let workspace = workspace.map_err(Error::from)?;
                if !live.contains(&workspace) {
                    stale.push(workspace);
                }
            }
            stale
        };
        for workspace in &stale {
            for table in [
                "DELETE FROM associations WHERE endpoint=?1 AND workspace=?2",
                // Keep revisions monotonic so a reused ID never matches an older fence.
                "UPDATE scopes SET revision=revision+1 WHERE endpoint=?1 AND workspace=?2",
            ] {
                tx.execute(table, params![endpoint, workspace])
                    .map_err(Error::from)?;
            }
        }
        tx.commit().map_err(Error::from)?;
        Ok(stale)
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
                "SELECT revision FROM scopes WHERE endpoint=?1 AND workspace=?2",
                params![scope.endpoint, scope.workspace],
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
                "SELECT revision FROM scopes WHERE endpoint=?1 AND workspace=?2",
                params![scope.endpoint, scope.workspace],
                |row| row.get(0),
            )
            .optional()?
            .unwrap_or(0);
        let pulls = {
            let mut statement = tx.prepare("SELECT url,registered_at,source FROM associations WHERE endpoint=?1 AND workspace=?2 ORDER BY registered_at DESC, rowid DESC")?;
            let rows = statement.query_map(params![scope.endpoint, scope.workspace], |row| {
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
        Scope::new(endpoint.into(), "w1".into()).unwrap()
    }
    fn url(number: u64) -> PullRequestUrl {
        PullRequestUrl::parse(&format!("https://github.com/example/project/pull/{number}")).unwrap()
    }
    #[test]
    fn registrations_are_idempotent_and_workspace_local() {
        let mut registry = Registry::initialize(Connection::open_in_memory().unwrap()).unwrap();
        let first =
            PullRequestUrl::parse("https://github.com/Example/Project/pull/1/files#diff").unwrap();
        let second = url(2);
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
    fn empty_or_control_scopes_are_rejected() {
        assert!(Scope::new("/session".into(), String::new()).is_err());
        assert!(Scope::new("/session".into(), "w1\n".into()).is_err());
        assert!(Scope::new(String::new(), "w1".into()).is_err());
    }
    #[test]
    fn closed_workspaces_cannot_pass_associations_to_a_reused_id() {
        let mut registry = Registry::initialize(Connection::open_in_memory().unwrap()).unwrap();
        let w2 = Scope::new("/session".into(), "w2".into()).unwrap();
        let other = Scope::new("/other".into(), "w3".into()).unwrap();
        registry
            .register(&scope("/session"), &url(1), Source::Agent, 1)
            .unwrap();
        registry.register(&w2, &url(2), Source::Agent, 1).unwrap();
        registry
            .register(&other, &url(3), Source::Agent, 1)
            .unwrap();
        let discarded = registry
            .retain("/session", || {
                Ok::<_, Error>(BTreeSet::from(["w1".to_owned()]))
            })
            .unwrap();
        assert_eq!(discarded, ["w2"]);
        let reused = registry.list(&w2).unwrap();
        assert!(reused.pulls.is_empty());
        assert_eq!(
            reused.revision, 2,
            "a reused ID must not match an older fence"
        );
        assert_eq!(registry.list(&scope("/session")).unwrap().pulls.len(), 1);
        assert_eq!(registry.list(&other).unwrap().pulls.len(), 1);
        assert!(
            registry
                .retain("/session", || Err::<BTreeSet<String>, _>(Error::Schema(9)))
                .is_err()
        );
        assert_eq!(registry.list(&scope("/session")).unwrap().pulls.len(), 1);
        let none = || Ok::<_, Error>(BTreeSet::new());
        assert_eq!(registry.retain("/session", none).unwrap(), ["w1"]);
        assert_eq!(
            registry.retain("/session", none).unwrap(),
            Vec::<String>::new()
        );
        assert_eq!(registry.list(&w2).unwrap().revision, 2);
    }
    #[test]
    fn uuid_keyed_registries_are_discarded() {
        let connection = Connection::open_in_memory().unwrap();
        connection
            .execute_batch(
                "CREATE TABLE scopes(endpoint TEXT NOT NULL, identity TEXT NOT NULL, revision INTEGER NOT NULL DEFAULT 0, PRIMARY KEY(endpoint, identity));
                 CREATE TABLE associations(endpoint TEXT NOT NULL, identity TEXT NOT NULL, url TEXT NOT NULL, registered_at INTEGER NOT NULL, source TEXT NOT NULL, PRIMARY KEY(endpoint, identity, url));
                 INSERT INTO associations VALUES('/session','w1','https://github.com/example/project/pull/1',1,'agent');
                 PRAGMA user_version=1;",
            )
            .unwrap();
        let mut registry = Registry::initialize(connection).unwrap();
        assert!(registry.list(&scope("/session")).unwrap().pulls.is_empty());
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
                    registry
                        .register(
                            &scope("/session"),
                            &url(number),
                            Source::Agent,
                            number.cast_signed(),
                        )
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
        registry.unregister(&scope("/session"), &url(1)).unwrap();
        assert!(
            registry
                .while_current(&scope("/session"), stale, || panic!(
                    "stale publication ran"
                ))
                .unwrap()
                .is_none()
        );
        drop(registry);
        std::fs::remove_dir_all(directory).unwrap();
    }
}
