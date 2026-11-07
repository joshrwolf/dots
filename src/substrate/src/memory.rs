//! The in-process reference backend, and a fake watch with injectable faults.
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::SystemTime;

use crate::{Backend, Fetched, Hash, Outcome, RefName, Result, Update, Version, Watch, invalid};

/// Refs and blobs held in memory. Clones share one store, as machines share a
/// remote.
#[derive(Clone, Debug, Default)]
pub struct MemoryBackend {
    store: Arc<Mutex<Store>>,
}

#[derive(Debug, Default)]
struct Store {
    refs: BTreeMap<RefName, Stored>,
    blobs: BTreeMap<Hash, Vec<u8>>,
}

#[derive(Debug)]
struct Stored {
    version: Version,
    root: Hash,
    carried: BTreeSet<Hash>,
}

impl MemoryBackend {
    pub fn new() -> Self {
        Self::default()
    }
}

/// Versions depend only on the proposal, so re-sending one is idempotent.
fn proposed(update: &Update) -> Result<Version> {
    let mut identity = format!(
        "{}\0{}\0{}",
        update.name,
        update.expected.as_ref().map_or("", Version::as_str),
        update.root
    );
    let blobs: BTreeSet<_> = update.new_blobs.iter().map(crate::Blob::hash).collect();
    for hash in blobs {
        identity.push('\0');
        identity.push_str(&hash.to_string());
    }
    Version::new(format!("v-{}", &Hash::of(identity.as_bytes()).hex()[..40]))
}

impl Backend for MemoryBackend {
    fn list(&self) -> Result<BTreeMap<RefName, Version>> {
        let store = self.store.lock().unwrap_or_else(PoisonError::into_inner);
        Ok(store
            .refs
            .iter()
            .map(|(name, stored)| (name.clone(), stored.version.clone()))
            .collect())
    }

    fn fetch(&self, name: &RefName, cached: &dyn Fn(&Hash) -> bool) -> Result<Option<Fetched>> {
        let store = self.store.lock().unwrap_or_else(PoisonError::into_inner);
        let Some(stored) = store.refs.get(name) else {
            return Ok(None);
        };
        let blobs = stored
            .carried
            .iter()
            .filter(|hash| !cached(hash))
            .map(|hash| {
                let content = store.blobs.get(hash).ok_or_else(|| {
                    crate::Error::Backend(format!("memory backend lost blob {hash}"))
                })?;
                Ok((*hash, content.clone()))
            })
            .collect::<Result<_>>()?;
        Ok(Some(Fetched {
            version: stored.version.clone(),
            root: stored.root,
            blobs,
        }))
    }

    fn update(&self, updates: &[Update]) -> Result<Vec<Outcome>> {
        let mut store = self.store.lock().unwrap_or_else(PoisonError::into_inner);
        updates
            .iter()
            .map(|update| {
                let proposed = proposed(update)?;
                for blob in &update.new_blobs {
                    store.blobs.insert(blob.hash(), blob.content().to_vec());
                }
                let current = store.refs.get(&update.name);
                let actual = current.map(|stored| &stored.version);
                if actual == Some(&proposed) {
                    return Ok(Outcome::Applied(proposed));
                }
                if actual != update.expected.as_ref() {
                    return Ok(Outcome::Conflict {
                        actual: actual.cloned(),
                    });
                }
                let mut carried = current
                    .map(|stored| stored.carried.clone())
                    .unwrap_or_default();
                carried.extend(update.new_blobs.iter().map(crate::Blob::hash));
                if !carried.contains(&update.root) {
                    return Err(invalid(
                        "update",
                        format!("root {} of {} is not carried", update.root, update.name),
                    ));
                }
                store.refs.insert(
                    update.name.clone(),
                    Stored {
                        version: proposed.clone(),
                        root: update.root,
                        carried,
                    },
                );
                Ok(Outcome::Applied(proposed))
            })
            .collect()
    }
}

/// A fault applied to the next hint sent through a [`HintLink`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fault {
    Drop,
    Duplicate,
    /// Hold the hint until [`HintLink::release`].
    Delay,
}

#[derive(Clone, Debug)]
enum Event {
    Hint { sequence: u64, name: RefName },
    Heartbeat { at: SystemTime, through: u64 },
}

#[derive(Debug, Default)]
struct Link {
    sequence: u64,
    delivered: VecDeque<Event>,
    delayed: Vec<Event>,
    fault: Option<Fault>,
}

