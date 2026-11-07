//! Reads and publication of knowledge snapshots through the substrate.
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Write as _;
use std::os::unix::fs::DirBuilderExt as _;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use dots_documents::mutation_lock::MutationLock;
use dots_documents::staged_file::StagedFile;
use dots_substrate::{Blob, Outcome, RefName, Substrate, Update, Version};
use serde::{Deserialize, Serialize};

use crate::integration::{Integration, IntegrationKind, Resolution};
use crate::manifest::{self, Entry, Manifest, Scope};
use crate::{Error, MAX_DOCUMENTS, ProjectId, Result, Snapshot, StoreId, invalid, io};

/// The substrate ref holding the latest published manifest.
const REF: &str = "memory";
const PENDING: &str = "pending.json";
const LOCK: &str = "mutation.lock";
/// Integration attempts while other machines keep publishing.
const ATTEMPTS: usize = 5;
const MAX_PENDING_BYTES: usize = 1024 * 1024;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Change {
    pub scope: Scope,
    pub path: PathBuf,
    pub action: Action,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Action {
    Write { content: String },
    Remove {},
}

impl Action {
    fn content(&self) -> Option<&str> {
        match self {
            Self::Write { content } => Some(content),
            Self::Remove {} => None,
        }
    }
}

/// How current a read is.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Freshness {
    Current,
    /// The remote is unreachable; the snapshot was last known current at `since`.
    Stale {
        since: Option<SystemTime>,
        reason: String,
    },
}

/// The latest published snapshot as this machine knows it.
#[derive(Clone, Debug)]
pub struct Head {
    pub snapshot: Snapshot,
    pub store: StoreId,
    pub freshness: Freshness,
    version: Version,
}

/// Where a publication landed.
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum Landing {
    /// `snapshot` is the published head, containing the publication.
    Published { snapshot: Snapshot },
    /// The remote was unreachable or its outcome unknown; the candidate is
    /// retained locally until a later publication or reconcile.
    Pending { candidate: Snapshot, reason: String },
}

#[derive(Clone, Debug, Serialize)]
pub struct Publication {
    #[serde(flatten)]
    pub landing: Landing,
    pub changed: usize,
    /// Earlier retained candidates published along the way.
    pub integrated: Vec<Snapshot>,
    /// Candidates still retained on this machine.
    pub pending: Vec<Snapshot>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Registration {
    #[serde(flatten)]
    pub publication: Publication,
    pub project: ProjectId,
}

/// An unpublished candidate retained on this machine.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pending {
    pub candidate: Snapshot,
    /// The published snapshot it was built from.
    pub base: Snapshot,
    /// Unix seconds when it was retained.
    pub since: u64,
}

/// A retained candidate and how it would integrate with the current head.
#[derive(Debug, Serialize)]
pub struct PendingPlan {
    #[serde(flatten)]
    pub pending: Pending,
    pub plan: Integration,
}

#[derive(Debug, Serialize)]
pub struct DocumentRead {
    pub snapshot: Snapshot,
    pub scope: Scope,
    pub document: dots_documents::Document,
}

#[derive(Debug, Serialize)]
pub struct CandidateRead {
    pub candidate: Snapshot,
    pub scope: Scope,
    pub document: dots_documents::Document,
}

#[derive(Debug, Serialize)]
pub struct Search {
    pub snapshot: Snapshot,
    pub scope: Scope,
    pub result: SearchResult,
}

#[derive(Debug, Serialize)]
pub struct Hit {
    pub path: PathBuf,
    pub line: usize,
    pub excerpt: String,
    pub truncated: bool,
}

#[derive(Debug, Serialize)]
pub struct SearchResult {
    pub backend: &'static str,
    pub hits: Vec<Hit>,
    pub has_more: bool,
    pub next_offset: Option<usize>,
}

/// Knowledge on one machine: the shared substrate plus a private local
/// directory for project bindings and retained candidates.
#[derive(Debug)]
pub struct Store {
    substrate: Substrate,
    local: PathBuf,
    manifests: RefCell<BTreeMap<Snapshot, Rc<Manifest>>>,
}

/// One snapshot's documents.
#[derive(Debug)]
pub struct View<'store> {
    store: &'store Store,
    snapshot: Snapshot,
    manifest: Rc<Manifest>,
}

enum Attempt {
    Published(Snapshot),
    Pending(String),
    Diverged { actual: Snapshot, conflicts: usize },
}

