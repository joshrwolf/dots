//! A Git remote backend, such as a private GitHub repository.
//!
//! Refs live under `refs/dots/` on one SHA-1 remote. Each ref names a commit
//! whose tree holds `ROOT` (the root hash as text) and `blobs/<hh>/<hex>` for
//! every blob the ref has carried; each update's commit has the previous one
//! as parent. Commits use fixed identities and dates, so re-sending a proposal
//! produces the same commit and the remote reports it up to date.
//!
//! The cache repository keeps a local ref for every ref value it fetched or
//! pushed, so its history stays reachable through garbage collection, and
//! [`Backend::maintain`] packs it once loose objects pile up.
//!
//! The installed `git` runs through dots-process with no user or system
//! configuration. Credentials come only from configuration the caller supplies
//! with [`GitBackend::config`] and [`GitBackend::env`].
use std::collections::BTreeMap;
use std::env;
use std::ffi::OsString;
use std::fmt::Write as _;
use std::fs;
use std::os::unix::fs::DirBuilderExt as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::OnceLock;
use std::time::Duration;

use dots_documents::staged_directory::StagedDirectory;

use crate::{
    Backend, Error, Fetched, Hash, MAX_BLOB_BYTES, Maintenance, Outcome, RefName, Result, Update,
    Version, invalid, io,
};

const NAMESPACE: &str = "refs/dots/";
const LOCAL_DEADLINE: Duration = Duration::from_secs(30);
const NETWORK_DEADLINE: Duration = Duration::from_secs(60);
/// Listing transfers only ref names, so an unresponsive remote is reported
/// unavailable well before interactive callers, such as Pi's 10 s context
/// client, give up and lose even the stale value.
const LIST_DEADLINE: Duration = Duration::from_secs(5);
/// Packing a cache with tens of thousands of loose objects takes seconds.
const MAINTENANCE_DEADLINE: Duration = Duration::from_secs(600);
/// Each publication leaves about eight loose objects; past a few thousand,
/// reading trees and packing pushes slow measurably.
const LOOSE_OBJECT_LIMIT: u64 = 3000;
const OUTPUT_LIMIT: usize = 256 * 1024 * 1024;
const ISOLATION: &[&str] = &[
    "core.hooksPath=/dev/null",
    "core.fsmonitor=false",
    "gc.auto=0",
    "maintenance.auto=false",
    "commit.gpgSign=false",
    "fetch.fsckObjects=true",
    "transfer.fsckObjects=true",
    "protocol.version=2",
];

/// A Git remote with a private local bare cache repository.
#[derive(Clone, Debug)]
pub struct GitBackend {
    url: String,
    root: PathBuf,
    config: Vec<(String, String)>,
    env: Vec<(OsString, OsString)>,
    deadline: Duration,
    loose_object_limit: u64,
    /// Set once this process has initialized the cache repository.
    initialized: OnceLock<()>,
}

impl GitBackend {
    /// Use `url` with a cache repository under `root`, created before the
    /// first Git command so that opening costs no process.
    pub fn open(root: &Path, url: impl Into<String>) -> Result<Self> {
        let url = url.into();
        if url.is_empty() || url.starts_with('-') || url.contains(char::is_whitespace) {
            return Err(invalid("remote URL", format!("{url:?} is not a URL")));
        }
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(root)
            .map_err(|source| io("create Git cache directory", root, source))?;
        Ok(Self {
            url,
            root: root.into(),
            config: Vec::new(),
            env: Vec::new(),
            deadline: NETWORK_DEADLINE,
            loose_object_limit: LOOSE_OBJECT_LIMIT,
            initialized: OnceLock::new(),
        })
    }

    /// Create or reinitialize the cache repository once per process.
    fn initialize(&self) -> Result<()> {
        if self.initialized.get().is_some() {
            return Ok(());
        }
        let command = self.command(&[
            "init",
            "--bare",
            "--quiet",
            "--object-format=sha1",
            "--template=",
        ])?;
        let output = Self::execute("initialize cache repository", command, None, None)?;
        checked(output, "initialize cache repository")?;
        let _ = self.initialized.set(());
        Ok(())
    }

    /// Pass `key=value` to every Git command, for example a credential helper.
    #[must_use]
    pub fn config(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.config.push((key.into(), value.into()));
        self
    }

