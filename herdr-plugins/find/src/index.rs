use std::collections::{HashMap, HashSet};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context as _, Result, bail};
use herdrkit::api::{Repository, Snapshot, Worktrees};
use herdrkit::{Client, WorkspaceId};
use serde::Deserialize;

use crate::Destination;

/// Everything the index is built from, in one value, so the transform below is
/// a pure function over a fixture.
#[derive(Debug, Deserialize)]
pub struct Sources {
    pub snapshot: Snapshot,
    /// One listing per repo herdr currently holds a workspace in. A repo with
    /// nothing open is reachable only as a directory.
    pub worktrees: Vec<Worktrees>,
    pub dirs: Vec<PathBuf>,
}

impl Sources {
    pub fn collect(client: &Client) -> Result<Self> {
        let snapshot = client.snapshot().context("reading the session")?;
        let mut roots: Vec<&Path> = snapshot
            .workspaces
            .iter()
            .filter_map(|w| w.worktree.as_ref())
            .map(|w| w.repository.root.as_path())
            .collect();
        roots.sort_unstable();
        roots.dedup();
        // A repo that has gone away since herdr saw it should cost that repo's
        // worktrees, not the whole list — but it must say so, or a socket
        // failure reads as "this repo has no worktrees" forever.
        let worktrees = roots
            .iter()
            .filter_map(|root| match client.worktree_list(root) {
                Ok(list) => Some(list),
                Err(error) => {
                    eprintln!(
                        "find: leaving out the worktrees of {}: {error:#}",
                        root.display()
                    );
                    None
                }
            })
            .collect();
        Ok(Self {
            snapshot,
            worktrees,
            dirs: zoxide_dirs(),
        })
    }

    pub fn destinations(&self, home: &Path) -> Vec<Destination> {
        let branches = self.branches_by_checkout();
        let workspace_directories: HashMap<&WorkspaceId, &Path> = self
            .snapshot
            .workspaces
            .iter()
            .filter_map(|workspace| {
                self.snapshot
                    .effective_workspace_dir(&workspace.workspace_id)
                    .map(|directory| (&workspace.workspace_id, directory))
            })
            .collect();
        let mut out = Vec::new();

        // A workspace knows its checkout but not its branch; the branch comes
        // from the worktree listing, joined on the path herdr reports for both.
        let mut workspace_repositories: HashMap<&WorkspaceId, (Option<&Repository>, &str)> =
            HashMap::new();
        for workspace in &self.snapshot.workspaces {
            let worktree = workspace.worktree.as_ref();
            let checkout = worktree.map_or_else(
                || Path::new(""),
                |worktree| worktree.checkout_path.as_path(),
            );
            let repository = worktree.map(|worktree| &worktree.repository);
            let branch = branches
                .get(checkout)
                .copied()
                .unwrap_or(workspace.label.as_str());
            let path = if checkout.as_os_str().is_empty() {
                workspace_directories
                    .get(&workspace.workspace_id)
                    .copied()
                    .unwrap_or_else(|| Path::new(""))
            } else {
                checkout
            };
            workspace_repositories.insert(&workspace.workspace_id, (repository, branch));
            out.push(Destination::Workspace {
                id: workspace.workspace_id.clone(),
                repository: repository.cloned(),
                branch: branch.to_owned(),
                display_path: tilde(path, home),
                status: workspace.agent_status,
                focused: workspace.focused,
            });
        }

        for agent in &self.snapshot.agents {
            let (repository, branch) = workspace_repositories
                .get(&agent.workspace_id)
                .copied()
                .unwrap_or((None, "-"));
            out.push(Destination::Agent {
                pane: agent.pane_id.clone(),
                name: agent.display_name().to_owned(),
                status: agent.agent_status,
                repository: repository.cloned(),
                branch: branch.to_owned(),
                task: agent.terminal_title_stripped.clone().unwrap_or_default(),
            });
        }

        for list in &self.worktrees {
            for worktree in list.worktrees.iter().filter(|w| w.is_openable()) {
                out.push(Destination::Worktree {
                    path: worktree.checkout_path.clone(),
                    repository: list.source.repository.clone(),
                    branch: worktree.branch_label().to_owned(),
                    display_path: tilde(&worktree.checkout_path, home),
                });
            }
        }

        // Anything already reachable above is dropped, so one place never
        // appears twice under two names.
        let seen = self.reachable_paths(&workspace_directories);
        for dir in &self.dirs {
            if seen.contains(dir.as_path()) {
                continue;
            }
            out.push(Destination::Directory {
                path: dir.clone(),
                display_path: tilde(dir, home),
            });
        }

        out
    }

    fn branches_by_checkout(&self) -> HashMap<&Path, &str> {
        self.worktrees
            .iter()
            .flat_map(|list| &list.worktrees)
            .filter_map(|w| {
                w.branch
                    .as_deref()
                    .map(|branch| (w.checkout_path.as_path(), branch))
            })
            .collect()
    }