fn name() -> RefName {
    REF.parse().expect("the memory ref name is valid")
}

fn unix(time: SystemTime) -> u64 {
    time.duration_since(UNIX_EPOCH)
        .map_or(0, |age| age.as_secs())
}

impl Store {
    /// Use `substrate` with machine-local state under `local`, created if absent.
    pub fn open(substrate: Substrate, local: &Path) -> Result<Self> {
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(local)
            .map_err(|source| io("create local knowledge directory", local, source))?;
        Ok(Self {
            substrate,
            local: local.to_owned(),
            manifests: RefCell::default(),
        })
    }

    pub fn substrate(&self) -> &Substrate {
        &self.substrate
    }

    /// The machine-local directory for bindings and retained candidates.
    pub fn local(&self) -> &Path {
        &self.local
    }

    /// Publish an empty first snapshot when no knowledge exists yet.
    pub fn initialize(&mut self, now: SystemTime) -> Result<Snapshot> {
        let manifest = Manifest {
            parents: Vec::new(),
            store: StoreId::allocate()?,
            projects: BTreeMap::new(),
            entries: BTreeMap::new(),
        };
        let blob = manifest.encode()?;
        self.create(blob, Vec::new(), now)
    }

    /// Publish `root` and `blobs` as the first `memory` value; refuse if one exists.
    pub(crate) fn create(
        &mut self,
        root: Blob,
        mut blobs: Vec<Blob>,
        now: SystemTime,
    ) -> Result<Snapshot> {
        if let dots_substrate::Freshness::Stale { reason, .. } =
            self.substrate.ensure_fresh(Duration::ZERO, now)?
        {
            return Err(Error::Unavailable(reason));
        }
        if let Some(value) = self.substrate.read(&name(), now)? {
            return Err(Error::AlreadyInitialized(Snapshot::new(value.root)));
        }
        let snapshot = Snapshot::new(root.hash());
        blobs.push(root);
        let update = Update {
            name: name(),
            expected: None,
            root: snapshot.hash(),
            new_blobs: blobs,
        };
        match self.substrate.update(&update, now)? {
            Outcome::Applied(_) => Ok(snapshot),
            Outcome::Conflict { .. } => match self.substrate.read(&name(), now)? {
                Some(value) => Err(Error::AlreadyInitialized(Snapshot::new(value.root))),
                None => Err(Error::Unavailable(
                    "the remote reported knowledge that it no longer lists".into(),
                )),
            },
            Outcome::Unknown => Err(Error::Unavailable(
                "the outcome is unknown; rerun the same command".into(),
            )),
        }
    }

    /// The latest published snapshot, listing the remote unless this machine
    /// listed it within `max_age`. Offline, the last known snapshot, marked stale.
    pub fn head(&mut self, max_age: Duration, now: SystemTime) -> Result<Head> {
        let freshness = self.substrate.ensure_fresh(max_age, now)?;
        self.current(freshness, now)
    }

    fn current(&self, freshness: dots_substrate::Freshness, now: SystemTime) -> Result<Head> {
        let Some(value) = self.substrate.read(&name(), now)? else {
            return Err(match freshness {
                dots_substrate::Freshness::Stale { reason, .. } => Error::Unavailable(reason),
                dots_substrate::Freshness::Current { .. } => Error::Uninitialized,
            });
        };
        let freshness = match (value.stale_since, freshness) {
            (None, dots_substrate::Freshness::Current { .. }) => Freshness::Current,
            (since, dots_substrate::Freshness::Stale { reason, .. }) => {
                Freshness::Stale { since, reason }
            }
            (since, dots_substrate::Freshness::Current { .. }) => Freshness::Stale {
                since,
                reason: "the published snapshot could not be fetched".into(),
            },
        };
        let snapshot = Snapshot::new(value.root);
        let store = self.manifest(&snapshot)?.store.clone();
        Ok(Head {
            snapshot,
            store,
            freshness,
            version: value.version,
        })
    }

