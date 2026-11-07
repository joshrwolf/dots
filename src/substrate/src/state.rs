//! The per-machine record shared by every process using one state directory.
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions};
use std::io::{ErrorKind, Write as _};
use std::os::unix::fs::{DirBuilderExt as _, OpenOptionsExt as _};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use dots_documents::staged_file::StagedFile;
use serde::{Deserialize, Serialize};

use crate::{Error, Hash, MachineId, RefName, Result, Version, io};

const MAX_STATE_BYTES: u64 = 16 * 1024 * 1024;

/// A deferred update waiting for a flush.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pending {
    pub name: RefName,
    pub expected: Option<Version>,
    pub root: Hash,
    /// Blobs to upload with the update, including those of coalesced proposals.
    pub blobs: BTreeSet<Hash>,
    #[serde(with = "millis")]
    pub since: SystemTime,
}

/// A deferred update another writer pre-empted. Its blobs stay cached.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Stranded {
    pub name: RefName,
    pub expected: Option<Version>,
    /// What the ref held instead (`None`: absent).
    pub actual: Option<Version>,
    pub root: Hash,
    pub blobs: BTreeSet<Hash>,
    #[serde(with = "millis")]
    pub at: SystemTime,
}

/// The last flush that sent updates.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FlushAttempt {
    #[serde(with = "millis")]
    pub at: SystemTime,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Known {
    pub(crate) version: Version,
    pub(crate) root: Hash,
    #[serde(with = "millis")]
    pub(crate) at: SystemTime,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Bucket {
    pub(crate) tokens: u32,
    #[serde(with = "millis")]
    pub(crate) at: SystemTime,
}

/// Listing has failed since `since`; the latest attempt failed at `checked_at`.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Offline {
    #[serde(with = "millis")]
    pub(crate) since: SystemTime,
    #[serde(with = "millis")]
    pub(crate) checked_at: SystemTime,
    pub(crate) reason: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct State {
    #[serde(with = "optional_millis")]
    pub(crate) listed_at: Option<SystemTime>,
    pub(crate) offline: Option<Offline>,
    pub(crate) listed: BTreeMap<RefName, Version>,
    pub(crate) known: BTreeMap<RefName, Known>,
    pub(crate) outbox: BTreeMap<RefName, Pending>,
    pub(crate) stranded: Vec<Stranded>,
    pub(crate) bucket: Option<Bucket>,
    pub(crate) last_flush: Option<FlushAttempt>,
}

impl State {
    /// Record a version this machine observed as current.
    pub(crate) fn observe(&mut self, name: &RefName, version: Option<&Version>) {
        if self.listed_at.is_none() {
            return;
        }
        match version {
            Some(version) => {
                self.listed.insert(name.clone(), version.clone());
            }
            None => {
                self.listed.remove(name);
            }
        }
    }
}

/// A state directory: machine identity, locked record and blob cache.
#[derive(Debug)]
pub(crate) struct Directory {
    root: PathBuf,
}

impl Directory {
    pub(crate) fn open(root: &Path) -> Result<Self> {
        for path in [root.to_owned(), root.join("blobs")] {
            fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(&path)
                .map_err(|source| io("create substrate state directory", &path, source))?;
        }
        Ok(Self { root: root.into() })
    }

    pub(crate) fn root(&self) -> &Path {
        &self.root
    }

    /// The persisted machine identity, allocated on first use.
    pub(crate) fn machine(&self) -> Result<MachineId> {
        let path = self.root.join("machine");
        loop {
            match fs::read_to_string(&path) {
                Ok(text) => {
                    return text
                        .strip_suffix('\n')
                        .ok_or_else(|| Error::State {
                            path: path.clone(),
                            reason: "machine identity lacks its newline".into(),
                        })?
                        .parse();
                }
                Err(error) if error.kind() == ErrorKind::NotFound => {}
                Err(source) => return Err(io("read machine identity", &path, source)),
            }
            let staged = StagedFile::create(&self.root)?;
            writeln!(staged.file(), "{}", MachineId::allocate()?)
                .map_err(|source| io("write machine identity", staged.path(), source))?;
            match staged.publish_new(&path) {
                Ok(()) => {}
                // Another process allocated first; read its identity.
                Err(dots_documents::Error::Io { source, .. })
                    if source.kind() == ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error.into()),
            }
        }
    }

    /// Read the record under a shared lock.
    pub(crate) fn read<T>(&self, read: impl FnOnce(&State) -> T) -> Result<T> {
        let lock = self.lock()?;
        lock.lock_shared()
            .map_err(|source| io("lock substrate state", &self.lock_path(), source))?;
        let state = self.load()?;
        Ok(read(&state))
    }

    /// Change the record under an exclusive lock, saving only on success.
    pub(crate) fn modify<T>(&self, change: impl FnOnce(&mut State) -> Result<T>) -> Result<T> {
        let lock = self.lock()?;
        lock.lock()
            .map_err(|source| io("lock substrate state", &self.lock_path(), source))?;
        let mut state = self.load()?;
        let value = change(&mut state)?;
        let staged = StagedFile::create(&self.root)?;
        serde_json::to_writer(staged.file(), &state).map_err(|error| Error::State {
            path: staged.path().into(),
            reason: error.to_string(),
        })?;
        staged.publish_replace(&self.state_path())?;
        Ok(value)
    }

    fn lock(&self) -> Result<File> {
        let path = self.lock_path();
        OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .open(&path)
            .map_err(|source| io("open substrate lock", &path, source))
    }

    fn load(&self) -> Result<State> {
        let path = self.state_path();
        let file = match File::open(&path) {
            Ok(file) => file,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(State::default()),
            Err(source) => return Err(io("open substrate state", &path, source)),
        };
        let size = file
            .metadata()
            .map_err(|source| io("inspect substrate state", &path, source))?
            .len();
        if size > MAX_STATE_BYTES {
            return Err(Error::State {
                path,
                reason: format!("exceeds {MAX_STATE_BYTES} bytes"),
            });
        }
        serde_json::from_reader(std::io::BufReader::new(file)).map_err(|error| Error::State {
            path,
            reason: error.to_string(),
        })
    }

    fn lock_path(&self) -> PathBuf {
        self.root.join("state.lock")
    }

    fn state_path(&self) -> PathBuf {
        self.root.join("state.json")
    }
}

