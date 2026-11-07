//! The backend conformance suite, written once and run against every backend.
//!
//! Each case gets a fresh [`Remote`] and its own ref prefix, so the suite can
//! also run against a shared real remote that keeps earlier runs' refs.
use std::collections::BTreeSet;
use std::fmt;
use std::path::PathBuf;
use std::sync::{Arc, Barrier, Mutex, PoisonError};
use std::thread;
use std::time::{Duration, SystemTime};

use dots_documents::staged_directory::StagedDirectory;

use crate::memory::{Fault, fake_watch};
use crate::{
    Backend, Blob, Budget, Config, Error, Fetched, Flush, Freshness, Hash, Outcome, RefName,
    Result, Substrate, Update, Version,
};

type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;
type Case = fn(&Lab) -> TestResult;

/// A backend under test: each client is a separate connection to one remote,
/// as separate machines would hold.
pub trait Remote: fmt::Debug {
    fn client(&self, label: &str) -> Result<Box<dyn Backend>>;
}

const CASES: &[(&str, Case)] = &[
    ("create if absent", create_if_absent),
    ("stale expectation", stale_expectation),
    ("idempotent re-apply", idempotent_reapply),
    ("batch with mixed outcomes", batch_mixed_outcomes),
    ("fetch carries every blob", fetch_carries_every_blob),
    ("root must be carried", root_must_be_carried),
    ("CAS race has one winner", cas_race_one_winner),
    ("unknown outcome resolution", unknown_outcome_resolution),
    ("blob verification failure", blob_verification_failure),
    ("freshness shared across instances", freshness_shared),
    ("offline reads", offline_reads),
    ("offline listing retried per max age", offline_retry),
    ("coalescing and budget", coalescing_and_budget),
    ("stranded conflict", stranded_conflict),
    ("flush with unknown outcomes", flush_unknown_outcomes),
    ("watch hint faults", watch_hint_faults),
];

/// Run every case, each against a remote from `new_remote`.
///
/// # Panics
///
/// When a case fails, naming it.
pub fn run(new_remote: &dyn Fn() -> Box<dyn Remote>) {
    for (name, case) in CASES {
        eprintln!("conformance: {name}");
        let lab = Lab::new(new_remote()).unwrap_or_else(|error| panic!("{name}: {error}"));
        if let Err(error) = case(&lab) {
            panic!("{name}: {error}");
        }
    }
}

struct Lab {
    remote: Box<dyn Remote>,
    scratch: StagedDirectory,
    prefix: String,
}

impl Lab {
    fn new(remote: Box<dyn Remote>) -> Result<Self> {
        Ok(Self {
            remote,
            scratch: StagedDirectory::create(&std::env::temp_dir())?,
            prefix: format!("conformance/c{:016x}", getrandom::u64()?),
        })
    }

    fn name(&self, suffix: &str) -> Result<RefName> {
        format!("{}/{suffix}", self.prefix).parse()
    }

    fn client(&self, label: &str) -> Result<Box<dyn Backend>> {
        self.remote.client(label)
    }

    fn probed(&self, label: &str) -> Result<(Probe, Box<dyn Backend>)> {
        let probe = Probe::default();
        let backend = Probed {
            inner: self.client(label)?,
            probe: probe.clone(),
        };
        Ok((probe, Box::new(backend)))
    }

    fn state(&self, label: &str) -> PathBuf {
        self.scratch.path().join(label)
    }

    fn substrate(&self, state: &str, backend: Box<dyn Backend>) -> Result<Substrate> {
        Substrate::open(&self.state(state), backend, Config::default())
    }
}

fn blob(text: &str) -> Result<Blob> {
    Blob::new(text.as_bytes().to_vec())
}

fn create(name: &RefName, root: &Blob) -> Update {
    change(name, None, root)
}

fn change(name: &RefName, expected: Option<&Version>, root: &Blob) -> Update {
    Update {
        name: name.clone(),
        expected: expected.cloned(),
        root: root.hash(),
        new_blobs: vec![root.clone()],
    }
}

fn one(backend: &dyn Backend, update: Update) -> Result<Outcome> {
    backend
        .update(&[update])?
        .pop()
        .ok_or_else(|| Error::Backend("no outcome".into()))
}

fn applied(outcome: Outcome) -> std::result::Result<Version, Box<dyn std::error::Error>> {
    match outcome {
        Outcome::Applied(version) => Ok(version),
        other => Err(format!("expected Applied, got {other:?}").into()),
    }
}

