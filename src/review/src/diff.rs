//! Unified-diff hunk headers: which lines a review can anchor to, and how
//! line numbers move between two revisions.
use std::collections::BTreeMap;

use crate::{Error, Result, Side};

/// One hunk header, `@@ -old_start,old_len +new_start,new_len @@`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Hunk {
    pub(crate) old_start: u32,
    pub(crate) old_len: u32,
    pub(crate) new_start: u32,
    pub(crate) new_len: u32,
}

/// One changed path and its hunks, in order.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct FilePatch {
    pub(crate) hunks: Vec<Hunk>,
    pub(crate) added: u32,
    pub(crate) removed: u32,
    pub(crate) binary: bool,
}

/// Every changed path of a patch. Renames are off, so a path names the same
/// file on both sides.
#[derive(Debug, Default)]
pub(crate) struct Patch {
    pub(crate) files: BTreeMap<String, FilePatch>,
}

impl Patch {
    pub(crate) fn parse(bytes: &[u8]) -> Result<Self> {
        let mut files = BTreeMap::new();
        let mut current: Option<(String, FilePatch)> = None;
        let mut in_hunk = false;
        for line in bytes.split(|byte| *byte == b'\n') {
            if let Some(rest) = line.strip_prefix(b"diff --git ") {
                if let Some((path, patch)) = current.take() {
                    files.insert(path, patch);
                }
                current = Some((header_path(rest)?, FilePatch::default()));
                in_hunk = false;
                continue;
            }
            let Some((_, patch)) = current.as_mut() else {
                if line.is_empty() {
                    continue;
                }
                return Err(malformed("content before the first file header"));
            };
            if in_hunk {
                match line.first() {
                    Some(b'+') => {
                        patch.added += 1;
                        continue;
                    }
                    Some(b'-') => {
                        patch.removed += 1;
                        continue;
                    }
                    Some(b' ' | b'\\') => continue,
                    _ => in_hunk = false,
                }
            }
            if line.starts_with(b"@@ ") {
                patch.hunks.push(header(line)?);
                in_hunk = true;
            } else if line.starts_with(b"Binary files ") {
                patch.binary = true;
            }
        }
        if let Some((path, patch)) = current {
            files.insert(path, patch);
        }
        Ok(Self { files })
    }

    /// Whether `start..=end` on `side` of `path` lies within one hunk.
    pub(crate) fn shows(&self, path: &str, side: Side, start: u32, end: u32) -> bool {
        self.files.get(path).is_some_and(|file| {
            file.hunks.iter().any(|hunk| {
                let (first, len) = match side {
                    Side::New => (hunk.new_start, hunk.new_len),
                    Side::Old => (hunk.old_start, hunk.old_len),
                };
                len > 0 && start >= first && end < first + len
            })
        })
    }

    /// Where `start..=end` of `path` lands after this zero-context patch, or
    /// `None` when any of those lines changed or the path was removed.
    pub(crate) fn map(&self, path: &str, start: u32, end: u32) -> Option<(u32, u32)> {
        let Some(file) = self.files.get(path) else {
            return Some((start, end));
        };
        let mut shift: i64 = 0;
        for hunk in &file.hunks {
            if hunk.old_len == 0 {
                // Insertion after line `old_start`.
                if hunk.old_start >= end {
                    break;
                }
                if hunk.old_start >= start {
                    return None;
                }
            } else {
                let last = hunk.old_start + hunk.old_len - 1;
                if hunk.old_start > end {
                    break;
                }
                if last >= start {
                    return None;
                }
            }
            shift += i64::from(hunk.new_len) - i64::from(hunk.old_len);
        }
        let moved = |line: u32| u32::try_from(i64::from(line) + shift).ok();
        Some((moved(start)?, moved(end)?))
    }
}

fn malformed(reason: &str) -> Error {
    Error::Git {
        operation: "parse diff",
        message: reason.to_owned(),
    }
}

/// The path of `a/PATH b/PATH` (identical sides, since renames are off),
/// either side possibly C-quoted.
fn header_path(rest: &[u8]) -> Result<String> {
    let path = if rest.first() == Some(&b'"') {
        let (quoted, _) = unquote(rest)?;
        quoted
            .strip_prefix(b"a/")
            .ok_or_else(|| malformed("file header"))?
            .to_vec()
    } else {
        // Unquoted: "a/" + P + " b/" + P, so P has a known length.
        let length = rest
            .len()
            .checked_sub(5)
            .filter(|length| length % 2 == 0)
            .ok_or_else(|| malformed("file header"))?
            / 2;
        let (old, new) = (&rest[2..2 + length], &rest[length + 5..]);
        if !rest.starts_with(b"a/") || &rest[2 + length..length + 5] != b" b/" || old != new {
            return Err(malformed("file header"));
        }
        old.to_vec()
    };
    String::from_utf8(path).map_err(|_| malformed("path is not UTF-8"))
}

