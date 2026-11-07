//! Local content-addressed blob cache; every read verifies content.
use std::fs::{self, File};
use std::io::{ErrorKind, Read as _, Write as _};
use std::os::unix::fs::DirBuilderExt as _;
use std::path::PathBuf;

use dots_documents::staged_file::StagedFile;

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
        Blob::verified(*hash, content)
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
        let staged = StagedFile::create(directory)?;
        staged
            .file()
            .write_all(blob.content())
            .map_err(|source| io("write cached blob", staged.path(), source))?;
        // Concurrent writers of one hash publish identical bytes.
        staged.publish_replace(&path)?;
        Ok(())
    }
}
