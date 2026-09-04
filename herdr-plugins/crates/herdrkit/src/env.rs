//! The `HERDR_*` variables herdr injects into every plugin command.

use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::api::{AgentStatus, WorkspaceWorktree};
use crate::id::{PaneId, TabId, WorkspaceId};
use crate::{Client, Error, Result};

fn optional_string(name: &'static str) -> Result<Option<String>> {
    match std::env::var(name) {
        Ok(value) => Ok((!value.is_empty()).then_some(value)),
        Err(std::env::VarError::NotPresent) => Ok(None),
        Err(std::env::VarError::NotUnicode(value)) => Err(Error::NonUnicodeEnv { name, value }),
    }
}

fn string(name: &'static str) -> Result<String> {
    optional_string(name)?.ok_or(Error::MissingEnv { name })
}

fn optional_path(name: &'static str) -> Option<PathBuf> {
    std::env::var_os(name)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

fn path(name: &'static str) -> Result<PathBuf> {
    optional_path(name).ok_or(Error::MissingEnv { name })
}

pub(crate) fn socket_path() -> Result<PathBuf> {
    path("HERDR_SOCKET_PATH")
}

pub(crate) fn effective_target_dir<'a>(
    checkout: Option<&'a Path>,
    pane_cwd: Option<&'a Path>,
    workspace_cwd: Option<&'a Path>,
) -> Option<&'a Path> {
    [checkout, pane_cwd, workspace_cwd]
        .into_iter()
        .flatten()
        .find(|path| !path.as_os_str().is_empty())
}

/// Where the invocation came from. A popup exports no pane id, so this is the
/// only route back to the pane the user was sitting in.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct InvocationContext {
    #[serde(default)]
    invocation_source: Option<String>,
    #[serde(default)]
    correlation_id: Option<String>,
    #[serde(default)]
    link_handler_id: Option<String>,
    #[serde(default)]
    workspace_id: Option<WorkspaceId>,
    #[serde(default)]
    workspace_label: Option<String>,
    #[serde(default)]
    workspace_cwd: Option<PathBuf>,
    #[serde(default)]
    tab_id: Option<TabId>,
    #[serde(default)]
    tab_label: Option<String>,
    #[serde(default)]
    focused_pane_id: Option<PaneId>,
    #[serde(default)]
    focused_pane_cwd: Option<PathBuf>,
    #[serde(default)]
    focused_pane_status: Option<AgentStatus>,
    #[serde(default)]
    #[serde(rename = "focused_pane_agent")]
    focused_agent_kind: Option<String>,
    #[serde(default)]
    selected_text: Option<String>,
    #[serde(default)]
    clicked_url: Option<String>,
    #[serde(default)]
    worktree: Option<WorkspaceWorktree>,
}

impl InvocationContext {
    pub(crate) fn from_env() -> Result<Self> {
        serde_json::from_str(&string("HERDR_PLUGIN_CONTEXT_JSON")?)
            .map_err(Error::InvalidPluginContext)
    }

    pub fn invocation_source(&self) -> Option<&str> {
        self.invocation_source.as_deref()
    }

    pub fn correlation_id(&self) -> Option<&str> {
        self.correlation_id.as_deref()
    }

    pub fn link_handler_id(&self) -> Option<&str> {
        self.link_handler_id.as_deref()
    }

    pub fn workspace_id(&self) -> Option<&WorkspaceId> {
        self.workspace_id.as_ref()
    }

    pub fn workspace_label(&self) -> Option<&str> {
        self.workspace_label.as_deref()
    }

    pub fn workspace_cwd(&self) -> Option<&Path> {
        self.workspace_cwd.as_deref()
    }

    pub fn tab_id(&self) -> Option<&TabId> {
        self.tab_id.as_ref()
    }

    pub fn tab_label(&self) -> Option<&str> {
        self.tab_label.as_deref()
    }

    pub fn focused_pane_id(&self) -> Option<&PaneId> {
        self.focused_pane_id.as_ref()
    }

