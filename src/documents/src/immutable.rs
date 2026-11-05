//! Durable non-replacing publication, shared by retained-document consumers.
use std::fs::File;
use std::io::Write as _;
use std::path::Path;

use crate::staged_file::StagedFile;
use crate::{Result, directories, invalid, io, paths, text};

#[derive(Debug, Clone, Copy)]
pub enum Presence {
    Required,
    MayCreate,
}

/// Caller holds the workspace mutation lock. Verify existing bytes rather than
/// replacing them, including complete files left by an interrupted publication.
pub fn publish(root: &Path, relative: &Path, content: &str, presence: Presence) -> Result<()> {
    let destination = paths::resolve(root, relative)?;
    let parent = destination
        .parent()
        .ok_or_else(|| invalid("document has no parent"))?;
    directories::create_within(parent, root)?;
    paths::resolve(root, relative)?;
    let stored = match text::read_bytes(&destination, content.len()) {
        Err(crate::Error::TooLarge { .. }) => {
            return Err(invalid(
                "existing immutable bytes differ; refusing replacement",
            ));
        }
        read => read?,
    };
    if let Some(stored) = stored {
        if stored != content.as_bytes() {
            return Err(invalid(
                "existing immutable bytes differ; refusing replacement",
            ));
        }
        File::open(&destination)
            .and_then(|file| file.sync_all())
            .map_err(|error| io("sync immutable document", &destination, error))?;
        directories::sync(parent)?;
    } else {
        if matches!(presence, Presence::Required) {
            return Err(invalid(
                "registered immutable bytes are missing; refusing implicit repair",
            ));
        }
        let staged = StagedFile::create(parent)?;
        staged
            .file()
            .write_all(content.as_bytes())
            .map_err(|error| io("write staged document", staged.path(), error))?;
        let mut permissions = staged
            .file()
            .metadata()
            .map_err(|error| io("inspect staged document", staged.path(), error))?
            .permissions();
        permissions.set_readonly(true);
        staged
            .file()
            .set_permissions(permissions)
            .map_err(|error| io("protect staged document", staged.path(), error))?;
        staged.publish_new(&destination)?;
    }
    Ok(())
}