    /// Set an environment variable for every Git command and its helpers.
    #[must_use]
    pub fn env(mut self, key: impl Into<OsString>, value: impl Into<OsString>) -> Self {
        self.env.push((key.into(), value.into()));
        self
    }

    /// Bound each network operation; a timeout makes its outcome unknown.
    /// Listing is bounded by the lesser of this and 5 s.
    #[must_use]
    pub fn deadline(mut self, deadline: Duration) -> Self {
        self.deadline = deadline;
        self
    }

    /// Pack the cache repository once it holds at least `limit` loose objects.
    #[must_use]
    pub fn loose_object_limit(mut self, limit: u64) -> Self {
        self.loose_object_limit = limit;
        self
    }

    fn repository(&self) -> PathBuf {
        self.root.join("cache.git")
    }

    fn command(&self, args: &[&str]) -> Result<Command> {
        let path = env::var_os("PATH").ok_or_else(|| invalid("environment", "PATH is unset"))?;
        let mut command = Command::new("git");
        command
            .env_clear()
            .env("PATH", path)
            .env("HOME", &self.root)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_NO_REPLACE_OBJECTS", "1")
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_ALLOW_PROTOCOL", "file:https:ssh")
            .env("GIT_AUTHOR_NAME", "dots")
            .env("GIT_AUTHOR_EMAIL", "dots@localhost")
            .env("GIT_AUTHOR_DATE", "@0 +0000")
            .env("GIT_COMMITTER_NAME", "dots")
            .env("GIT_COMMITTER_EMAIL", "dots@localhost")
            .env("GIT_COMMITTER_DATE", "@0 +0000")
            .env("TZ", "UTC")
            .env("LC_ALL", "C");
        for (key, value) in &self.env {
            command.env(key, value);
        }
        command
            .current_dir(&self.root)
            .args(["--no-pager", "--no-replace-objects", "--git-dir"])
            .arg(self.repository());
        for setting in ISOLATION {
            command.args(["-c", setting]);
        }
        for (key, value) in &self.config {
            command.arg("-c").arg(format!("{key}={value}"));
        }
        command.args(args);
        Ok(command)
    }

    /// Run git. A network timeout is [`Error::Unavailable`].
    fn run(
        &self,
        operation: &'static str,
        args: &[&str],
        input: Option<Vec<u8>>,
        network: bool,
    ) -> Result<Output> {
        self.run_with(operation, self.command(args)?, input, network)
    }

    fn run_with(
        &self,
        operation: &'static str,
        command: Command,
        input: Option<Vec<u8>>,
        network: bool,
    ) -> Result<Output> {
        self.initialize()?;
        Self::execute(operation, command, input, network.then_some(self.deadline))
    }

    /// Run a prepared command; `network` bounds a network operation.
    fn execute(
        operation: &'static str,
        command: Command,
        input: Option<Vec<u8>>,
        network: Option<Duration>,
    ) -> Result<Output> {
        let mut runner = dots_process::Runner::new(network.unwrap_or(LOCAL_DEADLINE), OUTPUT_LIMIT);
        if let Some(input) = input {
            runner = runner.stdin(input, OUTPUT_LIMIT);
        }
        match runner.output(command) {
            Ok(output) => Ok(output),
            Err(dots_process::Error::Timeout(limit)) if network.is_some() => Err(
                Error::Unavailable(format!("git {operation} exceeded {limit:?}")),
            ),
            Err(source) => Err(Error::Process { operation, source }),
        }
    }

    fn text(
        &self,
        operation: &'static str,
        args: &[&str],
        input: Option<Vec<u8>>,
    ) -> Result<String> {
        String::from_utf8(checked(
            self.run(operation, args, input, false)?,
            operation,
        )?)
        .map_err(|_| Error::Backend(format!("git {operation} wrote non-UTF-8 output")))
    }

