use std::borrow::Cow;
use std::path::PathBuf;

use herdrkit::api::{AgentStatus, Repository};
use herdrkit::picker::{Cell, Column, Entry, Group};
use herdrkit::{PaneId, Theme, WorkspaceId};

/// Column widths, in cells. Workspaces and worktrees share a set because they
/// are the same thing in two states, and a query mixes the groups back together.
const GLYPH: usize = 2;
const REPO: usize = 14;
const BRANCH: usize = 24;
const MARK: usize = 2;
const AGENT_NAME: usize = 16;
const AGENT_REPO: usize = 13;
const AGENT_BRANCH: usize = 20;

/// Cells the agent group spends before its filled column gets anything.
///
/// The popup width in `herdr-plugin.toml` has to clear this by enough to leave
/// a task readable, which is asserted rather than assumed — at 40% of a
/// 180-column terminal the task column came out at zero cells.
pub const FIXED_CELLS: usize = GLYPH + AGENT_NAME + AGENT_REPO + AGENT_BRANCH;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DestinationKind {
    Workspace,
    Agent,
    Worktree,
    Directory,
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
        repository: Option<Repository>,
        branch: String,
        display_path: String,
        status: AgentStatus,
        focused: bool,
    },
    Agent {
        pane: PaneId,
        name: String,
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
    type Group = DestinationKind;

    fn group(&self) -> DestinationKind {
        self.kind()
    }

    fn cells(&self, theme: &Theme) -> Vec<Cell> {
        match self {
            Destination::Workspace {
                repository,
                branch,
                display_path,
                status,
                focused,
                ..
            } => vec![
                glyph(*status, theme),
                Cell::new(repository_name(repository.as_ref()), theme.strong),
                Cell::new(branch.clone(), theme.blue),
                Cell::new(display_path.clone(), theme.muted),
                Cell::tag(if *focused { "←" } else { "" }, theme.accent),
            ],
            // No status column: the glyph in the gutter already carries the
            // status in its shape and its colour, and spelling it out again
            // cost ten cells of the task title beside it.
            Destination::Agent {
                name,
                status,
                repository,
                branch,
                task,
                ..
            } => vec![
                glyph(*status, theme),
                Cell::new(name.clone(), theme.strong),
                Cell::new(repository_name(repository.as_ref()), theme.muted),
                Cell::new(branch.clone(), theme.blue),
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
                Cell::new(branch.clone(), theme.blue),
                Cell::new(display_path.clone(), theme.muted),
                Cell::tag("", theme.accent),
            ],
            // A directory has no repo or branch until something opens it, and
            // its basename is no substitute: under a repo column every
            // `wt/<repo>/main` reads as a repo called "main". The path is the
            // only honest field, so it is the only one.
            Destination::Directory { display_path, .. } => vec![
                Cell::tag("", theme.muted),
                Cell::new(display_path.clone(), theme.strong),
            ],
        }
    }

    /// Terms worth typing that are not worth a column.
    ///
    /// The kind and an agent's status are drawn as a section divider and a
    /// glyph, neither of which is searchable, so both are repeated here — `agent
    /// working` narrows to live agents without either word occupying a cell.
    /// Ids are here because they are how you get back to a workspace you
    /// remember by number rather than by name.
    fn hidden_terms(&self) -> Option<Cow<'_, str>> {
        let kind = self.kind().label();
        Some(match self {
            Destination::Workspace { id, .. } => format!("{kind} {id}").into(),
            Destination::Agent { pane, status, .. } => {
                format!("{kind} {} {pane}", status.as_str()).into()
            }
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

/// Groups in the order they appear, which is what the picker preserves. Live
/// state first, then what could be live, then everywhere else.
pub fn groups() -> Vec<Group<DestinationKind>> {
    let repo_columns = |kind: DestinationKind| {
        Group::new(
            kind,
            kind.section(),
            vec![
                Column::new(GLYPH),
                Column::new(REPO),
                Column::new(BRANCH),
                Column::fill(),
                Column::new(MARK),
            ],
        )
    };
    vec![
        repo_columns(DestinationKind::Workspace),
        Group::new(
            DestinationKind::Agent,
            DestinationKind::Agent.section(),
            vec![
                Column::new(GLYPH),
                Column::new(AGENT_NAME),
                Column::new(AGENT_REPO),
                Column::new(AGENT_BRANCH),
                Column::fill(),
            ],
        ),
        repo_columns(DestinationKind::Worktree),
        Group::new(
            DestinationKind::Directory,
            DestinationKind::Directory.section(),
            vec![Column::new(GLYPH), Column::fill()],
        )
        .scoped('/'),
    ]
}
