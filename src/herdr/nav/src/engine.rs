//! Reconcile observations and commit traversal only after reading focus back.
use std::collections::HashSet;
use std::path::Path;

use anyhow::{Context as _, Result, bail, ensure};
use herdrkit::api::NavigationSnapshot;
use herdrkit::state_file::StateFile;
use herdrkit::{Client, TerminalId};
use serde::{Deserialize, Serialize};

use crate::history::{History, Travel};
use crate::workspaces::Workspaces;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Checkpoint {
    version: u32,
    history: History,
    pending: Option<TerminalId>,
    // Version-one pane checkpoints predate the workspace toggle.
    #[serde(default)]
    workspaces: Workspaces,
}

impl Default for Checkpoint {
    fn default() -> Self {
        Self {
            version: 1,
            history: History::default(),
            pending: None,
            workspaces: Workspaces::default(),
        }
    }
}

impl Checkpoint {
    fn validate(&self) -> Result<()> {
        ensure!(self.version == 1, "unsupported jumplist checkpoint version");
        self.history.validate()?;
        self.workspaces.validate()?;
        if let Some(target) = &self.pending {
            self.history.clone().select(target)?;
        }
        Ok(())
    }
}

#[derive(Debug)]
pub(crate) struct Engine {
    client: Client,
    owner: String,
    file: StateFile,
    checkpoint: Checkpoint,
}

impl Engine {
    pub(crate) fn open(client: Client, directory: &Path) -> Result<Self> {
        let mut engine = Self {
            owner: client
                .server_id()
                .context("identifying navigation server")?,
            client,
            file: StateFile::lock(directory).context("locking jumplist checkpoint")?,
            checkpoint: Checkpoint::default(),
        };
        engine.refresh()?;
        Ok(engine)
    }

    pub(crate) fn status(&self) -> serde_json::Value {
        serde_json::json!({
            "history": self.checkpoint.history,
            "pending": self.checkpoint.pending,
            "workspaces": self.checkpoint.workspaces,
        })
    }

    pub(crate) fn refresh(&mut self) -> Result<NavigationSnapshot> {
        self.check_owner()?;
        // Always reread: a previous rename may have succeeded even when syncing
        // its directory failed. Never mutate from an uncertain in-memory copy.
        let saved = self
            .file
            .read::<Checkpoint>()
            .context("reading jumplist checkpoint")?
            .unwrap_or_default();
        saved.validate()?;
        let snapshot = self
            .client
            .navigation_snapshot()
            .context("reading current navigation")?;
        validate_snapshot(&snapshot)?;
        let mut next = saved.clone();
        let current = focused_terminal(&snapshot);
        if let Some(target) = next.pending.take()
            && current == Some(&target)
        {
            next.history.select(&target)?;
        }
        let live = snapshot
            .panes
            .iter()
            .map(|pane| &pane.terminal_id)
            .collect();
        next.history.retain(&live);
        if let Some(current) = current {
            next.history.observe(current.clone());
        }
        next.workspaces.observe(
            snapshot.focused_workspace_id.as_ref(),
            &snapshot
                .workspaces
                .iter()
                .map(|workspace| &workspace.workspace_id)
                .collect(),
        );
        if next != saved {
            self.file
                .write(&next)
                .context("checkpointing observed navigation")?;
        }
        self.checkpoint = next;
        Ok(snapshot)
    }

    pub(crate) fn travel(&mut self, direction: Travel) -> Result<()> {
        let snapshot = self.refresh()?;
        let Some(target) = self.checkpoint.history.target(direction).cloned() else {
            return Ok(());
        };
        let pane = snapshot
            .panes
            .iter()
            .find(|pane| pane.terminal_id == target)
            .context("jumplist target disappeared from the current snapshot")?;
        self.check_owner()?;
        let mut reserved = self.checkpoint.clone();
        reserved.pending = Some(target.clone());
        self.file
            .write(&reserved)
            .context("reserving jumplist traversal")?;
        self.checkpoint = reserved;

        self.check_owner()?;
        match self.client.pane_focus(&pane.pane_id) {
            Ok(reply) => {
                ensure!(
                    reply.pane_id == pane.pane_id,
                    "focus response named another pane; traversal remains pending"
                );
            }
            Err(error) => {
                if matches!(error, herdrkit::Error::Rejected { .. }) {
                    let mut rejected = self.checkpoint.clone();
                    rejected.pending = None;
                    self.file
                        .write(&rejected)
                        .context("checkpointing rejected traversal")?;
                    self.checkpoint = rejected;
                }
                // An ambiguous error deliberately leaves the reservation for the
                // next read/restart. Recovery never issues another focus call.
                return Err(error).context("focusing jumplist destination");
            }
        }
        let confirmed = self.refresh()?;
        if focused_terminal(&confirmed) != Some(&target) {
            bail!("focus changed during traversal; recorded the actual destination");
        }
        Ok(())
    }

