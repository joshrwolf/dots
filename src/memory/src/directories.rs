//! Durable creation of directory entries for managed documents.
use std::fs::{self, File};
use std::path::Path;

use crate::{Result, invalid, io};

pub fn sync(path: &Path) -> Result<()> {
    File::open(path)
        .and_then(|directory| directory.sync_all())
        .map_err(|error| io("sync document directory", path, error))
}

/// Create a storage root from its nearest existing directory, without opening
/// unrelated ancestors. Existing external directory entries are caller-owned.
pub(crate) fn create(path: &Path) -> Result<()> {
    let mut missing = Vec::new();
    let mut existing = path;
    loop {
        match fs::metadata(existing) {
            Ok(metadata) if metadata.is_dir() => break,
            Ok(_) => return Err(invalid("document directory is not a directory")),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                missing.push(existing);
                existing = existing
                    .parent()
                    .ok_or_else(|| invalid("directory has no existing ancestor"))?;
            }
            Err(error) => return Err(io("inspect document directory", existing, error)),
        }
    }
    for directory in missing.into_iter().rev() {
        match fs::create_dir(directory) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(io("create document directory", directory, error)),
        }
        sync(directory)?;
        sync(
            directory
                .parent()
                .ok_or_else(|| invalid("directory has no parent"))?,
        )?;
    }
    sync(path)
}

/// Sync every managed link back to the established storage root, including
/// existing entries left by an interrupted writer.
pub fn create_within(path: &Path, root: &Path) -> Result<()> {
    if !path.starts_with(root) {
        return Err(invalid("document directory is outside the storage root"));
    }
    create(path)?;
    for ancestor in path.ancestors() {
        sync(ancestor)?;
        if ancestor == root {
            break;
        }
    }
    Ok(())
}