    pub fn focused_pane_cwd(&self) -> Option<&Path> {
        self.focused_pane_cwd.as_deref()
    }

    pub fn focused_pane_status(&self) -> Option<AgentStatus> {
        self.focused_pane_status
    }

    pub fn focused_agent_kind(&self) -> Option<&str> {
        self.focused_agent_kind.as_deref()
    }

    pub fn selected_text(&self) -> Option<&str> {
        self.selected_text.as_deref()
    }

    pub fn clicked_url(&self) -> Option<&str> {
        self.clicked_url.as_deref()
    }

    pub fn worktree(&self) -> Option<&WorkspaceWorktree> {
        self.worktree.as_ref()
    }

    /// The directory this invocation is about: the checkout the workspace is
    /// in, so a pane sitting in a subdirectory still resolves, and so a linked
    /// worktree resolves to itself rather than to the main clone it was made
    /// from.
    ///
    /// There is deliberately no `$PWD` fallback. Runtime commands run with the
    /// plugin directory as cwd, and that directory is inside the dotfiles
    /// checkout, where git discovery *succeeds* and answers confidently about
    /// the wrong repository.
    pub fn target_dir(&self) -> Option<&Path> {
        effective_target_dir(
            self.worktree
                .as_ref()
                .map(|worktree| worktree.checkout_path.as_path()),
            self.focused_pane_cwd.as_deref(),
            self.workspace_cwd.as_deref(),
        )
    }
}

/// The manifest entry that caused Herdr to launch a plugin process.
///
/// Link handlers are deliberately not a variant. A link handler invokes an
/// ordinary action and is recorded independently by
/// [`Invocation::link_handler_id`].
#[derive(Debug, Clone, PartialEq)]
pub enum InvocationKind {
    Action {
        id: String,
    },
    PaneEntrypoint {
        id: String,
    },
    Startup,
    Event {
        name: String,
        payload: serde_json::Value,
    },
}

impl InvocationKind {
    /// Reads only the environment variables that identify the manifest entry.
    /// This is independent of the socket and invocation context, so dispatch
    /// can be parsed and tested without constructing a full [`Invocation`].
    pub fn from_env() -> Result<Self> {
        classify(
            optional_string("HERDR_PLUGIN_ACTION_ID")?,
            optional_string("HERDR_PLUGIN_ENTRYPOINT_ID")?,
            optional_string("HERDR_PLUGIN_EVENT")?,
            optional_string("HERDR_PLUGIN_EVENT_JSON")?,
        )
    }

    pub fn action_id(&self) -> Option<&str> {
        match self {
            Self::Action { id } => Some(id),
            _ => None,
        }
    }

    pub fn entrypoint_id(&self) -> Option<&str> {
        match self {
            Self::PaneEntrypoint { id } => Some(id),
            _ => None,
        }
    }
}

fn classify(
    action: Option<String>,
    entrypoint: Option<String>,
    event: Option<String>,
    event_json: Option<String>,
) -> Result<InvocationKind> {
    let mut present = Vec::new();
    if action.is_some() {
        present.push("HERDR_PLUGIN_ACTION_ID");
    }
    if entrypoint.is_some() {
        present.push("HERDR_PLUGIN_ENTRYPOINT_ID");
    }
    if event.is_some() {
        present.push("HERDR_PLUGIN_EVENT");
    }
    if present.len() > 1 || (event.is_none() && event_json.is_some()) {
        if event.is_none() && event_json.is_some() {
            present.push("HERDR_PLUGIN_EVENT_JSON");
        }
        return Err(Error::ConflictingInvocation {
            fields: present.join(", "),
        });
    }

    if let Some(id) = action {
        return Ok(InvocationKind::Action { id });
    }
    if let Some(id) = entrypoint {
        return Ok(InvocationKind::PaneEntrypoint { id });
    }
    if let Some(name) = event {
        let raw = event_json.ok_or(Error::MissingEnv {
            name: "HERDR_PLUGIN_EVENT_JSON",
        })?;
        let payload = serde_json::from_str(&raw).map_err(Error::InvalidPluginEvent)?;
        return Ok(InvocationKind::Event { name, payload });
    }
    Ok(InvocationKind::Startup)
}

