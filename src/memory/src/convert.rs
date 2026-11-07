//! One-time conversion of a native Git knowledge store into substrate snapshots.
//!
//! The source is the private bare SHA-256 repository layout: `repo.git` with
//! history on `refs/heads/knowledge`, each commit's tree holding
//! `knowledge.json` (store and project registry) beside `global/` and
//! `projects/<id>/` documents, plus `store.json` and `bindings.json`. Only
//! read-only Git plumbing runs against the source.
use std::collections::BTreeMap;
use std::env;
use std::path::Path;
use std::process::Command;
use std::time::{Duration, SystemTime};

use dots_substrate::Blob;
use serde::{Deserialize, Serialize};

use crate::manifest::{Entry, Manifest};
use crate::{Error, MAX_FILE_BYTES, ProjectId, Result, Snapshot, Store, StoreId, invalid};

const HEAD: &str = "refs/heads/knowledge";
const MANIFEST: &str = "knowledge.json";
const DEADLINE: Duration = Duration::from_secs(60);
const OUTPUT_LIMIT: usize = 256 * 1024 * 1024;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
#[expect(dead_code, reason = "decoded to validate the source layout")]
struct Local {
    node: String,
    store: StoreId,
    genesis: String,
    role: String,
    actor: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Registry {
    store: StoreId,
    projects: BTreeMap<ProjectId, String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Bindings {
    store: StoreId,
    projects: BTreeMap<String, ProjectId>,
}

/// One source commit and the snapshot it became.
#[derive(Clone, Debug, Serialize)]
pub struct Converted {
    pub commit: String,
    pub snapshot: Snapshot,
}

#[derive(Debug, Serialize)]
pub struct Conversion {
    pub store: StoreId,
    pub head: Snapshot,
    /// Every source commit in topological order, oldest first.
    pub snapshots: Vec<Converted>,
    /// Document versions verified byte-identical in the published snapshots.
    pub verified_documents: usize,
    /// The source's local project bindings, for the caller to install.
    pub bindings: BTreeMap<String, ProjectId>,
}

struct Source<'a> {
    root: &'a Path,
}

impl Source<'_> {
    fn git(&self, args: &[&str], input: Option<Vec<u8>>) -> Result<Vec<u8>> {
        let path = env::var_os("PATH").ok_or_else(|| invalid("PATH is unavailable"))?;
        let mut command = Command::new("git");
        command
            .env_clear()
            .env("PATH", path)
            .env("HOME", self.root)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_NO_REPLACE_OBJECTS", "1")
            .env("GIT_NO_LAZY_FETCH", "1")
            .env("GIT_OPTIONAL_LOCKS", "0")
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("LC_ALL", "C")
            .current_dir(self.root)
            .args(["--no-pager", "--no-replace-objects", "--git-dir"])
            .arg(self.root.join("repo.git"))
            .args(["-c", "core.hooksPath=/dev/null", "-c", "gc.auto=0"])
            .args(["-c", "maintenance.auto=false", "-c", "core.fsmonitor=false"])
            .args(args);
        let mut runner = dots_process::Runner::new(DEADLINE, OUTPUT_LIMIT);
        if let Some(input) = input {
            runner = runner.stdin(input, OUTPUT_LIMIT);
        }
        let output = runner.output(command).map_err(|source| Error::Process {
            operation: "read source knowledge repository",
            source,
        })?;
        if !output.status.success() {
            return Err(Error::Git {
                operation: "read source knowledge repository",
                status: output.status.code(),
                stderr: String::from_utf8_lossy(&output.stderr).trim().into(),
            });
        }
        Ok(output.stdout)
    }

    fn text(&self, args: &[&str]) -> Result<String> {
        String::from_utf8(self.git(args, None)?).map_err(|_| invalid("non-UTF-8 Git metadata"))
    }

