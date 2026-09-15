use std::borrow::Cow;
use std::path::PathBuf;

use herdrkit::api::{AgentStatus, Repository};
use herdrkit::picker::{Cell, Column, Entry, Group};
use herdrkit::{PaneId, Theme, WorkspaceId};

/// Column widths, in cells.
const GLYPH: usize = 3;
const WORKSPACE: usize = 24;
const REPO: usize = 14;
const BRANCH: usize = 36;
const MARK: usize = 3;
const AGENT_NAME: usize = 12;
const AGENT_REPO: usize = 13;
const AGENT_BRANCH: usize = 36;
// Nine cells for the longest status plus the shared two-cell column gap.
const AGENT_STATUS: usize = 11;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DestinationKind {
    Workspace,
    Agent,
    Worktree,
    Directory,
}

/// Attention is a presentation group, not a different kind of destination.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DestinationGroup {
    Attention,
    Kind(DestinationKind),
}

impl DestinationKind {
    /// Singular, because this is what the user types to narrow to this kind.
    fn label(self) -> &'static str {
        match self {
            Self::Workspace => "workspace",
            Self::Agent => "agent",
            Self::Worktree => "worktree",
            Self::Directory => "directory",
        }
    }

    /// Plural, because this titles a divider over a run of rows.
    fn section(self) -> &'static str {
        match self {
            Self::Workspace => "workspaces",
            Self::Agent => "agents",
            Self::Worktree => "worktrees",
            Self::Directory => "directories",
        }
    }
}

/// `display_path` is already shortened for display; dispatch always uses the
/// full path Herdr supplied.
#[derive(Debug, Clone)]
pub enum Destination {
    Workspace {
        id: WorkspaceId,
        label: String,
        nested: bool,
        repository: Option<Repository>,
        branch: String,
        display_path: String,
        status: AgentStatus,
        focused: bool,
    },
    Agent {
        pane: PaneId,
        name: String,
        workspace_label: String,
        status: AgentStatus,
        repository: Option<Repository>,
        branch: String,
        task: String,
    },
    Worktree {
        path: PathBuf,
        repository: Repository,
        branch: String,
        display_path: String,
    },
    Directory {
        path: PathBuf,
        display_path: String,
    },
}

impl Destination {
    fn needs_attention(&self) -> bool {
        matches!(
            self,
            Self::Agent {
                status: AgentStatus::Blocked | AgentStatus::Done,
                ..
            }
        )
    }

    pub fn kind(&self) -> DestinationKind {
        match self {
            Destination::Workspace { .. } => DestinationKind::Workspace,
            Destination::Agent { .. } => DestinationKind::Agent,
            Destination::Worktree { .. } => DestinationKind::Worktree,
            Destination::Directory { .. } => DestinationKind::Directory,
        }
    }
}

impl Entry for Destination {
    type Group = DestinationGroup;

    fn group(&self) -> DestinationGroup {
        if self.needs_attention() {
            DestinationGroup::Attention
        } else {
            DestinationGroup::Kind(self.kind())
        }
    }

    fn cells(&self, theme: &Theme) -> Vec<Cell> {
        match self {
            Destination::Workspace {
                label,
                nested,
                repository,
                branch,
                display_path,
                status,
                focused,
                ..
            } => vec![
                glyph(*status, theme),
                Cell::new(label.clone(), theme.strong).prefix(if *nested { "└─ " } else { "" }),
                Cell::new(repository_name(repository.as_ref()), theme.muted),
                Cell::new(branch.clone(), theme.muted),
                Cell::path(display_path.clone(), theme.muted),
                Cell::tag(if *focused { "←" } else { "" }, theme.accent),
            ],
            // Words distinguish a request from a completion without relying
            // on the user's ability to distinguish the status colors.
            Destination::Agent {
                name,
                status,
                repository,
                branch,
                task,
                ..
            } => vec![
                agent_status(*status, theme),
                Cell::new(name.clone(), theme.strong),
                Cell::new(repository_name(repository.as_ref()), theme.muted),
                Cell::new(branch.clone(), theme.muted),
                Cell::new(task.clone(), theme.strong),
            ],
            Destination::Worktree {
                repository,
                branch,
                display_path,
                ..
            } => vec![
                Cell::tag("", theme.muted),
                Cell::new(repository.name.clone(), theme.strong),
                Cell::new(branch.clone(), theme.muted),
                Cell::path(display_path.clone(), theme.muted),
                Cell::tag("", theme.accent),
            ],
            // A directory has no repo or branch until something opens it, and
            // its basename is no substitute: under a repo column every
            // `wt/<repo>/main` reads as a repo called "main". The path is the
            // only honest field, so it is the only one.
            Destination::Directory { display_path, .. } => vec![
                Cell::tag("", theme.muted),
                Cell::path(display_path.clone(), theme.strong),
            ],
        }
    }

