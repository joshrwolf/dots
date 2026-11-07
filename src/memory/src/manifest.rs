//! Snapshot manifests: their canonical encoding, scopes and layout rules.
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use dots_substrate::{Blob, Hash};
use serde::{Deserialize, Serialize};

use crate::{
    Error, MAX_CORPUS_BYTES, MAX_DOCUMENTS, MAX_FILE_BYTES, ProjectId, Result, Snapshot, StoreId,
    invalid,
};

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

impl Scope {
    pub(crate) fn prefix(&self, manifest: &Manifest) -> Result<String> {
        match self {
            Self::Global => Ok("global/".into()),
            Self::Project(project) if manifest.projects.contains_key(project) => {
                Ok(format!("projects/{project}/"))
            }
            Self::Project(project) => Err(Error::UnknownProject(project.clone())),
        }
    }
}

/// A document location within a shared scope, not a machine-local filesystem path.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Resource {
    pub scope: Scope,
    pub path: PathBuf,
}

impl Resource {
    pub(crate) fn full_path(&self, manifest: &Manifest) -> Result<String> {
        dots_documents::paths::validate_markdown(&self.path)?;
        if self.path.components().count() > 8 {
            return Err(invalid("knowledge path exceeds eight document components"));
        }
        let path = self
            .path
            .to_str()
            .ok_or_else(|| invalid("knowledge path must be Unicode"))?;
        Ok(format!("{}{path}", self.scope.prefix(manifest)?))
    }

    pub(crate) fn from_path(path: &str, manifest: &Manifest) -> Result<Self> {
        let (scope, relative) = if let Some(relative) = path.strip_prefix("global/") {
            (Scope::Global, relative)
        } else if let Some(project) = path.strip_prefix("projects/") {
            let (id, relative) = project
                .split_once('/')
                .ok_or_else(|| invalid("knowledge project path is incomplete"))?;
            let id: ProjectId = id.parse()?;
            if !manifest.projects.contains_key(&id) {
                return Err(Error::UnknownProject(id));
            }
            (Scope::Project(id), relative)
        } else {
            return Err(invalid(format!("path is outside knowledge scopes: {path}")));
        };
        let resource = Self {
            scope,
            path: relative.into(),
        };
        resource.full_path(manifest)?;
        Ok(resource)
    }
}

/// One document: the hash and size of its content blob.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    pub hash: Hash,
    pub size: usize,
}

/// One snapshot. Its identity is the hash of its canonical encoding.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    /// None for the first snapshot, two for an integration.
    pub parents: Vec<Snapshot>,
    pub store: StoreId,
    /// Shared project identities and their display names.
    pub projects: BTreeMap<ProjectId, String>,
    /// Documents by full path: `global/...` or `projects/<id>/...`.
    pub entries: BTreeMap<String, Entry>,
}

impl Manifest {
    /// Validate and encode; the blob's hash is the snapshot identity.
    pub(crate) fn encode(&self) -> Result<Blob> {
        self.validate()?;
        let mut content = serde_json::to_vec(self).map_err(|source| Error::Json {
            operation: "knowledge manifest",
            source,
        })?;
        content.push(b'\n');
        Blob::new(content).map_err(|error| match error {
            dots_substrate::Error::TooLarge { .. } => {
                invalid("knowledge manifest exceeds the blob size limit")
            }
            error => error.into(),
        })
    }

    /// Decode a manifest blob, accepting only its canonical, valid encoding.
    pub(crate) fn decode(blob: &Blob) -> Result<Self> {
        let manifest: Self =
            serde_json::from_slice(blob.content()).map_err(|source| Error::Json {
                operation: "knowledge manifest",
                source,
            })?;
        if manifest.encode()?.content() != blob.content() {
            return Err(invalid("knowledge manifest is not canonical JSON"));
        }
        Ok(manifest)
    }

    pub(crate) fn validate(&self) -> Result<()> {
        if self.parents.len() > 2 || (self.parents.len() == 2 && self.parents[0] == self.parents[1])
        {
            return Err(invalid("a snapshot has at most two distinct parents"));
        }
        for name in self.projects.values() {
            validate_name(name)?;
        }
        validate_layout(&layout(&self.entries), self)
    }

    pub(crate) fn resource(&self, scope: &Scope, relative: &Path) -> Result<String> {
        Resource {
            scope: scope.clone(),
            path: relative.into(),
        }
        .full_path(self)
    }
}

pub(crate) fn validate_name(name: &str) -> Result<()> {
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

pub(crate) fn validate_content(content: &[u8]) -> Result<()> {
    if content.len() > MAX_FILE_BYTES {
        return Err(invalid("knowledge document exceeds 256 KiB"));
    }
    if content.contains(&0) || std::str::from_utf8(content).is_err() {
        return Err(invalid("knowledge content must be UTF-8 without NUL bytes"));
    }
    Ok(())
}

pub(crate) fn layout(entries: &BTreeMap<String, Entry>) -> BTreeMap<String, usize> {
    entries
        .iter()
        .map(|(path, entry)| (path.clone(), entry.size))
        .collect()
}

pub(crate) fn validate_layout(
    entries: &BTreeMap<String, usize>,
    manifest: &Manifest,
) -> Result<()> {
    if entries.len() > MAX_DOCUMENTS {
        return Err(invalid("knowledge corpus contains too many documents"));
    }
    let mut total = 0_usize;
    for (path, &size) in entries {
        total = total
            .checked_add(size)
            .ok_or_else(|| invalid("knowledge size overflow"))?;
        if size > MAX_FILE_BYTES || total > MAX_CORPUS_BYTES {
            return Err(invalid("knowledge corpus exceeds storage bounds"));
        }
        Resource::from_path(path, manifest)?;
    }
    if !colliding_paths(entries.keys().map(String::as_str)).is_empty() {
        return Err(invalid(
            "knowledge paths have a file/directory or case ambiguity",
        ));
    }
    Ok(())
}

/// Return every document involved in a directory, file or component-case collision.
/// Include unresolved document paths to account for every possible resolution.
pub(crate) fn colliding_paths<'a>(paths: impl IntoIterator<Item = &'a str>) -> BTreeSet<&'a str> {
    let mut components: BTreeMap<String, BTreeMap<&str, BTreeSet<&str>>> = BTreeMap::new();
    for path in paths {
        let mut component = path;
        loop {
            components
                .entry(component.to_lowercase())
                .or_default()
                .entry(component)
                .or_default()
                .insert(path);
            let Some((parent, _)) = component.rsplit_once('/') else {
                break;
            };
            component = parent;
        }
    }
    let mut conflicts = BTreeSet::new();
    for spellings in components.values() {
        let owners: BTreeSet<_> = spellings.values().flatten().copied().collect();
        let names_file = spellings
            .iter()
            .any(|(component, paths)| paths.contains(component));
        if spellings.len() > 1 || (names_file && owners.len() > 1) {
            conflicts.extend(owners);
        }
    }
    conflicts
}
