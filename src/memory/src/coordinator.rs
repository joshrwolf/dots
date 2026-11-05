//! Document-level reconciliation and fenced acceptance of validated candidates.
use std::collections::{BTreeMap, BTreeSet};

use dots_documents::mutation_lock::MutationLock;
use serde::{Deserialize, Serialize};

use crate::git::Entry;
use crate::store;
use crate::{Candidate, Identity, Manifest, Resource, Result, Role, Snapshot, Store};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum IntegrationKind {
    UpToDate,
    FastForward,
    Merge,
}

#[derive(Debug, Serialize)]
pub struct Conflict {
    pub resource: Resource,
    pub base_blob: Option<String>,
    pub local_blob: Option<String>,
    pub incoming_blob: Option<String>,
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

#[derive(Debug, Serialize)]
pub struct Integration {
    recipient: Identity,
    local: Snapshot,
    incoming: Candidate,
    base: Snapshot,
    kind: IntegrationKind,
    conflicts: Vec<Conflict>,
    #[serde(skip)]
    entries: BTreeMap<String, Entry>,
    #[serde(skip)]
    manifest: Manifest,
}

impl Integration {
    pub fn recipient(&self) -> &Identity {
        &self.recipient
    }
    pub fn local(&self) -> &Snapshot {
        &self.local
    }
    pub fn incoming(&self) -> &Candidate {
        &self.incoming
    }
    pub fn base(&self) -> &Snapshot {
        &self.base
    }
    pub fn kind(&self) -> IntegrationKind {
        self.kind
    }
    pub fn conflicts(&self) -> &[Conflict] {
        &self.conflicts
    }
}

#[derive(Debug)]
pub struct Coordinator<'store> {
    store: &'store Store,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Delivery {
    pub recipient: Identity,
    pub expected: Snapshot,
    pub candidate: Candidate,
}

