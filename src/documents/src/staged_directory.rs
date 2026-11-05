//! Private directory lifetimes for cooperating native writers.
use std::fs;
use std::path::{Path, PathBuf};

use crate::{Result, invalid, io};

#[derive(Debug)]
pub struct StagedDirectory {
    path: PathBuf,
}

impl StagedDirectory {
    /// Exclusively create a private directory under an existing caller-owned parent.
    pub fn create(parent: &Path) -> Result<Self> {
        use std::os::unix::fs::DirBuilderExt as _;
        for _ in 0..128 {
            let mut nonce = [0_u8; 16];
            getrandom::fill(&mut nonce).map_err(|error| {
                io(
                    "allocate staging directory name",
                    parent,
                    std::io::Error::other(error),
                )
            })?;
            let path = parent.join(format!(
                ".staged-directory-{:032x}",
                u128::from_ne_bytes(nonce)
            ));
            match fs::DirBuilder::new().mode(0o700).create(&path) {
                Ok(()) => return Ok(Self { path }),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(io("create staging directory", &path, error)),
            }
        }
        Err(invalid("cannot allocate a staging directory"))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for StagedDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}
