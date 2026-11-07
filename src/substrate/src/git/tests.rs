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
        Done\n";
    let report = parse_porcelain(stdout);
    assert_eq!(report.len(), 3);
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