impl Delivery {
    pub(crate) fn validate_recipient(&self, store: &Store) -> Result<()> {
        let identity = store.identity();
        if identity.role != Role::Replica || self.recipient != identity {
            return Err(crate::invalid(
                "delivery targets another node or a non-replica",
            ));
        }
        if self.candidate.source.role != Role::Authority
            || self.candidate.source.node == identity.node
            || self.candidate.source.store != identity.store
            || self.candidate.source.genesis != identity.genesis
        {
            return Err(crate::invalid(
                "delivery must come from the shared store's authority",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeliveryReceipt {
    pub recipient: Identity,
    pub snapshot: Snapshot,
    pub observed: Snapshot,
    pub already_present: bool,
}

impl<'store> Coordinator<'store> {
    pub fn new(store: &'store Store) -> Result<Self> {
        if store.role() != Role::Authority {
            return Err(crate::invalid(
                "only the knowledge authority integrates divergent history",
            ));
        }
        Ok(Self { store })
    }

    pub fn plan(&self, expected: &Snapshot, candidate: &Candidate) -> Result<Integration> {
        self.store.check_expected(expected)?;
        let incoming = candidate.view(self.store)?;
        let local = self.store.view(expected)?;
        let git = self.store.git();
        let (kind, base) = if git.is_ancestor(&candidate.snapshot, expected)? {
            (IntegrationKind::UpToDate, candidate.snapshot.clone())
        } else if git.is_ancestor(expected, &candidate.snapshot)? {
            (IntegrationKind::FastForward, expected.clone())
        } else {
            let bases = git.text(
                &[
                    "merge-base",
                    "--all",
                    expected.as_str(),
                    candidate.snapshot.as_str(),
                ],
                None,
            )?;
            let bases: Vec<_> = bases
                .lines()
                .map(str::parse::<Snapshot>)
                .collect::<Result<_>>()?;
            let [base] = bases.as_slice() else {
                return Err(crate::invalid(
                    "integration requires one unambiguous common snapshot",
                ));
            };
            (IntegrationKind::Merge, base.clone())
        };
        let ancestor = crate::View::load(git, &base, self.store.id())?;
        let mut manifest = local.manifest().clone();
        for (id, name) in &incoming.manifest().projects {
            match manifest.projects.get(id) {
                Some(existing) if existing != name => {
                    return Err(crate::invalid(
                        "shared project identity has conflicting definitions",
                    ));
                }
                Some(_) => {}
                None => {
                    manifest.projects.insert(id.clone(), name.clone());
                }
            }
        }
        let paths: BTreeSet<_> = ancestor
            .entries
            .keys()
            .chain(local.entries.keys())
            .chain(incoming.entries.keys())
            .filter(|path| path.as_str() != store::MANIFEST)
            .collect();
        let mut entries = BTreeMap::new();
        entries.insert(
            store::MANIFEST.into(),
            local.entries[store::MANIFEST].clone(),
        );
        let mut conflicts = Vec::new();
        for path in paths {
            let before = ancestor.entries.get(path);
            let ours = local.entries.get(path);
            let theirs = incoming.entries.get(path);
            let selected = if oid(ours) == oid(theirs) || oid(theirs) == oid(before) {
                ours
            } else if oid(ours) == oid(before) {
                theirs
            } else {
                conflicts.push(Conflict {
                    resource: Resource::from_path(path, &manifest)?,
                    base_blob: oid(before).map(str::to_owned),
                    local_blob: oid(ours).map(str::to_owned),
                    incoming_blob: oid(theirs).map(str::to_owned),
                });
                ours
            };
            if let Some(selected) = selected {
                entries.insert(path.clone(), selected.clone());
            }
        }
        Ok(Integration {
            recipient: self.store.identity(),
            local: expected.clone(),
            incoming: candidate.clone(),
            base,
            kind,
            conflicts,
            entries,
            manifest,
        })
    }

    /// Every conflicting document needs an explicit choice, even if Git could merge its text.
    pub fn accept(&self, plan: &Integration, resolutions: &[Resolution]) -> Result<Snapshot> {
        if plan.recipient != self.store.identity() {
            return Err(crate::invalid("integration belongs to another local node"));
        }
        let _lock = MutationLock::acquire(&self.store.root().join("mutation.lock"))?;
        self.store.check_expected(&plan.local)?;
        let local = self.store.view(&plan.local)?;
        let incoming = plan.incoming.view(self.store)?;
        let expected: BTreeSet<_> = plan
            .conflicts
            .iter()
            .map(|conflict| conflict.resource.full_path(&plan.manifest))
            .collect::<Result<_>>()?;
        let mut selected = BTreeSet::new();
        let mut prospective = store::layout(&plan.entries);
        let manifest = store::encode(&plan.manifest, "integrated project registry")?;
        store::validate_content(&manifest)?;
        prospective.insert(store::MANIFEST.into(), manifest.len());
        for resolution in resolutions {
            let path = resolution.resource.full_path(&plan.manifest)?;
            if !expected.contains(&path) || !selected.insert(path.clone()) {
                return Err(crate::invalid(
                    "resolution must name each conflicting document exactly once",
                ));
            }
            let size = match &resolution.choice {
                Choice::Local {} => local.entries.get(&path).map(|entry| entry.size),
                Choice::Incoming {} => incoming.entries.get(&path).map(|entry| entry.size),
                Choice::Write { content } => {
                    store::validate_content(content.as_bytes())?;
                    Some(content.len())
                }
                Choice::Remove {} => None,
            };
            if let Some(size) = size {
                prospective.insert(path, size);
            } else {
                prospective.remove(&path);
            }
        }
        if selected != expected {
            return Err(crate::invalid("integration has unresolved documents"));
        }
        store::validate_layout(&prospective, &plan.manifest)?;
        let proposed = match plan.kind {
            IntegrationKind::UpToDate => plan.local.clone(),
            IntegrationKind::FastForward => plan.incoming.snapshot.clone(),
            IntegrationKind::Merge => {
                let mut entries = plan.entries.clone();
                for resolution in resolutions {
                    let path = resolution.resource.full_path(&plan.manifest)?;
                    let value = match &resolution.choice {
                        Choice::Local {} => local.entries.get(&path).cloned(),
                        Choice::Incoming {} => incoming.entries.get(&path).cloned(),
                        Choice::Write { content } => Some(Entry {
                            oid: self.store.git().write_blob(content.as_bytes())?,
                            size: content.len(),
                        }),
                        Choice::Remove {} => None,
                    };
                    if let Some(value) = value {
                        entries.insert(path, value);
                    } else {
                        entries.remove(&path);
                    }
                }
                entries.insert(
                    store::MANIFEST.into(),
                    Entry {
                        oid: self.store.git().write_blob(&manifest)?,
                        size: manifest.len(),
                    },
                );
                let tree = self.store.git().tree(&entries)?;
                self.store.git().commit(
                    &tree,
                    &[&plan.local, &plan.incoming.snapshot],
                    "Integrate knowledge histories",
                )?
            }
        };
        if !self.store.git().is_ancestor(&plan.local, &proposed)?
            || !self
                .store
                .git()
                .is_ancestor(&plan.incoming.snapshot, &proposed)?
        {
            return Err(crate::invalid("integration would lose accepted history"));
        }
        self.store.git().publish(Some(&plan.local), &proposed)?;
        Ok(proposed)
    }
}

pub(crate) fn accept_return(store: &Store, delivery: &Delivery) -> Result<DeliveryReceipt> {
    delivery.validate_recipient(store)?;
    delivery.candidate.view(store)?;
    if !store
        .git()
        .is_ancestor(&delivery.expected, &delivery.candidate.snapshot)?
    {
        return Err(crate::invalid(
            "return delivery is not an append-only continuation",
        ));
    }
    let _lock = MutationLock::acquire(&store.root().join("mutation.lock"))?;
    let actual = store.snapshot()?;
    let already_present = store
        .git()
        .is_ancestor(&delivery.candidate.snapshot, &actual)?
        && store.git().is_ancestor(&delivery.expected, &actual)?;
    if !already_present {
        store.check_expected(&delivery.expected)?;
        store
            .git()
            .publish(Some(&delivery.expected), &delivery.candidate.snapshot)?;
    }
    Ok(DeliveryReceipt {
        recipient: store.identity(),
        snapshot: delivery.candidate.snapshot.clone(),
        observed: store.snapshot()?,
        already_present,
    })
}

#[cfg(test)]
mod tests;

fn oid(entry: Option<&Entry>) -> Option<&str> {
    entry.map(|entry| entry.oid.as_str())
}