fn at(seconds: u64) -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000 + seconds)
}

fn create_if_absent(lab: &Lab) -> TestResult {
    let backend = lab.client("a")?;
    let name = lab.name("x")?;
    let version = applied(one(&*backend, create(&name, &blob("first")?))?)?;
    assert_eq!(
        one(&*backend, create(&name, &blob("second")?))?,
        Outcome::Conflict {
            actual: Some(version.clone())
        }
    );
    assert_eq!(backend.list()?.get(&name), Some(&version));
    Ok(())
}

fn stale_expectation(lab: &Lab) -> TestResult {
    let backend = lab.client("a")?;
    let name = lab.name("x")?;
    let first = applied(one(&*backend, create(&name, &blob("one")?))?)?;
    let second = applied(one(&*backend, change(&name, Some(&first), &blob("two")?))?)?;
    assert_ne!(first, second);
    assert_eq!(
        one(&*backend, change(&name, Some(&first), &blob("three")?))?,
        Outcome::Conflict {
            actual: Some(second.clone())
        }
    );
    let fetched = backend
        .fetch(&name, &|_| true)?
        .ok_or("fetched ref is absent")?;
    assert_eq!(
        (fetched.version, fetched.root),
        (second, blob("two")?.hash())
    );
    Ok(())
}

fn idempotent_reapply(lab: &Lab) -> TestResult {
    let backend = lab.client("a")?;
    let name = lab.name("x")?;
    let first = applied(one(&*backend, create(&name, &blob("one")?))?)?;
    let update = change(&name, Some(&first), &blob("two")?);
    let second = applied(one(&*backend, update.clone())?)?;
    assert_eq!(one(&*backend, update)?, Outcome::Applied(second));
    Ok(())
}

fn batch_mixed_outcomes(lab: &Lab) -> TestResult {
    let backend = lab.client("a")?;
    let [a, b, c] = [lab.name("a")?, lab.name("b")?, lab.name("c")?];
    let va = applied(one(&*backend, create(&a, &blob("a0")?))?)?;
    let vb = applied(one(&*backend, create(&b, &blob("b0")?))?)?;
    let outcomes = backend.update(&[
        change(&a, Some(&va), &blob("a1")?),
        create(&b, &blob("b1")?),
        create(&c, &blob("c1")?),
    ])?;
    assert_eq!(outcomes.len(), 3);
    let listed = backend.list()?;
    assert_eq!(Some(&applied(outcomes[0].clone())?), listed.get(&a));
    assert_eq!(outcomes[1], Outcome::Conflict { actual: Some(vb) });
    assert_eq!(Some(&applied(outcomes[2].clone())?), listed.get(&c));
    Ok(())
}

fn fetch_carries_every_blob(lab: &Lab) -> TestResult {
    let backend = lab.client("a")?;
    let name = lab.name("x")?;
    assert!(backend.fetch(&name, &|_| false)?.is_none());
    let (root, extra, later) = (blob("root")?, blob("extra")?, blob("later")?);
    let mut update = create(&name, &root);
    update.new_blobs.push(extra.clone());
    let first = applied(one(&*backend, update)?)?;
    let second = applied(one(&*backend, change(&name, Some(&first), &later))?)?;
    let hashes = |fetched: &Fetched| -> BTreeSet<Hash> {
        fetched.blobs.iter().map(|(hash, _)| *hash).collect()
    };
    let all = backend.fetch(&name, &|_| false)?.ok_or("absent")?;
    assert_eq!((&all.version, all.root), (&second, later.hash()));
    assert_eq!(
        hashes(&all),
        [root.hash(), extra.hash(), later.hash()].into()
    );
    for (hash, content) in &all.blobs {
        assert_eq!(Hash::of(content), *hash);
    }
    let missing = backend
        .fetch(&name, &|hash| *hash != extra.hash())?
        .ok_or("absent")?;
    assert_eq!(hashes(&missing), [extra.hash()].into());
    Ok(())
}

fn root_must_be_carried(lab: &Lab) -> TestResult {
    let backend = lab.client("a")?;
    let name = lab.name("x")?;
    let mut update = create(&name, &blob("root")?);
    update.new_blobs.clear();
    assert!(backend.update(&[update]).is_err());
    Ok(())
}

