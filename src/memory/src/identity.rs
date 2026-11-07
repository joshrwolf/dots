//! Stable shared identities and content-addressed snapshot identifiers.
use std::fmt;
use std::str::FromStr;

use dots_substrate::Hash;
use serde::{Deserialize, Deserializer, Serialize};

use crate::{Result, invalid};

fn validate(value: &str, prefix: &str, digits: usize) -> Result<String> {
    let Some(hex) = value.strip_prefix(prefix) else {
        return Err(invalid("invalid knowledge identity prefix"));
    };
    if hex.len() != digits
        || !hex
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(invalid(
            "knowledge identity must contain exact lowercase hexadecimal digits",
        ));
    }
    Ok(value.into())
}

fn allocate(prefix: &str) -> Result<String> {
    let mut bytes = [0_u8; 16];
    getrandom::fill(&mut bytes)?;
    Ok(format!("{prefix}{}", hex::encode(bytes)))
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct StoreId(String);

impl StoreId {
    pub(crate) fn allocate() -> Result<Self> {
        Ok(Self(allocate("k-")?))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl FromStr for StoreId {
    type Err = crate::Error;
    fn from_str(value: &str) -> Result<Self> {
        Ok(Self(validate(value, "k-", 32)?))
    }
}
impl fmt::Display for StoreId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
impl<'de> Deserialize<'de> for StoreId {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        String::deserialize(deserializer)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct ProjectId(String);

impl ProjectId {
    pub(crate) fn allocate() -> Result<Self> {
        Ok(Self(allocate("p-")?))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl FromStr for ProjectId {
    type Err = crate::Error;
    fn from_str(value: &str) -> Result<Self> {
        Ok(Self(validate(value, "p-", 32)?))
    }
}
impl fmt::Display for ProjectId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
impl<'de> Deserialize<'de> for ProjectId {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        String::deserialize(deserializer)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}

/// A published or candidate knowledge snapshot: the hash of its manifest blob,
/// written `sha256:<hex>`.
#[derive(Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Snapshot(Hash);

impl Snapshot {
    pub(crate) fn new(hash: Hash) -> Self {
        Self(hash)
    }
    pub fn hash(&self) -> Hash {
        self.0
    }
}
impl FromStr for Snapshot {
    type Err = crate::Error;
    fn from_str(value: &str) -> Result<Self> {
        value
            .parse()
            .map(Self)
            .map_err(|_| invalid("snapshot must be sha256: and 64 lowercase hexadecimal digits"))
    }
}
impl fmt::Display for Snapshot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}
impl fmt::Debug for Snapshot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identities_validate_every_deserialization_boundary() {
        let store = StoreId::allocate().expect("allocate store");
        let project = ProjectId::allocate().expect("allocate project");
        assert_eq!(store.as_str().len(), 34);
        assert_eq!(project.as_str().len(), 34);
        assert!(project.as_str().parse::<StoreId>().is_err());
        assert!("a".repeat(64).parse::<Snapshot>().is_err());
        assert!(
            format!("sha256:{}", "A".repeat(64))
                .parse::<Snapshot>()
                .is_err()
        );
        assert!(serde_json::from_str::<StoreId>("\"k-not-an-id\"").is_err());
        assert!(serde_json::from_str::<Snapshot>("\"HEAD\"").is_err());
        let snapshot: Snapshot = format!("sha256:{}", "a".repeat(64)).parse().expect("parse");
        assert_eq!(snapshot.to_string(), format!("sha256:{}", "a".repeat(64)));
    }
}
