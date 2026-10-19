//! Checkout-scoped context shared by agent and editor surfaces.

use std::path::PathBuf;

use anyhow::{Context as _, Result};
use herdrkit::WorkspaceId;
use review_core::{BindingState, Repository, Store};
use serde::Serialize;

#[derive(Debug, Serialize)]
pub(crate) struct ReviewContext {
    pub state: BindingState,
    pub checkout_root: PathBuf,
    pub common_git_dir: PathBuf,
    pub head_oid: String,
    pub working_tree_dirty: bool,
}

pub(crate) fn load(
    store: &mut Store,
    server: &str,
    workspace: &WorkspaceId,
    cursor: Option<&review_core::BindingCursor>,
) -> Result<Option<ReviewContext>> {
    let Some(binding) = store.current_runtime_binding(server, workspace)? else {
        return Ok(None);
    };
    let state = store.binding_state_page(&binding.id, cursor)?;
    let repository = Repository::discover(std::path::Path::new(&binding.checkout_root))
        .context("inspecting the bound review checkout")?;
    Ok(Some(ReviewContext {
        checkout_root: repository.checkout_root().to_path_buf(),
        common_git_dir: repository.common_git_dir().to_path_buf(),
        head_oid: repository.head_oid()?,
        working_tree_dirty: !repository.checkout_status()?.is_empty(),
        state,
    }))
}