/// Everything Herdr injected for one plugin invocation, normalized once.
///
/// Direct variables win over their context equivalents because they describe
/// the concrete command Herdr launched. Popups legitimately omit direct pane
/// and tab variables, so those fall back to the underlying invocation context.
#[derive(Debug, Clone)]
pub struct Invocation {
    client: Client,
    context: InvocationContext,
    kind: InvocationKind,
    plugin_id: String,
    plugin_root: PathBuf,
    config_dir: PathBuf,
    state_dir: PathBuf,
    link_handler_id: Option<String>,
    clicked_url: Option<String>,
    workspace_id: Option<WorkspaceId>,
    tab_id: Option<TabId>,
    pane_id: Option<PaneId>,
}

impl Invocation {
    /// Loads a manifest-launched plugin invocation.
    pub fn load() -> Result<Self> {
        let kind = InvocationKind::from_env()?;
        let context = InvocationContext::from_env()?;
        let workspace_id = optional_string("HERDR_WORKSPACE_ID")?
            .map(WorkspaceId::from)
            .or_else(|| context.workspace_id.clone());
        let tab_id = optional_string("HERDR_TAB_ID")?
            .map(TabId::from)
            .or_else(|| context.tab_id.clone());
        let pane_id = optional_string("HERDR_PANE_ID")?
            .map(PaneId::from)
            .or_else(|| context.focused_pane_id.clone());
        let clicked_url =
            optional_string("HERDR_PLUGIN_CLICKED_URL")?.or_else(|| context.clicked_url.clone());
        let link_handler_id = optional_string("HERDR_PLUGIN_LINK_HANDLER_ID")?
            .or_else(|| context.link_handler_id.clone());

        Ok(Self {
            client: Client::from_env()?,
            context,
            kind,
            plugin_id: string("HERDR_PLUGIN_ID")?,
            plugin_root: path("HERDR_PLUGIN_ROOT")?,
            config_dir: path("HERDR_PLUGIN_CONFIG_DIR")?,
            state_dir: path("HERDR_PLUGIN_STATE_DIR")?,
            link_handler_id,
            clicked_url,
            workspace_id,
            tab_id,
            pane_id,
        })
    }

    pub fn client(&self) -> &Client {
        &self.client
    }

    pub fn context(&self) -> &InvocationContext {
        &self.context
    }

    pub fn kind(&self) -> &InvocationKind {
        &self.kind
    }

    pub fn plugin_id(&self) -> &str {
        &self.plugin_id
    }

    /// Managed and replaced on reinstall. Never persist anything here.
    pub fn plugin_root(&self) -> &Path {
        &self.plugin_root
    }

    pub fn config_dir(&self) -> &Path {
        &self.config_dir
    }

    pub fn state_dir(&self) -> &Path {
        &self.state_dir
    }

    pub fn link_handler_id(&self) -> Option<&str> {
        self.link_handler_id.as_deref()
    }

    pub fn clicked_url(&self) -> Option<&str> {
        self.clicked_url.as_deref()
    }

    pub fn workspace_id(&self) -> Option<&WorkspaceId> {
        self.workspace_id.as_ref()
    }

    pub fn tab_id(&self) -> Option<&TabId> {
        self.tab_id.as_ref()
    }

    /// The focused pane, including the pane beneath a popup.
    pub fn pane_id(&self) -> Option<&PaneId> {
        self.pane_id.as_ref()
    }

    pub fn target_dir(&self) -> Option<&Path> {
        self.context.target_dir()
    }

    pub fn require_target_dir(&self) -> Result<&Path> {
        self.target_dir().ok_or(Error::MissingInvocationField {
            field: "target directory",
        })
    }