fn cas_race_one_winner(lab: &Lab) -> TestResult {
    let clients = [lab.client("first")?, lab.client("second")?];
    let name = lab.name("race")?;
    let mut base: Option<Version> = None;
    for round in 0..5 {
        let updates = [0, 1].map(|side| {
            blob(&format!("round {round} side {side}"))
                .map(|root| change(&name, base.as_ref(), &root))
        });
        let barrier = Barrier::new(2);
        let outcomes = thread::scope(|scope| {
            let handles: Vec<_> = clients
                .iter()
                .zip(updates)
                .map(|(client, update)| {
                    let barrier = &barrier;
                    scope.spawn(move || {
                        let update = update?;
                        barrier.wait();
                        one(&**client, update)
                    })
                })
                .collect();
            handles
                .into_iter()
                .map(|handle| handle.join().map_err(|_| "racer panicked"))
                .collect::<std::result::Result<Vec<_>, _>>()
        })?;
        let outcomes = outcomes.into_iter().collect::<Result<Vec<_>>>()?;
        let winner = match (&outcomes[0], &outcomes[1]) {
            (Outcome::Applied(winner), Outcome::Conflict { actual })
            | (Outcome::Conflict { actual }, Outcome::Applied(winner)) => {
                assert_eq!(actual.as_ref(), Some(winner), "round {round}");
                winner.clone()
            }
            other => return Err(format!("round {round}: {other:?}").into()),
        };
        assert_eq!(clients[0].list()?.get(&name), Some(&winner));
        base = Some(winner);
    }
    Ok(())
}

fn unknown_outcome_resolution(lab: &Lab) -> TestResult {
    let (probe, backend) = lab.probed("a")?;
    let substrate = lab.substrate("a", backend)?;
    let other = lab.client("b")?;
    let name = lab.name("x")?;

    probe.set(Mode::LoseReply);
    let first = applied(substrate.update(&create(&name, &blob("one")?), at(0))?)?;
    assert_eq!(other.list()?.get(&name), Some(&first));

    probe.set(Mode::LoseRequest);
    let update = change(&name, Some(&first), &blob("two")?);
    assert_eq!(substrate.update(&update, at(1))?, Outcome::Unknown);
    assert_eq!(other.list()?.get(&name), Some(&first));
    probe.set(Mode::Normal);
    let second = applied(substrate.update(&update, at(2))?)?;

    let theirs = applied(one(
        &*other,
        change(&name, Some(&second), &blob("theirs")?),
    )?)?;
    probe.set(Mode::LoseRequest);
    assert_eq!(
        substrate.update(&change(&name, Some(&second), &blob("mine")?), at(3))?,
        Outcome::Conflict {
            actual: Some(theirs)
        }
    );
    Ok(())
}

fn blob_verification_failure(lab: &Lab) -> TestResult {
    let (probe, backend) = lab.probed("a")?;
    let substrate = lab.substrate("a", backend)?;
    let name = lab.name("x")?;
    let root = blob("content")?;
    applied(one(&*lab.client("b")?, create(&name, &root))?)?;

    probe.set(Mode::Corrupt);
    assert!(matches!(
        substrate.read(&name, at(0)),
        Err(Error::Integrity { .. })
    ));
    probe.set(Mode::Normal);
    let value = substrate.read(&name, at(1))?.ok_or("absent")?;
    assert_eq!(substrate.blob(&value.root)?, root);

    let hex = root.hash().hex();
    let cached = lab.state("a").join("blobs").join(&hex[..2]).join(&hex);
    std::fs::write(&cached, b"tampered")?;
    assert!(matches!(
        substrate.blob(&root.hash()),
        Err(Error::Integrity { .. })
    ));
    Ok(())
}

fn freshness_shared(lab: &Lab) -> TestResult {
    let (first_probe, first) = lab.probed("a1")?;
    let (second_probe, second) = lab.probed("a2")?;
    let mut first = lab.substrate("shared", first)?;
    let mut second = lab.substrate("shared", second)?;
    assert_eq!(first.machine(), second.machine());
    let name = lab.name("x")?;
    applied(one(&*lab.client("b")?, create(&name, &blob("value")?))?)?;

    assert_eq!(
        first.ensure_fresh(Duration::ZERO, at(0))?,
        Freshness::Current { as_of: at(0) }
    );
    assert_eq!(
        second.ensure_fresh(Duration::from_secs(30), at(10))?,
        Freshness::Current { as_of: at(0) }
    );
    assert_eq!(second_probe.counts().lists, 0);
    assert!(first.read(&name, at(11))?.is_some());
    assert!(second.read(&name, at(12))?.is_some());
    assert_eq!(second_probe.counts().fetches, 0);

    second.ensure_fresh(Duration::from_secs(30), at(31))?;
    assert_eq!(second_probe.counts().lists, 1);
    assert_eq!(first_probe.counts().lists, 1);
    Ok(())
}

