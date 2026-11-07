//! Validated ref names, opaque versions and machine identities.
use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize};

use crate::{Error, Result, invalid};

const MAX_NAME_BYTES: usize = 200;
const MAX_VERSION_BYTES: usize = 128;

/// A ref name relative to the substrate's fixed namespace, such as `memory` or
/// `tasks/p-0123/t-4567`. Segments use ASCII letters, digits, `-`, `_` and `.`,
/// never start with `.` and never end with `.lock`, so every name is also a
/// valid Git ref suffix.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct RefName(String);

impl RefName {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl FromStr for RefName {
    type Err = Error;

    fn from_str(value: &str) -> Result<Self> {
        let reject = |reason: &str| Err(invalid("ref name", format!("{value:?} {reason}")));
        if value.is_empty() || value.len() > MAX_NAME_BYTES {
            return reject("must have 1 to 200 bytes");
        }
        #[expect(
            clippy::case_sensitive_file_extension_comparisons,
            reason = "Git ref names are case-sensitive"
        )]
        for segment in value.split('/') {
            if segment.is_empty() {
                return reject("has an empty segment");
            }
            if segment.starts_with('.') || segment.ends_with(".lock") || segment.contains("..") {
                return reject("has a reserved segment");
            }
            if !segment
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
            {
                return reject("has a character outside [A-Za-z0-9._-]");
            }
        }
        Ok(Self(value.into()))
    }
}

impl fmt::Display for RefName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl fmt::Debug for RefName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.0, f)
    }
}

impl<'de> Deserialize<'de> for RefName {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        String::deserialize(deserializer)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}

/// A backend's opaque compare-and-swap token for one ref value. Only equality
/// is meaningful.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct Version(String);

impl Version {
    /// Accept 1 to 128 printable, non-space ASCII bytes.
    pub fn new(token: impl Into<String>) -> Result<Self> {
        let token = token.into();
        if token.is_empty()
            || token.len() > MAX_VERSION_BYTES
            || !token.bytes().all(|b| b.is_ascii_graphic())
        {
            return Err(invalid(
                "version",
                format!("{token:?} must be 1 to 128 printable ASCII bytes"),
            ));
        }
        Ok(Self(token))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl fmt::Debug for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.0, f)
    }
}

impl<'de> Deserialize<'de> for Version {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        Version::new(String::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

/// One machine's substrate identity, `m-` and 32 lowercase hex digits.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct MachineId(String);

impl MachineId {
    pub(crate) fn allocate() -> Result<Self> {
        let mut bytes = [0_u8; 16];
        getrandom::fill(&mut bytes)?;
        Ok(Self(format!("m-{}", hex::encode(bytes))))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl FromStr for MachineId {
    type Err = Error;

    fn from_str(value: &str) -> Result<Self> {
        let valid = value.strip_prefix("m-").is_some_and(|digits| {
            digits.len() == 32
                && digits
                    .bytes()
                    .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
        });
        if !valid {
            return Err(invalid(
                "machine id",
                format!("{value:?} is not m- and 32 hex digits"),
            ));
        }
        Ok(Self(value.into()))
    }
}

impl fmt::Display for MachineId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl fmt::Debug for MachineId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.0, f)
    }
}

impl<'de> Deserialize<'de> for MachineId {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        String::deserialize(deserializer)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ref_names_accept_namespaced_identities_only() {
        for good in ["memory", "tasks/p-0a/t-1b", "machines/m-00", "a.b_c-D"] {
            assert!(good.parse::<RefName>().is_ok(), "{good}");
        }
        for bad in [
            "", "/memory", "memory/", "a//b", ".hidden", "a/x.lock", "a..b", "a b", "a:b", "a*",
            "é",
        ] {
            assert!(bad.parse::<RefName>().is_err(), "{bad}");
        }
        assert!("a".repeat(201).parse::<RefName>().is_err());
    }

    #[test]
    fn versions_and_machines_validate() -> Result<()> {
        assert!(Version::new("").is_err());
        assert!(Version::new("a b").is_err());
        assert_eq!(Version::new("abc")?.as_str(), "abc");
        let machine = MachineId::allocate()?;
        assert_eq!(machine.as_str().parse::<MachineId>()?, machine);
        assert!("m-XYZ".parse::<MachineId>().is_err());
        Ok(())
    }
}
