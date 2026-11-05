//! Quarantined transfer from enrolled trusted producers; not a hostile-pack sandbox.
use std::collections::BTreeSet;
use std::io::Write as _;
use std::path::Path;
use std::time::Duration;

use dots_documents::staged_directory::StagedDirectory;
use dots_documents::staged_file::StagedFile;
use dots_process::Runner;
use serde::{Deserialize, Serialize};

use crate::git::Git;
use crate::store::{Identity, Store};
use crate::{MAX_METADATA_BYTES, Result, Snapshot};

pub const MAX_BUNDLE_BYTES: usize = 128 * 1024 * 1024;
const TRANSFER_DEADLINE: Duration = Duration::from_secs(60);

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BundleReceipt {
    pub source: Identity,
    pub snapshot: Snapshot,
    pub digest: String,
}

/// A validated retained candidate, not a published knowledge snapshot.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Candidate {
    pub source: Identity,
    pub snapshot: Snapshot,
}

#[derive(Debug, Serialize)]
pub struct CandidateRead {
    pub candidate: Candidate,
    pub scope: crate::Scope,
    pub document: dots_documents::Document,
}

impl Candidate {
    pub(crate) fn transfer_reference(&self, recipient: &Identity) -> String {
        format!(
            "refs/dots/transfer/{}/{}/{}",
            recipient.node, self.source.node, self.snapshot
        )
    }

    pub(crate) fn reference(&self) -> String {
        format!("refs/dots/incoming/{}/{}", self.source.node, self.snapshot)
    }

    pub(crate) fn view<'store>(&self, store: &'store Store) -> Result<crate::View<'store>> {
        let identity = store.identity();
        if self.source.store != identity.store || self.source.genesis != identity.genesis {
            return Err(crate::invalid(
                "candidate belongs to another knowledge store",
            ));
        }
        if store.git().reference(&self.reference())? != Some(self.snapshot.clone()) {
            return Err(crate::invalid("candidate is not retained in this store"));
        }
        crate::View::load(store.git(), &self.snapshot, &identity.store)
    }

    /// Inspect an explicitly selected candidate; ordinary memory reads remain published-only.
    pub fn inspect(
        &self,
        store: &Store,
        resource: &crate::Resource,
        start: usize,
        lines: usize,
    ) -> Result<CandidateRead> {
        let read = self
            .view(store)?
            .read(&resource.scope, &resource.path, start, lines)?;
        Ok(CandidateRead {
            candidate: self.clone(),
            scope: read.scope,
            document: read.document,
        })
    }

    pub(crate) fn import_bundle(
        store: &Store,
        path: &Path,
        receipt: &BundleReceipt,
        known: &BTreeSet<Snapshot>,
    ) -> Result<Self> {
        let identity = store.identity();
        if identity.store != receipt.source.store || identity.genesis != receipt.source.genesis {
            return Err(crate::invalid("bundle belongs to another knowledge store"));
        }
        if receipt.digest.strip_prefix("sha256:").is_none_or(|hash| {
            hash.len() != 64
                || !hash
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        }) {
            return Err(crate::invalid(
                "bundle requires its exact SHA-256 export digest",
            ));
        }
        let bytes = dots_documents::text::read_bytes(path, MAX_BUNDLE_BYTES)?
            .ok_or_else(|| crate::invalid("bundle file is missing"))?;
        if dots_documents::text::bytes_revision(&bytes) != receipt.digest {
            return Err(crate::invalid(
                "bundle bytes do not match the export digest",
            ));
        }
        let stage = StagedDirectory::create(store.root())?;
        let copy = StagedFile::create(stage.path())?;
        copy.file()
            .write_all(&bytes)
            .map_err(|error| crate::io("copy incoming bundle", copy.path(), error))?;
        copy.file()
            .sync_all()
            .map_err(|error| crate::io("sync incoming bundle", copy.path(), error))?;
        let git = Git::new(stage.path().to_owned(), "quarantine".into());
        git.initialize()?;
        let command = {
            let mut command = git.command(&["bundle", "verify"])?;
            command.arg(copy.path());
            command
        };
        Git::checked(
            Git::execute(
                command,
                "verify full knowledge bundle",
                Runner::new(TRANSFER_DEADLINE, MAX_METADATA_BYTES),
            )?,
            "verify full knowledge bundle",
        )?;
        let mut command = git.command(&["bundle", "list-heads"])?;
        command.arg(copy.path());
        let heads = Git::checked(
            Git::execute(
                command,
                "read bundle snapshot",
                Runner::new(TRANSFER_DEADLINE, MAX_METADATA_BYTES),
            )?,
            "read bundle snapshot",
        )?;
        let heads =
            String::from_utf8(heads).map_err(|_| crate::invalid("non-UTF-8 bundle references"))?;
        let reference = format!("refs/dots/export/{}", receipt.snapshot);
        if heads != format!("{} {reference}\n", receipt.snapshot) {
            return Err(crate::invalid(
                "bundle must advertise exactly its declared export snapshot",
            ));
        }
        fetch(&git, copy.path(), &reference, crate::git::HEAD)?;
        if git.head()? != receipt.snapshot {
            return Err(crate::invalid("bundle imported a different snapshot"));
        }
        Self::retain(store, &receipt.source, &receipt.snapshot, &git, known)
    }

    pub(crate) fn retain(
        store: &Store,
        source: &Identity,
        snapshot: &Snapshot,
        git: &Git,
        known: &BTreeSet<Snapshot>,
    ) -> Result<Self> {
        let identity = store.identity();
        if source.store != identity.store
            || source.genesis != identity.genesis
            || git.head()? != *snapshot
        {
            return Err(crate::invalid(
                "candidate identity or snapshot differs from the transfer",
            ));
        }
        crate::history::validate(git, &identity, snapshot, known)?;
        let candidate = Self {
            source: source.clone(),
            snapshot: snapshot.clone(),
        };
        if let Some(existing) = store.git().reference(&candidate.reference())? {
            if existing != candidate.snapshot {
                return Err(crate::invalid(
                    "retained candidate reference changed unexpectedly",
                ));
            }
        } else {
            fetch(
                store.git(),
                &git.repository(),
                crate::git::HEAD,
                &candidate.reference(),
            )?;
        }
        if store.git().reference(&candidate.reference())? != Some(candidate.snapshot.clone()) {
            return Err(crate::invalid("incoming candidate was not retained"));
        }
        Ok(candidate)
    }
}

pub(crate) fn fetch(git: &Git, source: &Path, reference: &str, destination: &str) -> Result<()> {
    let mapping = format!("{reference}:{destination}");
    let mut command = git.command(&[
        "-c",
        "pack.allowPackReuse=false",
        "fetch",
        "--quiet",
        "--no-tags",
        "--no-auto-maintenance",
        "--no-write-fetch-head",
        "--refmap=",
        "--",
    ])?;
    command.arg(source).arg(mapping);
    Git::checked(
        Git::execute(
            command,
            "stage knowledge candidate",
            Runner::new(TRANSFER_DEADLINE, MAX_METADATA_BYTES),
        )?,
        "stage knowledge candidate",
    )?;
    Ok(())
}