fn offline_reads(lab: &Lab) -> TestResult {
    let (probe, backend) = lab.probed("a")?;
    let mut substrate = lab.substrate("a", backend)?;
    let other = lab.client("b")?;
    let (cached, uncached) = (lab.name("cached")?, lab.name("uncached")?);
    let version = applied(one(&*other, create(&cached, &blob("cached")?))?)?;
    applied(one(&*other, create(&uncached, &blob("uncached")?))?)?;
    substrate.ensure_fresh(Duration::ZERO, at(0))?;
    assert!(substrate.read(&cached, at(1))?.is_some());

    probe.set(Mode::Offline);
    assert!(matches!(
        substrate.ensure_fresh(Duration::ZERO, at(10))?,
        Freshness::Stale { since: Some(since), .. } if since == at(0)
    ));
    let value = substrate.read(&cached, at(11))?.ok_or("absent")?;
    assert_eq!(value.version, version);
    assert_eq!(value.stale_since, Some(at(1)));
    assert!(matches!(
        substrate.read(&uncached, at(12)),
        Err(Error::Unavailable(_))
    ));

    probe.set(Mode::Normal);
    substrate.ensure_fresh(Duration::ZERO, at(20))?;
    assert_eq!(
        substrate
            .read(&cached, at(21))?
            .ok_or("absent")?
            .stale_since,
        None
    );
    Ok(())
}

fn offline_retry(lab: &Lab) -> TestResult {
    let (probe, backend) = lab.probed("a")?;
    let mut substrate = lab.substrate("a", backend)?;
    let max_age = Duration::from_secs(30);
    substrate.ensure_fresh(Duration::ZERO, at(0))?;
    probe.set(Mode::Offline);
    let stale = |freshness: Freshness| matches!(freshness, Freshness::Stale { since: Some(since), .. } if since == at(0));

    assert!(stale(substrate.ensure_fresh(max_age, at(40))?));
    assert_eq!(probe.counts().lists, 2);
    assert!(stale(substrate.ensure_fresh(max_age, at(70))?));
    assert_eq!(probe.counts().lists, 2, "a recent failure is not retried");
    assert!(stale(substrate.ensure_fresh(Duration::ZERO, at(71))?));
    assert_eq!(probe.counts().lists, 3, "zero tolerance always retries");
    assert!(stale(substrate.ensure_fresh(max_age, at(102))?));
    assert_eq!(probe.counts().lists, 4);
    assert_eq!(substrate.status()?.offline_since, Some(at(40)));

    probe.set(Mode::Normal);
    assert_eq!(
        substrate.ensure_fresh(max_age, at(140))?,
        Freshness::Current { as_of: at(140) }
    );
    assert_eq!(substrate.status()?.offline_since, None);
    Ok(())
}

fn coalescing_and_budget(lab: &Lab) -> TestResult {
    let (probe, backend) = lab.probed("a")?;
    let config = Config {
        budget: Budget::new(1, Duration::from_secs(60))?,
    };
    let substrate = Substrate::open(&lab.state("a"), backend, config)?;
    let (x, y) = (lab.name("x")?, lab.name("y")?);
    for text in ["x1", "x2", "x3"] {
        substrate.defer(create(&x, &blob(text)?), at(0))?;
    }
    substrate.defer(create(&y, &blob("y1")?), at(0))?;
    assert_eq!(substrate.status()?.pending.len(), 2);

    let Flush::Sent(report) = substrate.flush(at(1))? else {
        return Err("first flush did not send".into());
    };
    assert_eq!(report.applied.len(), 2, "{report:?}");
    assert_eq!(probe.counts().batches, vec![2]);
    let other = lab.client("b")?;
    let fetched = other.fetch(&x, &|_| false)?.ok_or("absent")?;
    assert_eq!(fetched.root, blob("x3")?.hash());
    assert_eq!(fetched.blobs.len(), 3);
    assert_eq!(substrate.status()?.pending, []);

    let version = report
        .applied
        .iter()
        .find(|(name, _)| *name == x)
        .map(|(_, version)| version.clone());
    substrate.defer(change(&x, version.as_ref(), &blob("x4")?), at(2))?;
    assert_eq!(
        substrate.flush(at(2))?,
        Flush::Throttled { retry_at: at(61) }
    );
    assert_eq!(substrate.status()?.pending.len(), 1);
    assert!(matches!(substrate.flush(at(61))?, Flush::Sent(r) if r.applied.len() == 1));
    assert_eq!(probe.counts().batches, vec![2, 1]);
    assert_eq!(substrate.flush(at(62))?, Flush::Idle);
    Ok(())
}

