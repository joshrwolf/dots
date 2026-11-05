//! Ownership of a private Git knowledge store and its immutable read views.
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, Metadata};
use std::io::Write as _;
use std::os::unix::fs::{DirBuilderExt as _, MetadataExt as _, PermissionsExt as _};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::git::{Entry, Git};
use crate::{
    Error, MAX_CORPUS_BYTES, MAX_DOCUMENTS, MAX_FILE_BYTES, MAX_METADATA_BYTES, NodeId, ProjectId,
    Result, Snapshot, StoreId, invalid, io,
};

const MANIFEST: &str = "knowledge.json";
const LOCAL: &str = "store.json";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Authority,
    Replica,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "project",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum Scope {
    Global,
    Project(ProjectId),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub store: StoreId,
    pub projects: BTreeMap<ProjectId, String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Local {
    node: NodeId,
    store: StoreId,
    genesis: Snapshot,
    role: Role,
    actor: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Change {
    pub scope: Scope,
    pub path: PathBuf,
    pub content: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Publication {
    pub snapshot: Snapshot,
    pub changed: usize,
}

#[derive(Clone, Debug, Serialize)]
pub struct Registration {
    pub snapshot: Snapshot,
    pub project: ProjectId,
}

#[derive(Debug, Serialize)]
pub struct DocumentRead {
    pub snapshot: Snapshot,
    pub scope: Scope,
    pub document: dots_documents::Document,
}

#[derive(Debug, Serialize)]
pub struct Search {
    pub snapshot: Snapshot,
    pub scope: Scope,
    pub result: dots_documents::Search,
}

#[derive(Clone, Debug)]
pub struct Store {
    root: PathBuf,
    local: Local,
    git: Git,
}

#[derive(Debug)]
pub struct View<'store> {
    store: &'store Store,
    snapshot: Snapshot,
    manifest: Manifest,
    entries: BTreeMap<String, Entry>,
}

#[derive(Debug)]
struct Initialization(PathBuf);
impl Drop for Initialization {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn private_metadata(path: &Path) -> Result<Metadata> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| io("inspect owned knowledge path", path, error))?;
    if metadata.file_type().is_symlink()
        || metadata.uid() != nix::unistd::geteuid().as_raw()
        || metadata.permissions().mode() & 0o077 != 0
    {
        return Err(invalid(format!(
            "knowledge path must be private, owned and not a symlink: {}",
            path.display()
        )));
    }
    Ok(metadata)
}

fn validate_name(name: &str) -> Result<()> {
    if name.is_empty()
        || name.len() > 200
        || name.trim() != name
        || name.chars().any(char::is_control)
    {
        return Err(invalid(
            "knowledge name must be 1..200 bytes, trimmed and without controls",
        ));
    }
    Ok(())
}

fn validate_actor(actor: &str) -> Result<()> {
    validate_name(actor)?;
    if actor.contains(['<', '>']) {
        return Err(invalid("knowledge actor must not contain angle brackets"));
    }
    Ok(())
}

fn reject_repository_ancestors(path: &Path) -> Result<()> {
    for ancestor in path.ancestors() {
        if fs::symlink_metadata(ancestor.join(".git")).is_ok()
            || (ancestor.join("HEAD").is_file()
                && ancestor.join("objects").is_dir()
                && ancestor.join("refs").is_dir())
        {
            return Err(invalid(
                "knowledge store must be outside other Git repositories",
            ));
        }
    }
    Ok(())
}

impl Store {
    /// Create a new authority atomically at an unused absolute location outside checkouts.
    /// Replica enrollment joins an existing store; it never invents another genesis.
    pub fn initialize(root: &Path, actor: &str) -> Result<Self> {
        validate_actor(actor)?;
        if !root.is_absolute() {
            return Err(invalid("knowledge store root must be absolute"));
        }
        match fs::symlink_metadata(root) {
            Ok(_) => {
                return Err(invalid(
                    "knowledge store destination already exists; refusing adoption",
                ));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(io("inspect knowledge destination", root, error)),
        }
        let parent = root
            .parent()
            .ok_or_else(|| invalid("knowledge store needs a parent"))?;
        reject_repository_ancestors(parent)?;
        dots_documents::directories::create_within(parent, parent)?;
        let parent = parent
            .canonicalize()
            .map_err(|error| io("resolve knowledge parent", parent, error))?;
        reject_repository_ancestors(&parent)?;
        let name = root
            .file_name()
            .ok_or_else(|| invalid("knowledge root needs a directory name"))?;
        let destination = parent.join(name);
        let store = StoreId::allocate()?;
        let staging_path = parent.join(format!(".knowledge-init-{store}"));
        fs::DirBuilder::new()
            .mode(0o700)
            .create(&staging_path)
            .map_err(|error| io("stage knowledge root", &staging_path, error))?;
        let stage = Initialization(staging_path);
        fs::DirBuilder::new()
            .mode(0o700)
            .create(stage.0.join("repo.git"))
            .map_err(|error| io("stage knowledge repository", &stage.0, error))?;
        let git = Git::new(stage.0.clone(), actor.into());
        git.initialize()?;
        let manifest = Manifest {
            store: store.clone(),
            projects: BTreeMap::new(),
        };
        let content = encode(&manifest, "knowledge manifest")?;
        let blob = git.write_blob(&content)?;
        let entries = BTreeMap::from([(
            MANIFEST.into(),
            Entry {
                oid: blob,
                size: content.len(),
            },
        )]);
        let tree = git.tree(&entries)?;
        let genesis = git.commit(&tree, &[], "Initialize knowledge")?;
        git.publish(None, &genesis)?;
        let local = Local {
            node: NodeId::allocate()?,
            store,
            genesis,
            role: Role::Authority,
            actor: actor.into(),
        };
        let staged = dots_documents::staged_file::StagedFile::create(&stage.0)?;
        let bytes = encode(&local, "local knowledge binding")?;
        staged
            .file()
            .write_all(&bytes)
            .map_err(|error| io("write knowledge binding", staged.path(), error))?;
        staged.publish_new(&stage.0.join(LOCAL))?;
        dots_documents::directories::sync(&stage.0)?;
        fs::rename(&stage.0, &destination)
            .map_err(|error| io("publish knowledge root", &destination, error))?;
        dots_documents::directories::sync(&parent)?;
        Self::open(&destination)
    }

    /// Open an existing owned store. This neither initializes data nor mutates knowledge.
    pub fn open(root: &Path) -> Result<Self> {
        if !root.is_absolute() || !private_metadata(root)?.is_dir() {
            return Err(invalid(
                "knowledge root must be an absolute private directory",
            ));
        }
        if !private_metadata(&root.join("repo.git"))?.is_dir() {
            return Err(invalid("knowledge repository must be a private directory"));
        }
        let local_path = root.join(LOCAL);
        if !private_metadata(&local_path)?.is_file() {
            return Err(invalid("knowledge binding must be a regular file"));
        }
        let bytes = dots_documents::text::read_bytes(&local_path, MAX_FILE_BYTES)?
            .ok_or_else(|| invalid("knowledge binding disappeared"))?;
        let local: Local = serde_json::from_slice(&bytes).map_err(|source| Error::Json {
            operation: "local knowledge binding",
            source,
        })?;
        validate_actor(&local.actor)?;
        let root = root
            .canonicalize()
            .map_err(|error| io("resolve knowledge root", root, error))?;
        let git = Git::new(root.clone(), local.actor.clone());
        reject_repository_ancestors(&root)?;
        git.validate_configuration()?;
        let store = Self { root, local, git };
        store.latest()?;
        Ok(store)
    }

    pub fn node_id(&self) -> &NodeId {
        &self.local.node
    }

    pub fn id(&self) -> &StoreId {
        &self.local.store
    }
    pub fn role(&self) -> Role {
        self.local.role
    }
    pub fn root(&self) -> &Path {
        &self.root
    }
    pub fn snapshot(&self) -> Result<Snapshot> {
        self.git.head()
    }
    pub fn latest(&self) -> Result<View<'_>> {
        self.view(&self.snapshot()?)
    }

    /// Select a previously published snapshot, never an unaccepted incoming candidate.
    pub fn view(&self, snapshot: &Snapshot) -> Result<View<'_>> {
        let head = self.git.head()?;
        if !self.git.is_ancestor(snapshot, &head)?
            || !self.git.is_ancestor(&self.local.genesis, snapshot)?
        {
            return Err(invalid("snapshot is not published knowledge in this store"));
        }
        let roots = self
            .git
            .text(&["rev-list", "--max-parents=0", snapshot.as_str()], None)?;
        if roots.trim() != self.local.genesis.as_str() {
            return Err(invalid("knowledge contains unrelated history"));
        }
        let entries = self.git.entries(snapshot)?;
        let entry = entries
            .get(MANIFEST)
            .ok_or_else(|| invalid("knowledge manifest is absent"))?;
        let bytes = self.git.blob(&entry.oid, entry.size)?;
        let manifest: Manifest = serde_json::from_slice(&bytes).map_err(|source| Error::Json {
            operation: "knowledge manifest",
            source,
        })?;
        if encode(&manifest, "knowledge manifest")? != bytes {
            return Err(invalid("knowledge manifest is not canonical JSON"));
        }
        if manifest.store != self.local.store {
            return Err(invalid("knowledge manifest belongs to another store"));
        }
        for name in manifest.projects.values() {
            validate_name(name)?;
        }
        validate_layout(&layout(&entries), &manifest)?;
        Ok(View {
            store: self,
            snapshot: snapshot.clone(),
            manifest,
            entries,
        })
    }

    /// Publish one batch against the complete snapshot the caller observed.
    /// Network, models and editors never execute under the short local mutation lock.
    pub fn publish(&self, expected: &Snapshot, changes: &[Change]) -> Result<Publication> {
        if changes.len() > MAX_DOCUMENTS {
            return Err(invalid("publication contains too many document changes"));
        }
        let _lock =
            dots_documents::mutation_lock::MutationLock::acquire(&self.root.join("mutation.lock"))?;
        self.check_expected(expected)?;
        let view = self.view(expected)?;
        let mut prospective = layout(&view.entries);
        let mut selected = BTreeSet::new();
        for change in changes {
            let path = view.resource(&change.scope, &change.path)?;
            if !selected.insert(path.clone()) {
                return Err(invalid(
                    "a publication names the same document more than once",
                ));
            }
            if let Some(content) = &change.content {
                validate_content(content.as_bytes())?;
                prospective.insert(path, content.len());
            } else {
                prospective.remove(&path);
            }
        }
        validate_layout(&prospective, &view.manifest)?;
        let mut entries = view.entries.clone();
        let mut count = 0;
        for change in changes {
            let path = view.resource(&change.scope, &change.path)?;
            if let Some(content) = &change.content {
                let oid = self.git.write_blob(content.as_bytes())?;
                if entries.get(&path).is_some_and(|entry| entry.oid == oid) {
                    continue;
                }
                entries.insert(
                    path,
                    Entry {
                        oid,
                        size: content.len(),
                    },
                );
                count += 1;
            } else if entries.remove(&path).is_some() {
                count += 1;
            }
        }
        let snapshot = self.publish_entries(expected, &entries, count, "Publish knowledge")?;
        Ok(Publication {
            snapshot,
            changed: count,
        })
    }

    /// Register an identity explicitly; display names never select or deduplicate projects.
    pub fn register_project(&self, expected: &Snapshot, name: &str) -> Result<Registration> {
        validate_name(name)?;
        if self.local.role != Role::Authority {
            return Err(invalid(
                "shared projects are registered by the knowledge authority",
            ));
        }
        let _lock =
            dots_documents::mutation_lock::MutationLock::acquire(&self.root.join("mutation.lock"))?;
        self.check_expected(expected)?;
        let view = self.view(expected)?;
        let project = ProjectId::allocate()?;
        let mut manifest = view.manifest.clone();
        manifest.projects.insert(project.clone(), name.into());
        let content = encode(&manifest, "knowledge manifest")?;
        validate_content(&content)?;
        let mut prospective = layout(&view.entries);
        prospective.insert(MANIFEST.into(), content.len());
        validate_layout(&prospective, &manifest)?;
        let mut entries = view.entries.clone();
        entries.insert(
            MANIFEST.into(),
            Entry {
                oid: self.git.write_blob(&content)?,
                size: content.len(),
            },
        );
        let snapshot = self.publish_entries(expected, &entries, 1, "Register knowledge project")?;
        Ok(Registration { snapshot, project })
    }

    fn check_expected(&self, expected: &Snapshot) -> Result<()> {
        let actual = self.git.head()?;
        if *expected != actual {
            return Err(Error::Conflict {
                expected: expected.clone(),
                actual,
            });
        }
        Ok(())
    }

    fn publish_entries(
        &self,
        expected: &Snapshot,
        entries: &BTreeMap<String, Entry>,
        changed: usize,
        message: &str,
    ) -> Result<Snapshot> {
        if changed == 0 {
            self.check_expected(expected)?;
            return Ok(expected.clone());
        }
        let tree = self.git.tree(entries)?;
        let snapshot = self.git.commit(&tree, &[expected], message)?;
        self.git.publish(Some(expected), &snapshot)?;
        Ok(snapshot)
    }
}