    pub fn require_workspace_id(&self) -> Result<&WorkspaceId> {
        self.workspace_id().ok_or(Error::MissingInvocationField {
            field: "workspace id",
        })
    }

    pub fn require_tab_id(&self) -> Result<&TabId> {
        self.tab_id()
            .ok_or(Error::MissingInvocationField { field: "tab id" })
    }

    pub fn require_pane_id(&self) -> Result<&PaneId> {
        self.pane_id()
            .ok_or(Error::MissingInvocationField { field: "pane id" })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repo_root_wins_over_a_pane_deep_inside_it() {
        let ctx: InvocationContext = serde_json::from_str(
            r#"{"focused_pane_cwd":"/repo/sub/dir",
                "worktree":{"repo_key":"k","repo_name":"r","repo_root":"/repo",
                            "checkout_path":"/repo","is_linked_worktree":false}}"#,
        )
        .unwrap();
        assert_eq!(ctx.target_dir(), Some(Path::new("/repo")));
    }

    /// `repo_root` is the main clone; a linked worktree lives somewhere else
    /// entirely, and a picker rooted at `repo_root` would index the wrong tree.
    #[test]
    fn a_linked_worktree_resolves_to_its_own_checkout_not_the_main_clone() {
        let ctx: InvocationContext = serde_json::from_str(
            r#"{"focused_pane_cwd":"/wt/service/feature/src",
                "worktree":{"repo_key":"gh:example/service","repo_name":"service","repo_root":"/src/service",
                            "checkout_path":"/wt/service/feature","is_linked_worktree":true}}"#,
        )
        .unwrap();
        assert_eq!(ctx.target_dir(), Some(Path::new("/wt/service/feature")));
    }

    #[test]
    fn no_directory_at_all_is_none_rather_than_cwd() {
        let ctx: InvocationContext = serde_json::from_str(r#"{"workspace_cwd":""}"#).unwrap();
        assert_eq!(ctx.target_dir(), None);
    }

    #[test]
    fn an_empty_worktree_path_does_not_hide_a_usable_pane_path() {
        let ctx: InvocationContext = serde_json::from_str(
            r#"{"focused_pane_cwd":"/repo/subdir",
                "worktree":{"repo_key":"gh:example/repo","repo_name":"repo",
                            "repo_root":"/repo","checkout_path":"",
                            "is_linked_worktree":false}}"#,
        )
        .unwrap();
        assert_eq!(ctx.target_dir(), Some(Path::new("/repo/subdir")));
    }

    #[test]
    fn invocation_identity_is_exhaustive_without_a_client_or_context() {
        assert_eq!(
            classify(Some("open".into()), None, None, None).unwrap(),
            InvocationKind::Action { id: "open".into() }
        );
        assert_eq!(
            classify(None, Some("picker".into()), None, None).unwrap(),
            InvocationKind::PaneEntrypoint {
                id: "picker".into()
            }
        );
        assert_eq!(
            classify(None, None, None, None).unwrap(),
            InvocationKind::Startup
        );
        assert_eq!(
            classify(
                None,
                None,
                Some("pane.closed".into()),
                Some(r#"{"pane_id":"w1:p1"}"#.into())
            )
            .unwrap(),
            InvocationKind::Event {
                name: "pane.closed".into(),
                payload: serde_json::json!({"pane_id": "w1:p1"})
            }
        );
    }

    #[test]
    fn conflicting_or_incomplete_identity_is_rejected() {
        assert!(matches!(
            classify(Some("open".into()), Some("picker".into()), None, None),
            Err(Error::ConflictingInvocation { .. })
        ));
        assert!(matches!(
            classify(None, None, Some("pane.closed".into()), None),
            Err(Error::MissingEnv {
                name: "HERDR_PLUGIN_EVENT_JSON"
            })
        ));
        assert!(matches!(
            classify(None, None, None, Some("{}".into())),
            Err(Error::ConflictingInvocation { .. })
        ));
    }
}