/// Time elapsed from `then` to `now`; zero if the clock moved backwards.
pub(crate) fn age(now: SystemTime, then: SystemTime) -> Duration {
    now.duration_since(then).unwrap_or_default()
}

mod millis {
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    use serde::{Deserialize as _, Deserializer, Serializer};

    pub(super) fn serialize<S: Serializer>(
        time: &SystemTime,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        let millis = time
            .duration_since(UNIX_EPOCH)
            .map_err(serde::ser::Error::custom)?
            .as_millis();
        serializer.serialize_u64(u64::try_from(millis).map_err(serde::ser::Error::custom)?)
    }

    pub(super) fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<SystemTime, D::Error> {
        let millis = u64::deserialize(deserializer)?;
        UNIX_EPOCH
            .checked_add(Duration::from_millis(millis))
            .ok_or_else(|| serde::de::Error::custom("time out of range"))
    }
}

mod optional_millis {
    use std::time::SystemTime;

    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    #[derive(Serialize, Deserialize)]
    #[serde(transparent)]
    struct Wrapped(#[serde(with = "super::millis")] SystemTime);

    #[expect(clippy::ref_option, reason = "serde's `with` passes a reference")]
    pub(super) fn serialize<S: Serializer>(
        time: &Option<SystemTime>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        time.map(Wrapped).serialize(serializer)
    }

    pub(super) fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<SystemTime>, D::Error> {
        Ok(Option::<Wrapped>::deserialize(deserializer)?.map(|Wrapped(time)| time))
    }
}
