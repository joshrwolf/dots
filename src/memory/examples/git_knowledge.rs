//! Exercise Git-native knowledge with two disposable replicas and no network.
//!
//! Run with `cargo run -p dots-memory --example git_knowledge`. Every Git command
//! uses an isolated HOME, accepts only file transport, and goes through dots-process.
//! This executable has no arguments and cannot open an existing knowledge store.
use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fmt;
use std::fs;
use std::os::unix::fs::DirBuilderExt as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::Duration;

const HEAD: &str = "refs/heads/knowledge";
const OBSERVED: &str = "refs/dots/observed/linux";
const MANIFEST: &str = "{\"store\":\"neutral-fixture\",\"projects\":[\"demo\"]}\n";
const MAX_COMMITS: usize = 128;
const MAX_DOCUMENTS: usize = 64;
const MAX_BYTES: usize = 2 * 1024 * 1024;

type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
enum Error {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Process(#[from] dots_process::Error),
    #[error("invalid fixture or knowledge: {0}")]
    Invalid(String),
    #[error("git {operation}: {stderr}")]
    Git { operation: String, stderr: String },
    #[error("publication rejected; expected {expected}, observed {actual}")]
    RefRejected {
        expected: Snapshot,
        actual: Snapshot,
    },
    #[error("Linux changed or disappeared before the return push")]
    ReturnRejected,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct Snapshot(String);

impl Snapshot {
    fn parse(value: &str) -> Result<Self> {
        if value.len() != 40
            || !value
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        {
            return Err(Error::Invalid("expected a SHA-1 fixture commit".into()));
        }
        Ok(Self(value.into()))
    }
}

impl fmt::Display for Snapshot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug)]
struct Scratch(PathBuf);

impl Scratch {
    fn create(parent: &Path) -> Result<Self> {
        let path = parent.join(format!(
            "dots-knowledge-probe-{:016x}",
            getrandom::u64().map_err(|e| Error::Invalid(e.to_string()))?
        ));
        fs::DirBuilder::new().mode(0o700).create(&path)?;
        Ok(Self(path))
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_dir_all(&self.0) {
            eprintln!("remove owned fixture {}: {error}", self.0.display());
        }
    }
}

fn path_text(path: &Path) -> Result<&str> {
    path.to_str()
        .ok_or_else(|| Error::Invalid("non-Unicode fixture path".into()))
}

#[derive(Clone, Debug)]
struct Repository {
    path: PathBuf,
    home: PathBuf,
}

impl Repository {
    fn init(root: &Path, name: &str) -> Result<Self> {
        let path = root.join(name);
        fs::DirBuilder::new().mode(0o700).create(&path)?;
        let repository = Self {
            path,
            home: root.join("home"),
        };
        repository.checked(&[
            "init",
            "--bare",
            "--quiet",
            "--initial-branch=knowledge",
            "--object-format=sha1",
            "--template=",
        ])?;
        // Receive-pack must retain these settings even without client -c options.
        for (key, value) in [
            ("core.fsync", "all"),
            ("core.fsyncMethod", "fsync"),
            ("gc.auto", "0"),
            ("maintenance.auto", "false"),
            ("receive.denyNonFastForwards", "true"),
            ("receive.denyDeletes", "true"),
        ] {
            repository.checked(&["config", key, value])?;
        }
        Ok(repository)
    }

    fn command(&self, args: &[&str], index: Option<&Path>) -> Command {
        let mut command = Command::new("git");
        command
            .env_clear()
            .env("PATH", env::var_os("PATH").expect("Git fixture needs PATH"))
            .env("HOME", &self.home)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_ALLOW_PROTOCOL", "file")
            .env("GIT_NO_REPLACE_OBJECTS", "1")
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_AUTHOR_NAME", "Knowledge Fixture")
            .env("GIT_AUTHOR_EMAIL", "fixture@example.invalid")
            .env("GIT_COMMITTER_NAME", "Knowledge Fixture")
            .env("GIT_COMMITTER_EMAIL", "fixture@example.invalid")
            .env("LC_ALL", "C")
            .current_dir(&self.path)
            .args([
                "-c",
                "core.hooksPath=/dev/null",
                "-c",
                "core.fsync=all",
                "-c",
                "core.fsyncMethod=fsync",
                "-c",
                "gc.auto=0",
                "-c",
                "maintenance.auto=false",
                "-c",
                "commit.gpgSign=false",
                "-c",
                "fetch.fsckObjects=true",
                "-c",
                "transfer.fsckObjects=true",
            ])
            .args(args);
        if let Some(index) = index {
            command.env("GIT_INDEX_FILE", index);
        }
        command
    }

    fn run(&self, args: &[&str], index: Option<&Path>) -> Result<Output> {
        Ok(dots_process::output(
            self.command(args, index),
            Duration::from_secs(10),
            MAX_BYTES,
        )?)
    }

    fn checked(&self, args: &[&str]) -> Result<String> {
        self.checked_index(args, None)
    }

    fn checked_index(&self, args: &[&str], index: Option<&Path>) -> Result<String> {
        let output = self.run(args, index)?;
        if !output.status.success() {
            return Err(Error::Git {
                operation: args.first().copied().unwrap_or("unknown").into(),
                stderr: String::from_utf8_lossy(&output.stderr).trim().into(),
            });
        }
        String::from_utf8(output.stdout)
            .map_err(|_| Error::Invalid("non-UTF-8 Git response".into()))
    }

    fn head(&self) -> Result<Snapshot> {
        Snapshot::parse(self.checked(&["rev-parse", "--verify", HEAD])?.trim())
    }

    fn tree_id(&self, snapshot: &Snapshot) -> Result<String> {
        Ok(self
            .checked(&["rev-parse", &format!("{}^{{tree}}", snapshot.0)])?
            .trim()
            .into())
    }