    /// Terms worth typing that are not worth a column.
    ///
    /// Keep `agent` searchable even when its row is in the attention group.
    /// IDs and workspace labels also find agents without taking title space.
    fn hidden_terms(&self) -> Option<Cow<'_, str>> {
        let kind = self.kind().label();
        Some(match self {
            Destination::Workspace { id, .. } => format!("{kind} {id}").into(),
            Destination::Agent {
                pane,
                workspace_label,
                ..
            } => format!(
                "{kind} {pane} {workspace_label} {}",
                if self.needs_attention() {
                    "attention waiting"
                } else {
                    ""
                },
            )
            .into(),
            Destination::Worktree { .. } | Destination::Directory { .. } => Cow::Borrowed(kind),
        })
    }
}

fn repository_name(repository: Option<&Repository>) -> String {
    repository.map_or_else(|| "-".to_owned(), |repository| repository.name.clone())
}

fn glyph(status: AgentStatus, theme: &Theme) -> Cell {
    let (glyph, colour) = theme.status(status);
    Cell::tag(glyph.to_string(), colour)
}

fn agent_status(status: AgentStatus, theme: &Theme) -> Cell {
    let (glyph, colour) = theme.status(status);
    Cell::new(format!("{glyph} {}", status.as_str()), colour)
}

/// Group order makes the first attention item the default Enter target. An
/// agent appears once, and a session with nothing waiting starts at workspaces.
pub fn groups() -> Vec<Group<DestinationGroup>> {
    let agent_group = |group, label| {
        Group::new(
            group,
            label,
            vec![
                Column::new(AGENT_STATUS).heading("status"),
                Column::content(AGENT_NAME).heading("agent"),
                Column::content(AGENT_REPO).heading("repository"),
                Column::content(AGENT_BRANCH).heading("branch"),
                Column::fill().heading("task"),
            ],
        )
    };
    let repo_columns = |kind: DestinationKind| {
        Group::new(
            DestinationGroup::Kind(kind),
            kind.section(),
            vec![
                Column::new(GLYPH),
                Column::content(REPO).heading("repository"),
                Column::content(BRANCH).heading("branch"),
                Column::fill().heading("directory"),
                Column::new(MARK),
            ],
        )
    };
    vec![
        agent_group(DestinationGroup::Attention, "needs attention"),
        Group::new(
            DestinationGroup::Kind(DestinationKind::Workspace),
            DestinationKind::Workspace.section(),
            vec![
                Column::new(GLYPH),
                Column::content(WORKSPACE).heading("workspace"),
                Column::content(REPO).heading("repository"),
                Column::content(BRANCH).heading("branch"),
                Column::fill().heading("directory"),
                Column::new(MARK),
            ],
        ),
        agent_group(
            DestinationGroup::Kind(DestinationKind::Agent),
            DestinationKind::Agent.section(),
        ),
        repo_columns(DestinationKind::Worktree),
        Group::new(
            DestinationGroup::Kind(DestinationKind::Directory),
            DestinationKind::Directory.section(),
            vec![Column::new(GLYPH), Column::fill().heading("directory")],
        )
        .scoped('/'),
    ]
}