    /// Fetch one remote ref into the cache repository; `None` if absent.
    fn fetch_tip(&self, name: &RefName) -> Result<Option<String>> {
        let reference = format!("{NAMESPACE}{name}");
        let output = self.run(
            "fetch ref",
            &[
                "fetch",
                "--quiet",
                "--no-tags",
                "--no-write-fetch-head",
                "--no-recurse-submodules",
                "--refmap=",
                &self.url,
                &format!("+{reference}:{reference}"),
            ],
            None,
            true,
        )?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            if stderr.contains("couldn't find remote ref") {
                return Ok(None);
            }
            return Err(Error::Unavailable(format!("git fetch: {}", stderr.trim())));
        }
        let tip = self.text(
            "read fetched ref",
            &["rev-parse", "--verify", &format!("{reference}^{{commit}}")],
            None,
        )?;
        Ok(Some(object_id(tip.trim())?.to_owned()))
    }

    fn has_commit(&self, oid: &str) -> Result<bool> {
        let output = self.run(
            "inspect cache",
            &["cat-file", "-e", &format!("{oid}^{{commit}}")],
            None,
            false,
        )?;
        Ok(output.status.success())
    }

    /// The blob entries of a ref commit's tree, and its root hash.
    fn carried(&self, commit: &str) -> Result<(Hash, BTreeMap<Hash, Entry>)> {
        let listing = self.text(
            "read ref tree",
            &["ls-tree", "-r", "-l", "-z", commit],
            None,
        )?;
        let mut root = None;
        let mut blobs = BTreeMap::new();
        for record in listing.split('\0').filter(|record| !record.is_empty()) {
            let malformed = || Error::Backend(format!("unexpected ref tree entry {record:?}"));
            let (metadata, path) = record.split_once('\t').ok_or_else(malformed)?;
            let [mode, kind, oid, size] = metadata.split_whitespace().collect::<Vec<_>>()[..]
            else {
                return Err(malformed());
            };
            if mode != "100644" || kind != "blob" {
                return Err(malformed());
            }
            let entry = Entry {
                oid: object_id(oid)?.to_owned(),
                size: size.parse().map_err(|_| malformed())?,
            };
            if path == "ROOT" {
                root = Some(entry);
                continue;
            }
            let hash = path
                .strip_prefix("blobs/")
                .and_then(|rest| rest.split_once('/'))
                .filter(|(fan, hex)| hex.get(..2) == Some(*fan))
                .ok_or_else(malformed)
                .and_then(|(_, hex)| Hash::from_hex(hex).map_err(|_| malformed()))?;
            blobs.insert(hash, entry);
        }
        let root = root.ok_or_else(|| Error::Backend(format!("ref commit {commit} lacks ROOT")))?;
        let text = self.objects(std::slice::from_ref(&root))?.remove(&root.oid);
        let root: Hash = String::from_utf8(text.unwrap_or_default())
            .ok()
            .and_then(|text| text.strip_suffix('\n')?.parse().ok())
            .ok_or_else(|| Error::Backend(format!("ref commit {commit} has a malformed ROOT")))?;
        if !blobs.contains_key(&root) {
            return Err(Error::Backend(format!(
                "ref commit {commit} does not carry its root {root}"
            )));
        }
        Ok((root, blobs))
    }

    /// Read blob objects from the cache repository.
    fn objects(&self, entries: &[Entry]) -> Result<BTreeMap<String, Vec<u8>>> {
        let mut input = Vec::new();
        for entry in entries {
            if entry.size > MAX_BLOB_BYTES {
                return Err(Error::TooLarge { size: entry.size });
            }
            input.extend_from_slice(entry.oid.as_bytes());
            input.push(b'\n');
        }
        let output = checked(
            self.run("read blobs", &["cat-file", "--batch"], Some(input), false)?,
            "read blobs",
        )?;
        parse_batch(&output, entries)
    }

    /// Build the commit for one update, or the conflict found while doing so.
    fn propose(&self, update: &Update) -> Result<std::result::Result<String, Outcome>> {
        let parent = match &update.expected {
            None => None,
            Some(expected) => {
                let oid = object_id(expected.as_str())?;
                if !self.has_commit(oid)? {
                    let tip = self.fetch_tip(&update.name)?;
                    if tip.as_deref() != Some(oid) {
                        return Ok(Err(Outcome::Conflict {
                            actual: tip.map(Version::new).transpose()?,
                        }));
                    }
                }
                Some(oid)
            }
        };
        let mut paths = match parent {
            Some(parent) => self
                .carried(parent)?
                .1
                .into_iter()
                .map(|(hash, entry)| (blob_path(&hash), entry.oid))
                .collect(),
            None => BTreeMap::new(),
        };
        let stage = StagedDirectory::create(&self.root)?;
        let mut contents: Vec<(String, &[u8])> = update
            .new_blobs
            .iter()
            .map(|blob| (blob_path(&blob.hash()), blob.content()))
            .collect();
        let root_text = format!("{}\n", update.root);
        contents.push(("ROOT".into(), root_text.as_bytes()));
        let mut list = String::new();
        for (index, (_, content)) in contents.iter().enumerate() {
            let file = stage.path().join(index.to_string());
            fs::write(&file, content).map_err(|source| io("stage blob", &file, source))?;
            writeln!(list, "{}", file.display())
                .map_err(|_| Error::Backend("format blob list".into()))?;
        }
        let oids = self.text(
            "write blobs",
            &["hash-object", "-w", "--no-filters", "--stdin-paths"],
            Some(list.into_bytes()),
        )?;
        let oids: Vec<_> = oids.lines().collect();
        if oids.len() != contents.len() {
            return Err(Error::Backend(
                "git hash-object returned a short list".into(),
            ));
        }
        for ((path, _), oid) in contents.into_iter().zip(oids) {
            paths.insert(path, object_id(oid)?.to_owned());
        }
        if !paths.contains_key(&blob_path(&update.root)) {
            return Err(invalid(
                "update",
                format!("root {} of {} is not carried", update.root, update.name),
            ));
        }
        let mut index_input = Vec::new();
        for (path, oid) in &paths {
            index_input.extend_from_slice(format!("100644 {oid}\t{path}\0").as_bytes());
        }
        let index = stage.path().join("index");
        let mut command = self.command(&["update-index", "-z", "--index-info"])?;
        command.env("GIT_INDEX_FILE", &index);
        checked(
            self.run_with("stage tree", command, Some(index_input), false)?,
            "stage tree",
        )?;
        let mut command = self.command(&["write-tree"])?;
        command.env("GIT_INDEX_FILE", &index);
        let tree = checked(
            self.run_with("write tree", command, None, false)?,
            "write tree",
        )?;
        let tree =
            String::from_utf8(tree).map_err(|_| Error::Backend("non-UTF-8 tree id".into()))?;
        let message = format!("dots {}", update.name);
        let mut args = vec!["commit-tree", object_id(tree.trim())?, "-m", &message];
        if let Some(parent) = parent {
            args.extend(["-p", parent]);
        }
        let commit = self.text("write commit", &args, None)?;
        Ok(Ok(object_id(commit.trim())?.to_owned()))
    }

    /// Push proposals in one non-atomic push and read each ref's result.
    fn push(&self, proposals: &[(&Update, String)]) -> Result<Vec<Outcome>> {
        let mut args: Vec<String> = vec!["push".into(), "--porcelain".into()];
        let mut specs = Vec::new();
        for (update, commit) in proposals {
            let reference = format!("{NAMESPACE}{}", update.name);
            let expected = update.expected.as_ref().map_or("", Version::as_str);
            args.push(format!("--force-with-lease={reference}:{expected}"));
            specs.push(format!("{commit}:{reference}"));
        }
        args.push(self.url.clone());
        args.extend(specs);
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        let output = match self.run("push refs", &args, None, true) {
            Ok(output) => output,
            Err(Error::Unavailable(_)) => return Ok(vec![Outcome::Unknown; proposals.len()]),
            Err(error) => return Err(error),
        };
        if !matches!(output.status.code(), Some(0 | 1)) {
            return Ok(vec![Outcome::Unknown; proposals.len()]);
        }
        let report = parse_porcelain(&String::from_utf8_lossy(&output.stdout));
        let mut listed = None;
        let outcomes = proposals
            .iter()
            .map(|(update, commit)| {
                let reference = format!("{NAMESPACE}{}", update.name);
                let Some(line) = report.get(&reference) else {
                    return Ok(Outcome::Unknown);
                };
                match line.flag {
                    ' ' | '+' | '*' | '=' => Ok(Outcome::Applied(Version::new(commit.as_str())?)),
                    '!' if is_lease_failure(&line.summary) => {
                        if let Some(actual) = reported_actual(&line.summary) {
                            return Ok(Outcome::Conflict {
                                actual: Some(Version::new(actual)?),
                            });
                        }
                        let listed = match &mut listed {
                            Some(listed) => listed,
                            None => listed.insert(self.list()),
                        };
                        Ok(match listed {
                            Ok(listed) => Outcome::Conflict {
                                actual: listed.get(&update.name).cloned(),
                            },
                            Err(_) => Outcome::Unknown,
                        })
                    }
                    _ => Err(Error::Backend(format!(
                        "push of {} rejected: {}",
                        update.name, line.summary
                    ))),
                }
            })
            .collect::<Result<Vec<_>>>()?;
        let mut landed = String::new();
        for ((update, commit), outcome) in proposals.iter().zip(&outcomes) {
            if matches!(outcome, Outcome::Applied(_)) {
                let _ = writeln!(landed, "update {NAMESPACE}{} {commit}", update.name);
            }
        }
        if !landed.is_empty() {
            // Keep what landed reachable in the cache, as a fetch would.
            self.text(
                "record pushed refs",
                &["update-ref", "--stdin"],
                Some(landed.into_bytes()),
            )?;
        }
        Ok(outcomes)
    }
}

