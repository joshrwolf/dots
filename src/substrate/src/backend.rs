//! What every storage backend provides, and the optional watch capability.
//!
//! Capabilities change cost, never semantics: batching, watching and
//! connection reuse may make the same results cheaper, not different.
use std::collections::BTreeMap;
use std::time::SystemTime;

use crate::{Blob, Hash, RefName, Result, Version};

/// A proposed compare-and-swap of one ref.
#[derive(Clone, Debug)]
pub struct Update {
    pub name: RefName,
    /// The version the ref must hold, or `None` when it must not exist.
    pub expected: Option<Version>,
    /// The new root blob. It must be in `new_blobs` or already carried by the ref.
    pub root: Hash,
    /// Blobs this update makes reachable from the ref.
    pub new_blobs: Vec<Blob>,
}

/// The result of one ref's update.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// The ref now holds the proposal at this version.
    Applied(Version),
    /// The ref held `actual` (`None`: absent) instead of the expected version.
    Conflict { actual: Option<Version> },
    /// The request or its reply was lost; the proposal may or may not have landed.
    Unknown,
}

/// A ref's current value with the blob bytes the local cache lacks.
#[derive(Clone, Debug)]
pub struct Fetched {
    pub version: Version,
    pub root: Hash,
    /// Claimed hash and unverified bytes; the substrate verifies each.
    pub blobs: Vec<(Hash, Vec<u8>)>,
}

/// A store of refs and the blobs they carry.
///
/// A rejected update may still have stored its blobs remotely; nothing relies
/// on rejection meaning "not stored". There is no delete, purge or multi-ref
/// atomicity.
pub trait Backend: Send + Sync {
    /// Every ref under the namespace with its version, in one request.
    ///
    /// Transport failures return [`crate::Error::Unavailable`].
    fn list(&self) -> Result<BTreeMap<RefName, Version>>;

    /// A ref's current value, with every blob it carries for which `cached`
    /// returns false.
    fn fetch(&self, name: &RefName, cached: &dyn Fn(&Hash) -> bool) -> Result<Option<Fetched>>;

    /// Compare-and-swap each ref independently, never atomically across refs.
    /// Returns one outcome per update, in order. Names must be distinct.
    /// Re-sending an applied proposal reports it applied at the same version.
    fn update(&self, updates: &[Update]) -> Result<Vec<Outcome>>;
}

/// Optional change notification. Hints only reduce listing; a watch that is
/// unsure of its coverage reports no confirmation.
pub trait Watch {
    /// Refs reported changed since the previous call. May repeat names and
    /// never blocks.
    fn hints(&mut self) -> Vec<RefName>;

    /// As of the last [`Watch::hints`] call, every change before this time has
    /// been hinted. `None` while the watch cannot vouch for its coverage.
    fn confirmed_as_of(&self) -> Option<SystemTime>;
}