/// The sending side of a fake watch, driven by tests.
#[derive(Clone, Debug)]
pub struct HintLink {
    link: Arc<Mutex<Link>>,
}

/// A watch fed by a [`HintLink`]. Hints carry sequence numbers and heartbeats
/// state the last sequence sent, so the watch confirms only gap-free coverage.
#[derive(Debug)]
pub struct FakeWatch {
    link: Arc<Mutex<Link>>,
    seen: BTreeSet<u64>,
    confirmed: Option<SystemTime>,
}

/// A connected link and watch.
pub fn fake_watch() -> (HintLink, FakeWatch) {
    let link = Arc::new(Mutex::new(Link::default()));
    (
        HintLink { link: link.clone() },
        FakeWatch {
            link,
            seen: BTreeSet::new(),
            confirmed: None,
        },
    )
}

impl HintLink {
    /// Report that `name` changed.
    pub fn changed(&self, name: &RefName) {
        let mut link = self.link.lock().unwrap_or_else(PoisonError::into_inner);
        link.sequence += 1;
        let event = Event::Hint {
            sequence: link.sequence,
            name: name.clone(),
        };
        match link.fault.take() {
            None => link.delivered.push_back(event),
            Some(Fault::Drop) => {}
            Some(Fault::Duplicate) => {
                link.delivered.push_back(event.clone());
                link.delivered.push_back(event);
            }
            Some(Fault::Delay) => link.delayed.push(event),
        }
    }

    /// Vouch that every change before `at` has been sent.
    pub fn heartbeat(&self, at: SystemTime) {
        let mut link = self.link.lock().unwrap_or_else(PoisonError::into_inner);
        let through = link.sequence;
        link.delivered.push_back(Event::Heartbeat { at, through });
    }

    pub fn fault_next(&self, fault: Fault) {
        self.link
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .fault = Some(fault);
    }

    /// Deliver delayed hints.
    pub fn release(&self) {
        let mut link = self.link.lock().unwrap_or_else(PoisonError::into_inner);
        let delayed = std::mem::take(&mut link.delayed);
        link.delivered.extend(delayed);
    }
}

impl Watch for FakeWatch {
    fn hints(&mut self) -> Vec<RefName> {
        let events: Vec<_> = self
            .link
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .delivered
            .drain(..)
            .collect();
        let mut hints = Vec::new();
        for event in events {
            match event {
                Event::Hint { sequence, name } => {
                    if self.seen.insert(sequence) {
                        hints.push(name);
                    }
                }
                Event::Heartbeat { at, through } => {
                    let complete = self.seen.range(..=through).count() as u64 == through;
                    self.confirmed = complete.then_some(at);
                }
            }
        }
        hints
    }

    fn confirmed_as_of(&self) -> Option<SystemTime> {
        self.confirmed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conformance() {
        crate::conformance::run(&|| Box::new(MemoryRemote(MemoryBackend::new())));
    }

    #[derive(Debug)]
    struct MemoryRemote(MemoryBackend);

    impl crate::conformance::Remote for MemoryRemote {
        fn client(&self, _label: &str) -> Result<Box<dyn Backend>> {
            Ok(Box::new(self.0.clone()))
        }
    }

    #[test]
    fn watch_confirms_only_gap_free_coverage() -> Result<()> {
        let name: RefName = "x".parse()?;
        let at = SystemTime::UNIX_EPOCH;
        let (link, mut watch) = fake_watch();
        link.heartbeat(at);
        assert_eq!(watch.hints(), []);
        assert_eq!(watch.confirmed_as_of(), Some(at));

        link.fault_next(Fault::Duplicate);
        link.changed(&name);
        link.heartbeat(at);
        assert_eq!(watch.hints(), vec![name.clone()]);
        assert_eq!(watch.confirmed_as_of(), Some(at));

        link.fault_next(Fault::Delay);
        link.changed(&name);
        link.heartbeat(at);
        assert_eq!(watch.hints(), []);
        assert_eq!(watch.confirmed_as_of(), None);
        link.release();
        link.heartbeat(at);
        assert_eq!(watch.hints(), vec![name.clone()]);
        assert_eq!(watch.confirmed_as_of(), Some(at));

        link.fault_next(Fault::Drop);
        link.changed(&name);
        link.heartbeat(at);
        assert_eq!(watch.hints(), []);
        assert_eq!(watch.confirmed_as_of(), None);
        Ok(())
    }
}