/// Undo Git's C-style quoting of the leading quoted string, returning it and
/// the bytes after its closing quote.
fn unquote(quoted: &[u8]) -> Result<(Vec<u8>, &[u8])> {
    let inner = quoted
        .strip_prefix(b"\"")
        .ok_or_else(|| malformed("quoted path"))?;
    let mut out = Vec::with_capacity(inner.len());
    let mut bytes = inner.iter().copied().enumerate();
    while let Some((index, byte)) = bytes.next() {
        if byte == b'"' {
            return Ok((out, &inner[index + 1..]));
        }
        if byte != b'\\' {
            out.push(byte);
            continue;
        }
        let (_, escaped) = bytes
            .next()
            .ok_or_else(|| malformed("dangling escape in path"))?;
        out.push(match escaped {
            b'n' => b'\n',
            b't' => b'\t',
            b'r' => b'\r',
            b'a' => 0x07,
            b'b' => 0x08,
            b'f' => 0x0c,
            b'v' => 0x0b,
            b'0'..=b'7' => {
                let mut value = u32::from(escaped - b'0');
                for _ in 0..2 {
                    let (_, digit) = bytes
                        .next()
                        .ok_or_else(|| malformed("short octal escape"))?;
                    if !(b'0'..=b'7').contains(&digit) {
                        return Err(malformed("bad octal escape"));
                    }
                    value = value * 8 + u32::from(digit - b'0');
                }
                u8::try_from(value).map_err(|_| malformed("octal escape out of range"))?
            }
            other => other,
        });
    }
    Err(malformed("unterminated quoted path"))
}

fn header(line: &[u8]) -> Result<Hunk> {
    let text = std::str::from_utf8(line).map_err(|_| malformed("hunk header is not UTF-8"))?;
    let ranges = text
        .strip_prefix("@@ ")
        .and_then(|rest| rest.split_once(" @@"))
        .map(|(ranges, _)| ranges)
        .ok_or_else(|| malformed("hunk header"))?;
    let (old, new) = ranges
        .split_once(' ')
        .ok_or_else(|| malformed("hunk header"))?;
    let range = |value: Option<&str>| -> Result<(u32, u32)> {
        let value = value.ok_or_else(|| malformed("hunk range"))?;
        let (start, len) = value.split_once(',').unwrap_or((value, "1"));
        let parse = |number: &str| number.parse().map_err(|_| malformed("hunk range"));
        Ok((parse(start)?, parse(len)?))
    };
    let (old_start, old_len) = range(old.strip_prefix('-'))?;
    let (new_start, new_len) = range(new.strip_prefix('+'))?;
    Ok(Hunk {
        old_start,
        old_len,
        new_start,
        new_len,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const PATCH: &str = "diff --git a/src/lib.rs b/src/lib.rs
index 1111111..2222222 100644
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -3,4 +3,5 @@ fn context() {
 three
-four
+four changed
+four and a half
 five
 six
@@ -20 +21,0 @@
-twenty
diff --git a/gone.txt b/gone.txt
deleted file mode 100644
--- a/gone.txt
+++ /dev/null
@@ -1,2 +0,0 @@
-one
-two
diff --git \"a/sp\\303\\251cial \\\"name\\\".txt\" \"b/sp\\303\\251cial \\\"name\\\".txt\"
new file mode 100644
--- /dev/null
+++ \"b/sp\\303\\251cial \\\"name\\\".txt\"
@@ -0,0 +1 @@
+hello
diff --git a/a/nested.txt b/a/nested.txt
--- a/a/nested.txt
+++ b/a/nested.txt
@@ -1 +1 @@
-x
+y
diff --git a/image.png b/image.png
new file mode 100644
Binary files /dev/null and b/image.png differ
";

    #[test]
    fn parses_paths_hunks_and_counts() {
        let patch = Patch::parse(PATCH.as_bytes()).unwrap();
        let paths: Vec<_> = patch.files.keys().map(String::as_str).collect();
        assert_eq!(
            paths,
            [
                "a/nested.txt",
                "gone.txt",
                "image.png",
                "spécial \"name\".txt",
                "src/lib.rs"
            ]
        );
        let lib = &patch.files["src/lib.rs"];
        assert_eq!(
            lib.hunks,
            [
                Hunk {
                    old_start: 3,
                    old_len: 4,
                    new_start: 3,
                    new_len: 5
                },
                Hunk {
                    old_start: 20,
                    old_len: 1,
                    new_start: 21,
                    new_len: 0
                },
            ]
        );
        assert_eq!((lib.added, lib.removed), (2, 2));
        assert!(patch.files["image.png"].binary);
    }

    #[test]
    fn shows_only_lines_inside_one_hunk_on_that_side() {
        let patch = Patch::parse(PATCH.as_bytes()).unwrap();
        assert!(patch.shows("src/lib.rs", Side::New, 3, 7));
        assert!(!patch.shows("src/lib.rs", Side::New, 7, 8));
        assert!(patch.shows("src/lib.rs", Side::Old, 20, 20));
        assert!(!patch.shows("src/lib.rs", Side::New, 21, 21));
        assert!(!patch.shows("gone.txt", Side::New, 1, 1));
        assert!(patch.shows("gone.txt", Side::Old, 1, 2));
        assert!(!patch.shows("missing.rs", Side::New, 1, 1));
    }

    #[test]
    fn maps_unchanged_lines_and_rejects_touched_ranges() {
        let zero = "diff --git a/f b/f
--- a/f
+++ b/f
@@ -2,0 +3,2 @@
+new
+new
@@ -10,2 +12 @@
-x
-y
+z
";
        let patch = Patch::parse(zero.as_bytes()).unwrap();
        assert_eq!(patch.map("f", 1, 2), Some((1, 2)));
        assert_eq!(patch.map("f", 2, 3), None, "insertion inside the range");
        assert_eq!(patch.map("f", 3, 9), Some((5, 11)));
        assert_eq!(patch.map("f", 9, 10), None);
        assert_eq!(patch.map("f", 12, 12), Some((13, 13)));
        assert_eq!(patch.map("other", 4, 4), Some((4, 4)));
    }
}