    fn is_ancestor(&self, old: &Snapshot, new: &Snapshot) -> Result<bool> {
        let output = self.run(&["merge-base", "--is-ancestor", &old.0, &new.0], None)?;
        match output.status.code() {
            Some(0) => Ok(true),
            Some(1) => Ok(false),
            _ => Err(Error::Git {
                operation: "merge-base".into(),
                stderr: String::from_utf8_lossy(&output.stderr).into(),
            }),
        }
    }

    fn compare_publish(&self, expected: &Snapshot, proposed: &Snapshot) -> Result<()> {
        let output = self.run(
            &["update-ref", "--no-deref", HEAD, &proposed.0, &expected.0],
            None,
        )?;
        if output.status.success() {
            return Ok(());
        }
        Err(Error::RefRejected {
            expected: expected.clone(),
            actual: self.head()?,
        })
    }

    // A private, disposable index is staging, never the published state.
    fn tree(&self, base: Option<&str>, edits: &[Edit<'_>]) -> Result<String> {
        let scratch = Scratch::create(&self.home)?;
        let index = scratch.0.join("index");
        self.checked_index(&["read-tree", "--empty"], Some(&index))?;
        if let Some(base) = base {
            let listing = self.checked(&["ls-tree", "-r", "-z", base])?;
            for record in listing.split('\0').filter(|record| !record.is_empty()) {
                let (metadata, path) = record
                    .split_once('\t')
                    .ok_or_else(|| Error::Invalid("malformed staging entry".into()))?;
                if edits.iter().any(|edit| edit.path == path) {
                    continue;
                }
                let parts: Vec<_> = metadata.split(' ').collect();
                if parts.len() != 3 {
                    return Err(Error::Invalid("malformed staging metadata".into()));
                }
                let entry = format!("{},{},{path}", parts[0], parts[2]);
                self.checked_index(
                    &["update-index", "--add", "--cacheinfo", &entry],
                    Some(&index),
                )?;
            }
        }
        for edit in edits {
            if let Some(content) = edit.content {
                let input = scratch.0.join("blob");
                fs::write(&input, content)?;
                let blob =
                    self.checked(&["hash-object", "--no-filters", "-w", path_text(&input)?])?;
                let entry = format!("100644,{},{}", blob.trim(), edit.path);
                self.checked_index(
                    &["update-index", "--add", "--cacheinfo", &entry],
                    Some(&index),
                )?;
            }
        }
        Ok(self
            .checked_index(&["write-tree"], Some(&index))?
            .trim()
            .into())
    }

    fn commit(&self, tree: &str, parents: &[&Snapshot], message: &str) -> Result<Snapshot> {
        let mut args = vec!["commit-tree", tree];
        for parent in parents {
            args.extend(["-p", &parent.0]);
        }
        args.extend(["-m", message]);
        Snapshot::parse(self.checked(&args)?.trim())
    }

    fn documents(&self, treeish: &str) -> Result<BTreeMap<String, String>> {
        let listing = self.checked(&["ls-tree", "-r", "-z", treeish])?;
        let mut result = BTreeMap::new();
        let mut folded = BTreeSet::new();
        let mut total = 0;
        for record in listing.split('\0').filter(|record| !record.is_empty()) {
            let (metadata, path) = record
                .split_once('\t')
                .ok_or_else(|| Error::Invalid("malformed tree entry".into()))?;
            let parts: Vec<_> = metadata.split(' ').collect();
            if parts.len() != 3 || parts[0] != "100644" || parts[1] != "blob" {
                return Err(Error::Invalid(format!(
                    "non-regular knowledge entry: {path}"
                )));
            }
            validate_path(path)?;
            if !folded.insert(path.to_ascii_lowercase()) || result.len() >= MAX_DOCUMENTS {
                return Err(Error::Invalid(
                    "case collision or too many documents".into(),
                ));
            }
            let size = self
                .checked(&["cat-file", "-s", parts[2]])?
                .trim()
                .parse::<usize>()
                .map_err(|_| Error::Invalid("invalid blob size".into()))?;
            total += size;
            if size > dots_documents::MAX_FILE_BYTES || total > MAX_BYTES {
                return Err(Error::Invalid("knowledge size limit exceeded".into()));
            }
            let content = self.checked(&["cat-file", "blob", parts[2]])?;
            if content.contains('\0') {
                return Err(Error::Invalid("NUL in knowledge document".into()));
            }
            result.insert(path.into(), content);
        }
        if result.get("manifest.json").map(String::as_str) != Some(MANIFEST) {
            return Err(Error::Invalid("wrong fixture store manifest".into()));
        }
        Ok(result)
    }
}

fn validate_path(path: &str) -> Result<()> {
    if path == "manifest.json" {
        return Ok(());
    }
    dots_documents::paths::validate_markdown(Path::new(path))
        .map_err(|e| Error::Invalid(e.to_string()))?;
    if !path.is_ascii()
        || path.split('/').count() > 8
        || path
            .split('/')
            .any(|part| part.ends_with(['.', ' ']) || part.contains(['<', '>', '"', '|', '?', '*']))
        || !(path.starts_with("global/") || path.starts_with("projects/demo/"))
    {
        return Err(Error::Invalid(format!(
            "path outside portable fixture scopes: {path}"
        )));
    }
    Ok(())
}

#[derive(Clone, Copy, Debug)]
struct Edit<'content> {
    path: &'content str,
    content: Option<&'content str>,
}

impl<'content> Edit<'content> {
    fn put(path: &'content str, content: &'content str) -> Self {
        Self {
            path,
            content: Some(content),
        }
    }
}

#[derive(Clone, Debug)]
struct Store {
    repository: Repository,
    genesis: Snapshot,
}