fn stranded_conflict(lab: &Lab) -> TestResult {
    let substrate = lab.substrate("a", lab.client("a")?)?;
    let other = lab.substrate("b", lab.client("b")?)?;
    let name = lab.name("tree")?;
    let (mine, theirs) = (blob("mine")?, blob("theirs")?);
    substrate.defer(create(&name, &mine), at(0))?;
    let taken = applied(other.update(&create(&name, &theirs), at(1))?)?;

    let Flush::Sent(report) = substrate.flush(at(2))? else {
        return Err("flush did not send".into());
    };
    assert_eq!(report.stranded, vec![name.clone()]);
    let status = substrate.status()?;
    assert_eq!(status.pending, []);
    let [stranded] = status.stranded.as_slice() else {
        return Err(format!("stranded: {:?}", status.stranded).into());
    };
    assert_eq!(
        (&stranded.name, stranded.root, &stranded.actual),
        (&name, mine.hash(), &Some(taken.clone()))
    );
    assert_eq!(substrate.blob(&mine.hash())?, mine);
    let value = substrate.read(&name, at(3))?.ok_or("absent")?;
    assert_eq!((value.version, value.root), (taken, theirs.hash()));
    Ok(())
}

fn flush_unknown_outcomes(lab: &Lab) -> TestResult {
    let (probe, backend) = lab.probed("a")?;
    let config = Config {
        budget: Budget::new(10, Duration::from_secs(1))?,
    };
    let substrate = Substrate::open(&lab.state("a"), backend, config)?;
    let name = lab.name("x")?;
    substrate.defer(create(&name, &blob("one")?), at(0))?;

    probe.set(Mode::LoseRequest);
    let Flush::Sent(report) = substrate.flush(at(1))? else {
        return Err("flush did not send".into());
    };
    assert_eq!(report.retained, vec![name.clone()]);
    assert_eq!(substrate.status()?.pending.len(), 1);

    probe.set(Mode::LoseReply);
    let Flush::Sent(report) = substrate.flush(at(2))? else {
        return Err("flush did not send".into());
    };
    assert_eq!(report.applied.len(), 1, "{report:?}");
    assert_eq!(substrate.status()?.pending, []);
    assert_eq!(substrate.status()?.stranded, []);

    probe.set(Mode::Offline);
    substrate.defer(
        change(&name, Some(&report.applied[0].1), &blob("two")?),
        at(3),
    )?;
    let Flush::Sent(report) = substrate.flush(at(4))? else {
        return Err("flush did not send".into());
    };
    assert!(report.error.is_some());
    let status = substrate.status()?;
    assert_eq!(status.pending.len(), 1);
    assert!(status.last_flush.and_then(|flush| flush.error).is_some());
    Ok(())
}

