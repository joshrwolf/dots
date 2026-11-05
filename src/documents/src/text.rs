//! Shared bounded UTF-8 reads, content revisions, and inspection pages.
use std::fs::{self, File};
use std::io::Read as _;
use std::path::Path;

use sha2::{Digest as _, Sha256};

use crate::{Document, Result, invalid, io, paths};

/// Read a regular file once, rejecting source symlinks and oversized/non-UTF-8 bytes.
/// Absence is distinct from an empty file. Native concurrent edits are not coordinated.
pub fn read(path: &Path, maximum: usize) -> Result<Option<String>> {
    read_bytes(path, maximum)?
        .map(|bytes| {
            String::from_utf8(bytes).map_err(|error| {
                io(
                    "decode UTF-8 text file",
                    path,
                    std::io::Error::new(std::io::ErrorKind::InvalidData, error),
                )
            })
        })
        .transpose()
}

/// Read bounded raw bytes so immutable consumers verify integrity before decoding.
pub fn read_bytes(path: &Path, maximum: usize) -> Result<Option<Vec<u8>>> {
    paths::reject_symlink(path)?;
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(io("inspect text file", path, error)),
        Ok(metadata) if !metadata.is_file() => {
            return Err(invalid("text path is not a regular file"));
        }
        Ok(_) => {}
    }
    let file = File::open(path).map_err(|error| io("open text file", path, error))?;
    let mut content = Vec::new();
    file.take((maximum + 1) as u64)
        .read_to_end(&mut content)
        .map_err(|error| io("read UTF-8 text file", path, error))?;
    if content.len() > maximum {
        return Err(crate::Error::TooLarge {
            path: path.to_owned(),
            maximum,
        });
    }
    Ok(Some(content))
}

#[must_use]
pub fn revision(content: Option<&str>) -> String {
    content.map_or_else(
        || "missing".into(),
        |content| bytes_revision(content.as_bytes()),
    )
}

#[must_use]
pub fn bytes_revision(content: &[u8]) -> String {
    format!("sha256:{}", hex::encode(Sha256::digest(content)))
}

/// Bound excerpts by bytes without splitting a Unicode grapheme.
#[must_use]
pub fn excerpt(text: &str, bytes: usize) -> String {
    use unicode_segmentation::UnicodeSegmentation as _;
    let mut output = String::new();
    for grapheme in text.graphemes(true) {
        if output.len() + grapheme.len() > bytes {
            break;
        }
        output.push_str(grapheme);
    }
    output
}
pub fn validate_page(start: usize, lines: usize) -> Result<()> {
    if start == 0 || !(1..=200).contains(&lines) {
        return Err(invalid("start must be positive; lines must be 1..200"));
    }
    Ok(())
}

/// Bound output independently of the maximum stored file size.
pub fn page(path: &Path, content: Option<&str>, start: usize, lines: usize) -> Result<Document> {
    validate_page(start, lines)?;
    let revision = revision(content);
    let exists = content.is_some();
    let content = content.unwrap_or_default();
    let total_lines = content.lines().count();
    let mut selected = String::new();
    let mut count = 0;
    for line in content.lines().skip(start - 1).take(lines) {
        if selected.len() + line.len() + 1 > 16 * 1024 {
            if count == 0 {
                return Err(invalid("text line exceeds the read budget"));
            }
            break;
        }
        selected.push_str(line);
        selected.push('\n');
        count += 1;
    }
    Ok(Document {
        path: path.to_owned(),
        revision,
        exists,
        content: selected,
        start,
        total_lines,
        next_line: (start.saturating_add(count) <= total_lines)
            .then_some(start.saturating_add(count)),
    })
}