    pub(crate) fn previous_workspace(&mut self) -> Result<()> {
        self.refresh()?;
        let Some(target) = self.checkpoint.workspaces.previous.clone() else {
            return Ok(());
        };
        self.check_owner()?;
        let mut reserved = self.checkpoint.clone();
        reserved.workspaces.pending = Some(target.clone());
        self.file
            .write(&reserved)
            .context("reserving workspace switch")?;
        self.checkpoint = reserved;
        self.check_owner()?;
        if let Err(error) = self.client.workspace_focus(&target) {
            if matches!(error, herdrkit::Error::Rejected { .. }) {
                let mut rejected = self.checkpoint.clone();
                rejected.workspaces.pending = None;
                self.file
                    .write(&rejected)
                    .context("checkpointing rejected workspace switch")?;
                self.checkpoint = rejected;
            }
            return Err(error).context("focusing previous workspace");
        }
        let confirmed = self.refresh()?;
        ensure!(
            confirmed.focused_workspace_id.as_ref() == Some(&target),
            "focus changed during workspace switch; recorded the actual destination"
        );
        Ok(())
    }

    fn check_owner(&self) -> Result<()> {
        ensure!(
            self.client.server_id()? == self.owner,
            "Herdr server was replaced; refusing to use the previous server's jumplist"
        );
        Ok(())
    }
}

fn focused_terminal(snapshot: &NavigationSnapshot) -> Option<&TerminalId> {
    snapshot
        .panes
        .iter()
        .find(|pane| Some(&pane.pane_id) == snapshot.focused_pane_id.as_ref())
        .map(|pane| &pane.terminal_id)
}

fn validate_snapshot(snapshot: &NavigationSnapshot) -> Result<()> {
    let version = snapshot.version.split('-').next().unwrap_or_default();
    let parts = version
        .split('.')
        .map(str::parse::<u32>)
        .collect::<Result<Vec<_>, _>>()?;
    ensure!(
        matches!(parts.as_slice(), [major, minor, patch] if (*major, *minor, *patch) >= (0, 9, 1)),
        "jumplist requires stock Herdr 0.9.1 or newer; restart the server after upgrading"
    );
    let terminals: HashSet<_> = snapshot
        .panes
        .iter()
        .map(|pane| &pane.terminal_id)
        .collect();
    let panes: HashSet<_> = snapshot.panes.iter().map(|pane| &pane.pane_id).collect();
    ensure!(
        terminals.len() == snapshot.panes.len() && panes.len() == snapshot.panes.len(),
        "navigation snapshot contains duplicate identities"
    );
    ensure!(
        snapshot
            .panes
            .iter()
            .all(|pane| !pane.pane_id.as_str().is_empty() && !pane.terminal_id.as_str().is_empty()),
        "navigation snapshot contains an empty identity"
    );
    ensure!(
        snapshot.focused_pane_id.is_none() || focused_terminal(snapshot).is_some(),
        "focused pane is missing from navigation inventory"
    );
    let workspaces: HashSet<_> = snapshot
        .workspaces
        .iter()
        .map(|workspace| &workspace.workspace_id)
        .collect();
    ensure!(
        workspaces.len() == snapshot.workspaces.len()
            && workspaces.iter().all(|id| !id.as_str().is_empty()),
        "navigation snapshot contains duplicate or empty workspace identities"
    );
    ensure!(
        snapshot
            .focused_workspace_id
            .as_ref()
            .is_none_or(|id| workspaces.contains(id))
            && snapshot
                .panes
                .iter()
                .all(|pane| workspaces.contains(&pane.workspace_id)),
        "workspace is missing from navigation inventory"
    );
    ensure!(
        snapshot
            .panes
            .iter()
            .find(|pane| Some(&pane.pane_id) == snapshot.focused_pane_id.as_ref())
            .is_none_or(|pane| Some(&pane.workspace_id) == snapshot.focused_workspace_id.as_ref()),
        "focused pane belongs to a different workspace"
    );
    Ok(())
}

#[cfg(test)]
pub(crate) mod tests;
