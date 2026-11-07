//! Document-level three-way integration of a candidate with the published head.
use std::collections::{BTreeMap, BTreeSet};

use dots_substrate::{Blob, Hash};
use serde::{Deserialize, Serialize};

use crate::manifest::{self, Entry, Manifest, Resource};
use crate::{ProjectId, Result, Snapshot, Store, StoreId, invalid};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum IntegrationKind {
    /// The published history already contains the candidate.
    UpToDate,
    /// The candidate descends from the published head.
    FastForward,
    /// Both moved on from a common snapshot.
    Merge,
}

/// A document changed differently on both sides, or a layout collision.
#[derive(Debug, Serialize)]
pub struct Conflict {
    pub resource: Resource,
    pub base_blob: Option<Hash>,
    pub local_blob: Option<Hash>,
    pub incoming_blob: Option<Hash>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Resolution {
    pub resource: Resource,
    pub choice: Choice,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Choice {
    Local {},
    Incoming {},
    Write { content: String },
    Remove {},
}

/// How a local candidate integrates with a published snapshot.
#[derive(Debug, Serialize)]
pub struct Integration {
    pub local: Snapshot,
    pub incoming: Snapshot,
    pub base: Snapshot,
    pub kind: IntegrationKind,
    pub conflicts: Vec<Conflict>,
    #[serde(skip)]
    store: StoreId,
    #[serde(skip)]
    projects: BTreeMap<ProjectId, String>,
    /// Documents chosen without judgment; conflicting ones keep the local side.
    #[serde(skip)]
    entries: BTreeMap<String, Entry>,
}

impl Integration {
    fn manifest(&self) -> Manifest {
        Manifest {
            parents: vec![self.incoming, self.local],
            store: self.store.clone(),
            projects: self.projects.clone(),
            entries: self.entries.clone(),
        }
    }
}

fn hash(entry: Option<&Entry>) -> Option<Hash> {
    entry.map(|entry| entry.hash)
}

impl Store {
    /// Compare `local` with `incoming` from their unique common snapshot.
    pub fn plan(&self, local: &Snapshot, incoming: &Snapshot) -> Result<Integration> {
        let ours = self.manifest(local)?;
        let theirs = self.manifest(incoming)?;
        if ours.store != theirs.store {
            return Err(invalid("candidate belongs to another knowledge store"));
        }
        let (kind, base) = self.integration_base(local, incoming)?;
        let ancestor = self.manifest(&base)?;
        let mut projects = ours.projects.clone();
        for (id, name) in &theirs.projects {
            match projects.get(id) {
                Some(existing) if existing != name => {
                    return Err(invalid(
                        "shared project identity has conflicting definitions",
                    ));
                }
                Some(_) => {}
                None => {
                    projects.insert(id.clone(), name.clone());
                }
            }
        }
        let registry = Manifest {
            parents: Vec::new(),
            store: ours.store.clone(),
            projects: projects.clone(),
            entries: BTreeMap::new(),
        };
        let paths: BTreeSet<_> = ancestor
            .entries
            .keys()
            .chain(ours.entries.keys())
            .chain(theirs.entries.keys())
            .collect();
        let mut entries = BTreeMap::new();
        let mut conflicts = Vec::new();
        for path in paths {
            let before = ancestor.entries.get(path);
            let mine = ours.entries.get(path);
            let other = theirs.entries.get(path);
            let selected = if hash(mine) == hash(other) || hash(other) == hash(before) {
                mine
            } else if hash(mine) == hash(before) {
                other
            } else {
                conflicts.push(Conflict {
                    resource: Resource::from_path(path, &registry)?,
                    base_blob: hash(before),
                    local_blob: hash(mine),
                    incoming_blob: hash(other),
                });
                mine
            };
            if let Some(selected) = selected {
                entries.insert(path.clone(), *selected);
            }
        }
        let possible: BTreeSet<_> = entries
            .keys()
            .cloned()
            .chain(
                conflicts
                    .iter()
                    .map(|conflict| conflict.resource.full_path(&registry))
                    .collect::<Result<Vec<_>>>()?,
            )
            .collect();
        for path in manifest::colliding_paths(possible.iter().map(String::as_str)) {
            let resource = Resource::from_path(path, &registry)?;
            if conflicts
                .iter()
                .any(|conflict| conflict.resource == resource)
            {
                continue;
            }
            conflicts.push(Conflict {
                resource,
                base_blob: hash(ancestor.entries.get(path)),
                local_blob: hash(ours.entries.get(path)),
                incoming_blob: hash(theirs.entries.get(path)),
            });
        }
        Ok(Integration {
            local: *local,
            incoming: *incoming,
            base,
            kind,
            conflicts,
            store: ours.store.clone(),
            projects,
            entries,
        })
    }

    /// Build the two-parent snapshot of a merge plan. Every conflicting
    /// document needs an explicit choice, even if its text could merge.
    /// Returns the manifest and the content blobs written by choices.
    pub(crate) fn accept(
        &self,
        plan: &Integration,
        resolutions: &[Resolution],
    ) -> Result<(Manifest, Vec<Blob>)> {
        if plan.kind != IntegrationKind::Merge {
            return Err(invalid("only diverged histories need an integration"));
        }
        let ours = self.manifest(&plan.local)?;
        let theirs = self.manifest(&plan.incoming)?;
        let mut manifest = plan.manifest();
        let expected: BTreeSet<_> = plan
            .conflicts
            .iter()
            .map(|conflict| conflict.resource.full_path(&manifest))
            .collect::<Result<_>>()?;
        let mut selected = BTreeSet::new();
        let mut blobs = Vec::new();
        for resolution in resolutions {
            let path = resolution.resource.full_path(&manifest)?;
            if !expected.contains(&path) || !selected.insert(path.clone()) {
                return Err(invalid(
                    "resolution must name each conflicting document exactly once",
                ));
            }
            let value = match &resolution.choice {
                Choice::Local {} => ours.entries.get(&path).copied(),
                Choice::Incoming {} => theirs.entries.get(&path).copied(),
                Choice::Write { content } => {
                    manifest::validate_content(content.as_bytes())?;
                    let blob = Blob::new(content.as_bytes().to_vec())?;
                    let entry = Entry {
                        hash: blob.hash(),
                        size: content.len(),
                    };
                    blobs.push(blob);
                    Some(entry)
                }
                Choice::Remove {} => None,
            };
            match value {
                Some(value) => manifest.entries.insert(path, value),
                None => manifest.entries.remove(&path),
            };
        }
        if selected != expected {
            return Err(invalid("integration has unresolved documents"));
        }
        manifest.validate()?;
        Ok((manifest, blobs))
    }

    /// Select the topology before reconciling document contents.
    fn integration_base(
        &self,
        local: &Snapshot,
        incoming: &Snapshot,
    ) -> Result<(IntegrationKind, Snapshot)> {
        let theirs = self.ancestors(incoming)?;
        if theirs.contains(local) {
            return Ok((IntegrationKind::UpToDate, *local));
        }
        let ours = self.ancestors(local)?;
        if ours.contains(incoming) {
            return Ok((IntegrationKind::FastForward, *incoming));
        }
        let common: BTreeSet<_> = ours.intersection(&theirs).copied().collect();
        // Every proper ancestor of a common snapshot is common, so the bases
        // are the common snapshots no other common snapshot descends from.
        let mut covered = BTreeSet::new();
        let mut stack: Vec<_> = common
            .iter()
            .map(|snapshot| self.parents(snapshot))
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .flatten()
            .collect();
        while let Some(snapshot) = stack.pop() {
            if covered.insert(snapshot) {
                stack.extend(self.parents(&snapshot)?);
            }
        }
        let bases: Vec<_> = common.difference(&covered).collect();
        let [base] = bases.as_slice() else {
            return Err(invalid(
                "integration requires one unambiguous common snapshot",
            ));
        };
        Ok((IntegrationKind::Merge, **base))
    }
}
