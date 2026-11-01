//! One byte-level integrity classifier for retained text reads and acceptance.
use std::path::Path;

use crate::{Error, paths, text};

#[derive(Debug)]
pub enum State {
    Available(String),
    Missing,
    Corrupt,
    Unreadable(String),
}

/// Check the exact full-file digest and size before interpreting UTF-8.
/// The catalog owner separately supplies purge state and coordinates writers.
pub fn inspect(root: &Path, relative: &Path, digest: &str, bytes: u32, maximum: usize) -> State {
    let read = paths::resolve(root, relative).and_then(|path| text::read_bytes(&path, maximum));
    match read {
        Ok(Some(content))
            if text::bytes_revision(&content) != digest
                || content.len() as u64 != u64::from(bytes) =>
        {
            State::Corrupt
        }
        Ok(Some(content)) => match String::from_utf8(content) {
            Ok(content) => State::Available(content),
            Err(_) => State::Corrupt,
        },
        Ok(None) => State::Missing,
        Err(Error::TooLarge { .. }) => State::Corrupt,
        Err(error) => State::Unreadable(error.to_string()),
    }
}
