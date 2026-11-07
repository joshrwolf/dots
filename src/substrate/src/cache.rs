//! Local content-addressed blob cache; every read verifies content.
//!
//! Writes are atomic (a private file renamed into place) but not synced: a
//! blob is checked against its hash on every read and can be fetched again,
//! so a write lost or torn by a crash is detected, removed and refetched
//! rather than served.
use std::fs::{self, File, OpenOptions};
use std::io::{ErrorKind, Read as _, Write as _};
use std::os::unix::fs::{DirBuilderExt as _, OpenOptionsExt as _};
use std::path::PathBuf;

use crate::{Blob, Error, Hash, MAX_BLOB_BYTES, Result, io};

#[derive(Debug)]
pub(crate) struct Cache {
    root: PathBuf,
}

impl Cache {
    pub(crate) fn new(root: PathBuf) -> Self {
        Self { root }
    }

    fn path(&self, hash: &Hash) -> PathBuf {
        let hex = hash.hex();
        self.root.join(&hex[..2]).join(hex)
    }

    pub(crate) fn contains(&self, hash: &Hash) -> bool {
        self.path(hash).is_file()
    }

    /// A verified blob. A blob whose content does not match is removed, so the
    /// next fetch of a ref carrying it brings it back.
    pub(crate) fn get(&self, hash: &Hash) -> Result<Blob> {
        let path = self.path(hash);
        let file = match File::open(&path) {
            Ok(file) => file,
            Err(error) if error.kind() == ErrorKind::NotFound => {
                return Err(Error::MissingBlob(*hash));
            }
            Err(source) => return Err(io("open cached blob", &path, source)),
        };
        let mut content = Vec::new();
        file.take(MAX_BLOB_BYTES as u64 + 1)
            .read_to_end(&mut content)
            .map_err(|source| io("read cached blob", &path, source))?;
        let verified = Blob::verified(*hash, content);
        if let Err(Error::Integrity { .. } | Error::TooLarge { .. }) = &verified {
            match fs::remove_file(&path) {
                Ok(()) => {}
                Err(error) if error.kind() == ErrorKind::NotFound => {}
                Err(source) => return Err(io("remove damaged cached blob", &path, source)),
            }
            return Err(Error::Integrity { expected: *hash });
        }
        verified
    }

    pub(crate) fn put(&self, blob: &Blob) -> Result<()> {
        if self.contains(&blob.hash()) {
            return Ok(());
        }
        let path = self.path(&blob.hash());
        let directory = path
            .parent()
            .ok_or_else(|| crate::invalid("cache path", "has no parent"))?;
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(directory)
            .map_err(|source| io("create blob cache directory", directory, source))?;
        let staged = directory.join(format!(".{}-{:016x}", blob.hash().hex(), getrandom::u64()?));
        let written = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&staged)
            .and_then(|mut file| file.write_all(blob.content()))
            // Concurrent writers of one hash rename identical bytes.
            .and_then(|()| fs::rename(&staged, &path));
        if let Err(source) = written {
            let _ = fs::remove_file(&staged);
            return Err(io("write cached blob", &staged, source));
        }
        Ok(())
    }
}
