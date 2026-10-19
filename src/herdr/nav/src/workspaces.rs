//! An alternate workspace, independent of pane jumplist traversal.
use std::collections::HashSet;

use anyhow::{Result, ensure};
use herdrkit::WorkspaceId;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Workspaces {
    pub(crate) current: Option<WorkspaceId>,
    pub(crate) previous: Option<WorkspaceId>,
    pub(crate) pending: Option<WorkspaceId>,
}

impl Workspaces {
    pub(crate) fn validate(&self) -> Result<()> {
        ensure!(
            self.current
                .iter()
                .chain(&self.previous)
                .all(|id| !id.as_str().is_empty()),
            "workspace switch contains an empty identity"
        );
        ensure!(
            self.previous.is_none() || self.previous != self.current,
            "previous workspace is the current workspace"
        );
        ensure!(
            self.pending.is_none() || self.pending == self.previous,
            "pending workspace is not the previous workspace"
        );
        Ok(())
    }

    pub(crate) fn observe(&mut self, current: Option<&WorkspaceId>, live: &HashSet<&WorkspaceId>) {
        // Resolve uncertainty from actual focus, never by replaying the request.
        self.pending = None;
        if self.current.as_ref().is_some_and(|id| !live.contains(id)) {
            self.current = None;
        }
        if self.previous.as_ref().is_some_and(|id| !live.contains(id)) {
            self.previous = None;
        }
        if let Some(current) = current
            && self.current.as_ref() != Some(current)
        {
            if self.previous.as_ref() == Some(current) {
                self.previous = None;
            }
            if let Some(departure) = self.current.replace(current.clone()) {
                self.previous = Some(departure);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn observations_toggle_only_on_workspace_changes() {
        let [a, b, c] = ["a", "b", "c"].map(WorkspaceId::new);
        let live = HashSet::from([&a, &b, &c]);
        let mut state = Workspaces::default();
        for id in [&a, &a, &b, &b, &a, &b] {
            state.observe(Some(id), &live);
        }
        assert_eq!(state.current, Some(b.clone()));
        assert_eq!(state.previous, Some(a.clone()));
        state.observe(Some(&c), &live);
        assert_eq!(state.previous, Some(b));
    }

    #[test]
    fn closures_prune_without_inventing_a_previous_workspace() {
        let [a, b, c] = ["a", "b", "c"].map(WorkspaceId::new);
        let mut state = Workspaces {
            current: Some(b.clone()),
            previous: Some(a),
            pending: None,
        };
        state.observe(Some(&b), &HashSet::from([&b, &c]));
        assert_eq!(state.previous, None);
        state.observe(Some(&c), &HashSet::from([&c]));
        assert_eq!(state.current, Some(c));
        assert_eq!(state.previous, None);
        state.validate().unwrap();
    }
}