    /// The published `snapshot`, or the head itself when `None`. Selecting an
    /// unpublished candidate fails.
    pub fn view(&self, head: &Head, snapshot: Option<&Snapshot>) -> Result<View<'_>> {
        let snapshot = *snapshot.unwrap_or(&head.snapshot);
        if !self.ancestors(&head.snapshot)?.contains(&snapshot) {
            return Err(invalid(format!(
                "snapshot {snapshot} is not published knowledge"
            )));
        }
        Ok(View {
            store: self,
            manifest: self.manifest(&snapshot)?,
            snapshot,
        })
    }

    /// Publish one batch built from the published `expected` snapshot.
    ///
    /// If knowledge moved on since `expected`, changes to other documents
    /// merge into a two-parent snapshot; changes to a document that also
    /// changed since `expected` fail with [`Error::Diverged`] and retain the
    /// candidate for [`Store::reconcile`]. Offline, the candidate is retained
    /// and reported pending. Retained candidates are published first.
    pub fn publish(
        &mut self,
        expected: &Snapshot,
        changes: &[Change],
        now: SystemTime,
    ) -> Result<Publication> {
        if changes.len() > MAX_DOCUMENTS {
            return Err(invalid("publication contains too many document changes"));
        }
        let head = self.head(Duration::ZERO, now)?;
        let mut manifest = (*self.view(&head, Some(expected))?.manifest).clone();
        let mut selected = BTreeSet::new();
        let mut blobs = Vec::new();
        let mut modified = 0;
        for change in changes {
            let path = manifest.resource(&change.scope, &change.path)?;
            if !selected.insert(path.clone()) {
                return Err(invalid(
                    "a publication names the same document more than once",
                ));
            }
            if let Some(content) = change.action.content() {
                manifest::validate_content(content.as_bytes())?;
                let blob = Blob::new(content.as_bytes().to_vec())?;
                let entry = Entry {
                    hash: blob.hash(),
                    size: content.len(),
                };
                if manifest.entries.insert(path, entry) != Some(entry) {
                    modified += 1;
                }
                blobs.push(blob);
            } else if manifest.entries.remove(&path).is_some() {
                modified += 1;
            }
        }
        manifest.parents = vec![*expected];
        let root = manifest.encode()?;
        if modified == 0 {
            return Ok(Publication {
                landing: Landing::Published {
                    snapshot: head.snapshot,
                },
                changed: 0,
                integrated: Vec::new(),
                pending: self.pending_candidates()?,
            });
        }
        for blob in blobs.iter().chain([&root]) {
            self.substrate.put(blob)?;
        }
        self.land(head, Snapshot::new(root.hash()), *expected, modified, now)
    }

    /// Register a shared project identity; display names never select or
    /// deduplicate projects.
    pub fn register_project(
        &mut self,
        expected: &Snapshot,
        name: &str,
        now: SystemTime,
    ) -> Result<Registration> {
        manifest::validate_name(name)?;
        let head = self.head(Duration::ZERO, now)?;
        let mut manifest = (*self.view(&head, Some(expected))?.manifest).clone();
        let project = ProjectId::allocate()?;
        manifest.projects.insert(project.clone(), name.into());
        manifest.parents = vec![*expected];
        let root = manifest.encode()?;
        self.substrate.put(&root)?;
        let publication = self.land(head, Snapshot::new(root.hash()), *expected, 1, now)?;
        Ok(Registration {
            publication,
            project,
        })
    }

    fn land(
        &mut self,
        head: Head,
        candidate: Snapshot,
        base: Snapshot,
        changed: usize,
        now: SystemTime,
    ) -> Result<Publication> {
        let pending = Pending {
            candidate,
            base,
            since: unix(now),
        };
        if let Freshness::Stale { reason, .. } = head.freshness {
            self.retain(pending)?;
            return Ok(Publication {
                landing: Landing::Pending { candidate, reason },
                changed,
                integrated: Vec::new(),
                pending: self.pending_candidates()?,
            });
        }
        let integrated = self.integrate_pending(now)?;
        let landing = match self.integrate(&candidate, &[], None, now)? {
            Attempt::Published(snapshot) => Landing::Published { snapshot },
            Attempt::Pending(reason) => {
                self.retain(pending)?;
                Landing::Pending { candidate, reason }
            }
            Attempt::Diverged { actual, conflicts } => {
                self.retain(pending)?;
                return Err(Error::Diverged {
                    expected: base,
                    actual,
                    candidate,
                    conflicts,
                });
            }
        };
        Ok(Publication {
            landing,
            changed,
            integrated,
            pending: self.pending_candidates()?,
        })
    }

    /// Publish every retained candidate that integrates without judgment.
    fn integrate_pending(&mut self, now: SystemTime) -> Result<Vec<Snapshot>> {
        let mut integrated = Vec::new();
        for pending in self.pending()? {
            match self.integrate(&pending.candidate, &[], None, now)? {
                Attempt::Published(_) => {
                    self.release(&pending.candidate)?;
                    integrated.push(pending.candidate);
                }
                Attempt::Pending(_) => break,
                Attempt::Diverged { .. } => {}
            }
        }
        Ok(integrated)
    }

    /// Bring `candidate` into the published history by compare-and-swap,
    /// merging with whatever the head has become. With `pinned`, the head must
    /// still be that snapshot.
    fn integrate(
        &mut self,
        candidate: &Snapshot,
        resolutions: &[Resolution],
        pinned: Option<&Snapshot>,
        now: SystemTime,
    ) -> Result<Attempt> {
        let mut freshness = self.substrate.ensure_fresh(Duration::MAX, now)?;
        for _ in 0..ATTEMPTS {
            let head = self.current(freshness, now)?;
            if let Freshness::Stale { reason, .. } = head.freshness {
                return Ok(Attempt::Pending(reason));
            }
            if let Some(pinned) = pinned
                && *pinned != head.snapshot
            {
                return Err(Error::Conflict {
                    expected: *pinned,
                    actual: head.snapshot,
                });
            }
            let plan = self.plan(candidate, &head.snapshot)?;
            let (root, mut blobs) = match plan.kind {
                IntegrationKind::UpToDate => return Ok(Attempt::Published(head.snapshot)),
                IntegrationKind::FastForward => (*candidate, Vec::new()),
                IntegrationKind::Merge => {
                    if resolutions.is_empty() && !plan.conflicts.is_empty() {
                        return Ok(Attempt::Diverged {
                            actual: head.snapshot,
                            conflicts: plan.conflicts.len(),
                        });
                    }
                    let (merged, written) = self.accept(&plan, resolutions)?;
                    let current = self.manifest(&head.snapshot)?;
                    if merged.entries == current.entries && merged.projects == current.projects {
                        return Ok(Attempt::Published(head.snapshot));
                    }
                    let blob = merged.encode()?;
                    for blob in written.iter().chain([&blob]) {
                        self.substrate.put(blob)?;
                    }
                    let root = Snapshot::new(blob.hash());
                    let mut blobs = written;
                    blobs.push(blob);
                    (root, blobs)
                }
            };
            blobs.extend(self.unpublished(candidate, &head.snapshot)?);
            let update = Update {
                name: name(),
                expected: Some(head.version.clone()),
                root: root.hash(),
                new_blobs: blobs,
            };
            match self.substrate.update(&update, now) {
                Ok(Outcome::Applied(_)) => return Ok(Attempt::Published(root)),
                Ok(Outcome::Conflict { .. }) => {
                    freshness = dots_substrate::Freshness::Current { as_of: now };
                }
                Ok(Outcome::Unknown) => {
                    return Ok(Attempt::Pending(
                        "the remote did not confirm the update".into(),
                    ));
                }
                Err(dots_substrate::Error::Unavailable(reason)) => {
                    return Ok(Attempt::Pending(reason));
                }
                Err(error) => return Err(error.into()),
            }
        }
        Err(invalid(
            "knowledge kept changing during publication; retry later",
        ))
    }

    /// Resolve a retained candidate against the published `incoming` snapshot
    /// its plan was made for, and publish the result.
    pub fn reconcile(
        &mut self,
        candidate: &Snapshot,
        incoming: &Snapshot,
        resolutions: &[Resolution],
        now: SystemTime,
    ) -> Result<Snapshot> {
        if !self.pending()?.iter().any(|p| p.candidate == *candidate) {
            return Err(invalid(format!(
                "{candidate} is not a candidate retained on this machine"
            )));
        }
        let head = self.head(Duration::ZERO, now)?;
        if let Freshness::Stale { reason, .. } = head.freshness {
            return Err(Error::Unavailable(reason));
        }
        match self.integrate(candidate, resolutions, Some(incoming), now)? {
            Attempt::Published(snapshot) => {
                self.release(candidate)?;
                Ok(snapshot)
            }
            Attempt::Pending(reason) => Err(Error::Unavailable(reason)),
            Attempt::Diverged { actual, conflicts } => Err(Error::Diverged {
                expected: *incoming,
                actual,
                candidate: *candidate,
                conflicts,
            }),
        }
    }

    /// Retained candidates, oldest first.
    pub fn pending(&self) -> Result<Vec<Pending>> {
        let path = self.local.join(PENDING);
        let Some(bytes) = dots_documents::text::read_bytes(&path, MAX_PENDING_BYTES)? else {
            return Ok(Vec::new());
        };
        serde_json::from_slice(&bytes).map_err(|source| Error::Json {
            operation: "retained knowledge candidates",
            source,
        })
    }

    fn pending_candidates(&self) -> Result<Vec<Snapshot>> {
        Ok(self.pending()?.into_iter().map(|p| p.candidate).collect())
    }

    /// Each retained candidate with its plan against `head`.
    pub fn pending_plans(&self, head: &Head) -> Result<Vec<PendingPlan>> {
        self.pending()?
            .into_iter()
            .map(|pending| {
                let plan = self.plan(&pending.candidate, &head.snapshot)?;
                Ok(PendingPlan { pending, plan })
            })
            .collect()
    }

    /// Read a document of a retained candidate, labeled as unpublished.
    pub fn inspect(
        &self,
        candidate: &Snapshot,
        scope: &Scope,
        relative: &Path,
        start: usize,
        lines: usize,
    ) -> Result<CandidateRead> {
        if !self.pending()?.iter().any(|p| p.candidate == *candidate) {
            return Err(invalid(format!(
                "{candidate} is not a candidate retained on this machine"
            )));
        }
        let view = View {
            store: self,
            manifest: self.manifest(candidate)?,
            snapshot: *candidate,
        };
        let read = view.read(scope, relative, start, lines)?;
        Ok(CandidateRead {
            candidate: *candidate,
            scope: read.scope,
            document: read.document,
        })
    }

    fn retain(&self, pending: Pending) -> Result<()> {
        self.modify_pending(|list| {
            if !list.iter().any(|p| p.candidate == pending.candidate) {
                list.push(pending);
            }
        })
    }

    fn release(&self, candidate: &Snapshot) -> Result<()> {
        self.modify_pending(|list| list.retain(|p| p.candidate != *candidate))
    }

    fn modify_pending(&self, change: impl FnOnce(&mut Vec<Pending>)) -> Result<()> {
        let _lock = MutationLock::acquire(&self.local.join(LOCK))?;
        let mut list = self.pending()?;
        let before = list.clone();
        change(&mut list);
        if list == before {
            return Ok(());
        }
        let path = self.local.join(PENDING);
        if list.is_empty() {
            fs::remove_file(&path)
                .map_err(|source| io("remove retained candidates", &path, source))?;
            return Ok(());
        }
        let bytes = serde_json::to_vec_pretty(&list).map_err(|source| Error::Json {
            operation: "retained knowledge candidates",
            source,
        })?;
        let staged = StagedFile::create(&self.local)?;
        staged
            .file()
            .write_all(&bytes)
            .map_err(|source| io("write retained candidates", staged.path(), source))?;
        staged.publish_replace(&path)?;
        Ok(())
    }

    /// A snapshot's manifest from the verified cache.
    pub(crate) fn manifest(&self, snapshot: &Snapshot) -> Result<Rc<Manifest>> {
        if let Some(manifest) = self.manifests.borrow().get(snapshot) {
            return Ok(Rc::clone(manifest));
        }
        let blob = match self.substrate.blob(&snapshot.hash()) {
            Ok(blob) => blob,
            Err(dots_substrate::Error::MissingBlob(_)) => {
                return Err(invalid(format!("snapshot {snapshot} is not available")));
            }
            Err(error) => return Err(error.into()),
        };
        let manifest = Rc::new(Manifest::decode(&blob)?);
        self.manifests
            .borrow_mut()
            .insert(*snapshot, Rc::clone(&manifest));
        Ok(manifest)
    }

    pub(crate) fn parents(&self, snapshot: &Snapshot) -> Result<Vec<Snapshot>> {
        Ok(self.manifest(snapshot)?.parents.clone())
    }

    /// `tip` and every snapshot it descends from, all of one store.
    pub(crate) fn ancestors(&self, tip: &Snapshot) -> Result<BTreeSet<Snapshot>> {
        let store = self.manifest(tip)?.store.clone();
        let mut seen = BTreeSet::new();
        let mut stack = vec![*tip];
        while let Some(snapshot) = stack.pop() {
            if !seen.insert(snapshot) {
                continue;
            }
            let manifest = self.manifest(&snapshot)?;
            if manifest.store != store {
                return Err(invalid("knowledge history mixes stores"));
            }
            stack.extend(manifest.parents.iter().copied());
        }
        Ok(seen)
    }

    /// Blobs `local` carries that `remote`'s history lacks: each such
    /// snapshot's manifest and the documents it introduced.
    fn unpublished(&self, local: &Snapshot, remote: &Snapshot) -> Result<Vec<Blob>> {
        let published = self.ancestors(remote)?;
        let mut seen = BTreeSet::new();
        let mut hashes = BTreeSet::new();
        let mut blobs = Vec::new();
        let mut stack = vec![*local];
        while let Some(snapshot) = stack.pop() {
            if published.contains(&snapshot) || !seen.insert(snapshot) {
                continue;
            }
            let manifest = self.manifest(&snapshot)?;
            let parents = manifest
                .parents
                .iter()
                .map(|parent| self.manifest(parent))
                .collect::<Result<Vec<_>>>()?;
            let introduced = manifest.entries.iter().filter(|(path, entry)| {
                !parents
                    .iter()
                    .any(|parent| parent.entries.get(*path) == Some(entry))
            });
            for (_, entry) in introduced {
                if hashes.insert(entry.hash) {
                    blobs.push(self.substrate.blob(&entry.hash)?);
                }
            }
            blobs.push(self.substrate.blob(&snapshot.hash())?);
            stack.extend(manifest.parents.iter().copied());
        }
        Ok(blobs)
    }
}

