//! Converting a native Git knowledge store into substrate snapshots.
mod support;

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};

use dots_documents::staged_directory::StagedDirectory;
use dots_memory::{Error, Scope, Snapshot};
use support::{Machine, Remote, TestResult, git};

const STORE: &str = "k-0123456789abcdef0123456789abcdef";
const PROJECT: &str = "p-fedcba9876543210fedcba9876543210";

/// A source store in the native Git layout: a bare SHA-256 repository with
/// history on `refs/heads/knowledge`, `store.json` and `bindings.json`.
struct Source {
    scratch: StagedDirectory,
}

impl Source {
    fn root(&self) -> PathBuf {
        self.scratch.path().join("knowledge")
    }

    fn git(&self, args: &[&str], input: Option<&[u8]>) -> String {
        let mut full = vec!["--git-dir", "repo.git"];
        full.extend(args);
        String::from_utf8(git(&self.root(), &full, input)).expect("UTF-8 git output")
    }

    fn blob(&self, content: &str) -> String {
        self.git(&["hash-object", "-w", "--stdin"], Some(content.as_bytes()))
            .trim()
            .to_owned()
    }

    /// Commit a tree of `knowledge.json` plus `documents`, as the store did.
    fn commit(
        &self,
        projects: &BTreeMap<&str, &str>,
        documents: &BTreeMap<&str, &str>,
        parents: &[&str],
    ) -> String {
        let registry = serde_json::json!({"store": STORE, "projects": projects});
        let mut registry = serde_json::to_string_pretty(&registry).expect("encode registry");
        registry.push('\n');
        let mut index = String::new();
        for (path, content) in std::iter::once(("knowledge.json", registry.as_str()))
            .chain(documents.iter().map(|(p, c)| (*p, *c)))
        {
            write!(index, "100644 {}\t{path}\0", self.blob(content)).expect("format index");
        }
        let index_file = self.scratch.path().join("index");
        let _ = fs::remove_file(&index_file);
        let mut command = std::process::Command::new("git");
        command
            .env_clear()
            .env("PATH", std::env::var_os("PATH").expect("test PATH"))
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_INDEX_FILE", &index_file)
            .current_dir(self.root())
            .args([
                "--git-dir",
                "repo.git",
                "update-index",
                "-z",
                "--index-info",
            ]);
        let output = dots_process::Runner::new(std::time::Duration::from_secs(30), 1024 * 1024)
            .stdin(index.clone().into_bytes(), index.len() + 1)
            .output(command)
            .expect("stage index");
        assert!(output.status.success());
        let mut command = std::process::Command::new("git");
        command
            .env_clear()
            .env("PATH", std::env::var_os("PATH").expect("test PATH"))
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_INDEX_FILE", &index_file)
            .current_dir(self.root())
            .args(["--git-dir", "repo.git", "write-tree"]);
        let output = dots_process::Runner::new(std::time::Duration::from_secs(30), 1024 * 1024)
            .output(command)
            .expect("write tree");
        let tree = String::from_utf8(output.stdout).expect("tree id");
        let mut args = vec!["commit-tree", tree.trim(), "-m", "Publish knowledge"];
        for parent in parents {
            args.extend(["-p", parent]);
        }
        self.git(&args, None).trim().to_owned()
    }
}

/// Neutral history with a registration, an empty page, a removal, a page
/// reused at two paths and a two-parent integration.
fn source() -> (Source, Vec<String>) {
    let source = Source {
        scratch: StagedDirectory::create(&std::env::temp_dir()).expect("create source"),
    };
    let root = source.root();
    fs::create_dir(&root).expect("create store root");
    git(
        &root,
        &[
            "init",
            "--bare",
            "--quiet",
            "--object-format=sha256",
            "--initial-branch=knowledge",
            "--template=",
            "repo.git",
        ],
        None,
    );
    let none = BTreeMap::new();
    let registered = BTreeMap::from([(PROJECT, "neutral project")]);
    let project_index = format!("projects/{PROJECT}/MEMORY.md");
    let genesis = source.commit(&none, &BTreeMap::new(), &[]);
    let index = source.commit(
        &none,
        &BTreeMap::from([("global/MEMORY.md", "index\n")]),
        &[&genesis],
    );
    let project = source.commit(
        &registered,
        &BTreeMap::from([("global/MEMORY.md", "index\n")]),
        &[&index],
    );
    let pages = BTreeMap::from([
        ("global/MEMORY.md", "index\n"),
        ("global/empty.md", ""),
        ("global/wiki/topic.md", "café\r\nno newline"),
        ("global/wiki/copy.md", "café\r\nno newline"),
        (project_index.as_str(), "project index\n"),
    ]);
    let published = source.commit(&registered, &pages, &[&project]);
    let mut removed = pages.clone();
    removed.remove("global/empty.md");
    let local = source.commit(&registered, &removed, &[&published]);
    let mut side = pages.clone();
    side.insert("global/remote.md", "remote\n");
    let remote = source.commit(&registered, &side, &[&published]);
    let mut merged = removed.clone();
    merged.insert("global/remote.md", "remote\n");
    let integration = source.commit(&registered, &merged, &[&local, &remote]);
    merged.insert("global/MEMORY.md", "after integration\n");
    let head = source.commit(&registered, &merged, &[&integration]);
    source.git(&["update-ref", "refs/heads/knowledge", &head], None);
    let store = serde_json::json!({
        "node": "n-00000000000000000000000000000001",
        "store": STORE,
        "genesis": genesis,
        "role": "authority",
        "actor": "neutral",
    });
    fs::write(
        root.join("store.json"),
        serde_json::to_vec_pretty(&store).expect("encode"),
    )
    .expect("write store.json");
    let bindings = serde_json::json!({"store": STORE, "projects": {"local-project": PROJECT}});
    fs::write(
        root.join("bindings.json"),
        serde_json::to_vec(&bindings).expect("encode"),
    )
    .expect("write bindings.json");
    (
        source,
        vec![
            genesis,
            index,
            project,
            published,
            local,
            remote,
            integration,
            head,
        ],
    )
}