fn watch_hint_faults(lab: &Lab) -> TestResult {
    let (probe, backend) = lab.probed("a")?;
    let mut substrate = lab.substrate("a", backend)?;
    let other = lab.substrate("b", lab.client("b")?)?;
    let name = lab.name("x")?;
    let max_age = Duration::from_secs(30);
    let mut version = None;
    let mut write = |text: &str, now| -> std::result::Result<Version, Box<dyn std::error::Error>> {
        let written = applied(other.update(&change(&name, version.as_ref(), &blob(text)?), now)?)?;
        version = Some(written.clone());
        Ok(written)
    };
    let lists = || probe.counts().lists;

    let (link, watch) = fake_watch();
    substrate.attach_watch(Box::new(watch));
    substrate.ensure_fresh(max_age, at(0))?;
    link.heartbeat(at(40));
    assert_eq!(
        substrate.ensure_fresh(max_age, at(50))?,
        Freshness::Current { as_of: at(40) }
    );
    assert_eq!(lists(), 1);

    let written = write("hinted", at(52))?;
    link.changed(&name);
    substrate.ensure_fresh(max_age, at(56))?;
    assert_eq!(lists(), 2);
    assert_eq!(
        substrate.read(&name, at(57))?.ok_or("absent")?.version,
        written
    );

    link.fault_next(Fault::Duplicate);
    let written = write("duplicated", at(58))?;
    link.changed(&name);
    link.heartbeat(at(60));
    substrate.ensure_fresh(max_age, at(61))?;
    link.heartbeat(at(95));
    substrate.ensure_fresh(max_age, at(100))?;
    assert_eq!(lists(), 3);
    assert_eq!(
        substrate.read(&name, at(101))?.ok_or("absent")?.version,
        written
    );

    link.fault_next(Fault::Drop);
    let written = write("dropped", at(110))?;
    link.changed(&name);
    link.heartbeat(at(120));
    substrate.ensure_fresh(max_age, at(130))?;
    assert_eq!(lists(), 4);
    assert_eq!(
        substrate.read(&name, at(131))?.ok_or("absent")?.version,
        written
    );

    let (link, watch) = fake_watch();
    substrate.attach_watch(Box::new(watch));
    substrate.ensure_fresh(max_age, at(200))?;
    link.fault_next(Fault::Delay);
    let written = write("delayed", at(230))?;
    link.changed(&name);
    link.heartbeat(at(240));
    substrate.ensure_fresh(max_age, at(250))?;
    assert_eq!(lists(), 6);
    assert_eq!(
        substrate.read(&name, at(251))?.ok_or("absent")?.version,
        written
    );
    link.release();
    substrate.ensure_fresh(max_age, at(252))?;
    assert_eq!(lists(), 7);
    link.heartbeat(at(260));
    assert_eq!(
        substrate.ensure_fresh(max_age, at(285))?,
        Freshness::Current { as_of: at(260) }
    );
    assert_eq!(lists(), 7);
    Ok(())
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Mode {
    #[default]
    Normal,
    /// Every call fails as unreachable.
    Offline,
    /// Updates apply but report [`Outcome::Unknown`].
    LoseReply,
    /// Updates report [`Outcome::Unknown`] without reaching the backend.
    LoseRequest,
    /// Fetched blob bytes are altered.
    Corrupt,
}

#[derive(Clone, Debug, Default)]
struct Counts {
    lists: usize,
    fetches: usize,
    batches: Vec<usize>,
}

#[derive(Clone, Debug, Default)]
struct Probe {
    state: Arc<Mutex<(Mode, Counts)>>,
}

impl Probe {
    fn set(&self, mode: Mode) {
        self.state.lock().unwrap_or_else(PoisonError::into_inner).0 = mode;
    }

    fn counts(&self) -> Counts {
        self.state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .1
            .clone()
    }

    fn record(&self, count: impl FnOnce(&mut Counts)) -> Mode {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        count(&mut state.1);
        state.0
    }
}

/// Counts calls and injects faults around a real backend.
struct Probed {
    inner: Box<dyn Backend>,
    probe: Probe,
}

fn offline() -> Error {
    Error::Unavailable("probe is offline".into())
}

impl Backend for Probed {
    fn list(&self) -> Result<std::collections::BTreeMap<RefName, Version>> {
        match self.probe.record(|counts| counts.lists += 1) {
            Mode::Offline => Err(offline()),
            _ => self.inner.list(),
        }
    }

    fn fetch(&self, name: &RefName, cached: &dyn Fn(&Hash) -> bool) -> Result<Option<Fetched>> {
        match self.probe.record(|counts| counts.fetches += 1) {
            Mode::Offline => Err(offline()),
            Mode::Corrupt => Ok(self.inner.fetch(name, cached)?.map(|mut fetched| {
                for (_, content) in &mut fetched.blobs {
                    content.push(b'!');
                }
                fetched
            })),
            _ => self.inner.fetch(name, cached),
        }
    }

    fn update(&self, updates: &[Update]) -> Result<Vec<Outcome>> {
        match self
            .probe
            .record(|counts| counts.batches.push(updates.len()))
        {
            Mode::Offline => Err(offline()),
            Mode::LoseRequest => Ok(vec![Outcome::Unknown; updates.len()]),
            Mode::LoseReply => {
                self.inner.update(updates)?;
                Ok(vec![Outcome::Unknown; updates.len()])
            }
            Mode::Normal | Mode::Corrupt => self.inner.update(updates),
        }
    }
}