impl View<'_> {
    pub fn snapshot(&self) -> &Snapshot {
        &self.snapshot
    }

    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    fn document(&self, entry: &Entry) -> Result<String> {
        let blob = self.store.substrate.blob(&entry.hash)?;
        if blob.content().len() != entry.size {
            return Err(invalid("knowledge document size differs from its manifest"));
        }
        manifest::validate_content(blob.content())?;
        String::from_utf8(blob.into_content())
            .map_err(|_| invalid("knowledge document is not UTF-8"))
    }

    /// Return exact bounded UTF-8 bytes as text, without inspection-page normalization.
    pub fn content(&self, scope: &Scope, relative: &Path) -> Result<Option<String>> {
        let path = self.manifest.resource(scope, relative)?;
        self.manifest
            .entries
            .get(&path)
            .map(|entry| self.document(entry))
            .transpose()
    }

    pub fn read(
        &self,
        scope: &Scope,
        relative: &Path,
        start: usize,
        lines: usize,
    ) -> Result<DocumentRead> {
        dots_documents::text::validate_page(start, lines)?;
        let content = self.content(scope, relative)?;
        let document = dots_documents::text::page(relative, content.as_deref(), start, lines)?;
        Ok(DocumentRead {
            snapshot: self.snapshot,
            scope: scope.clone(),
            document,
        })
    }

    pub fn search(
        &self,
        scope: &Scope,
        query: &str,
        offset: usize,
        limit: usize,
    ) -> Result<Search> {
        if query.trim().is_empty() || query.len() > 1024 || !(1..=20).contains(&limit) {
            return Err(invalid(
                "query must contain 1..1024 bytes; limit must be 1..20",
            ));
        }
        let prefix = scope.prefix(&self.manifest)?;
        let query = query.to_lowercase();
        let mut hits = Vec::new();
        let mut skipped = 0;
        let mut has_more = false;
        let selected = self
            .manifest
            .entries
            .iter()
            .filter(|(path, _)| path.starts_with(&prefix));
        'documents: for (path, entry) in selected {
            let content = self.document(entry)?;
            for (line, text) in content.lines().enumerate() {
                if !text.to_lowercase().contains(&query) {
                    continue;
                }
                if skipped < offset {
                    skipped += 1;
                    continue;
                }
                if hits.len() == limit {
                    has_more = true;
                    break 'documents;
                }
                let excerpt = dots_documents::text::excerpt(text, 600);
                let truncated = excerpt.len() < text.len();
                hits.push(Hit {
                    path: PathBuf::from(&path[prefix.len()..]),
                    line: line + 1,
                    excerpt,
                    truncated,
                });
            }
        }
        Ok(Search {
            snapshot: self.snapshot,
            scope: scope.clone(),
            result: SearchResult {
                backend: "literal",
                hits,
                has_more,
                next_offset: has_more.then_some(offset.saturating_add(limit)),
            },
        })
    }
}

#[cfg(test)]
mod tests;