impl Backend for GitBackend {
    fn list(&self) -> Result<BTreeMap<RefName, Version>> {
        let pattern = format!("{NAMESPACE}*");
        self.initialize()?;
        let output = Self::execute(
            "list refs",
            self.command(&["ls-remote", &self.url, &pattern])?,
            None,
            Some(self.deadline.min(LIST_DEADLINE)),
        )?;
        if !output.status.success() {
            return Err(Error::Unavailable(format!(
                "git ls-remote: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            )));
        }
        let text = String::from_utf8(output.stdout)
            .map_err(|_| Error::Backend("git ls-remote wrote non-UTF-8 output".into()))?;
        let mut refs = BTreeMap::new();
        for line in text.lines() {
            let (oid, reference) = line
                .split_once('\t')
                .ok_or_else(|| Error::Backend(format!("unexpected ls-remote line {line:?}")))?;
            // Patterns match ref tails, so filter on the full prefix here.
            let Some(name) = reference.strip_prefix(NAMESPACE) else {
                continue;
            };
            refs.insert(name.parse()?, Version::new(object_id(oid)?)?);
        }
        Ok(refs)
    }

    fn fetch(&self, name: &RefName, cached: &dyn Fn(&Hash) -> bool) -> Result<Option<Fetched>> {
        let Some(tip) = self.fetch_tip(name)? else {
            return Ok(None);
        };
        let (root, carried) = self.carried(&tip)?;
        let wanted: Vec<(Hash, Entry)> = carried
            .into_iter()
            .filter(|(hash, _)| !cached(hash))
            .collect();
        let entries: Vec<Entry> = wanted.iter().map(|(_, entry)| entry.clone()).collect();
        let mut contents = self.objects(&entries)?;
        let blobs = wanted
            .into_iter()
            .map(|(hash, entry)| {
                contents
                    .remove(&entry.oid)
                    .map(|content| (hash, content))
                    .ok_or_else(|| Error::Backend(format!("cache lacks blob {}", entry.oid)))
            })
            .collect::<Result<_>>()?;
        Ok(Some(Fetched {
            version: Version::new(tip)?,
            root,
            blobs,
        }))
    }

    fn update(&self, updates: &[Update]) -> Result<Vec<Outcome>> {
        let mut outcomes = Vec::with_capacity(updates.len());
        let mut proposals = Vec::new();
        for update in updates {
            match self.propose(update)? {
                Ok(commit) => {
                    outcomes.push(None);
                    proposals.push((update, commit));
                }
                Err(outcome) => outcomes.push(Some(outcome)),
            }
        }
        let mut pushed = if proposals.is_empty() {
            Vec::new()
        } else {
            self.push(&proposals)?
        }
        .into_iter();
        outcomes
            .into_iter()
            .map(|outcome| {
                outcome
                    .or_else(|| pushed.next())
                    .ok_or_else(|| Error::Backend("push returned too few outcomes".into()))
            })
            .collect()
    }

    fn maintain(&self) -> Result<Maintenance> {
        if !self.repository().exists() {
            return Ok(Maintenance::Skipped);
        }
        let counts = self.text("count cache objects", &["count-objects", "-v"], None)?;
        let loose = counts
            .lines()
            .find_map(|line| line.strip_prefix("count: "))
            .and_then(|count| count.parse::<u64>().ok())
            .ok_or_else(|| Error::Backend(format!("unexpected count-objects output {counts:?}")))?;
        if loose < self.loose_object_limit {
            return Ok(Maintenance::Skipped);
        }
        // Every ref value is reachable from a local ref, so the history
        // survives; unreachable rejected proposals expire on Git's schedule.
        let output = dots_process::Runner::new(MAINTENANCE_DEADLINE, OUTPUT_LIMIT)
            .output(self.command(&["gc", "--quiet"])?)
            .map_err(|source| Error::Process {
                operation: "pack cache repository",
                source,
            })?;
        checked(output, "pack cache repository")?;
        Ok(Maintenance::Ran(format!("packed {loose} loose objects")))
    }
}

#[derive(Clone, Debug)]
struct Entry {
    oid: String,
    size: usize,
}

fn blob_path(hash: &Hash) -> String {
    let hex = hash.hex();
    format!("blobs/{}/{hex}", &hex[..2])
}

/// Accept a full lowercase SHA-1 object id.
fn object_id(oid: &str) -> Result<&str> {
    if oid.len() == 40 && oid.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')) {
        Ok(oid)
    } else {
        Err(Error::Backend(format!("{oid:?} is not a SHA-1 object id")))
    }
}

fn checked(output: Output, operation: &'static str) -> Result<Vec<u8>> {
    if !output.status.success() {
        return Err(Error::Git {
            operation,
            status: output.status.code(),
            stderr: String::from_utf8_lossy(&output.stderr).trim().into(),
        });
    }
    Ok(output.stdout)
}

fn parse_batch(bytes: &[u8], entries: &[Entry]) -> Result<BTreeMap<String, Vec<u8>>> {
    let malformed = || Error::Backend("malformed git cat-file output".into());
    let mut offset = 0;
    let mut result = BTreeMap::new();
    for entry in entries {
        let header_end = bytes
            .get(offset..)
            .and_then(|rest| rest.iter().position(|&byte| byte == b'\n'))
            .map(|length| offset + length)
            .ok_or_else(malformed)?;
        let header = std::str::from_utf8(&bytes[offset..header_end]).map_err(|_| malformed())?;
        let expected = format!("{} blob {}", entry.oid, entry.size);
        if header != expected {
            return Err(Error::Backend(format!(
                "git cat-file returned {header:?}, expected {expected:?}"
            )));
        }
        let start = header_end + 1;
        let end = start + entry.size;
        let content = bytes.get(start..end).ok_or_else(malformed)?;
        if bytes.get(end) != Some(&b'\n') {
            return Err(malformed());
        }
        result.insert(entry.oid.clone(), content.to_vec());
        offset = end + 1;
    }
    if offset != bytes.len() {
        return Err(malformed());
    }
    Ok(result)
}

#[derive(Debug, PartialEq, Eq)]
struct PorcelainLine {
    flag: char,
    summary: String,
}

/// Per-ref results of `git push --porcelain`: `<flag>\t<from>:<to>\t<summary>`.
fn parse_porcelain(stdout: &str) -> BTreeMap<String, PorcelainLine> {
    stdout
        .lines()
        .filter_map(|line| {
            let mut fields = line.splitn(3, '\t');
            let mut flag = fields.next()?.chars();
            let (Some(flag), None) = (flag.next(), flag.next()) else {
                return None;
            };
            let (_, to) = fields.next()?.split_once(':')?;
            Some((
                to.to_owned(),
                PorcelainLine {
                    flag,
                    summary: fields.next().unwrap_or_default().to_owned(),
                },
            ))
        })
        .collect()
}

/// Rejections that mean the ref did not hold the expected value.
fn is_lease_failure(summary: &str) -> bool {
    summary.contains("(stale info)")
        // Git 2.50+ names an old-value mismatch found under the ref lock.
        || summary.contains("(incorrect old value provided)")
        || summary.contains("reference already exists")
        || (summary.contains("cannot lock ref") && summary.contains("but expected"))
}

/// The actual oid in a server-side lease failure: `... is at <oid> but expected <oid>`.
fn reported_actual(summary: &str) -> Option<&str> {
    let (_, rest) = summary.split_once(" is at ")?;
    object_id(rest.get(..40)?).ok()
}

#[cfg(test)]
mod tests;
