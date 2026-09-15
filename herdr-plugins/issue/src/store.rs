use std::fs::{self, DirBuilder, OpenOptions};
use std::os::unix::fs::{DirBuilderExt as _, OpenOptionsExt as _, PermissionsExt as _};
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context as _, Result, ensure};
use rusqlite::{Connection, OptionalExtension as _, Transaction, TransactionBehavior, params};

use crate::Issue;

#[derive(Debug)]
pub(crate) struct Store {
    connection: Connection,
    path: PathBuf,
}

impl Store {
    pub(crate) fn open(directory: &Path) -> Result<Self> {
        DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(directory)
            .context("creating private issue state directory")?;
        ensure!(
            fs::symlink_metadata(directory)?.is_dir(),
            "issue state directory must be a directory, not a symlink"
        );
        fs::set_permissions(directory, fs::Permissions::from_mode(0o700))?;
        let path = directory.join("issues.sqlite3");
        match fs::symlink_metadata(&path) {
            Ok(metadata) => ensure!(
                metadata.is_file(),
                "issue database must be a regular file, not a symlink"
            ),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error).context("inspecting issue database"),
        }
        OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .open(&path)
            .context("creating private issue database")?;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
        let connection = Connection::open(&path).context("opening issue database")?;
        connection.busy_timeout(Duration::from_secs(10))?;
        connection.execute_batch(
            "PRAGMA synchronous = FULL;
            CREATE TABLE IF NOT EXISTS issues (
                repository TEXT NOT NULL,
                branch TEXT NOT NULL,
                identifier TEXT NOT NULL,
                uuid TEXT,
                prompt TEXT,
                checkout TEXT,
                PRIMARY KEY(repository, branch),
                UNIQUE(repository, identifier),
                UNIQUE(repository, uuid)
            );",
        )?;
        Ok(Self { connection, path })
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    pub(crate) fn lock(&mut self) -> Result<Transaction<'_>> {
        self.connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .context("another issue launch is still active; retry after it finishes")
    }

    pub(crate) fn reserve(&mut self, repository: &Path, issue: &Issue) -> Result<()> {
        let transaction = self.lock()?;
        verify(&transaction, repository, issue)?;
        transaction.execute(
            "INSERT INTO issues(repository, branch, identifier, uuid, prompt)
            VALUES (?1, ?2, ?3, ?4, ?5)
            ON CONFLICT(repository, branch) DO UPDATE SET
                uuid = COALESCE(excluded.uuid, issues.uuid),
                prompt = COALESCE(excluded.prompt, issues.prompt)",
            params![
                path_text(repository)?,
                issue.branch,
                issue.identifier,
                issue.uuid,
                issue.prompt
            ],
        )?;
        transaction
            .commit()
            .context("reserving issue workspace before creation")
    }

    pub(crate) fn unbind(&mut self, repository: &Path, branch: &str) -> Result<()> {
        let transaction = self.lock()?;
        let changed = transaction.execute(
            "DELETE FROM issues WHERE repository = ?1 AND branch = ?2",
            params![path_text(repository)?, branch],
        )?;
        ensure!(
            changed == 1,
            "no issue binding exists for this repository and branch"
        );
        transaction.commit().context("removing local issue binding")
    }

    pub(crate) fn context(&self, repository: &Path, branch: &str) -> Result<Issue> {
        let issue = self.connection.query_row(
            "SELECT identifier, branch, uuid, prompt FROM issues WHERE repository = ?1 AND branch = ?2",
            params![path_text(repository)?, branch],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, Option<String>>(2)?, row.get::<_, Option<String>>(3)?)),
        ).optional()?.context("no issue context is stored for this repository and branch")?;
        Issue::new(issue.0, issue.1, issue.2, issue.3)
    }
}

fn path_text(path: &Path) -> Result<&str> {
    path.to_str().context("repository path must be UTF-8")
}

/// Reject identity drift instead of silently creating a second checkout.
pub(crate) fn verify(
    transaction: &Transaction<'_>,
    repository: &Path,
    issue: &Issue,
) -> Result<()> {
    let mut statement = transaction.prepare("SELECT branch, identifier, uuid FROM issues
        WHERE repository = ?1 AND (branch = ?2 OR identifier = ?3 OR (?4 IS NOT NULL AND uuid = ?4))")?;
    let bindings = statement.query_map(
        params![
            path_text(repository)?,
            issue.branch,
            issue.identifier,
            issue.uuid
        ],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
            ))
        },
    )?;
    for binding in bindings {
        let (branch, identifier, uuid) = binding?;
        ensure!(
            branch == issue.branch && identifier == issue.identifier,
            "issue binding has changed (stored {identifier} on {branch}); reconcile the existing checkout before opening another branch"
        );
        ensure!(
            uuid.as_ref()
                .zip(issue.uuid.as_ref())
                .is_none_or(|(stored, supplied)| stored == supplied),
            "issue UUID conflicts with the stored binding"
        );
    }
    Ok(())
}

pub(crate) fn complete(
    transaction: &Transaction<'_>,
    repository: &Path,
    issue: &Issue,
    checkout: &Path,
) -> Result<()> {
    transaction.execute(
        "UPDATE issues SET checkout = ?3 WHERE repository = ?1 AND branch = ?2",
        params![path_text(repository)?, issue.branch, path_text(checkout)?],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);

    fn directory() -> PathBuf {
        std::env::temp_dir().join(format!(
            "herdr-issue-store-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ))
    }

    #[test]
    fn reservation_survives_restart_and_missing_prompt_preserves_context() {
        let dir = directory();
        let root = Path::new("/src/example");
        let issue = Issue::new(
            "ENG-12".into(),
            "eng-12".into(),
            None,
            Some("cafe\u{301} 👨‍👩‍👧".into()),
        )
        .unwrap();
        Store::open(&dir).unwrap().reserve(root, &issue).unwrap();
        let mut store = Store::open(&dir).unwrap();
        let retry = Issue::new("ENG-12".into(), "eng-12".into(), None, None).unwrap();
        store.reserve(root, &retry).unwrap();
        assert_eq!(store.context(root, "eng-12").unwrap().prompt, issue.prompt);
        assert_eq!(
            fs::metadata(store.path()).unwrap().permissions().mode() & 0o777,
            0o600
        );
        drop(store);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn changed_identity_cannot_reserve_a_second_checkout() {
        let dir = directory();
        let mut store = Store::open(&dir).unwrap();
        let root = Path::new("/src/example");
        store
            .reserve(
                root,
                &Issue::new("ENG-12".into(), "eng-12".into(), None, None).unwrap(),
            )
            .unwrap();
        assert!(
            store
                .reserve(
                    root,
                    &Issue::new("ENG-12".into(), "renamed".into(), None, None).unwrap()
                )
                .is_err()
        );
        assert!(
            store
                .reserve(
                    root,
                    &Issue::new("ENG-13".into(), "eng-12".into(), None, None).unwrap()
                )
                .is_err()
        );
        // Another repository is an independent worktree binding.
        store
            .reserve(
                Path::new("/src/another"),
                &Issue::new("ENG-12".into(), "eng-12".into(), None, None).unwrap(),
            )
            .unwrap();
        drop(store);
        fs::remove_dir_all(dir).unwrap();
    }
}
