//! Stable shared identities and exact SHA-256 Git snapshot identifiers.
use std::fmt;
use std::str::FromStr;

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

/// A local store instance, independent of the shared dataset and its host address.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct NodeId(String);

impl NodeId {
    pub(crate) fn allocate() -> Result<Self> {
        Ok(Self(allocate("n-")?))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl FromStr for NodeId {
    type Err = crate::Error;
    fn from_str(value: &str) -> Result<Self> {
        Ok(Self(validate(value, "n-", 32)?))
    }
}
impl fmt::Display for NodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
impl<'de> Deserialize<'de> for NodeId {
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

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct Snapshot(String);

impl Snapshot {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl FromStr for Snapshot {
    type Err = crate::Error;
    fn from_str(value: &str) -> Result<Self> {
        Ok(Self(validate(value, "", 64)?))
    }
}
impl fmt::Display for Snapshot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
impl<'de> Deserialize<'de> for Snapshot {
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
    fn identities_validate_every_deserialization_boundary() {
        let store = StoreId::allocate().expect("allocate store");
        let project = ProjectId::allocate().expect("allocate project");
        assert_eq!(store.as_str().len(), 34);
        assert_eq!(project.as_str().len(), 34);
        assert!(project.as_str().parse::<StoreId>().is_err());
        assert!("a".repeat(40).parse::<Snapshot>().is_err());
        assert!("A".repeat(64).parse::<Snapshot>().is_err());
        assert!(serde_json::from_str::<StoreId>("\"k-not-an-id\"").is_err());
        assert!(serde_json::from_str::<Snapshot>("\"HEAD\"").is_err());
        assert!("a".repeat(64).parse::<Snapshot>().is_ok());
    }
}
