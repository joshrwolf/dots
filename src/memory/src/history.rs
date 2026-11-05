//! Integrity and schema validation of reachable knowledge history from trusted producers.
use std::collections::{BTreeMap, BTreeSet};

use crate::git::Git;
use crate::store::{Identity, View};
use crate::{MAX_FILE_BYTES, MAX_METADATA_BYTES, Result, Snapshot};

const MAX_HISTORY_METADATA: usize = 32 * 1024 * 1024;
const MAX_HISTORY_BYTES: usize = 256 * 1024 * 1024;
const MAX_OBJECTS: usize = 250_000;
const MAX_COMMIT_BYTES: usize = 4096;

/// Accepted local ancestry is trusted by induction through the publication boundary.
pub(crate) fn accepted(git: &Git, tip: &Snapshot) -> Result<BTreeSet<Snapshot>> {
    metadata(git, &["rev-list", tip.as_str()])?
        .lines()
        .map(str::parse::<Snapshot>)
        .collect()
}

/// Validate before moving candidate objects into the owned knowledge repository.
/// `known` is ancestry already accepted by this store, never arbitrary supplied refs.
pub(crate) fn validate(
    git: &Git,
    identity: &Identity,
    tip: &Snapshot,
    known: &BTreeSet<Snapshot>,
) -> Result<BTreeSet<Snapshot>> {
    let reachable = metadata(
        git,
        &["rev-list", "--objects", "--no-object-names", tip.as_str()],
    )?;
    let reachable: BTreeSet<_> = reachable
        .lines()
        .map(str::parse::<Snapshot>)
        .collect::<Result<_>>()?;
    inventory(git, &reachable)?;
    git.text(
        &[
            "fsck",
            "--strict",
            "--full",
            "--no-reflogs",
            "--no-dangling",
            tip.as_str(),
        ],
        None,
    )?;
    let roots = git.text(&["rev-list", "--max-parents=0", tip.as_str()], None)?;
    if roots.trim() != identity.genesis.as_str() {
        return Err(crate::invalid("candidate has unrelated knowledge lineage"));
    }
    let commits = metadata(
        git,
        &[
            "log",
            "--format=%H %T %P",
            "--no-show-signature",
            "--reverse",
            "--topo-order",
            tip.as_str(),
        ],
    )?;
    let mut visited = BTreeSet::new();
    let mut trees = BTreeSet::new();
    let mut blobs = BTreeSet::new();
    let mut registries = BTreeMap::new();
    for record in commits.lines() {
        let fields: Vec<_> = record.split_whitespace().collect();
        if !(2..=4).contains(&fields.len()) {
            return Err(crate::invalid("knowledge commits need at most two parents"));
        }
        let commit: Snapshot = fields[0].parse()?;
        let tree: Snapshot = fields[1].parse()?;
        let parents: Vec<_> = fields[2..]
            .iter()
            .map(|id| id.parse::<Snapshot>())
            .collect::<Result<_>>()?;
        if (parents.is_empty() && commit != identity.genesis)
            || !parents.iter().all(|parent| visited.contains(parent))
        {
            return Err(crate::invalid(
                "knowledge history has an unexpected root or parent order",
            ));
        }
        if !visited.insert(commit.clone()) {
            return Err(crate::invalid("duplicate knowledge commit"));
        }
        if known.contains(&commit) {
            continue;
        }
        let view = View::load(git, &commit, &identity.store)?;
        for parent in &parents {
            if !registries.contains_key(parent) {
                registries.insert(
                    parent.clone(),
                    View::load(git, parent, &identity.store)?.manifest().clone(),
                );
            }
            let previous = &registries[parent];
            if previous
                .projects
                .iter()
                .any(|(id, name)| view.manifest().projects.get(id) != Some(name))
            {
                return Err(crate::invalid("shared project definitions are append-only"));
            }
        }
        registries.insert(commit, view.manifest().clone());
        if !trees.insert(tree) {
            continue;
        }
        let selected: Vec<_> = view
            .entries
            .values()
            .filter(|entry| blobs.insert(entry.oid.clone()))
            .collect();
        let ids: Vec<_> = selected.iter().map(|entry| entry.oid.as_str()).collect();
        let maximum = selected
            .iter()
            .map(|entry| entry.size + 128)
            .sum::<usize>()
            .max(1);
        for bytes in git.blobs(&ids, maximum)?.values() {
            crate::store::validate_content(bytes)?;
        }
    }
    Ok(visited)
}

fn metadata(git: &Git, args: &[&str]) -> Result<String> {
    String::from_utf8(git.bytes(args, None, MAX_HISTORY_METADATA)?)
        .map_err(|_| crate::invalid("non-UTF-8 Git history metadata"))
}

fn inventory(git: &Git, reachable: &BTreeSet<Snapshot>) -> Result<()> {
    if reachable.len() > MAX_OBJECTS {
        return Err(crate::invalid(
            "knowledge history contains too many objects",
        ));
    }
    let selected: Vec<_> = reachable.iter().collect();
    let mut total = 0_usize;
    for chunk in selected.chunks(4096) {
        let mut input = Vec::new();
        for oid in chunk {
            input.extend_from_slice(oid.as_str().as_bytes());
            input.push(b'\n');
        }
        let listing = git.text(
            &[
                "cat-file",
                "--batch-check=%(objectname) %(objecttype) %(objectsize)",
            ],
            Some(input),
        )?;
        let records: Vec<_> = listing.lines().collect();
        if records.len() != chunk.len() {
            return Err(crate::invalid("incomplete Git object inventory"));
        }
        for (record, expected) in records.into_iter().zip(chunk) {
            let fields: Vec<_> = record.split_whitespace().collect();
            if fields.len() != 3 || fields[0] != expected.as_str() {
                return Err(crate::invalid("invalid Git object inventory"));
            }
            let size: usize = fields[2]
                .parse()
                .map_err(|_| crate::invalid("invalid Git history object size"))?;
            let maximum = match fields[1] {
                "commit" => MAX_COMMIT_BYTES,
                "tree" => MAX_METADATA_BYTES,
                "blob" => MAX_FILE_BYTES,
                _ => {
                    return Err(crate::invalid(
                        "knowledge permits only commit, tree and blob objects",
                    ));
                }
            };
            total = total
                .checked_add(size)
                .ok_or_else(|| crate::invalid("knowledge history size overflow"))?;
            if size > maximum || total > MAX_HISTORY_BYTES {
                return Err(crate::invalid("knowledge history exceeds storage bounds"));
            }
        }
    }
    Ok(())
}