/// Every file's path, mode and content under `root`.
fn inventory(root: &Path) -> BTreeMap<PathBuf, (u32, Vec<u8>)> {
    let mut files = BTreeMap::new();
    let mut stack = vec![root.to_owned()];
    while let Some(path) = stack.pop() {
        let metadata = fs::symlink_metadata(&path).expect("inspect source");
        let content = if metadata.is_dir() {
            stack.extend(
                fs::read_dir(&path)
                    .expect("list source")
                    .map(|entry| entry.expect("entry").path()),
            );
            Vec::new()
        } else {
            fs::read(&path).expect("read source")
        };
        files.insert(path, (metadata.permissions().mode(), content));
    }
    files
}

fn set_modes(root: &Path, directory: u32, file: u32) {
    let mut stack = vec![root.to_owned()];
    while let Some(path) = stack.pop() {
        let metadata = fs::symlink_metadata(&path).expect("inspect source");
        if metadata.is_dir() {
            fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).expect("open directory");
            stack.extend(
                fs::read_dir(&path)
                    .expect("list source")
                    .map(|entry| entry.expect("entry").path()),
            );
            fs::set_permissions(&path, fs::Permissions::from_mode(directory))
                .expect("set directory mode");
        } else {
            fs::set_permissions(&path, fs::Permissions::from_mode(file)).expect("set file mode");
        }
    }
}

#[test]
#[expect(clippy::too_many_lines, reason = "one fixture checked end to end")]
fn conversion_preserves_every_snapshot_and_reads_the_source_only() -> TestResult {
    let (source, commits) = source();
    let root = source.root();
    // The source is unwritable during conversion and unchanged after it.
    set_modes(&root, 0o500, 0o400);
    let before = inventory(&root);
    let remote = Remote::new();
    let mut a = Machine::new(&remote.url());
    let now = a.now();
    let conversion = dots_memory::convert(&root, &mut a.store, now);
    let after = inventory(&root);
    set_modes(&root, 0o700, 0o600);
    let conversion = conversion?;
    assert_eq!(before, after);
    assert_eq!(conversion.store.as_str(), STORE);
    assert_eq!(
        conversion
            .bindings
            .get("local-project")
            .map(dots_memory::ProjectId::as_str),
        Some(PROJECT)
    );
    let converted: Vec<_> = conversion
        .snapshots
        .iter()
        .map(|c| c.commit.clone())
        .collect();
    let mut expected = commits.clone();
    expected.sort();
    let mut sorted = converted.clone();
    sorted.sort();
    assert_eq!(sorted, expected);
    assert_eq!(
        conversion.head,
        conversion.snapshots.last().expect("snapshots").snapshot
    );
    let snapshots: BTreeMap<&str, Snapshot> = conversion
        .snapshots
        .iter()
        .map(|c| (c.commit.as_str(), c.snapshot))
        .collect();

    // Another machine sees the converted history only through the remote.
    let mut b = Machine::new(&remote.url());
    let head = b.head();
    assert_eq!(head.snapshot, conversion.head);
    assert_eq!(head.store.as_str(), STORE);
    let mut documents = 0;
    for commit in &commits {
        let snapshot = snapshots[commit.as_str()];
        let view = b.store.view(&head, Some(&snapshot))?;
        let manifest = view.manifest();
        let parents: Vec<_> = source
            .git(&["rev-list", "--parents", "-n", "1", commit], None)
            .split_whitespace()
            .skip(1)
            .map(|parent| snapshots[parent])
            .collect();
        assert_eq!(manifest.parents, parents, "{commit}");
        let tree = source.git(&["ls-tree", "-r", "--name-only", commit], None);
        let paths: Vec<_> = tree
            .lines()
            .filter(|path| *path != "knowledge.json")
            .collect();
        assert_eq!(
            manifest
                .entries
                .keys()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            paths
        );
        let registry: serde_json::Value = serde_json::from_str(
            &source.git(&["show", &format!("{commit}:knowledge.json")], None),
        )?;
        assert_eq!(
            serde_json::to_value(&manifest.projects)?,
            registry["projects"]
        );
        for path in paths {
            let original = git(
                &root,
                &[
                    "--git-dir",
                    "repo.git",
                    "cat-file",
                    "blob",
                    &format!("{commit}:{path}"),
                ],
                None,
            );
            let (scope, relative) = match path.strip_prefix("global/") {
                Some(relative) => (Scope::Global, relative),
                None => (
                    Scope::Project(PROJECT.parse()?),
                    path.strip_prefix(&format!("projects/{PROJECT}/"))
                        .expect("project path"),
                ),
            };
            let content = view
                .content(&scope, Path::new(relative))?
                .expect("document present");
            assert_eq!(content.as_bytes(), original.as_slice(), "{commit}:{path}");
            documents += 1;
        }
    }
    assert_eq!(documents, conversion.verified_documents);

    // The published memory ref is never replaced.
    let now = b.now();
    assert!(matches!(
        dots_memory::convert(&root, &mut b.store, now),
        Err(Error::AlreadyInitialized(s)) if s == conversion.head
    ));
    Ok(())
}
