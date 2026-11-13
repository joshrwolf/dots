//! Tree updates the remote refused because another machine took the tree
//! over first. The substrate keeps each as a stranded entry; this compares
//! it with what the remote holds, so the user sees what never published.
use std::collections::{BTreeMap, BTreeSet};
use std::time::UNIX_EPOCH;

use dots_substrate::{Hash, Substrate};
use serde::Serialize;

use super::format::{Manifest, Segment, TaskRecord, decode};
use crate::store::Error;

/// One refused tree update and what it held that the remote does not.
#[derive(Debug, Serialize)]
pub struct StrandedTree {
    pub shared_project: String,
    pub root: String,
    /// When the remote refused it, as a Unix timestamp.
    pub at: i64,
    /// The machine that homes the tree on the remote now.
    pub home: Option<String>,
    /// The refused manifest, kept in this machine's substrate cache.
    pub manifest: Hash,
    /// Tasks whose refused record differs from the remote's, or that the
    /// remote lacks.
    pub tasks: Vec<UnpublishedTask>,
    /// Events the remote lacks, oldest first.
    pub events: Vec<UnpublishedEvent>,
    /// Outputs whose refused records differ.
    pub outputs: Vec<String>,
    /// Set when the refused records could not be read back.
    pub error: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct UnpublishedTask {
    pub id: String,
    pub title: String,
    pub status: String,
}

#[derive(Debug, Serialize)]
pub struct UnpublishedEvent {
    pub uid: String,
    pub task: String,
    pub at: i64,
    pub kind: String,
    pub detail: Option<String>,
}

/// Every stranded tree update on this machine, newest last.
pub fn stranded(substrate: &Substrate) -> Result<Vec<StrandedTree>, Error> {
    let mut trees = Vec::new();
    for entry in substrate.status()?.stranded {
        let mut parts = entry.name.as_str().splitn(3, '/');
        let (Some("tasks"), Some(shared), Some(root)) = (parts.next(), parts.next(), parts.next())
        else {
            continue;
        };
        let mut tree = StrandedTree {
            shared_project: shared.to_owned(),
            root: root.to_owned(),
            at: entry
                .at
                .duration_since(UNIX_EPOCH)
                .map_or(0, |age| i64::try_from(age.as_secs()).unwrap_or(i64::MAX)),
            home: None,
            manifest: entry.root,
            tasks: Vec::new(),
            events: Vec::new(),
            outputs: Vec::new(),
            error: None,
        };
        if let Err(error) = compare(substrate, &entry.name, &mut tree) {
            tree.error = Some(super::import::describe(&error));
        }
        trees.push(tree);
    }
    Ok(trees)
}

fn compare(
    substrate: &Substrate,
    name: &dots_substrate::RefName,
    tree: &mut StrandedTree,
) -> Result<(), Error> {
    let refused: Manifest = decode(&substrate.blob(&tree.manifest)?, "tree manifest")?;
    let remote = match substrate.read(name)? {
        Some(value) => Some(decode::<Manifest>(
            &substrate.blob(&value.root)?,
            "tree manifest",
        )?),
        None => None,
    };
    tree.home = remote.as_ref().map(|remote| remote.home.to_string());
    let differs = |ours: &BTreeMap<String, Hash>, theirs: Option<&BTreeMap<String, Hash>>| {
        ours.iter()
            .filter(|(id, hash)| theirs.and_then(|theirs| theirs.get(*id)) != Some(*hash))
            .map(|(id, hash)| (id.clone(), *hash))
            .collect::<Vec<_>>()
    };
    for (_, hash) in differs(&refused.tasks, remote.as_ref().map(|r| &r.tasks)) {
        let record: TaskRecord = decode(&substrate.blob(&hash)?, "task record")?;
        tree.tasks.push(UnpublishedTask {
            id: record.id,
            title: record.title,
            status: record.status,
        });
    }
    tree.outputs = differs(&refused.outputs, remote.as_ref().map(|r| &r.outputs))
        .into_iter()
        .map(|(id, _)| id)
        .collect();
    // Shared segments hold the same events; read only the others.
    let shared: BTreeSet<Hash> = remote
        .as_ref()
        .map(|remote| remote.segments.iter().copied().collect())
        .unwrap_or_default();
    let mut published = BTreeSet::new();
    if let Some(remote) = &remote {
        for hash in remote
            .segments
            .iter()
            .filter(|hash| !refused.segments.contains(hash))
        {
            let segment: Segment = decode(&substrate.blob(hash)?, "event segment")?;
            published.extend(segment.events.into_iter().map(|event| event.uid));
        }
    }
    for hash in refused
        .segments
        .iter()
        .filter(|hash| !shared.contains(hash))
    {
        let segment: Segment = decode(&substrate.blob(hash)?, "event segment")?;
        tree.events.extend(
            segment
                .events
                .into_iter()
                .filter(|event| !published.contains(&event.uid))
                .map(|event| UnpublishedEvent {
                    uid: event.uid,
                    task: event.task,
                    at: event.at,
                    kind: event.kind,
                    detail: event.detail,
                }),
        );
    }
    Ok(())
}
