use std::process::Command;

use super::*;
use crate::Blob;
use crate::conformance::{self, Remote};

type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;

/// A bare SHA-1 repository in a scratch directory, reached over `file://`.
#[derive(Debug)]
struct LocalRemote {
    scratch: StagedDirectory,
}

impl LocalRemote {
    fn new() -> Result<Self> {
        let remote = Self {
            scratch: StagedDirectory::create(&std::env::temp_dir())?,
        };
        remote.git(&[
            "init",
            "--bare",
            "--quiet",
            "--object-format=sha1",
            "--template=",
            "remote.git",
        ])?;
        Ok(remote)
    }

    fn url(&self) -> String {
        format!(
            "file://{}",
            self.scratch.path().join("remote.git").display()
        )
    }

    /// Inspect the remote directly, outside the backend.
    fn git(&self, args: &[&str]) -> Result<String> {
        let mut command = Command::new("git");
        command
            .env_clear()
            .env("PATH", env::var_os("PATH").unwrap_or_default())
            .env("HOME", self.scratch.path())
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .current_dir(self.scratch.path())
            .args(args);
        let output =
            dots_process::output(command, LOCAL_DEADLINE, OUTPUT_LIMIT).map_err(|source| {
                Error::Process {
                    operation: "test git",
                    source,
                }
            })?;
        String::from_utf8(checked(output, "test git")?)
            .map_err(|_| Error::Backend("non-UTF-8 test output".into()))
    }
}

impl Remote for LocalRemote {
    fn client(&self, label: &str) -> Result<Box<dyn Backend>> {
        Ok(Box::new(GitBackend::open(
            &self.scratch.path().join("clients").join(label),
            self.url(),
        )?))
    }
}

#[test]
fn conformance_over_file_remote() {
    conformance::run(&|| Box::new(LocalRemote::new().expect("create local remote")));
}

#[test]
fn ref_commits_carry_every_blob_and_chain() -> TestResult {
    let remote = LocalRemote::new()?;
    let backend = remote.client("a")?;
    let name: RefName = "tasks/p-1/t-2".parse()?;
    let (one, two) = (Blob::new(b"one".to_vec())?, Blob::new(b"two".to_vec())?);
    let first = backend.update(&[Update {
        name: name.clone(),
        expected: None,
        root: one.hash(),
        new_blobs: vec![one.clone()],
    }])?;
    let Outcome::Applied(first) = &first[0] else {
        return Err(format!("{first:?}").into());
    };
    let second = backend.update(&[Update {
        name: name.clone(),
        expected: Some(first.clone()),
        root: two.hash(),
        new_blobs: vec![two.clone()],
    }])?;
    let Outcome::Applied(second) = &second[0] else {
        return Err(format!("{second:?}").into());
    };

    let repository = ["--git-dir", "remote.git"];
    let reference = "refs/dots/tasks/p-1/t-2";
    let tip = remote.git(&[&repository[..], &["rev-parse", reference]].concat())?;
    assert_eq!(tip.trim(), second.as_str());
    let parent =
        remote.git(&[&repository[..], &["rev-parse", &format!("{reference}^")]].concat())?;
    assert_eq!(parent.trim(), first.as_str());
    let paths = remote.git(
        &[
            &repository[..],
            &["ls-tree", "-r", "--name-only", reference],
        ]
        .concat(),
    )?;
    let mut expected = vec![
        blob_path(&one.hash()),
        blob_path(&two.hash()),
        "ROOT".into(),
    ];
    expected.sort();
    assert_eq!(paths.lines().collect::<Vec<_>>(), expected);
    let root = remote.git(&[&repository[..], &["show", &format!("{reference}:ROOT")]].concat())?;
    assert_eq!(root, format!("{}\n", two.hash()));
    assert_eq!(
        remote.git(&[&repository[..], &["for-each-ref", "--format=%(refname)"]].concat())?,
        format!("{reference}\n")
    );
    Ok(())
}

#[test]
fn refs_outside_the_namespace_are_not_listed() -> TestResult {
    let remote = LocalRemote::new()?;
    let backend = remote.client("a")?;
    let blob = Blob::new(b"x".to_vec())?;
    backend.update(&[Update {
        name: "x".parse()?,
        expected: None,
        root: blob.hash(),
        new_blobs: vec![blob],
    }])?;
    let tip = remote.git(&["--git-dir", "remote.git", "rev-parse", "refs/dots/x"])?;
    remote.git(&[
        "--git-dir",
        "remote.git",
        "update-ref",
        "refs/heads/refs/dots/y",
        tip.trim(),
    ])?;
    assert_eq!(
        backend.list()?.into_keys().collect::<Vec<_>>(),
        vec!["x".parse::<RefName>()?]
    );
    Ok(())
}

#[test]
fn unreachable_remote_is_unavailable_or_unknown() -> TestResult {
    let scratch = StagedDirectory::create(&std::env::temp_dir())?;
    let url = format!("file://{}", scratch.path().join("missing.git").display());
    let backend = GitBackend::open(&scratch.path().join("client"), url)?;
    let name: RefName = "x".parse()?;
    assert!(matches!(backend.list(), Err(Error::Unavailable(_))));
    assert!(matches!(
        backend.fetch(&name, &|_| false),
        Err(Error::Unavailable(_))
    ));
    let blob = Blob::new(b"x".to_vec())?;
    let outcomes = backend.update(&[Update {
        name,
        expected: None,
        root: blob.hash(),
        new_blobs: vec![blob],
    }])?;
    assert_eq!(outcomes, vec![Outcome::Unknown]);
    Ok(())
}

