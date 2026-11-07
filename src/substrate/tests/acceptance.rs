//! The backend conformance suite against a real Git remote, with latency samples.
//!
//! Set `DOTS_SUBSTRATE_TEST_REMOTE` to a disposable remote URL; the suite leaves
//! refs under `refs/dots/conformance/`. Set
//! `DOTS_SUBSTRATE_TEST_CREDENTIAL_HELPER` to a Git credential helper, such as
//! `!gh auth git-credential`, to authenticate; `HOME` then passes through so the
//! helper finds its own configuration.
use std::path::Path;
use std::time::{Duration, Instant};

use dots_documents::staged_directory::StagedDirectory;
use dots_substrate::conformance::{self, Remote};
use dots_substrate::git::GitBackend;
use dots_substrate::{Backend, Blob, Outcome, RefName, Update};

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[derive(Debug)]
struct Real {
    url: String,
    helper: Option<String>,
    scratch: StagedDirectory,
}

impl Real {
    fn from_env() -> Option<Self> {
        let Some(url) = std::env::var("DOTS_SUBSTRATE_TEST_REMOTE")
            .ok()
            .filter(|url| !url.is_empty())
        else {
            eprintln!("skipping: set DOTS_SUBSTRATE_TEST_REMOTE to a disposable Git remote URL");
            return None;
        };
        let helper = std::env::var("DOTS_SUBSTRATE_TEST_CREDENTIAL_HELPER").ok();
        let scratch = StagedDirectory::create(&std::env::temp_dir()).expect("create scratch");
        Some(Self {
            url,
            helper,
            scratch,
        })
    }

    fn backend(&self, root: &Path) -> dots_substrate::Result<GitBackend> {
        let mut backend = GitBackend::open(root, self.url.clone())?;
        if let Some(helper) = &self.helper {
            backend = backend
                .config("credential.helper", "")
                .config("credential.helper", helper.clone());
            if let Some(home) = std::env::var_os("HOME") {
                backend = backend.env("HOME", home);
            }
        }
        Ok(backend)
    }
}

impl Remote for Real {
    fn client(&self, label: &str) -> dots_substrate::Result<Box<dyn Backend>> {
        Ok(Box::new(self.backend(&self.scratch.path().join(label))?))
    }
}

#[test]
fn conformance_against_real_remote() {
    if Real::from_env().is_none() {
        return;
    }
    conformance::run(&|| Box::new(Real::from_env().expect("remote is configured")));
}

#[test]
fn latency_against_real_remote() -> TestResult {
    let Some(real) = Real::from_env() else {
        return Ok(());
    };
    let backend = real.backend(&real.scratch.path().join("latency"))?;
    let name: RefName = format!("conformance/latency-{:016x}", getrandom::u64()?).parse()?;
    let (mut lists, mut pushes, mut fetches) = (Vec::new(), Vec::new(), Vec::new());
    let mut expected = None;
    for round in 0..5 {
        let started = Instant::now();
        backend.list()?;
        lists.push(started.elapsed());

        let root = Blob::new(format!("latency round {round}").into_bytes())?;
        let started = Instant::now();
        let outcomes = backend.update(&[Update {
            name: name.clone(),
            expected: expected.clone(),
            root: root.hash(),
            new_blobs: vec![root],
        }])?;
        pushes.push(started.elapsed());
        let [Outcome::Applied(version)] = outcomes.as_slice() else {
            return Err(format!("round {round}: {outcomes:?}").into());
        };
        expected = Some(version.clone());

        let fresh = real.backend(&real.scratch.path().join(format!("fetch-{round}")))?;
        let started = Instant::now();
        fresh.fetch(&name, &|_| false)?;
        fetches.push(started.elapsed());
    }
    for (operation, samples) in [("list", lists), ("update", pushes), ("cold fetch", fetches)] {
        println!(
            "{operation}: median {:?}, samples {samples:?}",
            median(samples.clone())
        );
    }
    Ok(())
}

fn median(mut samples: Vec<Duration>) -> Duration {
    samples.sort();
    samples[samples.len() / 2]
}
