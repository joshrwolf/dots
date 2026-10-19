use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result};
use review_core::Repository;
use rusqlite::{Connection, params};

const DATABASE_FILE: &str = "repositories.sqlite3";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CatalogEntry {
    pub checkout_root: PathBuf,
    pub name: String,
}

/// A small Herdr-global index of clones known to the review launcher.
/// Review content never lives here; it remains in each clone's Git common dir.
#[derive(Debug)]
pub(crate) struct RepositoryCatalog {
    connection: Connection,
    path: PathBuf,
}

impl RepositoryCatalog {
    pub(crate) fn open(state_dir: &Path) -> Result<Self> {
        fs::create_dir_all(state_dir)
            .with_context(|| format!("creating review plugin state at {}", state_dir.display()))?;
        let path = state_dir.join(DATABASE_FILE);
        let connection = Connection::open(&path)
            .with_context(|| format!("opening repository catalog {}", path.display()))?;
        connection
            .execute_batch(
                "PRAGMA foreign_keys = ON;
                 PRAGMA busy_timeout = 5000;
                 PRAGMA journal_mode = WAL;
                 PRAGMA synchronous = NORMAL;
                 CREATE TABLE IF NOT EXISTS repositories (
                     common_git_dir TEXT PRIMARY KEY NOT NULL,
                     checkout_root TEXT NOT NULL,
                     name TEXT NOT NULL,
                     last_seen_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
                 );",
            )
            .with_context(|| format!("initializing repository catalog {}", path.display()))?;
        Ok(Self { connection, path })
    }

    pub(crate) fn remember(&self, repository: &Repository, name: &str) -> Result<()> {
        let common_git_dir = utf8(repository.common_git_dir(), "Git common directory")?;
        let checkout_root = utf8(repository.checkout_root(), "checkout root")?;
        self.connection
            .execute(
                "INSERT INTO repositories (common_git_dir, checkout_root, name)
                 VALUES (?1, ?2, ?3)
                 ON CONFLICT(common_git_dir) DO UPDATE SET
                     checkout_root = excluded.checkout_root,
                     name = excluded.name,
                     last_seen_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
                params![common_git_dir, checkout_root, name],
            )
            .with_context(|| format!("updating repository catalog {}", self.path.display()))?;
        Ok(())
    }

    pub(crate) fn entries(&self) -> Result<Vec<CatalogEntry>> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT checkout_root, name
                 FROM repositories
                 ORDER BY last_seen_at DESC, common_git_dir",
            )
            .with_context(|| format!("reading repository catalog {}", self.path.display()))?;
        statement
            .query_map([], |row| {
                Ok(CatalogEntry {
                    checkout_root: PathBuf::from(row.get::<_, String>(0)?),
                    name: row.get(1)?,
                })
            })
            .with_context(|| format!("querying repository catalog {}", self.path.display()))?
            .collect::<rusqlite::Result<Vec<_>>>()
            .with_context(|| format!("decoding repository catalog {}", self.path.display()))
    }
}

fn utf8<'a>(path: &'a Path, field: &str) -> Result<&'a str> {
    path.to_str()
        .with_context(|| format!("{field} is not valid UTF-8: {}", path.display()))
}

#[cfg(test)]
mod tests {
    use std::process::Command;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    static NEXT_TEMP: AtomicUsize = AtomicUsize::new(0);

    #[test]
    fn catalog_is_beneath_plugin_state_and_remembers_a_clone() {
        let root = std::env::temp_dir().join(format!(
            "herdr-review-catalog-test-{}-{}",
            std::process::id(),
            NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
        ));
        let state = root.join("plugin-state");
        let checkout = root.join("checkout");
        fs::create_dir_all(&checkout).unwrap();
        let status = Command::new("git")
            .args(["init", "--initial-branch=main"])
            .current_dir(&checkout)
            .status()
            .unwrap();
        assert!(status.success());
        let repository = Repository::discover(&checkout).unwrap();
        let catalog = RepositoryCatalog::open(&state).unwrap();
        assert_eq!(catalog.path, state.join(DATABASE_FILE));
        catalog.remember(&repository, "checkout").unwrap();
        drop(catalog);
        assert_eq!(
            RepositoryCatalog::open(&state).unwrap().entries().unwrap(),
            [CatalogEntry {
                checkout_root: repository.checkout_root().to_path_buf(),
                name: "checkout".to_owned(),
            }]
        );
        let _ = fs::remove_dir_all(root);
    }
}
