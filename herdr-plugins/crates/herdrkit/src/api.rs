//! The subset of herdr's socket API that plugins here actually call.
//!
//! Hand-written rather than generated: the schema describes far more than a
//! plugin touches, and every unmodelled field is one that cannot break.
//! Unknown fields are ignored on purpose, so a herdr release adding one is a
//! no-op; a release *removing* one is caught by the fixture test.
//!
//! Each request type carries its method name, its result tag and its reply
//! type, so the three cannot drift apart. The convenience methods on [`Client`]
//! are one line each and exist for discoverability.

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::id::{PaneId, TabId, WorkspaceId};
use crate::socket::{Query, Request};
use crate::tokens::Tokens;
use crate::{Client, Result};

/// The protocol these types were written against.
pub const PROTOCOL: u32 = 22;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentStatus {
    Idle,
    Working,
    Blocked,
    Done,
    #[serde(other)]
    Unknown,
}

impl AgentStatus {
    /// herdr's own name for the state, for a row that spells it out.
    pub fn as_str(self) -> &'static str {
        match self {
            AgentStatus::Idle => "idle",
            AgentStatus::Working => "working",
            AgentStatus::Blocked => "blocked",
            AgentStatus::Done => "done",
            AgentStatus::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    Left,
    Right,
    Up,
    Down,
}

impl Direction {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Left => "left",
            Self::Right => "right",
            Self::Up => "up",
            Self::Down => "down",
        }
    }
}

impl fmt::Display for Direction {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for Direction {
    type Err = ParseDirectionError;

