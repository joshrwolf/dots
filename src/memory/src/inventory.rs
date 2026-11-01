//! Bounded filesystem inventory without following symlinks or adopting files.
use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::{Result, invalid, io, paths};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    File,
    Symlink,
    Other,
}

#[derive(Debug, Serialize)]
pub struct Entry {
    pub path: PathBuf,
    pub kind: Kind,
}

/// Enumerate at most 10,000 entries (including directories) and 16 directory levels. Partial scans
/// fail rather than implying a complete integrity or erasure guarantee.
pub fn scan(root: &Path) -> Result<Vec<Entry>> {
    paths::reject_symlink(root)?;
    let mut entries = Vec::new();
    visit(root, root, 0, &mut entries, &mut 0)?;
    entries.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(entries)
}

fn visit(
    root: &Path,
    directory: &Path,
    depth: usize,
    entries: &mut Vec<Entry>,
    visited: &mut usize,
) -> Result<()> {
    if depth > 16 {
        return Err(invalid("document inventory exceeds 16 directory levels"));
    }
    let children = match fs::read_dir(directory) {
        Ok(children) => children,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound && directory == root => {
            return Ok(());
        }
        Err(error) => return Err(io("inventory document directory", directory, error)),
    };
    for child in children {
        if *visited == 10_000 {
            return Err(invalid("document inventory exceeds 10000 entries"));
        }
        *visited += 1;
        let child = child.map_err(|error| io("inventory document entry", directory, error))?;
        let path = child.path();
        let kind = child
            .file_type()
            .map_err(|error| io("inspect document entry", &path, error))?;
        if kind.is_dir() {
            visit(root, &path, depth + 1, entries, visited)?;
        } else {
            entries.push(Entry {
                path,
                kind: if kind.is_symlink() {
                    Kind::Symlink
                } else if kind.is_file() {
                    Kind::File
                } else {
                    Kind::Other
                },
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn empty_directories_consume_the_inventory_budget() {
        let root =
            std::env::temp_dir().join(format!("dots-inventory-{}", getrandom::u64().unwrap()));
        std::fs::create_dir(&root).unwrap();
        std::fs::create_dir(root.join("a")).unwrap();
        std::fs::create_dir(root.join("b")).unwrap();
        let mut visited = 9_999;
        let mut entries = Vec::new();
        let error = super::visit(&root, &root, 0, &mut entries, &mut visited).unwrap_err();
        assert_eq!(visited, 10_000);
        assert!(error.to_string().contains("10000 entries"));
        std::fs::remove_dir_all(root).unwrap();
    }
}