impl View<'_> {
    pub fn snapshot(&self) -> &Snapshot {
        &self.snapshot
    }
    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    fn prefix(&self, scope: &Scope) -> Result<String> {
        match scope {
            Scope::Global => Ok("global/".into()),
            Scope::Project(project) if self.manifest.projects.contains_key(project) => {
                Ok(format!("projects/{project}/"))
            }
            Scope::Project(project) => Err(Error::UnknownProject(project.clone())),
        }
    }

    fn resource(&self, scope: &Scope, relative: &Path) -> Result<String> {
        dots_documents::paths::validate_markdown(relative)?;
        if relative.components().count() > 8 {
            return Err(invalid("knowledge path exceeds eight document components"));
        }
        let relative = relative
            .to_str()
            .ok_or_else(|| invalid("knowledge path must be Unicode"))?;
        Ok(format!("{}{relative}", self.prefix(scope)?))
    }

    /// Return exact bounded UTF-8 bytes as text, without inspection-page normalization.
    pub fn content(&self, scope: &Scope, relative: &Path) -> Result<Option<String>> {
        let path = self.resource(scope, relative)?;
        let content = self
            .entries
            .get(&path)
            .map(|entry| self.store.git.blob(&entry.oid, entry.size))
            .transpose()?;
        if let Some(content) = &content {
            validate_content(content)?;
        }
        content
            .map(String::from_utf8)
            .transpose()
            .map_err(|_| invalid("knowledge document is not UTF-8"))
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
            snapshot: self.snapshot.clone(),
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
        let prefix = self.prefix(scope)?;
        let selected: Vec<_> = self
            .entries
            .iter()
            .filter(|(path, _)| {
                path.starts_with(&prefix) && path.rsplit('/').next() != Some("log.md")
            })
            .collect();
        let ids: BTreeSet<_> = selected
            .iter()
            .map(|(_, entry)| entry.oid.as_str())
            .collect();
        let ids: Vec<_> = ids.into_iter().collect();
        let blobs = self
            .store
            .git
            .blobs(&ids, MAX_CORPUS_BYTES + MAX_METADATA_BYTES)?;
        let query = query.to_lowercase();
        let mut hits = Vec::new();
        let mut skipped = 0;
        let mut has_more = false;
        'documents: for (path, entry) in selected {
            let bytes = blobs
                .get(&entry.oid)
                .ok_or_else(|| invalid("batch omitted a knowledge document"))?;
            validate_content(bytes)?;
            let content = std::str::from_utf8(bytes)
                .map_err(|_| invalid("knowledge document is not UTF-8"))?;
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
                hits.push(dots_documents::Hit {
                    path: PathBuf::from(&path[prefix.len()..]),
                    line: line + 1,
                    excerpt,
                    truncated,
                });
            }
        }
        Ok(Search {
            snapshot: self.snapshot.clone(),
            scope: scope.clone(),
            result: dots_documents::Search {
                backend: "literal",
                hits,
                has_more,
                next_offset: has_more.then_some(offset.saturating_add(limit)),
            },
        })
    }
}