    fn from_str(value: &str) -> std::result::Result<Self, Self::Err> {
        match value {
            "left" => Ok(Self::Left),
            "right" => Ok(Self::Right),
            "up" => Ok(Self::Up),
            "down" => Ok(Self::Down),
            _ => Err(ParseDirectionError {
                value: value.to_owned(),
            }),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("unknown pane direction {value:?}")]
pub struct ParseDirectionError {
    value: String,
}

// ---------------------------------------------------------------- reply types

#[derive(Debug, Clone, Deserialize)]
pub struct Pong {
    pub version: String,
    pub protocol: u32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Repository {
    #[serde(rename = "repo_key")]
    pub key: String,
    #[serde(rename = "repo_name")]
    pub name: String,
    #[serde(rename = "repo_root")]
    pub root: PathBuf,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WorkspaceWorktree {
    #[serde(flatten)]
    pub repository: Repository,
    pub checkout_path: PathBuf,
    pub is_linked_worktree: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Workspace {
    pub workspace_id: WorkspaceId,
    pub label: String,
    pub focused: bool,
    pub agent_status: AgentStatus,
    #[serde(default)]
    pub worktree: Option<WorkspaceWorktree>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Pane {
    pub pane_id: PaneId,
    pub workspace_id: WorkspaceId,
    pub tab_id: TabId,
    #[serde(default)]
    pub cwd: Option<PathBuf>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Tab {
    pub tab_id: TabId,
    pub workspace_id: WorkspaceId,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Agent {
    pub pane_id: PaneId,
    pub workspace_id: WorkspaceId,
    pub tab_id: TabId,
    pub agent_status: AgentStatus,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default, rename = "agent")]
    pub agent_kind: Option<String>,
    #[serde(default, rename = "display_agent")]
    pub display_agent_kind: Option<String>,
    #[serde(default)]
    pub cwd: Option<PathBuf>,
    #[serde(default)]
    pub terminal_title_stripped: Option<String>,
}

impl Agent {
    /// The name to show: the user-assigned agent name, else the agent kind,
    /// else the pane it lives in — never empty, because a blank cell in a
    /// picker is a row you cannot tell apart from its neighbour.
    pub fn display_name(&self) -> &str {
        self.name
            .as_deref()
            .filter(|name| !name.is_empty())
            .or_else(|| {
                self.display_agent_kind
                    .as_deref()
                    .filter(|name| !name.is_empty())
            })
            .or(self.agent_kind.as_deref())
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| self.pane_id.as_str())
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct Snapshot {
    pub version: String,
    pub protocol: u32,
    pub workspaces: Vec<Workspace>,
    pub tabs: Vec<Tab>,
    pub panes: Vec<Pane>,
    pub agents: Vec<Agent>,
    #[serde(default)]
    pub focused_workspace_id: Option<WorkspaceId>,
    #[serde(default)]
    pub focused_tab_id: Option<TabId>,
    #[serde(default)]
    pub focused_pane_id: Option<PaneId>,
}

impl Snapshot {
    pub fn workspace(&self, id: &WorkspaceId) -> Option<&Workspace> {
        self.workspaces
            .iter()
            .find(|workspace| workspace.workspace_id == *id)
    }

    pub fn workspace_for_checkout(&self, checkout: &Path) -> Option<&Workspace> {
        self.workspaces.iter().find(|workspace| {
            workspace
                .worktree
                .as_ref()
                .is_some_and(|worktree| worktree.checkout_path == checkout)
        })
    }

    /// Distinct repository roots currently represented by Herdr workspaces.
    pub fn repository_roots(&self) -> Vec<&Path> {
        let mut roots = self
            .workspaces
            .iter()
            .filter_map(|workspace| workspace.worktree.as_ref())
            .map(|worktree| worktree.repository.root.as_path())
            .collect::<Vec<_>>();
        roots.sort_unstable();
        roots.dedup();
        roots
    }

    pub fn panes_in_tab(&self, tab: &TabId) -> impl Iterator<Item = &Pane> {
        self.panes.iter().filter(move |pane| pane.tab_id == *tab)
    }

    /// The checkout a workspace represents, falling back to its first pane's
    /// cwd when it is not backed by a Herdr-managed worktree.
    ///
    /// This is the snapshot equivalent of [`crate::InvocationContext::target_dir`]:
    /// both prefer a non-empty checkout path over a non-empty pane cwd.
    pub fn effective_workspace_dir(&self, workspace: &WorkspaceId) -> Option<&Path> {
        let workspace_info = self.workspace(workspace)?;
        let pane_cwd = self
            .panes
            .iter()
            .filter(|pane| pane.workspace_id == *workspace)
            .filter_map(|pane| pane.cwd.as_deref())
            .find(|cwd| !cwd.as_os_str().is_empty());
        crate::env::effective_target_dir(
            workspace_info
                .worktree
                .as_ref()
                .map(|worktree| worktree.checkout_path.as_path()),
            pane_cwd,
            None,
        )
    }
}

#[expect(
    clippy::struct_excessive_bools,
    reason = "mirrors herdr's wire shape; collapsing these would need a translation layer that can only lose information"
)]
#[derive(Debug, Clone, Deserialize)]
pub struct Worktree {
    #[serde(rename = "path")]
    pub checkout_path: PathBuf,
    pub label: String,
    pub is_bare: bool,
    pub is_detached: bool,
    pub is_prunable: bool,
    pub is_linked_worktree: bool,
    #[serde(default)]
    pub branch: Option<String>,
    #[serde(default)]
    pub open_workspace_id: Option<WorkspaceId>,
}

impl Worktree {
    /// A bare or prunable worktree has no checkout to open, and one already
    /// owned by a workspace is reachable as that workspace instead.
    pub fn is_openable(&self) -> bool {
        !self.is_bare
            && !self.is_prunable
            && self
                .open_workspace_id
                .as_ref()
                .is_none_or(|id| id.as_str().is_empty())
    }

    pub fn branch_label(&self) -> &str {
        if self.is_detached {
            "(detached)"
        } else {
            self.branch.as_deref().unwrap_or("-")
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct WorktreeSource {
    #[serde(flatten)]
    pub repository: Repository,
    pub source_checkout_path: PathBuf,
    #[serde(default)]
    pub source_workspace_id: Option<WorkspaceId>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Worktrees {
    pub source: WorktreeSource,
    pub worktrees: Vec<Worktree>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Process {
    pub pid: u32,
    pub name: String,
    #[serde(default)]
    pub argv0: Option<String>,
}

impl Process {
    /// The command name with any directory stripped, so an absolute path or a
    /// wrapper still matches.
    pub fn command(&self) -> &str {
        let argv0 = self.argv0.as_deref().unwrap_or(&self.name);
        argv0.rsplit('/').next().unwrap_or(argv0)
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct ProcessInfo {
    pub pane_id: PaneId,
    #[serde(default)]
    pub foreground_processes: Vec<Process>,
}

impl ProcessInfo {
    /// The first foreground process whose command name satisfies `matches`,
    /// wherever it sits in the group — herdr does not order the list by depth.
    ///
    /// Both the pane a key should be forwarded to and the pane an editor is
    /// running in are this question, so the basename handling lives here once.
    pub fn find(&self, matches: impl Fn(&str) -> bool) -> Option<&Process> {
        self.foreground_processes
            .iter()
            .find(|process| matches(process.command()))
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct Neighbor {
    #[serde(default)]
    pub neighbor_pane_id: Option<PaneId>,
}

// -------------------------------------------------------------- request types

/// Pairs a request type with its method, its result tag and its reply.
///
/// The `blocking` form is for a request the server holds open for the caller's
/// own `timeout_ms`. It reads that field to set the socket deadline, so the
/// server's timeout is the one that fires — the default 3s would otherwise cut
/// off a 60s wait and report a timeout against the wrong clock.
macro_rules! request {
    ($ty:ty, $method:literal, blocking, $tag:literal => $reply:ty) => {
        impl Request for $ty {
            const METHOD: &'static str = $method;

            fn timeout(&self) -> Option<std::time::Duration> {
                crate::socket::blocking_timeout(self.timeout_ms)
            }
        }
        impl Query for $ty {
            const TAG: &'static str = $tag;
            type Reply = $reply;
        }
    };
    ($ty:ty, $method:literal) => {
        impl Request for $ty {
            const METHOD: &'static str = $method;
        }
    };
    ($ty:ty, $method:literal, $tag:literal => $reply:ty) => {
        request!($ty, $method);
        impl Query for $ty {
            const TAG: &'static str = $tag;
            type Reply = $reply;
        }
    };
}

/// Braces rather than a unit struct: herdr expects an object, and a unit
/// struct serialises as `null`.
#[derive(Debug, Default, Serialize)]
pub(crate) struct Ping {}
request!(Ping, "ping", "pong" => Pong);

#[derive(Debug, Default, Serialize)]
struct SessionSnapshot {}
request!(SessionSnapshot, "session.snapshot", "session_snapshot" => SnapshotReply);

#[derive(Debug, Serialize)]
struct WorktreeList<'a> {
    pub cwd: &'a str,
}
request!(WorktreeList<'_>, "worktree.list", "worktree_list" => Worktrees);

#[derive(Debug, Default, Serialize)]
struct WorktreeCreate<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace_id: Option<&'a WorkspaceId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<&'a str>,
    pub focus: bool,
}
request!(WorktreeCreate<'_>, "worktree.create", "worktree_created" => WorktreeCreated);

#[derive(Debug, Default, Serialize)]
struct WorktreeOpen<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace_id: Option<&'a WorkspaceId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<&'a str>,
    pub focus: bool,
}
request!(WorktreeOpen<'_>, "worktree.open", "worktree_opened" => WorktreeOpened);

#[derive(Debug, Serialize)]
struct WorktreeRemove<'a> {
    pub workspace_id: &'a WorkspaceId,
    pub force: bool,
}
request!(WorktreeRemove<'_>, "worktree.remove", "worktree_removed" => WorktreeRemoved);

#[derive(Debug, Default, Serialize)]
struct WorkspaceCreate<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<&'a str>,
    pub focus: bool,
}
request!(WorkspaceCreate<'_>, "workspace.create", "workspace_created" => WorkspaceCreated);

#[derive(Debug, Serialize)]
struct WorkspaceFocus<'a> {
    pub workspace_id: &'a WorkspaceId,
}
request!(WorkspaceFocus<'_>, "workspace.focus");

#[derive(Debug, Serialize)]
struct WorkspaceClose<'a> {
    pub workspace_id: &'a WorkspaceId,
}
request!(WorkspaceClose<'_>, "workspace.close");

#[derive(Debug, Serialize)]
struct PluginPaneOpen<'a> {
    pub plugin_id: &'a str,
    pub entrypoint: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace_id: Option<&'a WorkspaceId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub env: Option<&'a BTreeMap<String, String>>,
    pub focus: bool,
}
request!(PluginPaneOpen<'_>, "plugin.pane.open", "plugin_pane_opened" => PluginPaneOpened);

#[derive(Debug, Default, Serialize)]
struct TabCreate<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace_id: Option<&'a WorkspaceId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<&'a str>,
    pub focus: bool,
}
request!(TabCreate<'_>, "tab.create", "tab_created" => TabCreated);

#[derive(Debug, Serialize)]
struct TabFocus<'a> {
    pub tab_id: &'a TabId,
}
request!(TabFocus<'_>, "tab.focus");

#[derive(Debug, Serialize)]
struct TabClose<'a> {
    pub tab_id: &'a TabId,
}
request!(TabClose<'_>, "tab.close");

/// The three kinds of name an agent can be addressed by. Every `target`
/// parameter takes this rather than a bare string, so a tab id cannot be
/// handed to a call that wanted a pane.
#[derive(Debug, Clone, Copy)]
pub enum AgentRef<'a> {
    Pane(&'a PaneId),
    Name(&'a str),
    Workspace(&'a WorkspaceId),
}

impl<'a> AgentRef<'a> {
    pub fn as_str(self) -> &'a str {
        match self {
            AgentRef::Pane(id) => id.as_str(),
            AgentRef::Name(name) => name,
            AgentRef::Workspace(id) => id.as_str(),
        }
    }
}

impl Serialize for AgentRef<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

#[derive(Debug, Serialize)]
struct AgentFocus<'a> {
    pub target: AgentRef<'a>,
}
request!(AgentFocus<'_>, "agent.focus");

#[derive(Debug, Serialize)]
pub(crate) struct AgentStart<'a> {
    pub name: &'a str,
    #[serde(rename = "kind")]
    pub agent_kind: &'a str,
    pub pane_id: &'a PaneId,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,
}
impl Request for AgentStart<'_> {
    const METHOD: &'static str = "agent.start";

    fn timeout(&self) -> Option<Duration> {
        Some(
            self.timeout_ms
                .map_or(AGENT_START_DEFAULT_TIMEOUT, Duration::from_millis)
                + crate::socket::SLACK,
        )
    }
}
impl Query for AgentStart<'_> {
    const TAG: &'static str = "agent_started";
    type Reply = AgentStarted;
}

#[derive(Debug, Serialize)]
pub(crate) struct AgentPrompt<'a> {
    pub target: AgentRef<'a>,
    pub text: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wait: Option<AgentPromptWait<'a>>,
}

impl Request for AgentPrompt<'_> {
    const METHOD: &'static str = "agent.prompt";

    fn timeout(&self) -> Option<Duration> {
        self.wait
            .as_ref()
            .map_or(Some(crate::socket::TIMEOUT), |wait| {
                crate::socket::blocking_timeout(wait.timeout_ms)
            })
    }
}
impl Query for AgentPrompt<'_> {
    const TAG: &'static str = "agent_prompted";
    type Reply = AgentPrompted;
}

#[derive(Debug, Serialize)]
pub(crate) struct AgentPromptWait<'a> {
    #[serde(skip_serializing_if = "slice_is_empty")]
    pub(crate) until: &'a [AgentStatus],
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) timeout_ms: Option<u64>,
}

#[derive(Debug, Default, Serialize)]
struct PaneList {}
request!(PaneList, "pane.list", "pane_list" => Panes);

#[derive(Debug, Serialize)]
struct PaneProcessInfo<'a> {
    pub pane_id: &'a PaneId,
}
request!(PaneProcessInfo<'_>, "pane.process_info", "pane_process_info" => ProcessInfoReply);

#[derive(Debug, Serialize)]
struct PaneNeighbor<'a> {
    pub pane_id: &'a PaneId,
    pub direction: Direction,
}
request!(PaneNeighbor<'_>, "pane.neighbor", "pane_neighbor" => NeighborReply);

#[derive(Debug, Serialize)]
struct PaneFocusDirection<'a> {
    pub pane_id: &'a PaneId,
    pub direction: Direction,
}
request!(PaneFocusDirection<'_>, "pane.focus_direction");

/// `keys` is a sequence: herdr takes a run of chords, not one string.
#[derive(Debug, Serialize)]
struct PaneSendKeys<'a> {
    pub pane_id: &'a PaneId,
    pub keys: &'a [&'a str],
}
request!(PaneSendKeys<'_>, "pane.send_keys");

/// Which of a pane's buffers to read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReadSource {
    /// The rendered viewport.
    Visible,
    /// Recent output including soft wraps.
    Recent,
    /// Recent output with soft wraps joined. Prefer it for logs and
    /// transcripts, where a wrap in the middle of a path or a stack frame
    /// breaks anything scanning the text.
    RecentUnwrapped,
    /// The plain-text buffer herdr uses for agent detection.
    Detection,
}

#[derive(Debug, Serialize)]
struct PaneRead<'a> {
    pub pane_id: &'a PaneId,
    pub source: ReadSource,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lines: Option<u32>,
    pub strip_ansi: bool,
}
request!(PaneRead<'_>, "pane.read", "pane_read" => PaneReadReply);

/// Tokens for one workspace, from one named source.
///
/// `source` namespaces the push: two plugins reporting on the same workspace do
/// not overwrite each other, and a plugin can clear only its own tokens.
#[derive(Debug, Serialize)]
pub(crate) struct WorkspaceReportMetadata<'a> {
    pub workspace_id: &'a WorkspaceId,
    pub source: &'a str,
    pub tokens: &'a Tokens,
    /// Omit to retain until cleared; otherwise expire after these milliseconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ttl_ms: Option<u64>,
    /// Monotonic per source. herdr ignores a push older than one it has, which
    /// is what stops a slow refresh from overwriting a fast one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seq: Option<u64>,
}
request!(WorkspaceReportMetadata<'_>, "workspace.report_metadata");

#[derive(Debug, Serialize)]
struct PluginList<'a> {
    plugin_id: &'a str,
}
request!(PluginList<'_>, "plugin.list", "plugin_list" => PluginListReply);

#[derive(Debug, Deserialize)]
struct PluginListReply {
    plugins: Vec<PluginRegistration>,
}

/// Minimal live registration needed to tie services to their plugin owner.
#[derive(Debug, Deserialize)]
pub struct PluginRegistration {
    pub plugin_id: String,
    pub plugin_root: std::path::PathBuf,
    pub enabled: bool,
}

impl Client {
    pub fn plugin_registration(&self, plugin_id: &str) -> Result<Option<PluginRegistration>> {
        let reply = self.call(&PluginList { plugin_id })?;
        Ok(reply
            .plugins
            .into_iter()
            .find(|plugin| plugin.plugin_id == plugin_id))
    }
}

#[derive(Debug, Serialize)]
pub(crate) struct PaneReportMetadata<'a> {
    pub pane_id: &'a PaneId,
    pub source: &'a str,
    pub tokens: &'a Tokens,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ttl_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seq: Option<u64>,
}
request!(PaneReportMetadata<'_>, "pane.report_metadata");

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Sound {
    #[default]
    None,
    Done,
    Request,
}

#[derive(Debug, Serialize)]
struct NotificationShow<'a> {
    pub title: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<&'a str>,
    pub sound: Sound,
}
request!(NotificationShow<'_>, "notification.show", "notification_show" => Notification);

/// Blocks until an agent reaches one of `until`.
///
/// This is what an autofix loop advances on. Polling `agent_status` instead
/// re-reads the state that triggered the loop, which is how a loop ends up
/// acting twice on one transition.
#[derive(Debug, Serialize)]
pub(crate) struct AgentWait<'a> {
    pub target: AgentRef<'a>,
    #[serde(skip_serializing_if = "slice_is_empty")]
    pub until: &'a [AgentStatus],
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,
}
request!(AgentWait<'_>, "agent.wait", blocking, "wait_matched" => WaitMatchedWire);

/// Blocks until a pane's output matches, scanned server-side.
#[derive(Debug, Serialize)]
struct PaneWaitForOutput<'a> {
    pub pane_id: &'a PaneId,
    pub source: ReadSource,
    #[serde(rename = "match")]
    pub pattern: crate::events::OutputMatch<'a>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lines: Option<u32>,
    pub strip_ansi: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,
}
request!(PaneWaitForOutput<'_>, "pane.wait_for_output", blocking, "output_matched" => OutputMatched);

// Each result wraps its payload in one named field alongside the `type` tag.

#[derive(Debug, Clone, Deserialize)]
struct SnapshotReply {
    pub snapshot: Snapshot,
}

#[derive(Debug, Clone, Deserialize)]
struct Panes {
    pub panes: Vec<Pane>,
}

#[derive(Debug, Clone, Deserialize)]
struct ProcessInfoReply {
    pub process_info: ProcessInfo,
}

#[derive(Debug, Clone, Deserialize)]
struct NeighborReply {
    pub neighbor: Neighbor,
}

#[derive(Debug, Clone, Deserialize)]
struct PaneReadReply {
    pub read: PaneReadResult,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WorkspaceCreated {
    pub workspace: Workspace,
    pub tab: Tab,
    pub root_pane: Pane,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WorktreeCreated {
    pub workspace: Workspace,
    pub tab: Tab,
    pub root_pane: Pane,
    pub worktree: Worktree,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WorktreeOpened {
    pub workspace: Workspace,
    pub tab: Tab,
    pub root_pane: Pane,
    pub worktree: Worktree,
    pub already_open: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WorktreeRemoved {
    pub workspace_id: WorkspaceId,
    pub path: PathBuf,
    pub forced: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PluginPaneOpened {
    pub plugin_pane: PluginPaneInfo,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PluginPaneInfo {
    pub plugin_id: String,
    pub entrypoint: String,
    pub pane: Pane,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TabCreated {
    pub tab: Tab,
    pub root_pane: Pane,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AgentStarted {
    pub agent: Agent,
    pub argv: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AgentPrompted {
    pub agent: Agent,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PaneReadResult {
    pub pane_id: PaneId,
    pub workspace_id: WorkspaceId,
    pub tab_id: TabId,
    pub text: String,
    /// Bumped by every change to the pane. Carry it into a
    /// `pane.output_matched` subscription to skip what you have already seen.
    pub revision: u64,
    /// The read hit its line cap, so `text` is not the whole buffer.
    pub truncated: bool,
}

/// Why a notification did or did not appear.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotificationReason {
    Shown,
    /// The user turned notifications off in config.
    Disabled,
    RateLimited,
    /// Nothing is attached to show it.
    NoForegroundClient,
    Busy,
    #[serde(other)]
    Unknown,
}

/// The outcome of [`Client::notify`].
///
/// `shown` is false for four ordinary reasons, so a successful call is not a
/// delivered notification. Anything using one as the only report of a failure
/// needs to check this, or a disabled toast turns a failed handoff into
/// silence.
#[derive(Debug, Clone, Copy, Deserialize)]
pub struct Notification {
    pub shown: bool,
    pub reason: NotificationReason,
}

/// The event [`Client::agent_wait`] stopped on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentWaitResult {
    pub kind: crate::events::EventKind,
    pub pane_id: PaneId,
    pub workspace_id: WorkspaceId,
    pub agent_status: AgentStatus,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct WaitMatchedWire {
    event: WaitEventWire,
}

#[derive(Debug, Clone, Deserialize)]
struct WaitEventWire {
    event: crate::events::EventKind,
    data: WaitDataWire,
}

#[derive(Debug, Clone, Deserialize)]
struct WaitDataWire {
    pane_id: PaneId,
    workspace_id: WorkspaceId,
    agent_status: AgentStatus,
}

impl From<WaitMatchedWire> for AgentWaitResult {
    fn from(value: WaitMatchedWire) -> Self {
        Self {
            kind: value.event.event,
            pane_id: value.event.data.pane_id,
            workspace_id: value.event.data.workspace_id,
            agent_status: value.event.data.agent_status,
        }
    }
}

/// The match [`Client::pane_wait_for_output`] stopped on.
#[derive(Debug, Clone, Deserialize)]
pub struct OutputMatched {
    pub pane_id: PaneId,
    pub revision: u64,
    #[serde(default)]
    pub matched_line: Option<String>,
    pub read: PaneReadResult,
}

// --------------------------------------------------------------- convenience
//
// Wire request types stay inside this crate. Plugins use these named methods,
// which preserve domain types such as paths, identifiers, and durations and
// keep protocol defaults in one place.

fn path_parameter<'a>(field: &'static str, path: &'a Path) -> Result<&'a str> {
    path.to_str().ok_or_else(|| crate::Error::NonUnicodePath {
        field,
        path: path.to_path_buf(),
    })
}

fn timeout_ms(field: &'static str, timeout: Option<Duration>) -> Result<Option<u64>> {
    timeout
        .map(|duration| {
            u64::try_from(duration.as_millis())
                .map_err(|_| crate::Error::DurationOverflow { field, duration })
        })
        .transpose()
}

fn slice_is_empty<T>(values: &&[T]) -> bool {
    values.is_empty()
}

const AGENT_START_DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);
const AGENT_START_MIN_TIMEOUT: Duration = Duration::from_secs(3);
const AGENT_START_MAX_TIMEOUT: Duration = Duration::from_secs(5 * 60);
const AGENT_START_MIN_TIMEOUT_MS: u64 = 3_000;
const AGENT_START_MAX_TIMEOUT_MS: u64 = 300_000;

fn agent_start_timeout_ms(timeout: Option<Duration>) -> Result<Option<u64>> {
    let Some(duration) = timeout else {
        return Ok(None);
    };
    let milliseconds = timeout_ms("agent.start timeout", Some(duration))?.unwrap_or_default();
    if milliseconds <= AGENT_START_MIN_TIMEOUT_MS || milliseconds > AGENT_START_MAX_TIMEOUT_MS {
        return Err(crate::Error::DurationOutOfRange {
            field: "agent.start timeout",
            duration,
            minimum: AGENT_START_MIN_TIMEOUT,
            maximum: AGENT_START_MAX_TIMEOUT,
        });
    }
    Ok(Some(milliseconds))
}

impl Client {
    pub fn ping(&self) -> Result<Pong> {
        self.call(&Ping {})
    }

    /// Workspaces, panes, agents and the focus pointers, in one call.
    pub fn snapshot(&self) -> Result<Snapshot> {
        self.call(&SessionSnapshot {}).map(|r| r.snapshot)
    }

    pub fn worktree_list(&self, cwd: &Path) -> Result<Worktrees> {
        self.call(&WorktreeList {
            cwd: path_parameter("worktree.list cwd", cwd)?,
        })
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "mirrors Herdr's worktree.create operation without exposing wire request types"
    )]
    pub fn worktree_create(
        &self,
        workspace_id: Option<&WorkspaceId>,
        cwd: Option<&Path>,
        branch: Option<&str>,
        base: Option<&str>,
        path: Option<&Path>,
        label: Option<&str>,
        focus: bool,
    ) -> Result<WorktreeCreated> {
        self.call(&WorktreeCreate {
            workspace_id,
            cwd: cwd
                .map(|path| path_parameter("worktree.create cwd", path))
                .transpose()?,
            branch,
            base,
            path: path
                .map(|path| path_parameter("worktree.create path", path))
                .transpose()?,
            label,
            focus,
        })
    }

    pub fn worktree_open(
        &self,
        repo_root: &Path,
        checkout: &Path,
        label: Option<&str>,
        focus: bool,
    ) -> Result<WorktreeOpened> {
        self.call(&WorktreeOpen {
            workspace_id: None,
            cwd: Some(path_parameter("worktree.open cwd", repo_root)?),
            path: Some(path_parameter("worktree.open path", checkout)?),
            branch: None,
            label,
            focus,
        })
    }

    pub fn worktree_remove(
        &self,
        workspace_id: &WorkspaceId,
        force: bool,
    ) -> Result<WorktreeRemoved> {
        self.call(&WorktreeRemove {
            workspace_id,
            force,
        })
    }

    pub fn workspace_create(
        &self,
        cwd: &Path,
        label: Option<&str>,
        focus: bool,
    ) -> Result<WorkspaceCreated> {
        self.call(&WorkspaceCreate {
            cwd: Some(path_parameter("workspace.create cwd", cwd)?),
            label,
            focus,
        })
    }

    pub fn workspace_focus(&self, workspace_id: &WorkspaceId) -> Result<()> {
        self.invoke(&WorkspaceFocus { workspace_id })
    }

    pub fn workspace_close(&self, workspace_id: &WorkspaceId) -> Result<()> {
        self.invoke(&WorkspaceClose { workspace_id })
    }

    pub fn plugin_pane_open(
        &self,
        plugin_id: &str,
        entrypoint: &str,
        workspace_id: Option<&WorkspaceId>,
        cwd: Option<&Path>,
        env: Option<&BTreeMap<String, String>>,
        focus: bool,
    ) -> Result<PluginPaneOpened> {
        self.call(&PluginPaneOpen {
            plugin_id,
            entrypoint,
            workspace_id,
            cwd: cwd
                .map(|path| path_parameter("plugin.pane.open cwd", path))
                .transpose()?,
            env,
            focus,
        })
    }

    pub fn tab_create(
        &self,
        workspace_id: &WorkspaceId,
        cwd: &Path,
        label: Option<&str>,
        focus: bool,
    ) -> Result<TabCreated> {
        self.call(&TabCreate {
            workspace_id: Some(workspace_id),
            cwd: Some(path_parameter("tab.create cwd", cwd)?),
            label,
            focus,
        })
    }

    pub fn tab_close(&self, tab_id: &TabId) -> Result<()> {
        self.invoke(&TabClose { tab_id })
    }

    pub fn tab_focus(&self, tab_id: &TabId) -> Result<()> {
        self.invoke(&TabFocus { tab_id })
    }

    pub fn agent_focus(&self, target: AgentRef<'_>) -> Result<()> {
        self.invoke(&AgentFocus { target })
    }

    pub fn agent_start(
        &self,
        name: &str,
        agent_kind: &str,
        pane_id: &PaneId,
        timeout: Option<Duration>,
    ) -> Result<AgentStarted> {
        self.call(&AgentStart {
            name,
            agent_kind,
            pane_id,
            timeout_ms: agent_start_timeout_ms(timeout)?,
        })
    }

    pub fn agent_prompt(&self, target: AgentRef<'_>, text: &str) -> Result<AgentPrompted> {
        self.call(&AgentPrompt {
            target,
            text,
            wait: None,
        })
    }

    /// Atomically submits a prompt and waits for a later matching state.
    ///
    /// An empty `until` uses Herdr's settled-state default (idle, done, or
    /// blocked). A `None` timeout waits indefinitely and therefore installs no
    /// socket deadline.
    pub fn agent_prompt_and_wait(
        &self,
        target: AgentRef<'_>,
        text: &str,
        until: &[AgentStatus],
        timeout: Option<Duration>,
    ) -> Result<AgentPrompted> {
        self.call(&AgentPrompt {
            target,
            text,
            wait: Some(AgentPromptWait {
                until,
                timeout_ms: timeout_ms("agent.prompt wait timeout", timeout)?,
            }),
        })
    }

    pub fn pane_list(&self) -> Result<Vec<Pane>> {
        self.call(&PaneList {}).map(|r| r.panes)
    }

    pub fn pane_process_info(&self, pane_id: &PaneId) -> Result<ProcessInfo> {
        self.call(&PaneProcessInfo { pane_id })
            .map(|r| r.process_info)
    }

    /// The pane adjacent to `pane_id`, or `None` at the edge of the layout.
    pub fn pane_neighbor(&self, pane_id: &PaneId, direction: Direction) -> Result<Option<PaneId>> {
        self.call(&PaneNeighbor { pane_id, direction })
            .map(|r| r.neighbor.neighbor_pane_id)
    }

    /// herdr can only move focus to a neighbour, so reaching a specific pane
    /// means discovering which direction it lies in.
    pub fn pane_focus_direction(&self, pane_id: &PaneId, direction: Direction) -> Result<()> {
        self.invoke(&PaneFocusDirection { pane_id, direction })
    }

    /// Each of `keys` is one chord in herdr's syntax, such as `ctrl+l`.
    pub fn pane_send_keys(&self, pane_id: &PaneId, keys: &[&str]) -> Result<()> {
        self.invoke(&PaneSendKeys { pane_id, keys })
    }

    /// Reads a pane's buffer. `lines` of `None` is whatever herdr keeps.
    pub fn pane_read(
        &self,
        pane_id: &PaneId,
        source: ReadSource,
        lines: Option<u32>,
    ) -> Result<PaneReadResult> {
        self.call(&PaneRead {
            pane_id,
            source,
            lines,
            strip_ansi: true,
        })
        .map(|r| r.read)
    }

    /// Shows a notification, and reports whether it actually appeared.
    ///
    /// Check the result. A notification is the usual way to surface a failure
    /// that has no window to report into, and it silently does not appear when
    /// toasts are disabled, rate limited, or nothing is attached.
    pub fn notify(&self, title: &str, body: Option<&str>, sound: Sound) -> Result<Notification> {
        self.call(&NotificationShow { title, body, sound })
    }

    /// Blocks until `target` reaches one of `until`, or `timeout` passes.
    pub fn agent_wait(
        &self,
        target: AgentRef<'_>,
        until: &[AgentStatus],
        timeout: Option<Duration>,
    ) -> Result<AgentWaitResult> {
        self.call(&AgentWait {
            target,
            until,
            timeout_ms: timeout_ms("agent.wait timeout", timeout)?,
        })
        .map(Into::into)
    }

    /// Blocks until `pane_id`'s output matches, scanned by the server.
    pub fn pane_wait_for_output(
        &self,
        pane_id: &PaneId,
        pattern: crate::events::OutputMatch<'_>,
        timeout: Option<Duration>,
    ) -> Result<OutputMatched> {
        self.call(&PaneWaitForOutput {
            pane_id,
            source: ReadSource::RecentUnwrapped,
            pattern,
            lines: None,
            strip_ansi: true,
            timeout_ms: timeout_ms("pane.wait_for_output timeout", timeout)?,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt as _;

    use super::*;

    const SNAPSHOT: &str = include_str!("../tests/fixtures/session_snapshot.json");

    #[test]
    fn worktree_creation_outlasts_the_immediate_request_deadline() {
        let create = WorktreeCreate {
            workspace_id: None,
            cwd: Some("/repo"),
            branch: None,
            base: Some("abc123"),
            path: None,
            label: Some("Review"),
            focus: true,
        };
        let client = Client::new("/unused");
        assert_eq!(
            client.request_timeout(&create),
            Some(Duration::from_secs(600))
        );
        assert_eq!(
            client.request_timeout(&WorktreeOpen::default()),
            Some(Duration::from_secs(600))
        );
        assert_eq!(
            client.request_timeout(&WorktreeRemove {
                workspace_id: &WorkspaceId::new("w1"),
                force: false
            }),
            Some(Duration::from_secs(600))
        );
    }

    #[test]
    fn snapshot_fixture_deserialises() {
        let result: SnapshotReply = serde_json::from_str(SNAPSHOT).unwrap();
        let mut snap = result.snapshot;
        assert_eq!(snap.protocol, PROTOCOL);
        assert!(!snap.workspaces.is_empty());
        assert!(snap.workspaces.iter().any(|w| w.worktree.is_some()));
        assert!(snap.panes.iter().any(|p| p.cwd.is_some()));
        let repository = snap
            .workspaces
            .iter()
            .filter_map(|workspace| workspace.worktree.as_ref())
            .map(|worktree| &worktree.repository)
            .find(|repository| repository.key == "gh:example/dots")
            .unwrap();
        assert_eq!(repository.key, "gh:example/dots");
        assert_eq!(repository.root, Path::new("/home/dev/src/dots"));
        snap.panes.insert(
            0,
            Pane {
                pane_id: PaneId::new("w1:empty"),
                workspace_id: WorkspaceId::new("w1"),
                tab_id: TabId::new("w1:t1"),
                cwd: Some(PathBuf::new()),
            },
        );
        assert_eq!(
            snap.effective_workspace_dir(&WorkspaceId::new("w1")),
            Some(Path::new("/home/dev")),
            "a non-worktree workspace falls back to its first non-empty pane cwd"
        );
        let managed = snap
            .workspaces
            .iter()
            .find(|workspace| workspace.worktree.is_some())
            .unwrap();
        assert_eq!(
            snap.effective_workspace_dir(&managed.workspace_id),
            managed
                .worktree
                .as_ref()
                .map(|worktree| worktree.checkout_path.as_path())
        );
    }

    #[test]
    fn an_empty_request_serialises_as_an_object() {
        assert_eq!(serde_json::to_string(&Ping {}).unwrap(), "{}");
        assert_eq!(serde_json::to_string(&SessionSnapshot {}).unwrap(), "{}");
    }

    /// herdr rejects a bare string with `invalid_request`, and because it
    /// cannot parse the request it answers with an empty id.
    #[test]
    fn keys_serialise_as_a_sequence() {
        let pane = PaneId::new("w2:p1");
        let json = serde_json::to_string(&PaneSendKeys {
            pane_id: &pane,
            keys: &["ctrl+h"],
        })
        .unwrap();
        assert_eq!(json, r#"{"pane_id":"w2:p1","keys":["ctrl+h"]}"#);
    }

    #[test]
    fn an_optional_param_is_omitted_rather_than_sent_as_null() {
        let json = serde_json::to_string(&WorktreeOpen {
            workspace_id: None,
            cwd: Some("/repo"),
            path: None,
            branch: None,
            label: None,
            focus: true,
        })
        .unwrap();
        assert_eq!(json, r#"{"cwd":"/repo","focus":true}"#);
    }

    #[test]
    fn tab_creation_keeps_workspace_and_cwd_typed() {
        let workspace = WorkspaceId::new("w2");
        let json = serde_json::to_string(&TabCreate {
            workspace_id: Some(&workspace),
            cwd: Some("/repo"),
            label: Some("github-ci-fix"),
            focus: false,
        })
        .unwrap();
        assert_eq!(
            json,
            r#"{"workspace_id":"w2","cwd":"/repo","label":"github-ci-fix","focus":false}"#
        );
    }

    #[test]
    fn agent_start_and_prompt_use_the_same_typed_target() {
        let pane = PaneId::new("w2:p1");
        let start = serde_json::to_string(&AgentStart {
            name: "ghci-service-42",
            agent_kind: "claude",
            pane_id: &pane,
            timeout_ms: Some(30_000),
        })
        .unwrap();
        assert_eq!(
            start,
            r#"{"name":"ghci-service-42","kind":"claude","pane_id":"w2:p1","timeout_ms":30000}"#
        );
        let prompt = serde_json::to_string(&AgentPrompt {
            target: AgentRef::Name("ghci-service-42"),
            text: "fix it",
            wait: None,
        })
        .unwrap();
        assert_eq!(prompt, r#"{"target":"ghci-service-42","text":"fix it"}"#);

        let atomic = serde_json::to_string(&AgentPrompt {
            target: AgentRef::Name("ghci-service-42"),
            text: "fix it",
            wait: Some(AgentPromptWait {
                until: &[],
                timeout_ms: None,
            }),
        })
        .unwrap();
        assert_eq!(
            atomic, r#"{"target":"ghci-service-42","text":"fix it","wait":{}}"#,
            "an empty status set must select Herdr's default, not send an empty list"
        );
    }

    #[test]
    fn agent_start_timeout_obeys_the_documented_millisecond_bounds() {
        for rejected in [
            Duration::ZERO,
            Duration::from_millis(3_000),
            Duration::from_millis(3_000) + Duration::from_nanos(1),
            Duration::from_millis(300_001),
        ] {
            assert!(matches!(
                agent_start_timeout_ms(Some(rejected)),
                Err(crate::Error::DurationOutOfRange { duration, .. }) if duration == rejected
            ));
        }
        assert_eq!(
            agent_start_timeout_ms(Some(Duration::from_millis(3_001))).unwrap(),
            Some(3_001)
        );
        assert_eq!(
            agent_start_timeout_ms(Some(Duration::from_millis(300_000))).unwrap(),
            Some(300_000)
        );
        assert_eq!(agent_start_timeout_ms(None).unwrap(), None);
    }

    #[test]
    fn prompt_and_wait_is_one_request_and_accepts_an_owned_fake_reply() {
        use crate::testing::{RecordedRequest, Server, Step};

        let response = String::from(
            r#"{"id":"{id}","result":{"type":"agent_prompted","agent":{"pane_id":"w1:p1","workspace_id":"w1","tab_id":"w1:t1","agent_status":"done"}}}"#,
        );
        let server = Server::start(vec![Step::reply(response)]);
        let result = server
            .client()
            .agent_prompt_and_wait(
                AgentRef::Name("worker"),
                "fix it",
                &[AgentStatus::Done, AgentStatus::Blocked],
                Some(Duration::from_secs(60)),
            )
            .unwrap();

        assert_eq!(result.agent.agent_status, AgentStatus::Done);
        assert_eq!(
            server.requests(),
            [RecordedRequest {
                method: "agent.prompt".to_owned(),
                params: serde_json::json!({
                    "target": "worker",
                    "text": "fix it",
                    "wait": {
                        "until": ["done", "blocked"],
                        "timeout_ms": 60_000,
                    },
                }),
            }]
        );
    }

    #[test]
    fn an_agent_reference_says_which_kind_of_name_it_is() {
        let pane = PaneId::new("w2:p1");
        let workspace = WorkspaceId::new("w2");
        for (target, expected) in [
            (AgentRef::Pane(&pane), "w2:p1"),
            (AgentRef::Name("plugin-dev"), "plugin-dev"),
            (AgentRef::Workspace(&workspace), "w2"),
        ] {
            let json = serde_json::to_string(&AgentFocus { target }).unwrap();
            assert_eq!(json, format!(r#"{{"target":"{expected}"}}"#));
        }
    }

    #[test]
    fn unknown_agent_status_does_not_fail() {
        let status: AgentStatus = serde_json::from_str("\"something_new\"").unwrap();
        assert_eq!(status, AgentStatus::Unknown);
    }

    #[test]
    fn a_wait_match_keeps_the_nested_event_data() {
        let matched: WaitMatchedWire = serde_json::from_str(
            r#"{"type":"wait_matched","event":{"event":"pane_agent_status_changed","data":{"type":"pane_agent_status_changed","pane_id":"w1:p2","workspace_id":"w1","agent_status":"done"}}}"#,
        )
        .unwrap();
        assert_eq!(
            matched.event.event,
            crate::events::EventKind::PaneAgentStatusChanged
        );
        assert_eq!(matched.event.data.pane_id, PaneId::new("w1:p2"));
        assert_eq!(matched.event.data.agent_status, AgentStatus::Done);

        let result = AgentWaitResult::from(matched);
        assert_eq!(result.pane_id, PaneId::new("w1:p2"));
        assert_eq!(result.workspace_id, WorkspaceId::new("w1"));
        assert_eq!(result.agent_status, AgentStatus::Done);
    }

    #[test]
    fn a_wait_match_missing_its_status_fails_loudly() {
        let matched = serde_json::from_str::<WaitMatchedWire>(
            r#"{"type":"wait_matched","event":{"event":"pane_agent_status_changed","data":{"type":"pane_agent_status_changed","pane_id":"w1:p2","workspace_id":"w1"}}}"#,
        );
        assert!(matched.is_err());
    }

    #[test]
    fn agent_and_tab_creation_replies_match_the_schema_shape() {
        let tab: TabCreated = serde_json::from_str(
            r#"{"type":"tab_created","tab":{"tab_id":"w1:t2","workspace_id":"w1"},"root_pane":{"pane_id":"w1:p2","workspace_id":"w1","tab_id":"w1:t2"}}"#,
        )
        .unwrap();
        assert_eq!(tab.tab.tab_id, TabId::new("w1:t2"));
        assert_eq!(tab.root_pane.pane_id, PaneId::new("w1:p2"));

        let started: AgentStarted = serde_json::from_str(
            r#"{"type":"agent_started","agent":{"pane_id":"w1:p2","workspace_id":"w1","tab_id":"w1:t2","agent_status":"working"},"argv":["claude"]}"#,
        )
        .unwrap();
        assert_eq!(started.agent.agent_status, AgentStatus::Working);
        assert_eq!(started.argv, ["claude"]);
    }

    #[test]
    fn workspace_creation_returns_the_created_topology() {
        use crate::testing::{RecordedRequest, Server, Step};

        let server = Server::start(vec![Step::reply(
            r#"{"id":"{id}","result":{"type":"workspace_created","workspace":{"workspace_id":"w2","label":"review","focused":true,"agent_status":"idle"},"tab":{"tab_id":"w2:t1","workspace_id":"w2"},"root_pane":{"pane_id":"w2:p1","workspace_id":"w2","tab_id":"w2:t1","cwd":"/repo"}}}"#,
        )]);

        let created = server
            .client()
            .workspace_create(Path::new("/repo"), Some("review"), true)
            .unwrap();

        assert_eq!(created.workspace.workspace_id, WorkspaceId::new("w2"));
        assert_eq!(created.tab.tab_id, TabId::new("w2:t1"));
        assert_eq!(created.root_pane.pane_id, PaneId::new("w2:p1"));
        assert_eq!(
            server.requests(),
            [RecordedRequest {
                method: "workspace.create".to_owned(),
                params: serde_json::json!({
                    "cwd": "/repo",
                    "label": "review",
                    "focus": true,
                }),
            }]
        );
    }

    #[test]
    fn runtime_focus_and_close_requests_keep_workspace_and_tab_ids_typed() {
        use crate::testing::{RecordedRequest, Server, Step};

        let server = Server::start(vec![
            Step::reply(r#"{"id":"{id}","result":{}}"#),
            Step::reply(r#"{"id":"{id}","result":{}}"#),
            Step::reply(r#"{"id":"{id}","result":{}}"#),
        ]);
        let workspace = WorkspaceId::new("w2");
        let tab = TabId::new("w2:t3");

        server.client().workspace_focus(&workspace).unwrap();
        server.client().tab_focus(&tab).unwrap();
        server.client().workspace_close(&workspace).unwrap();

        assert_eq!(
            server.requests(),
            [
                RecordedRequest {
                    method: "workspace.focus".to_owned(),
                    params: serde_json::json!({ "workspace_id": "w2" }),
                },
                RecordedRequest {
                    method: "tab.focus".to_owned(),
                    params: serde_json::json!({ "tab_id": "w2:t3" }),
                },
                RecordedRequest {
                    method: "workspace.close".to_owned(),
                    params: serde_json::json!({ "workspace_id": "w2" }),
                },
            ]
        );
    }

    #[test]
    fn plugin_pane_open_targets_a_workspace_and_passes_environment() {
        use crate::testing::{RecordedRequest, Server, Step};

        let server = Server::start(vec![Step::reply(
            r#"{"id":"{id}","result":{"type":"plugin_pane_opened","plugin_pane":{"plugin_id":"herdr-review","entrypoint":"editor","pane":{"pane_id":"w3:p2","workspace_id":"w3","tab_id":"w3:t2","cwd":"/worktrees/pr-42"}}}}"#,
        )]);
        let workspace = WorkspaceId::new("w3");
        let env = BTreeMap::from([
            ("EXAMPLE_MODE".to_owned(), "inspect".to_owned()),
            ("EXAMPLE_TARGET".to_owned(), "change-42".to_owned()),
        ]);

        let opened = server
            .client()
            .plugin_pane_open(
                "herdr-review",
                "editor",
                Some(&workspace),
                Some(Path::new("/worktrees/pr-42")),
                Some(&env),
                true,
            )
            .unwrap();

        assert_eq!(opened.plugin_pane.plugin_id, "herdr-review");
        assert_eq!(opened.plugin_pane.entrypoint, "editor");
        assert_eq!(opened.plugin_pane.pane.pane_id, PaneId::new("w3:p2"));
        assert_eq!(
            server.requests(),
            [RecordedRequest {
                method: "plugin.pane.open".to_owned(),
                params: serde_json::json!({
                    "plugin_id": "herdr-review",
                    "entrypoint": "editor",
                    "workspace_id": "w3",
                    "cwd": "/worktrees/pr-42",
                    "env": {
                        "EXAMPLE_MODE": "inspect",
                        "EXAMPLE_TARGET": "change-42",
                    },
                    "focus": true,
                }),
            }]
        );
    }

    #[test]
    fn worktree_mutations_return_their_protocol_results() {
        use crate::testing::{RecordedRequest, Server, Step};

        let worktree = r#"{"path":"/worktrees/pr-42","label":"PR #42","is_bare":false,"is_detached":false,"is_prunable":false,"is_linked_worktree":true,"branch":"review/pr-42","open_workspace_id":"w3"}"#;
        let workspace =
            r#"{"workspace_id":"w3","label":"PR #42","focused":false,"agent_status":"idle"}"#;
        let tab = r#"{"tab_id":"w3:t1","workspace_id":"w3"}"#;
        let pane =
            r#"{"pane_id":"w3:p1","workspace_id":"w3","tab_id":"w3:t1","cwd":"/worktrees/pr-42"}"#;
        let server = Server::start(vec![
            Step::reply(format!(
                r#"{{"id":"{{id}}","result":{{"type":"worktree_created","workspace":{workspace},"tab":{tab},"root_pane":{pane},"worktree":{worktree}}}}}"#
            )),
            Step::reply(format!(
                r#"{{"id":"{{id}}","result":{{"type":"worktree_opened","workspace":{workspace},"tab":{tab},"root_pane":{pane},"worktree":{worktree},"already_open":true}}}}"#
            )),
            Step::reply(
                r#"{"id":"{id}","result":{"type":"worktree_removed","workspace_id":"w3","path":"/worktrees/pr-42","forced":false}}"#,
            ),
        ]);
        let client = server.client();
        let source_workspace = WorkspaceId::new("w1");

        let created = client
            .worktree_create(
                Some(&source_workspace),
                None,
                Some("review/pr-42"),
                Some("abc123"),
                Some(Path::new("/worktrees/pr-42")),
                Some("PR #42"),
                false,
            )
            .unwrap();
        assert_eq!(created.workspace.workspace_id, WorkspaceId::new("w3"));
        assert_eq!(
            created.worktree.checkout_path,
            Path::new("/worktrees/pr-42")
        );

        let opened = client
            .worktree_open(
                Path::new("/repo"),
                Path::new("/worktrees/pr-42"),
                Some("PR #42"),
                true,
            )
            .unwrap();
        assert!(opened.already_open);
        assert_eq!(opened.root_pane.pane_id, PaneId::new("w3:p1"));

        let workspace_id = WorkspaceId::new("w3");
        let removed = client.worktree_remove(&workspace_id, false).unwrap();
        assert_eq!(removed.workspace_id, workspace_id);
        assert_eq!(removed.path, Path::new("/worktrees/pr-42"));
        assert!(!removed.forced);

        assert_eq!(
            server.requests(),
            [
                RecordedRequest {
                    method: "worktree.create".to_owned(),
                    params: serde_json::json!({
                        "workspace_id": "w1",
                        "branch": "review/pr-42",
                        "base": "abc123",
                        "path": "/worktrees/pr-42",
                        "label": "PR #42",
                        "focus": false,
                    }),
                },
                RecordedRequest {
                    method: "worktree.open".to_owned(),
                    params: serde_json::json!({
                        "cwd": "/repo",
                        "path": "/worktrees/pr-42",
                        "label": "PR #42",
                        "focus": true,
                    }),
                },
                RecordedRequest {
                    method: "worktree.remove".to_owned(),
                    params: serde_json::json!({
                        "workspace_id": "w3",
                        "force": false,
                    }),
                },
            ]
        );
    }

    #[test]
    fn agent_falls_back_to_pane_id_for_a_name() {
        let agent: Agent = serde_json::from_str(
            r#"{"pane_id":"p1","workspace_id":"w1","tab_id":"t1","agent_status":"idle"}"#,
        )
        .unwrap();
        assert_eq!(agent.display_name(), "p1");
    }

    #[test]
    fn an_open_worktree_is_not_openable_again() {
        let base = r#""path":"/x","label":"x","is_bare":false,"is_detached":false,
                      "is_prunable":false,"is_linked_worktree":true"#;
        let free: Worktree = serde_json::from_str(&format!("{{{base}}}")).unwrap();
        let taken: Worktree =
            serde_json::from_str(&format!("{{{base},\"open_workspace_id\":\"w1\"}}")).unwrap();
        assert!(free.is_openable());
        assert!(!taken.is_openable());
    }

    #[test]
    fn find_strips_any_directory_from_the_command() {
        let info: ProcessInfo = serde_json::from_str(
            r#"{"pane_id":"p1","foreground_processes":[
                 {"pid":1,"name":"zsh","argv0":"-zsh"},
                 {"pid":2,"name":"nvim","argv0":"/opt/homebrew/bin/nvim"}]}"#,
        )
        .unwrap();
        assert_eq!(info.find(|c| c == "nvim").map(|p| p.pid), Some(2));
        assert_eq!(info.find(|c| c == "fzf").map(|p| p.pid), None);
    }

    #[test]
    fn a_non_unicode_path_fails_before_any_socket_call() {
        let client = Client::new("/not-used");
        let path = PathBuf::from(OsString::from_vec(b"/repo/\xff".to_vec()));
        assert!(matches!(
            client.worktree_list(&path),
            Err(crate::Error::NonUnicodePath { .. })
        ));
    }

    #[test]
    fn a_duration_that_cannot_fit_the_protocol_fails_before_the_socket() {
        let client = Client::new("/not-used");
        let pane = PaneId::new("w1:p1");
        assert!(matches!(
            client.agent_wait(
                AgentRef::Pane(&pane),
                &[AgentStatus::Done],
                Some(Duration::from_secs(u64::MAX)),
            ),
            Err(crate::Error::DurationOverflow { .. })
        ));
    }
}
