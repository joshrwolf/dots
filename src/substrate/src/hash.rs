//! Content identity and verified blobs.
use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use sha2::{Digest as _, Sha256};

use crate::{Error, Result, invalid};

/// Largest blob a ref may carry (GitHub's recommended object size).
pub const MAX_BLOB_BYTES: usize = 1024 * 1024;

const PREFIX: &str = "sha256:";

/// SHA-256 of a blob's content, written `sha256:<hex>`.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Hash([u8; 32]);

impl Hash {
    pub fn of(content: &[u8]) -> Self {
        Self(Sha256::digest(content).into())
    }

    /// The 64 lowercase hexadecimal digits, without the prefix.
    pub fn hex(&self) -> String {
        hex::encode(self.0)
    }
}

impl fmt::Display for Hash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{PREFIX}{}", self.hex())
    }
}

impl fmt::Debug for Hash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

impl FromStr for Hash {
    type Err = Error;

    fn from_str(value: &str) -> Result<Self> {
        let digits = value
            .strip_prefix(PREFIX)
            .ok_or_else(|| invalid("hash", format!("{value:?} lacks the {PREFIX} prefix")))?;
        Self::from_hex(digits)
    }
}

impl Hash {
    /// Parse exactly 64 lowercase hexadecimal digits.
    pub fn from_hex(digits: &str) -> Result<Self> {
        if digits.len() != 64
            || !digits
                .bytes()
                .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
        {
            return Err(invalid(
                "hash",
                format!("{digits:?} is not 64 lowercase hex digits"),
            ));
        }
        let mut bytes = [0; 32];
        hex::decode_to_slice(digits, &mut bytes)
            .map_err(|error| invalid("hash", error.to_string()))?;
        Ok(Self(bytes))
    }
}

impl Serialize for Hash {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Hash {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        String::deserialize(deserializer)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}

/// Immutable bytes whose content matches their hash.
#[derive(Clone, PartialEq, Eq)]
pub struct Blob {
    hash: Hash,
    content: Vec<u8>,
}

impl Blob {
    /// Hash new content.
    pub fn new(content: Vec<u8>) -> Result<Self> {
        check_size(content.len())?;
        Ok(Self {
            hash: Hash::of(&content),
            content,
        })
    }

    /// Accept content read from a backend or cache only if it matches `expected`.
    pub fn verified(expected: Hash, content: Vec<u8>) -> Result<Self> {
        check_size(content.len())?;
        if Hash::of(&content) != expected {
            return Err(Error::Integrity { expected });
        }
        Ok(Self {
            hash: expected,
            content,
        })
    }

    pub fn hash(&self) -> Hash {
        self.hash
    }

    pub fn content(&self) -> &[u8] {
        &self.content
    }

    pub fn into_content(self) -> Vec<u8> {
        self.content
    }
}

impl fmt::Debug for Blob {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Blob")
            .field("hash", &self.hash)
            .field("len", &self.content.len())
            .finish()
    }
}

fn check_size(size: usize) -> Result<()> {
    if size > MAX_BLOB_BYTES {
        return Err(Error::TooLarge { size });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_round_trips_and_rejects_malformed_text() -> Result<()> {
        let hash = Hash::of(b"abc");
        assert_eq!(
            hash.to_string(),
            "sha256:ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(hash.to_string().parse::<Hash>()?, hash);
        for bad in [
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
            "sha256:BA7816BF8F01CFEA414140DE5DAE2223B00361A396177A9CB410FF61F20015AD",
            "sha256:ba78",
        ] {
            assert!(bad.parse::<Hash>().is_err(), "{bad}");
        }
        Ok(())
    }

    #[test]
    fn blob_verification_rejects_mismatch_and_oversize() {
        let expected = Hash::of(b"one");
        assert!(matches!(
            Blob::verified(expected, b"two".to_vec()),
            Err(Error::Integrity { expected: e }) if e == expected
        ));
        assert!(matches!(
            Blob::new(vec![0; MAX_BLOB_BYTES + 1]),
            Err(Error::TooLarge { .. })
        ));
    }
}