    fn reachable_paths<'a>(
        &'a self,
        workspace_directories: &HashMap<&'a WorkspaceId, &'a Path>,
    ) -> HashSet<&'a Path> {
        self.snapshot
            .workspaces
            .iter()
            .filter_map(|w| w.worktree.as_ref())
            .map(|w| w.checkout_path.as_path())
            .chain(workspace_directories.values().copied())
            .chain(
                self.worktrees
                    .iter()
                    .flat_map(|l| &l.worktrees)
                    .map(|w| w.checkout_path.as_path()),
            )
            .collect()
    }
}

/// Frecency order, which the picker keeps as the tie-break for an empty query.
fn zoxide_dirs() -> Vec<PathBuf> {
    optional_zoxide_dirs(OsStr::new("zoxide"))
}

fn optional_zoxide_dirs(program: &OsStr) -> Vec<PathBuf> {
    match query_zoxide(program) {
        Ok(dirs) => dirs,
        Err(error) => {
            eprintln!("find: leaving out zoxide directories: {error:#}");
            Vec::new()
        }
    }
}

fn query_zoxide(program: &OsStr) -> Result<Vec<PathBuf>> {
    let output = Command::new(program)
        .arg("query")
        .arg("-l")
        .output()
        .with_context(|| format!("running {} query -l", Path::new(program).display()))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!(
            "{} query -l failed with {}: {}",
            Path::new(program).display(),
            output.status,
            stderr.trim()
        );
    }
    parse_zoxide_output(&output.stdout)
}

fn parse_zoxide_output(output: &[u8]) -> Result<Vec<PathBuf>> {
    Ok(std::str::from_utf8(output)
        .context("zoxide query output was not valid UTF-8")?
        .lines()
        .filter(|line| !line.is_empty())
        .map(PathBuf::from)
        .collect())
}