    /// Path to (object id, size) for every blob of a commit's tree.
    fn tree(&self, commit: &str) -> Result<BTreeMap<String, (String, usize)>> {
        let listing = self.text(&["ls-tree", "-r", "-l", "-z", commit])?;
        let mut entries = BTreeMap::new();
        for record in listing.split('\0').filter(|record| !record.is_empty()) {
            let malformed = || invalid(format!("unexpected source tree entry {record:?}"));
            let (metadata, path) = record.split_once('\t').ok_or_else(malformed)?;
            let [mode, kind, oid, size] = metadata.split_whitespace().collect::<Vec<_>>()[..]
            else {
                return Err(malformed());
            };
            if mode != "100644" || kind != "blob" {
                return Err(malformed());
            }
            let size = size.parse().map_err(|_| malformed())?;
            entries.insert(path.to_owned(), (object_id(oid)?.to_owned(), size));
        }
        Ok(entries)
    }

    /// The content of each object id.
    fn blobs(&self, ids: &[&String]) -> Result<BTreeMap<String, Vec<u8>>> {
        let mut input = Vec::new();
        for id in ids {
            input.extend_from_slice(id.as_bytes());
            input.push(b'\n');
        }
        let bytes = self.git(&["cat-file", "--batch"], Some(input))?;
        let mut offset = 0;
        let mut result = BTreeMap::new();
        for id in ids {
            let malformed = || invalid("unexpected source object batch output");
            let end = bytes[offset..]
                .iter()
                .position(|&byte| byte == b'\n')
                .ok_or_else(malformed)?
                + offset;
            let header = std::str::from_utf8(&bytes[offset..end]).map_err(|_| malformed())?;
            let [oid, "blob", size] = header.split(' ').collect::<Vec<_>>()[..] else {
                return Err(malformed());
            };
            if oid != id.as_str() {
                return Err(malformed());
            }
            let size: usize = size.parse().map_err(|_| malformed())?;
            let start = end + 1;
            let content = bytes.get(start..start + size).ok_or_else(malformed)?;
            if bytes.get(start + size) != Some(&b'\n') {
                return Err(malformed());
            }
            result.insert((*id).clone(), content.to_vec());
            offset = start + size + 1;
        }
        if offset != bytes.len() {
            return Err(invalid("unexpected trailing source object output"));
        }
        Ok(result)
    }
}

fn object_id(value: &str) -> Result<&str> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
    {
        return Err(invalid(format!("{value:?} is not a SHA-256 object id")));
    }
    Ok(value)
}

fn json<T: for<'de> Deserialize<'de>>(path: &Path, operation: &'static str) -> Result<Option<T>> {
    dots_documents::text::read_bytes(path, MAX_FILE_BYTES)?
        .map(|bytes| {
            serde_json::from_slice(&bytes).map_err(|source| Error::Json { operation, source })
        })
        .transpose()
}