#[derive(Debug)]
struct Candidate {
    base: Snapshot,
    proposed: Snapshot,
}

impl Store {
    fn validate_history(&self, tip: &Snapshot) -> Result<()> {
        if !self.repository.is_ancestor(&self.genesis, tip)? {
            return Err(Error::Invalid("unknown knowledge lineage".into()));
        }
        let roots = self
            .repository
            .checked(&["rev-list", "--max-parents=0", &tip.0])?;
        if roots.trim() != self.genesis.0 {
            return Err(Error::Invalid("unrelated parent history".into()));
        }
        let history = self.repository.checked(&["rev-list", &tip.0])?;
        if history.lines().count() > MAX_COMMITS {
            return Err(Error::Invalid("fixture history limit exceeded".into()));
        }
        for commit in history.lines() {
            self.repository.documents(commit)?;
        }
        Ok(())
    }

    fn prepare(&self, base: &Snapshot, edits: &[Edit<'_>]) -> Result<Candidate> {
        let actual = self.repository.head()?;
        if actual != *base {
            return Err(Error::RefRejected {
                expected: base.clone(),
                actual,
            });
        }
        let mut paths = BTreeSet::new();
        for edit in edits {
            validate_path(edit.path)?;
            if !paths.insert(edit.path) {
                return Err(Error::Invalid("duplicate path in publication".into()));
            }
        }
        let tree = self.repository.tree(Some(&base.0), edits)?;
        self.repository.documents(&tree)?;
        let previous = self.repository.tree_id(base)?;
        let proposed = if previous == tree {
            base.clone()
        } else {
            self.repository
                .commit(&tree, &[base], "Publish knowledge")?
        };
        Ok(Candidate {
            base: base.clone(),
            proposed,
        })
    }

    fn publish(&self, candidate: &Candidate) -> Result<Snapshot> {
        self.validate_history(&candidate.proposed)?;
        if !self
            .repository
            .is_ancestor(&candidate.base, &candidate.proposed)?
        {
            return Err(Error::Invalid(
                "publication would rewrite accepted history".into(),
            ));
        }
        self.repository
            .compare_publish(&candidate.base, &candidate.proposed)?;
        Ok(candidate.proposed.clone())
    }

    fn write(&self, edits: &[Edit<'_>]) -> Result<Snapshot> {
        self.publish(&self.prepare(&self.repository.head()?, edits)?)
    }

    fn read(&self, snapshot: &Snapshot, path: &str) -> Result<Option<String>> {
        validate_path(path)?;
        self.validate_history(snapshot)?;
        Ok(self.repository.documents(&snapshot.0)?.remove(path))
    }

    fn fetch_candidate(&self, source: &Path) -> Result<Snapshot> {
        let destination = format!(
            "refs/dots/incoming/{:016x}",
            getrandom::u64().map_err(|e| Error::Invalid(e.to_string()))?
        );
        let mapping = format!("{HEAD}:{destination}");
        self.repository.checked(&[
            "fetch",
            "--quiet",
            "--no-tags",
            "--no-auto-maintenance",
            "--no-write-fetch-head",
            "--refmap=",
            path_text(source)?,
            &mapping,
        ])?;
        let tip = Snapshot::parse(
            self.repository
                .checked(&["rev-parse", "--verify", &destination])?
                .trim(),
        )?;
        self.validate_history(&tip)?;
        Ok(tip)
    }

    fn export_bundle(&self, path: &Path) -> Result<Snapshot> {
        let snapshot = self.repository.head()?;
        let export = format!("refs/dots/export/{}", snapshot.0);
        self.repository
            .checked(&["update-ref", &export, &snapshot.0])?;
        // Only this exact validated snapshot's history, never --all or repository files.
        self.validate_history(&snapshot)?;
        self.repository
            .checked(&["bundle", "create", path_text(path)?, &export])?;
        Ok(snapshot)
    }

    fn import_bundle(&self, path: &Path) -> Result<Snapshot> {
        self.repository
            .checked(&["bundle", "verify", path_text(path)?])?;
        let heads = self
            .repository
            .checked(&["bundle", "list-heads", path_text(path)?])?;
        let records: Vec<_> = heads.lines().collect();
        if records.len() != 1 {
            return Err(Error::Invalid(
                "bundle must advertise exactly one snapshot".into(),
            ));
        }
        let (oid, name) = records[0]
            .split_once(' ')
            .ok_or_else(|| Error::Invalid("invalid bundle head".into()))?;
        let tip = Snapshot::parse(oid)?;
        if name != format!("refs/dots/export/{oid}") {
            return Err(Error::Invalid("unexpected bundle reference".into()));
        }
        let mapping = format!("{name}:refs/dots/bundle/{oid}");
        self.repository.checked(&[
            "fetch",
            "--quiet",
            "--no-tags",
            "--no-auto-maintenance",
            "--no-write-fetch-head",
            "--refmap=",
            path_text(path)?,
            &mapping,
        ])?;
        self.validate_history(&tip)?;
        // Import is not publication. A stale bundle must not rewind accepted knowledge.
        Ok(tip)
    }
}

#[derive(Debug)]
struct Integration {
    local: Snapshot,
    linux: Snapshot,
    tree: String,
    conflicts: Vec<String>,
}

#[derive(Clone, Debug)]
struct Coordinator {
    mac: Store,
    linux_path: PathBuf,
}

impl Coordinator {
    fn plan(&self, requested: &Path) -> Result<Integration> {
        if fs::canonicalize(requested)? != self.linux_path {
            return Err(Error::Invalid(
                "destination does not match the bound fixture replica".into(),
            ));
        }
        let local = self.mac.repository.head()?;
        let linux = self.mac.fetch_candidate(requested)?;
        let previous = Snapshot::parse(
            self.mac
                .repository
                .checked(&["rev-parse", "--verify", OBSERVED])?
                .trim(),
        )?;
        if !self.mac.repository.is_ancestor(&previous, &linux)? {
            return Err(Error::Invalid(
                "Linux head rewound; explicit recovery required".into(),
            ));
        }
        self.mac
            .repository
            .checked(&["update-ref", OBSERVED, &linux.0, &previous.0])?;
        let (tree, conflicts) = if self.mac.repository.is_ancestor(&local, &linux)? {
            (self.mac.repository.tree_id(&linux)?, Vec::new())
        } else if self.mac.repository.is_ancestor(&linux, &local)? {
            (self.mac.repository.tree_id(&local)?, Vec::new())
        } else {
            let bases =
                self.mac
                    .repository
                    .checked(&["merge-base", "--all", &local.0, &linux.0])?;
            if bases.lines().count() != 1 {
                return Err(Error::Invalid(
                    "ambiguous merge base requires explicit recovery".into(),
                ));
            }
            let base = self.mac.repository.documents(bases.trim())?;
            let left = self.mac.repository.documents(&local.0)?;
            let right = self.mac.repository.documents(&linux.0)?;
            let mut conflicts = BTreeSet::new();
            let all_paths: BTreeSet<_> =
                base.keys().chain(left.keys()).chain(right.keys()).collect();
            for path in all_paths {
                if left.get(path) != base.get(path)
                    && right.get(path) != base.get(path)
                    && left.get(path) != right.get(path)
                {
                    conflicts.insert(path.clone());
                }
            }
            let merge = self.mac.repository.run(
                &[
                    "merge-tree",
                    "--write-tree",
                    "--name-only",
                    "-z",
                    &local.0,
                    &linux.0,
                ],
                None,
            )?;
            if !matches!(merge.status.code(), Some(0 | 1)) {
                return Err(Error::Invalid(
                    "Git could not construct a merge candidate".into(),
                ));
            }
            let output = String::from_utf8(merge.stdout)
                .map_err(|_| Error::Invalid("non-UTF-8 merge output".into()))?;
            let mut fields = output.split('\0');
            let tree = fields
                .next()
                .ok_or_else(|| Error::Invalid("missing merge tree".into()))?;
            Snapshot::parse(tree)?;
            if !merge.status.success() {
                for path in fields.take_while(|path| !path.is_empty()) {
                    conflicts.insert(path.into());
                }
                if conflicts.is_empty() {
                    return Err(Error::Invalid(
                        "structural conflict requires explicit recovery".into(),
                    ));
                }
            }
            (tree.into(), conflicts.into_iter().collect())
        };
        Ok(Integration {
            local,
            linux,
            tree,
            conflicts,
        })
    }

    fn accept(&self, plan: &Integration, resolution: &[Edit<'_>]) -> Result<Snapshot> {
        if self.mac.repository.head()? != plan.local {
            return Err(Error::RefRejected {
                expected: plan.local.clone(),
                actual: self.mac.repository.head()?,
            });
        }
        let supplied: BTreeSet<_> = resolution.iter().map(|edit| edit.path).collect();
        let required: BTreeSet<_> = plan.conflicts.iter().map(String::as_str).collect();
        if supplied != required || supplied.len() != resolution.len() {
            return Err(Error::Invalid(
                "resolution must cover exactly the conflicting documents".into(),
            ));
        }
        for edit in resolution {
            validate_path(edit.path)?;
        }
        let tree = if resolution.is_empty() {
            plan.tree.clone()
        } else {
            self.mac.repository.tree(Some(&plan.tree), resolution)?
        };
        self.mac.repository.documents(&tree)?;
        let proposed = if self.mac.repository.is_ancestor(&plan.local, &plan.linux)? {
            plan.linux.clone()
        } else if self.mac.repository.is_ancestor(&plan.linux, &plan.local)? {
            plan.local.clone()
        } else {
            self.mac.repository.commit(
                &tree,
                &[&plan.local, &plan.linux],
                "Integrate Linux knowledge",
            )?
        };
        self.mac.publish(&Candidate {
            base: plan.local.clone(),
            proposed,
        })
    }

    fn return_push(&self, plan: &Integration, accepted: &Snapshot) -> Result<()> {
        self.mac.validate_history(accepted)?;
        if !self
            .mac
            .repository
            .is_ancestor(accepted, &self.mac.repository.head()?)?
        {
            return Err(Error::Invalid(
                "return push is not an accepted Mac snapshot".into(),
            ));
        }
        if !self.mac.repository.is_ancestor(&plan.linux, accepted)? {
            return Err(Error::Invalid(
                "return push would discard Linux history".into(),
            ));
        }
        let lease = format!("--force-with-lease={HEAD}:{}", plan.linux.0);
        let mapping = format!("{}:{HEAD}", accepted.0);
        let result = self.mac.repository.run(
            &[
                "push",
                "--quiet",
                "--porcelain",
                &lease,
                path_text(&self.linux_path)?,
                &mapping,
            ],
            None,
        )?;
        if !result.status.success() {
            return Err(Error::ReturnRejected);
        }
        Ok(())
    }
}

#[derive(Debug)]
struct Lab {
    scratch: Scratch,
    mac: Store,
    linux: Store,
    coordinator: Coordinator,
}

impl Lab {
    fn create() -> Result<Self> {
        let scratch = Scratch::create(&env::temp_dir())?;
        fs::DirBuilder::new()
            .mode(0o700)
            .create(scratch.0.join("home"))?;
        fs::write(
            scratch.0.join("fixture-marker"),
            "neutral-git-knowledge-probe\n",
        )?;
        let mac_repository = Repository::init(&scratch.0, "mac.git")?;
        let tree = mac_repository.tree(
            None,
            &[
                Edit::put("manifest.json", MANIFEST),
                Edit::put("global/MEMORY.md", "# Memory\n"),
                Edit::put(
                    "projects/demo/wiki/topic.md",
                    "First\n2\n3\n4\n5\n6\n7\n8\n9\nLast\n",
                ),
            ],
        )?;
        let genesis = mac_repository.commit(&tree, &[], "Initialize neutral knowledge")?;
        mac_repository.checked(&["update-ref", HEAD, &genesis.0, ""])?;
        mac_repository.checked(&["update-ref", OBSERVED, &genesis.0, ""])?;
        fs::write(scratch.0.join("genesis"), &genesis.0)?;
        let mac = Store {
            repository: mac_repository,
            genesis: genesis.clone(),
        };
        let linux_repository = Repository::init(&scratch.0, "linux.git")?;
        let linux = Store {
            repository: linux_repository,
            genesis,
        };
        let initial = linux.fetch_candidate(&mac.repository.path)?;
        linux
            .repository
            .checked(&["update-ref", HEAD, &initial.0, ""])?;
        let coordinator = Coordinator {
            mac: mac.clone(),
            linux_path: fs::canonicalize(&linux.repository.path)?,
        };
        Ok(Self {
            scratch,
            mac,
            linux,
            coordinator,
        })
    }
}

fn main() -> Result<()> {
    if env::args_os().len() != 1 {
        return Err(Error::Invalid(
            "the disposable probe takes no arguments".into(),
        ));
    }
    let lab = Lab::create()?;
    let original = lab.mac.repository.head()?;
    let mac = lab.mac.write(&[Edit::put(
        "global/wiki/editor.md",
        "Use the shared editor configuration.\n",
    )])?;
    let linux = lab.linux.write(&[Edit::put(
        "projects/demo/wiki/build.md",
        "Build the affected package.\n",
    )])?;
    println!("Mac and Linux published independently; the original snapshot remains readable.");
    let plan = lab.coordinator.plan(&lab.linux.repository.path)?;
    let accepted = lab.coordinator.accept(&plan, &[])?;
    lab.coordinator.return_push(&plan, &accepted)?;
    if lab.linux.repository.head()? != accepted
        || !lab.mac.repository.is_ancestor(&mac, &accepted)?
        || !lab.mac.repository.is_ancestor(&linux, &accepted)?
    {
        return Err(Error::Invalid("round trip lost a publication".into()));
    }
    println!("Mac integrated both publications and returned the combined snapshot.");
    lab.mac.write(&[Edit::put(
        "projects/demo/wiki/topic.md",
        "Mac interpretation\n",
    )])?;
    lab.linux.write(&[Edit::put(
        "projects/demo/wiki/topic.md",
        "Linux interpretation\n",
    )])?;
    let conflict = lab.coordinator.plan(&lab.linux.repository.path)?;
    if conflict.conflicts != ["projects/demo/wiki/topic.md"] {
        return Err(Error::Invalid(
            "same-document conflict was not surfaced".into(),
        ));
    }
    println!("Concurrent interpretations are an explicit conflict; neither is overwritten.");
    let resolved = lab.coordinator.accept(
        &conflict,
        &[Edit::put(
            "projects/demo/wiki/topic.md",
            "Explicitly reconciled interpretation\n",
        )],
    )?;
    lab.coordinator.return_push(&conflict, &resolved)?;
    let bundle = lab.scratch.0.join("recovery.bundle");
    let exported = lab.mac.export_bundle(&bundle)?;
    let imported = lab.linux.import_bundle(&bundle)?;
    if exported != imported
        || lab.mac.read(&original, "global/MEMORY.md")?.as_deref() != Some("# Memory\n")
    {
        return Err(Error::Invalid(
            "bundle or snapshot verification failed".into(),
        ));
    }
    println!("Full bundle recovery verified; original snapshot still intact.");
    println!("PASS: disposable repositories removed on exit; no real memory or network used.");
    Ok(())
}

#[cfg(test)]
mod acceptance {
    use super::*;
    use std::sync::Barrier;
    use std::thread;

    #[test]
    fn atomic_batch_snapshot_noop_and_missing_versus_empty() -> Result<()> {
        let lab = Lab::create()?;
        let base = lab.mac.repository.head()?;
        let candidate = lab.mac.prepare(
            &base,
            &[
                Edit::put("global/MEMORY.md", "# Memory\nSee empty.md\n"),
                Edit::put("global/empty.md", ""),
            ],
        )?;
        assert_eq!(lab.mac.repository.head()?, base, "staging must not publish");
        let published = lab.mac.publish(&candidate)?;
        let rewind = Candidate {
            base: published.clone(),
            proposed: base.clone(),
        };
        assert!(matches!(lab.mac.publish(&rewind), Err(Error::Invalid(_))));
        assert_eq!(lab.mac.repository.head()?, published);
        assert_eq!(lab.mac.read(&base, "global/empty.md")?, None);
        assert_eq!(
            lab.mac.read(&published, "global/empty.md")?,
            Some(String::new())
        );
        assert_eq!(
            lab.mac.read(&base, "global/MEMORY.md")?.as_deref(),
            Some("# Memory\n")
        );
        assert_eq!(
            lab.mac.write(&[Edit::put("global/empty.md", "")])?,
            published
        );
        let deleted = lab.mac.write(&[Edit {
            path: "global/empty.md",
            content: None,
        }])?;
        assert_eq!(lab.mac.read(&deleted, "global/empty.md")?, None);
        assert!(!lab.mac.repository.path.join("index").exists());
        Ok(())
    }

    #[test]
    fn simultaneous_publishers_do_not_overwrite_each_other() -> Result<()> {
        let lab = Lab::create()?;
        let base = lab.mac.repository.head()?;
        let first = lab
            .mac
            .prepare(&base, &[Edit::put("global/one.md", "one")])?;
        let second = lab
            .mac
            .prepare(&base, &[Edit::put("global/two.md", "two")])?;
        let barrier = Barrier::new(2);
        let (left, right) = thread::scope(|scope| {
            let left = scope.spawn(|| {
                barrier.wait();
                lab.mac.publish(&first)
            });
            let right = scope.spawn(|| {
                barrier.wait();
                lab.mac.publish(&second)
            });
            (
                left.join().expect("publisher must not panic"),
                right.join().expect("publisher must not panic"),
            )
        });
        assert_ne!(
            left.is_ok(),
            right.is_ok(),
            "exactly one base-CAS may succeed"
        );
        let rejected = if left.is_err() { left } else { right };
        assert!(
            matches!(rejected, Err(Error::RefRejected { .. })),
            "{rejected:?}"
        );
        assert!(matches!(
            lab.mac.prepare(&base, &[]),
            Err(Error::RefRejected { .. })
        ));
        Ok(())
    }

    #[test]
    fn mac_integrates_disjoint_contributions_and_idle_sync_is_stable() -> Result<()> {
        let lab = Lab::create()?;
        let left = lab.mac.write(&[Edit::put("global/mac.md", "mac")])?;
        let right = lab
            .linux
            .write(&[Edit::put("projects/demo/linux.md", "linux")])?;
        let plan = lab.coordinator.plan(&lab.linux.repository.path)?;
        assert!(plan.conflicts.is_empty());
        let integrated = lab.coordinator.accept(&plan, &[])?;
        lab.coordinator.return_push(&plan, &integrated)?;
        assert_eq!(lab.linux.repository.head()?, integrated);
        assert!(lab.mac.repository.is_ancestor(&left, &integrated)?);
        assert!(lab.mac.repository.is_ancestor(&right, &integrated)?);
        let again = lab.coordinator.plan(&lab.linux.repository.path)?;
        assert_eq!(lab.coordinator.accept(&again, &[])?, integrated);
        lab.coordinator.return_push(&again, &integrated)?;
        assert_eq!(
            lab.mac.repository.head()?,
            integrated,
            "idle sync must not create merge ping-pong"
        );
        Ok(())
    }

    #[test]
    fn a_clean_text_merge_still_requires_document_reconciliation() -> Result<()> {
        let lab = Lab::create()?;
        let local = lab.mac.write(&[Edit::put(
            "projects/demo/wiki/topic.md",
            "Mac first\n2\n3\n4\n5\n6\n7\n8\n9\nLast\n",
        )])?;
        lab.linux.write(&[Edit::put(
            "projects/demo/wiki/topic.md",
            "First\n2\n3\n4\n5\n6\n7\n8\n9\nLinux last\n",
        )])?;
        let plan = lab.coordinator.plan(&lab.linux.repository.path)?;
        assert_eq!(plan.conflicts, ["projects/demo/wiki/topic.md"]);
        assert!(matches!(
            lab.coordinator.accept(&plan, &[]),
            Err(Error::Invalid(_))
        ));
        assert_eq!(lab.mac.repository.head()?, local);
        let resolved = lab.coordinator.accept(
            &plan,
            &[Edit::put(
                "projects/demo/wiki/topic.md",
                "Reviewed combination\n",
            )],
        )?;
        lab.coordinator.return_push(&plan, &resolved)?;
        assert_eq!(
            lab.linux
                .read(&resolved, "projects/demo/wiki/topic.md")?
                .as_deref(),
            Some("Reviewed combination\n")
        );
        Ok(())
    }

    #[test]
    fn local_publication_after_planning_rejects_stale_integration() -> Result<()> {
        let lab = Lab::create()?;
        lab.linux.write(&[Edit::put("global/linux.md", "linux")])?;
        let plan = lab.coordinator.plan(&lab.linux.repository.path)?;
        assert!(
            matches!(
                lab.coordinator.return_push(&plan, &plan.linux),
                Err(Error::Invalid(_))
            ),
            "a fetched candidate is not yet an accepted Mac snapshot"
        );
        let newer = lab.mac.write(&[Edit::put("global/mac.md", "new")])?;
        assert!(matches!(
            lab.coordinator.accept(&plan, &[]),
            Err(Error::RefRejected { .. })
        ));
        assert_eq!(lab.mac.repository.head()?, newer);
        Ok(())
    }

    #[test]
    fn linux_publication_after_fetch_rejects_return_push_without_losing_either() -> Result<()> {
        let lab = Lab::create()?;
        lab.mac.write(&[Edit::put("global/mac.md", "mac")])?;
        let plan = lab.coordinator.plan(&lab.linux.repository.path)?;
        let accepted = lab.coordinator.accept(&plan, &[])?;
        let newer = lab
            .linux
            .write(&[Edit::put("global/linux.md", "linux after fetch")])?;
        assert!(matches!(
            lab.coordinator.return_push(&plan, &accepted),
            Err(Error::ReturnRejected)
        ));
        assert_eq!(lab.linux.repository.head()?, newer);
        let retry = lab.coordinator.plan(&lab.linux.repository.path)?;
        let combined = lab.coordinator.accept(&retry, &[])?;
        lab.coordinator.return_push(&retry, &combined)?;
        assert!(lab.mac.repository.is_ancestor(&newer, &combined)?);
        assert!(lab.mac.repository.is_ancestor(&accepted, &combined)?);
        Ok(())
    }

    #[test]
    fn wrong_destination_deleted_and_rewound_replica_are_not_adopted() -> Result<()> {
        let lab = Lab::create()?;
        assert!(matches!(
            lab.coordinator.plan(&lab.mac.repository.path),
            Err(Error::Invalid(_))
        ));
        let linux = lab.linux.write(&[Edit::put("global/linux.md", "linux")])?;
        let plan = lab.coordinator.plan(&lab.linux.repository.path)?;
        let accepted = lab.coordinator.accept(&plan, &[])?;
        lab.linux
            .repository
            .checked(&["update-ref", "-d", HEAD, &linux.0])?;
        assert!(matches!(
            lab.coordinator.return_push(&plan, &accepted),
            Err(Error::ReturnRejected)
        ));
        assert!(lab.coordinator.plan(&lab.linux.repository.path).is_err());
        lab.linux
            .repository
            .checked(&["update-ref", HEAD, &lab.linux.genesis.0, ""])?;
        assert!(matches!(
            lab.coordinator.plan(&lab.linux.repository.path),
            Err(Error::Invalid(_))
        ));
        Ok(())
    }

    #[test]
    fn clean_tip_cannot_hide_invalid_ancestor_content() -> Result<()> {
        let lab = Lab::create()?;
        let base = lab.linux.repository.head()?;
        let bad_tree = lab.linux.repository.tree(
            Some(&base.0),
            &[Edit::put("unexpected.bin", "neutral invalid fixture")],
        )?;
        let bad = lab
            .linux
            .repository
            .commit(&bad_tree, &[&base], "Invalid ancestor fixture")?;
        let clean_tree = lab.linux.repository.tree(
            Some(&bad.0),
            &[Edit {
                path: "unexpected.bin",
                content: None,
            }],
        )?;
        let clean = lab
            .linux
            .repository
            .commit(&clean_tree, &[&bad], "Clean tip fixture")?;
        lab.linux.repository.compare_publish(&base, &clean)?;
        let mac_before = lab.mac.repository.head()?;
        assert!(matches!(
            lab.coordinator.plan(&lab.linux.repository.path),
            Err(Error::Invalid(_))
        ));
        assert_eq!(lab.mac.repository.head()?, mac_before);
        Ok(())
    }

    #[test]
    fn path_mode_size_and_scope_validation_precede_publication() -> Result<()> {
        let lab = Lab::create()?;
        let base = lab.mac.repository.head()?;
        for path in [
            "../escape.md",
            "global/.hidden.md",
            "other/topic.md",
            "global/trailing./topic.md",
        ] {
            assert!(
                lab.mac.prepare(&base, &[Edit::put(path, "bad")]).is_err(),
                "{path}"
            );
        }
        let oversized = "x".repeat(dots_documents::MAX_FILE_BYTES + 1);
        assert!(
            lab.mac
                .prepare(&base, &[Edit::put("global/large.md", &oversized)])
                .is_err()
        );
        assert!(
            lab.mac
                .prepare(
                    &base,
                    &[
                        Edit::put("global/Foo.md", "one"),
                        Edit::put("global/foo.md", "two")
                    ]
                )
                .is_err()
        );
        let tree = lab.mac.repository.tree(
            Some(&base.0),
            &[Edit::put("global/link.md", "not a symlink target")],
        )?;
        let stage = Scratch::create(&lab.scratch.0)?;
        let index = stage.0.join("index");
        lab.mac
            .repository
            .checked_index(&["read-tree", &tree], Some(&index))?;
        let blob = lab
            .mac
            .repository
            .checked(&["rev-parse", &format!("{tree}:global/link.md")])?;
        let mode = format!("120000,{},global/link.md", blob.trim());
        lab.mac
            .repository
            .checked_index(&["update-index", "--cacheinfo", &mode], Some(&index))?;
        let link_tree = lab
            .mac
            .repository
            .checked_index(&["write-tree"], Some(&index))?;
        assert!(lab.mac.repository.documents(link_tree.trim()).is_err());
        assert_eq!(lab.mac.repository.head()?, base);
        Ok(())
    }

    #[test]
    fn same_shape_with_unrelated_history_is_not_the_same_store() -> Result<()> {
        let lab = Lab::create()?;
        let base = lab.linux.repository.head()?;
        let tree = lab.linux.repository.tree_id(&base)?;
        let unrelated = lab
            .linux
            .repository
            .commit(&tree, &[], "Different genesis")?;
        lab.linux.repository.compare_publish(&base, &unrelated)?;
        let before = lab.mac.repository.head()?;
        assert!(matches!(
            lab.coordinator.plan(&lab.linux.repository.path),
            Err(Error::Invalid(_))
        ));
        assert_eq!(lab.mac.repository.head()?, before);
        Ok(())
    }

    #[test]
    fn identical_contributions_converge_without_a_semantic_conflict() -> Result<()> {
        let lab = Lab::create()?;
        let mac = lab
            .mac
            .write(&[Edit::put("global/same.md", "same contribution")])?;
        let linux = lab
            .linux
            .write(&[Edit::put("global/same.md", "same contribution")])?;
        let plan = lab.coordinator.plan(&lab.linux.repository.path)?;
        assert!(plan.conflicts.is_empty());
        let accepted = lab.coordinator.accept(&plan, &[])?;
        lab.coordinator.return_push(&plan, &accepted)?;
        assert!(lab.mac.repository.is_ancestor(&mac, &accepted)?);
        assert!(lab.mac.repository.is_ancestor(&linux, &accepted)?);
        Ok(())
    }

    #[test]
    fn truncated_bundle_and_network_transport_cannot_change_memory() -> Result<()> {
        let lab = Lab::create()?;
        let before = lab.mac.repository.head()?;
        let broken = lab.scratch.0.join("broken.bundle");
        fs::write(&broken, "# v2 git bundle\ntruncated\n")?;
        assert!(lab.mac.import_bundle(&broken).is_err());
        let external = lab
            .mac
            .repository
            .checked(&["ls-remote", "https://example.invalid/never-contacted.git"]);
        assert!(matches!(external, Err(Error::Git { .. })));
        assert_eq!(lab.mac.repository.head()?, before);
        Ok(())
    }

    #[test]
    fn full_bundle_replay_and_out_of_order_import_never_publish_implicitly() -> Result<()> {
        let lab = Lab::create()?;
        let old = lab.mac.repository.head()?;
        let older = lab.scratch.0.join("older.bundle");
        lab.mac.export_bundle(&older)?;
        let new = lab.mac.write(&[Edit::put("global/new.md", "new")])?;
        let latest = lab.scratch.0.join("latest.bundle");
        lab.mac.export_bundle(&latest)?;
        let recovery = Store {
            repository: Repository::init(&lab.scratch.0, "recovered.git")?,
            genesis: lab.mac.genesis.clone(),
        };
        assert_eq!(recovery.import_bundle(&latest)?, new);
        assert!(
            recovery.repository.head().is_err(),
            "recovery import is still only a candidate"
        );
        recovery
            .repository
            .checked(&["update-ref", HEAD, &new.0, ""])?;
        assert_eq!(
            recovery.read(&new, "global/new.md")?.as_deref(),
            Some("new")
        );
        assert_eq!(lab.linux.import_bundle(&latest)?, new);
        assert_eq!(lab.linux.import_bundle(&latest)?, new);
        assert_eq!(lab.linux.import_bundle(&older)?, old);
        assert_eq!(
            lab.linux.repository.head()?,
            old,
            "import must not publish, let alone rewind"
        );
        lab.linux.repository.compare_publish(&old, &new)?;
        assert_eq!(lab.linux.import_bundle(&older)?, old);
        assert_eq!(lab.linux.repository.head()?, new);
        Ok(())
    }

    #[test]
    fn process_boundary_fixture() -> Result<()> {
        let Some(root) = env::var_os("DOTS_KNOWLEDGE_FIXTURE") else {
            return Ok(());
        };
        let root = PathBuf::from(root);
        if !root.starts_with(env::temp_dir())
            || fs::read_to_string(root.join("fixture-marker"))? != "neutral-git-knowledge-probe\n"
        {
            return Err(Error::Invalid("not an owned neutral fixture".into()));
        }
        let genesis = Snapshot::parse(&fs::read_to_string(root.join("genesis"))?)?;
        let mac = Store {
            repository: Repository {
                path: root.join("mac.git"),
                home: root.join("home"),
            },
            genesis,
        };
        let phase = env::var("DOTS_KNOWLEDGE_PHASE").map_err(|e| Error::Invalid(e.to_string()))?;
        if phase == "push" {
            let coordinator = Coordinator {
                mac,
                linux_path: fs::canonicalize(root.join("linux.git"))?,
            };
            let plan = coordinator.plan(&coordinator.linux_path)?;
            let accepted = coordinator.accept(&plan, &[])?;
            coordinator.return_push(&plan, &accepted)?;
        } else {
            let candidate = mac.prepare(
                &mac.repository.head()?,
                &[Edit::put("global/interrupted.md", "complete publication")],
            )?;
            if phase == "after" {
                mac.publish(&candidate)?;
            } else if phase != "before" {
                return Err(Error::Invalid("unknown fixture phase".into()));
            }
        }
        // Terminate the real fixture process without destructors or a publication receipt.
        std::process::exit(73);
    }

    #[test]
    fn process_exit_before_and_after_publication_and_push_is_recoverable() -> Result<()> {
        for phase in ["before", "after", "push"] {
            let lab = Lab::create()?;
            let base = lab.mac.repository.head()?;
            if phase == "push" {
                lab.mac
                    .write(&[Edit::put("global/outbound.md", "deliver me")])?;
            }
            let expected = lab.mac.repository.head()?;
            let mut command = Command::new(env::current_exe()?);
            command
                .args([
                    "--exact",
                    "acceptance::process_boundary_fixture",
                    "--nocapture",
                ])
                .env("DOTS_KNOWLEDGE_FIXTURE", &lab.scratch.0)
                .env("DOTS_KNOWLEDGE_PHASE", phase)
                .env("GIT_DIR", lab.scratch.0.join("must-not-be-used"))
                .env("GIT_CONFIG_COUNT", "1")
                .env(
                    "GIT_CONFIG_KEY_0",
                    "url.https://must-not-be-used.invalid/.insteadOf",
                )
                .env("GIT_CONFIG_VALUE_0", "/");
            let output = dots_process::output(command, Duration::from_secs(20), 64 * 1024)?;
            assert_eq!(
                output.status.code(),
                Some(73),
                "{phase}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            let observed = lab.mac.repository.head()?;
            if phase == "before" {
                assert_eq!(observed, base);
                assert_eq!(lab.mac.read(&observed, "global/interrupted.md")?, None);
                lab.mac
                    .write(&[Edit::put("global/after-restart.md", "still writable")])?;
            } else if phase == "after" {
                assert_ne!(observed, base);
                assert_eq!(
                    lab.mac.read(&observed, "global/interrupted.md")?.as_deref(),
                    Some("complete publication")
                );
            } else {
                assert_eq!(lab.linux.repository.head()?, expected);
                let retry = lab.coordinator.plan(&lab.linux.repository.path)?;
                assert_eq!(lab.coordinator.accept(&retry, &[])?, expected);
                lab.coordinator.return_push(&retry, &expected)?;
                assert_eq!(
                    lab.mac.repository.head()?,
                    expected,
                    "retry after lost receipt must not create another publication"
                );
            }
        }
        Ok(())
    }
}