fn encode(value: &impl Serialize, operation: &'static str) -> Result<Vec<u8>> {
    let mut content =
        serde_json::to_vec_pretty(value).map_err(|source| Error::Json { operation, source })?;
    content.push(b'\n');
    Ok(content)
}

fn validate_content(content: &[u8]) -> Result<()> {
    if content.len() > MAX_FILE_BYTES {
        return Err(invalid("knowledge document exceeds 256 KiB"));
    }
    if content.contains(&0) || std::str::from_utf8(content).is_err() {
        return Err(invalid("knowledge content must be UTF-8 without NUL bytes"));
    }
    Ok(())
}

fn layout(entries: &BTreeMap<String, Entry>) -> BTreeMap<String, usize> {
    entries
        .iter()
        .map(|(path, entry)| (path.clone(), entry.size))
        .collect()
}

fn validate_layout(entries: &BTreeMap<String, usize>, manifest: &Manifest) -> Result<()> {
    let mut folded = BTreeMap::new();
    let mut total = 0_usize;
    let mut metadata = 0_usize;
    if entries.len() > MAX_DOCUMENTS {
        return Err(invalid("knowledge corpus contains too many documents"));
    }
    for (path, &size) in entries {
        metadata = metadata
            .checked_add(path.len() + 128)
            .ok_or_else(|| invalid("knowledge metadata size overflow"))?;
        if metadata > MAX_METADATA_BYTES {
            return Err(invalid("knowledge tree metadata exceeds the read budget"));
        }
        total = total
            .checked_add(size)
            .ok_or_else(|| invalid("knowledge size overflow"))?;
        if size > MAX_FILE_BYTES || total > MAX_CORPUS_BYTES {
            return Err(invalid("knowledge corpus exceeds storage bounds"));
        }
        let mut component = path.as_str();
        loop {
            if let Some(previous) = folded.insert(component.to_lowercase(), component)
                && previous != component
            {
                return Err(invalid("knowledge paths have a case ambiguity"));
            }
            let Some((parent, _)) = component.rsplit_once('/') else {
                break;
            };
            component = parent;
        }
        if path == MANIFEST {
            continue;
        }
        let relative = if let Some(relative) = path.strip_prefix("global/") {
            relative
        } else if let Some(project) = path.strip_prefix("projects/") {
            let (id, relative) = project
                .split_once('/')
                .ok_or_else(|| invalid("knowledge project path is incomplete"))?;
            let id: ProjectId = id.parse()?;
            if !manifest.projects.contains_key(&id) {
                return Err(Error::UnknownProject(id));
            }
            relative
        } else {
            return Err(invalid(format!("path is outside knowledge scopes: {path}")));
        };
        dots_documents::paths::validate_markdown(Path::new(relative))?;
        if relative.split('/').count() > 8 {
            return Err(invalid("knowledge path exceeds eight document components"));
        }
        let mut ancestor = path.as_str();
        while let Some((parent, _)) = ancestor.rsplit_once('/') {
            if entries.contains_key(parent) {
                return Err(invalid("a knowledge document also names a directory"));
            }
            ancestor = parent;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