/// Convert the store at `source` and publish it as the first `memory` value
/// of `store`'s substrate, refusing if knowledge is already published.
#[expect(
    clippy::too_many_lines,
    reason = "one ordered pass from source history to published snapshots"
)]
pub fn convert(source: &Path, store: &mut Store, now: SystemTime) -> Result<Conversion> {
    let local: Local = json(&source.join("store.json"), "source store binding")?
        .ok_or_else(|| invalid("source store.json is absent"))?;
    let bindings = match json::<Bindings>(&source.join("bindings.json"), "source bindings")? {
        Some(bindings) if bindings.store != local.store => {
            return Err(invalid("source bindings belong to another store"));
        }
        Some(bindings) => bindings.projects,
        None => BTreeMap::new(),
    };
    let git = Source { root: source };
    let history = git.text(&["rev-list", "--topo-order", "--reverse", "--parents", HEAD])?;
    let commits: Vec<Vec<&str>> = history
        .lines()
        .map(|line| line.split(' ').collect())
        .collect();
    let mut trees = Vec::new();
    for fields in &commits {
        let commit = object_id(fields[0])?;
        if fields.len() > 3 {
            return Err(invalid(format!("{commit} has more than two parents")));
        }
        if (fields.len() == 1) != (commit == local.genesis) {
            return Err(invalid("source history has an unexpected root"));
        }
        trees.push(git.tree(commit)?);
    }
    let mut ids: Vec<&String> = trees
        .iter()
        .flat_map(|tree| tree.values().map(|(oid, _)| oid))
        .collect();
    ids.sort();
    ids.dedup();
    let contents = git.blobs(&ids)?;

    let mut snapshots: BTreeMap<&str, Snapshot> = BTreeMap::new();
    let mut converted = Vec::new();
    let mut blobs: BTreeMap<_, Blob> = BTreeMap::new();
    let mut manifests = Vec::new();
    for (fields, tree) in commits.iter().zip(&trees) {
        let content = |oid: &String| {
            contents
                .get(oid)
                .ok_or_else(|| invalid(format!("source object {oid} is missing")))
        };
        let (oid, _) = tree
            .get(MANIFEST)
            .ok_or_else(|| invalid(format!("{} lacks {MANIFEST}", fields[0])))?;
        let registry: Registry =
            serde_json::from_slice(content(oid)?).map_err(|source| Error::Json {
                operation: "source knowledge manifest",
                source,
            })?;
        if registry.store != local.store {
            return Err(invalid("source history mixes stores"));
        }
        let mut entries = BTreeMap::new();
        for (path, (oid, size)) in tree.iter().filter(|(path, _)| *path != MANIFEST) {
            let blob = Blob::new(content(oid)?.clone())?;
            if blob.content().len() != *size {
                return Err(invalid(format!("source object {oid} has the wrong size")));
            }
            entries.insert(
                path.clone(),
                Entry {
                    hash: blob.hash(),
                    size: *size,
                },
            );
            blobs.insert(blob.hash(), blob);
        }
        let parents = fields[1..]
            .iter()
            .map(|parent| {
                snapshots
                    .get(parent)
                    .copied()
                    .ok_or_else(|| invalid("source history is not topologically ordered"))
            })
            .collect::<Result<_>>()?;
        let manifest = Manifest {
            parents,
            store: local.store.clone(),
            projects: registry.projects,
            entries,
        };
        let blob = manifest.encode()?;
        let snapshot = Snapshot::new(blob.hash());
        snapshots.insert(fields[0], snapshot);
        converted.push(Converted {
            commit: fields[0].to_owned(),
            snapshot,
        });
        manifests.push(blob);
    }
    let root = manifests
        .pop()
        .ok_or_else(|| invalid("source history is empty"))?;
    let head = git.text(&["rev-parse", "--verify", &format!("{HEAD}^{{commit}}")])?;
    if converted.last().map(|c| c.commit.as_str()) != Some(head.trim()) {
        return Err(invalid("source history does not end at its published head"));
    }
    let mut carried: Vec<Blob> = blobs.into_values().collect();
    carried.extend(manifests);
    let published = store.create(root, carried, now)?;
    let verified = verify(store, &converted, &trees, &contents)?;
    Ok(Conversion {
        store: local.store,
        head: published,
        snapshots: converted,
        verified_documents: verified,
        bindings,
    })
}

/// Check every converted document against its source bytes; returns the count.
fn verify(
    store: &Store,
    converted: &[Converted],
    trees: &[BTreeMap<String, (String, usize)>],
    contents: &BTreeMap<String, Vec<u8>>,
) -> Result<usize> {
    let mut verified = 0;
    for (item, tree) in converted.iter().zip(trees) {
        let manifest = store.manifest(&item.snapshot)?;
        for (path, (oid, _)) in tree.iter().filter(|(path, _)| *path != MANIFEST) {
            let entry = manifest
                .entries
                .get(path)
                .ok_or_else(|| invalid(format!("converted {path} is missing")))?;
            if store.substrate().blob(&entry.hash)?.content() != contents[oid].as_slice() {
                return Err(invalid(format!("converted {path} differs from its source")));
            }
            verified += 1;
        }
    }
    Ok(verified)
}