fn tilde(path: &Path, home: &Path) -> String {
    if home.as_os_str().is_empty() {
        return path.to_string_lossy().into_owned();
    }
    if path == home {
        return "~".to_owned();
    }
    match path.strip_prefix(home) {
        Ok(rest) => format!("~/{}", rest.to_string_lossy()),
        Err(_) => path.to_string_lossy().into_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DestinationKind;

    fn sources() -> Sources {
        serde_json::from_str(include_str!("../tests/fixtures/sources.json")).unwrap()
    }

    fn built() -> Vec<Destination> {
        sources().destinations(Path::new("/home/dev"))
    }

    fn of_kind(kind: DestinationKind) -> Vec<Destination> {
        built().into_iter().filter(|d| d.kind() == kind).collect()
    }

    #[test]
    fn every_workspace_becomes_a_workspace_destination() {
        assert_eq!(of_kind(DestinationKind::Workspace).len(), 3);
    }

    #[test]
    fn a_workspace_takes_its_branch_from_the_worktree_listing() {
        let workspaces = of_kind(DestinationKind::Workspace);
        let branches: Vec<String> = workspaces
            .iter()
            .map(|d| match d {
                Destination::Workspace { branch, .. } => branch.clone(),
                _ => unreachable!(),
            })
            .collect();
        assert_eq!(branches, ["~", "main", "feature-branch"]);
    }

    #[test]
    fn a_workspace_with_no_checkout_falls_back_to_its_panes_cwd() {
        let workspaces = of_kind(DestinationKind::Workspace);
        let Some(Destination::Workspace { display_path, .. }) = workspaces.first() else {
            panic!("no workspaces");
        };
        assert_eq!(display_path, "~");
    }

    #[test]
    fn an_agent_inherits_the_repo_and_branch_of_its_workspace() {
        let agents = of_kind(DestinationKind::Agent);
        let pairs: Vec<(String, String)> = agents
            .iter()
            .map(|d| match d {
                Destination::Agent {
                    repository, branch, ..
                } => (
                    repository
                        .as_ref()
                        .map_or_else(|| "-".to_owned(), |repository| repository.name.clone()),
                    branch.clone(),
                ),
                _ => unreachable!(),
            })
            .collect();
        assert_eq!(
            pairs,
            [
                ("dots".to_owned(), "main".to_owned()),
                ("service".to_owned(), "feature-branch".to_owned())
            ]
        );
    }

    #[test]
    fn a_worktree_already_open_as_a_workspace_is_not_offered_twice() {
        let paths: Vec<PathBuf> = of_kind(DestinationKind::Worktree)
            .iter()
            .map(|d| match d {
                Destination::Worktree { path, .. } => path.clone(),
                _ => unreachable!(),
            })
            .collect();
        assert_eq!(paths, [PathBuf::from("/home/dev/wt/dots/release")]);
    }

    #[test]
    fn a_bare_or_prunable_worktree_is_left_out() {
        let display_paths: Vec<String> = of_kind(DestinationKind::Worktree)
            .iter()
            .map(|d| match d {
                Destination::Worktree { display_path, .. } => display_path.clone(),
                _ => unreachable!(),
            })
            .collect();
        assert!(!display_paths.iter().any(|path| path.contains("bare")));
        assert!(!display_paths.iter().any(|path| path.contains("stale")));
    }

    #[test]
    fn a_directory_reachable_another_way_is_dropped() {
        let paths: Vec<PathBuf> = of_kind(DestinationKind::Directory)
            .iter()
            .map(|d| match d {
                Destination::Directory { path, .. } => path.clone(),
                _ => unreachable!(),
            })
            .collect();
        assert_eq!(
            paths,
            [
                PathBuf::from("/home/dev/notes"),
                PathBuf::from("/opt/tools")
            ]
        );
    }

    #[test]
    fn paths_under_home_are_shown_with_a_tilde_and_others_are_not() {
        assert_eq!(tilde(Path::new("/home/dev"), Path::new("/home/dev")), "~");
        assert_eq!(
            tilde(Path::new("/home/dev/src"), Path::new("/home/dev")),
            "~/src"
        );
        assert_eq!(
            tilde(Path::new("/opt/tools"), Path::new("/home/dev")),
            "/opt/tools"
        );
        // A sibling directory whose name merely starts with the home path.
        assert_eq!(
            tilde(Path::new("/home/developer"), Path::new("/home/dev")),
            "/home/developer"
        );
    }

    #[test]
    fn a_missing_zoxide_is_diagnosable_but_optional() {
        let missing = OsStr::new("/definitely/not/a/herdr-zoxide-binary");
        let error = query_zoxide(missing).unwrap_err();
        assert!(
            format!("{error:#}").contains("running /definitely/not/a/herdr-zoxide-binary query -l")
        );
        assert!(optional_zoxide_dirs(missing).is_empty());
    }

    #[test]
    fn a_failed_zoxide_query_reports_its_exit_status() {
        let error = query_zoxide(OsStr::new("/usr/bin/false")).unwrap_err();
        assert!(format!("{error:#}").contains("query -l failed with exit status"));
    }

    #[test]
    fn invalid_zoxide_output_is_diagnosed_instead_of_lossily_decoded() {
        let error = parse_zoxide_output(b"/valid\n/bad-\xff\n").unwrap_err();
        assert!(format!("{error:#}").contains("not valid UTF-8"));
    }

    /// The popup is sized in `herdr-plugin.toml`, and its widest group spends
    /// `FIXED_CELLS` before the task column gets anything. Rendering at the
    /// narrowest terminal the manifest percentage could land on proves the
    /// column that carries the most information is not squeezed to nothing.
    #[test]
    fn an_agent_task_is_still_readable_at_the_popup_width() {
        let mut model = herdrkit::picker::Picker::new(built())
            .groups(crate::groups())
            .build()
            .unwrap();
        for ch in "plugin-dev".chars() {
            model.apply(herdrkit::picker::Command::Insert(ch));
        }
        let width = crate::FIXED_CELLS + 24;
        model.scroll_into_view(4);
        let drawn = model
            .lines(width, 4)
            .iter()
            .map(std::string::ToString::to_string)
            .collect::<String>();
        assert!(
            drawn.contains("Port the picker"),
            "task was squeezed out: {drawn:?}"
        );
    }

    fn matches(query: &str) -> usize {
        let mut model = herdrkit::picker::Picker::new(built())
            .groups(crate::groups())
            .build()
            .unwrap();
        for ch in query.chars() {
            model.apply(herdrkit::picker::Command::Insert(ch));
        }
        model.matched()
    }

    /// Neither the kind nor an agent's status occupies a column — one is a
    /// divider and the other a glyph, and the picker searches neither. They are
    /// in `hidden_terms` so that both stay typeable, which is the only thing
    /// keeping this from being a silent loss of two filters.
    ///
    /// Asserted with fzf exact atoms (`'word`). A fuzzy `space` also matches an
    /// agent, through `service` → `approval` → `force`, which is the matcher
    /// working correctly and not something to assert a count against.
    #[test]
    fn a_kind_and_a_status_are_typeable_without_being_columns() {
        assert_eq!(matches("'workspace"), 3);
        assert_eq!(matches("'agent"), 2, "both agents");
        assert_eq!(matches("'worktree"), 1);
        assert_eq!(matches("'agent 'working"), 1, "only the working one");
        assert_eq!(matches("'agent 'blocked"), 1, "only the blocked one");
    }

    /// Every kind is declared, and every row draws one cell per column of its
    /// group. The picker rejects either mistake, so building is the assertion.
    #[test]
    fn the_declared_groups_accept_every_destination() {
        let mut model = herdrkit::picker::Picker::new(built())
            .groups(crate::groups())
            .build()
            .expect("groups and cells agree");
        assert_eq!(model.matched(), 6, "workspaces, agents and worktrees");
        model.apply(herdrkit::picker::Command::Insert('/'));
        assert_eq!(model.matched(), 2, "just the directories");
    }
}