#[test]
fn porcelain_lines_map_to_refs() {
    let stdout = "To file:///r.git\n\
        =\taaaa:refs/dots/same\t[up to date]\n\
        !\tbbbb:refs/dots/race\t[remote rejected] (cannot lock ref 'refs/dots/race': is at \
        0123456789abcdef0123456789abcdef01234567 but expected \
        89abcdef0123456789abcdef0123456789abcdef)\n\
        !\tcccc:refs/dots/stale\t[rejected] (stale info)\n\
        !\tdddd:refs/dots/moved\t[remote rejected] (incorrect old value provided)\n\
        Done\n";
    let report = parse_porcelain(stdout);
    assert_eq!(report.len(), 4);
    assert!(is_lease_failure(&report["refs/dots/moved"].summary));
    assert_eq!(report["refs/dots/same"].flag, '=');
    let race = &report["refs/dots/race"].summary;
    assert!(is_lease_failure(race));
    assert_eq!(
        reported_actual(race),
        Some("0123456789abcdef0123456789abcdef01234567")
    );
    let stale = &report["refs/dots/stale"].summary;
    assert!(is_lease_failure(stale));
    assert_eq!(reported_actual(stale), None);
    assert!(!is_lease_failure(
        "[remote rejected] (deny updating a hidden ref)"
    ));
}

/// Push `count` successive values of `name` and return each version.
fn push_chain(backend: &GitBackend, name: &RefName, count: usize) -> Result<Vec<Version>> {
    let mut versions: Vec<Version> = Vec::new();
    for index in 0..count {
        let blob = Blob::new(format!("value {index}").into_bytes())?;
        let outcome = backend.update(&[Update {
            name: name.clone(),
            expected: versions.last().cloned(),
            root: blob.hash(),
            new_blobs: vec![blob],
        }])?;
        let [Outcome::Applied(version)] = outcome.as_slice() else {
            return Err(Error::Backend(format!("{outcome:?}")));
        };
        versions.push(version.clone());
    }
    Ok(versions)
}

fn cache_git(backend: &GitBackend, args: &[&str]) -> Result<String> {
    backend.text("inspect cache", args, None)
}

#[test]
fn pushed_history_stays_reachable_through_garbage_collection() -> TestResult {
    let remote = LocalRemote::new()?;
    let backend = GitBackend::open(&remote.scratch.path().join("writer"), remote.url())?;
    let name: RefName = "memory".parse()?;
    let versions = push_chain(&backend, &name, 3)?;
    let local = cache_git(&backend, &["rev-parse", "refs/dots/memory"])?;
    assert_eq!(local.trim(), versions[2].as_str());

    cache_git(&backend, &["gc", "--quiet", "--prune=now"])?;
    for version in &versions {
        assert!(
            backend.has_commit(version.as_str())?,
            "{version} was pruned"
        );
    }
    // The next proposal builds on the cached parent without fetching it.
    let more = push_chain_from(&backend, &name, &versions[2])?;
    assert_eq!(
        remote
            .git(&["--git-dir", "remote.git", "rev-parse", "refs/dots/memory^"])?
            .trim(),
        versions[2].as_str()
    );
    assert_eq!(
        cache_git(&backend, &["rev-parse", "refs/dots/memory"])?.trim(),
        more.as_str()
    );
    Ok(())
}

fn push_chain_from(backend: &GitBackend, name: &RefName, base: &Version) -> Result<Version> {
    let blob = Blob::new(b"after gc".to_vec())?;
    match backend
        .update(&[Update {
            name: name.clone(),
            expected: Some(base.clone()),
            root: blob.hash(),
            new_blobs: vec![blob],
        }])?
        .as_slice()
    {
        [Outcome::Applied(version)] => Ok(version.clone()),
        other => Err(Error::Backend(format!("{other:?}"))),
    }
}

#[test]
fn maintenance_packs_the_cache_past_its_loose_object_limit() -> TestResult {
    let remote = LocalRemote::new()?;
    let root = remote.scratch.path().join("writer");
    let unused = GitBackend::open(&root.join("unused"), remote.url())?;
    assert_eq!(unused.maintain()?, Maintenance::Skipped);

    let backend = GitBackend::open(&root, remote.url())?.loose_object_limit(20);
    let name: RefName = "memory".parse()?;
    let versions = push_chain(&backend, &name, 2)?;
    assert_eq!(backend.maintain()?, Maintenance::Skipped);
    push_chain_from(&backend, &name, &versions[1])?;
    let loose = |backend: &GitBackend| -> Result<u64> {
        let counts = cache_git(backend, &["count-objects", "-v"])?;
        Ok(counts
            .lines()
            .find_map(|line| line.strip_prefix("count: "))
            .and_then(|count| count.parse().ok())
            .unwrap_or(u64::MAX))
    };
    for _ in 0..4 {
        let blob = Blob::new(getrandom::u64()?.to_string().into_bytes())?;
        let tip = Version::new(cache_git(&backend, &["rev-parse", "refs/dots/memory"])?.trim())?;
        backend.update(&[Update {
            name: name.clone(),
            expected: Some(tip),
            root: blob.hash(),
            new_blobs: vec![blob],
        }])?;
    }
    assert!(loose(&backend)? >= 20);
    assert!(matches!(backend.maintain()?, Maintenance::Ran(_)));
    assert_eq!(loose(&backend)?, 0);
    for version in &versions {
        assert!(backend.has_commit(version.as_str())?);
    }
    assert_eq!(backend.maintain()?, Maintenance::Skipped);
    Ok(())
}
