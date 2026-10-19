use std::collections::{HashMap, HashSet};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context as _, Result, bail};
use herdrkit::api::{AgentStatus, Repository, Snapshot, Workspace, Worktrees};
use herdrkit::{Client, WorkspaceId};
use serde::Deserialize;

use crate::Destination;

/// Everything the index is built from, in one value, so the transform below is
/// a pure function over a fixture.
#[derive(Debug, Deserialize)]
pub struct Sources {
    pub snapshot: Snapshot,
    /// One listing per repo discovered through live workspaces. A repo with
    /// nothing open is reachable only as a directory.
    pub worktrees: Vec<Worktrees>,
    pub dirs: Vec<PathBuf>,
}

struct Checkout<'a> {
    repository: &'a Repository,
    path: &'a Path,
    linked: bool,
}

impl Sources {
    pub fn collect(client: &Client) -> Result<Self> {
        let snapshot = client.snapshot().context("reading the session")?;
        let worktrees = Self::collect_worktrees(client, &snapshot);
        Ok(Self {
            snapshot,
            worktrees,
            dirs: zoxide_dirs(),
        })
    }

    fn collect_worktrees(client: &Client, snapshot: &Snapshot) -> Vec<Worktrees> {
        let mut roots = snapshot.repository_roots();
        roots.extend(
            snapshot
                .workspaces
                .iter()
                .filter(|w| w.worktree.is_none())
                .filter_map(|w| snapshot.effective_workspace_dir(&w.workspace_id)),
        );
        roots.sort_unstable();
        roots.dedup();
        let mut repositories = HashSet::new();
        // A repo that has gone away since herdr saw it should cost that repo's
        // worktrees, not the whole list — but it must say so, or a socket
        // failure reads as "this repo has no worktrees" forever.
        roots
            .iter()
            .filter_map(|root| match client.worktree_list(root) {
                Ok(list) => repositories
                    .insert(list.source.repository.key.clone())
                    .then_some(list),
                // A plain workspace need not contain Git. Other failures remain
                // visible rather than masquerading as an empty repository.
                Err(herdrkit::Error::Rejected { code, .. }) if code == "not_git_worktree" => None,
                Err(error) => {
                    eprintln!(
                        "find: leaving out the worktrees of {}: {error:#}",
                        root.display()
                    );
                    None
                }
            })
            .collect()
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

        // Native discovery can identify a plain workspace absent from the
        // snapshot's worktree metadata. Only Herdr's open_workspace_id grants
        // that association; a pane directory alone is just a discovery hint.
        let mut workspace_repositories: HashMap<&WorkspaceId, (Option<&Repository>, &str)> =
            HashMap::new();
        for workspace in &self.snapshot.workspaces {
            let identity = self.workspace_checkout(workspace);
            let checkout = identity
                .as_ref()
                .map_or_else(|| Path::new(""), |checkout| checkout.path);
            let repository = identity.as_ref().map(|checkout| checkout.repository);
            let branch = branches.get(checkout).copied().unwrap_or("-");
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
                label: workspace.label.clone(),
                nested: identity.as_ref().is_some_and(|checkout| checkout.linked),
                repository: repository.cloned(),
                branch: branch.to_owned(),
                display_path: tilde(path, home),
                status: workspace.agent_status,
                focused: workspace.focused,
            });
        }

        arrange_workspaces(&mut out);
        let mut agents: Vec<_> = self.snapshot.agents.iter().collect();
        agents.sort_by_key(|agent| {
            (
                std::cmp::Reverse(attention(agent.agent_status)),
                std::cmp::Reverse(agent.state_change_seq),
            )
        });
        for agent in agents {
            let (repository, branch) = workspace_repositories
                .get(&agent.workspace_id)
                .copied()
                .unwrap_or((None, "-"));
            out.push(Destination::Agent {
                pane: agent.pane_id.clone(),
                name: agent.display_name().to_owned(),
                workspace_label: self
                    .snapshot
                    .workspace(&agent.workspace_id)
                    .map_or_else(String::new, |workspace| workspace.label.clone()),
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

    fn workspace_checkout<'a>(&'a self, workspace: &'a Workspace) -> Option<Checkout<'a>> {
        if let Some(worktree) = &workspace.worktree {
            return Some(Checkout {
                repository: &worktree.repository,
                path: &worktree.checkout_path,
                linked: worktree.is_linked_worktree,
            });
        }
        self.worktrees.iter().find_map(|list| {
            list.worktrees
                .iter()
                .find(|checkout| {
                    checkout.open_workspace_id.as_ref() == Some(&workspace.workspace_id)
                })
                .map(|checkout| Checkout {
                    repository: &list.source.repository,
                    path: &checkout.checkout_path,
                    linked: checkout.is_linked_worktree,
                })
        })
    }

    fn branches_by_checkout(&self) -> HashMap<&Path, &str> {
        self.worktrees
            .iter()
            .flat_map(|list| &list.worktrees)
            .map(|w| (w.checkout_path.as_path(), w.branch_label()))
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

fn attention(status: AgentStatus) -> u8 {
    match status {
        AgentStatus::Blocked => 4,
        AgentStatus::Done => 3,
        AgentStatus::Working => 2,
        AgentStatus::Idle => 1,
        AgentStatus::Unknown => 0,
    }
}

/// Emit a repository at its first member's position, with its parent first.
/// Repositories without an open parent keep their original order and labels.
fn arrange_workspaces(workspaces: &mut [Destination]) {
    let parents: HashSet<String> = workspaces
        .iter()
        .filter_map(|workspace| match workspace {
            Destination::Workspace {
                repository: Some(repository),
                nested: false,
                ..
            } => Some(repository.key.clone()),
            _ => None,
        })
        .collect();
    let mut anchors = HashMap::new();
    let mut positions = HashMap::new();
    for (index, workspace) in workspaces.iter_mut().enumerate() {
        if let Destination::Workspace {
            id,
            repository,
            nested,
            ..
        } = workspace
        {
            positions.insert(id.clone(), index);
            if let Some(repository) = repository
                .as_ref()
                .filter(|repo| parents.contains(&repo.key))
            {
                anchors.entry(repository.key.clone()).or_insert(index);
            } else {
                *nested = false;
            }
        }
    }
    workspaces.sort_by_key(|workspace| match workspace {
        Destination::Workspace {
            id,
            repository,
            nested,
            ..
        } => (
            repository
                .as_ref()
                .and_then(|repo| anchors.get(&repo.key))
                .copied()
                .or_else(|| positions.get(id).copied())
                .unwrap_or(usize::MAX),
            *nested,
        ),
        _ => (usize::MAX, false),
    });
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

    #[test]
    fn related_workspaces_follow_their_parent_at_the_first_members_position() {
        let mut sources = sources();
        let repository = sources
            .snapshot
            .workspaces
            .get(1)
            .unwrap()
            .worktree
            .as_ref()
            .unwrap()
            .repository
            .clone();
        let child = sources.snapshot.workspaces.get_mut(2).unwrap();
        child.label = "cafe\u{301} 👨‍👩‍👧".into();
        let checkout = child.worktree.as_mut().unwrap();
        checkout.repository = repository;
        checkout.checkout_path = "/home/dev/wt/dots/release".into();
        sources.snapshot.workspaces.rotate_right(1);
        let destinations = sources.destinations(Path::new("/home/dev"));
        let workspaces: Vec<_> = destinations
            .iter()
            .filter_map(|d| match d {
                Destination::Workspace { id, nested, .. } => Some((id.as_str(), *nested)),
                _ => None,
            })
            .collect();
        assert_eq!(workspaces, [("w2", false), ("w3", true), ("w1", false)]);
        let mut model = herdrkit::picker::Picker::new(destinations)
            .groups(crate::groups())
            .build()
            .unwrap();
        for ch in "'cafe\u{301}".chars() {
            model.apply(herdrkit::picker::Command::Insert(ch));
        }
        assert!(
            model
                .lines(116, 10)
                .iter()
                .any(|line| line.to_string().contains("└─ cafe\u{301} 👨‍👩‍👧"))
        );

        sources
            .snapshot
            .workspaces
            .retain(|workspace| workspace.workspace_id.as_str() != "w2");
        assert!(
            sources
                .destinations(Path::new("/home/dev"))
                .iter()
                .all(|d| !matches!(d, Destination::Workspace { nested: true, .. }))
        );
    }

    #[test]
    fn agents_order_by_attention_then_recency_with_stable_ties() {
        let mut sources = sources();
        let template = sources.snapshot.agents.first().unwrap().clone();
        sources.snapshot.agents = [
            (AgentStatus::Idle, 99),
            (AgentStatus::Blocked, 1),
            (AgentStatus::Working, 99),
            (AgentStatus::Done, 1),
            (AgentStatus::Blocked, 5),
            (AgentStatus::Unknown, 999),
            (AgentStatus::Blocked, 5),
        ]
        .into_iter()
        .enumerate()
        .map(|(index, (status, seq))| {
            let mut agent = template.clone();
            agent.pane_id = herdrkit::PaneId::new(format!("w2:p{index}"));
            agent.agent_status = status;
            agent.state_change_seq = seq;
            agent
        })
        .collect();
        let destinations = sources.destinations(Path::new("/home/dev"));
        let panes: Vec<_> = destinations
            .iter()
            .filter_map(|d| match d {
                Destination::Agent { pane, .. } => Some(pane.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(
            panes,
            [
                "w2:p4", "w2:p6", "w2:p1", "w2:p3", "w2:p2", "w2:p0", "w2:p5"
            ]
        );
        let mut model = herdrkit::picker::Picker::new(destinations.clone())
            .groups(crate::groups())
            .build()
            .unwrap();
        for expected in ["w2:p4", "w2:p6", "w2:p1", "w2:p3"] {
            let Some(Destination::Agent { pane, .. }) =
                model.selected().and_then(|index| destinations.get(index))
            else {
                panic!("attention agent should be selected before any workspace");
            };
            assert_eq!(pane.as_str(), expected);
            model.apply(herdrkit::picker::Command::Next(1));
        }
    }

    fn of_kind(kind: DestinationKind) -> Vec<Destination> {
        built().into_iter().filter(|d| d.kind() == kind).collect()
    }

    #[test]
    fn attention_rows_are_explicit_and_not_duplicated_in_agents() {
        use herdrkit::picker::{Command, Picker};

        let mut sources = sources();
        sources.snapshot.agents.first_mut().unwrap().agent_status = AgentStatus::Done;
        let mut model = Picker::new(sources.destinations(Path::new("/home/dev")))
            .groups(crate::groups())
            .build()
            .unwrap();
        assert_eq!(model.matched(), 6);
        let drawn = model
            .lines(116, 24)
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(drawn.contains("needs attention (2)"));
        assert!(drawn.contains("blocked"), "{drawn}");
        assert!(drawn.contains("done"));
        assert_eq!(drawn.matches("Port the picker").count(), 1);
        assert_eq!(drawn.matches("Waiting on approval").count(), 1);
        model.apply(Command::NextScope);
        assert_eq!(model.scope_label(), "needs attention");
        assert_eq!(model.matched(), 2);
        model.apply(Command::NextScope);
        model.apply(Command::NextScope);
        assert_eq!(model.scope_label(), "agents");
        assert_eq!(model.matched(), 0);
    }

    #[test]
    fn no_attention_preserves_workspace_first_navigation() {
        let mut sources = sources();
        for agent in &mut sources.snapshot.agents {
            agent.agent_status = AgentStatus::Idle;
        }
        let mut model = herdrkit::picker::Picker::new(sources.destinations(Path::new("/home/dev")))
            .groups(crate::groups())
            .build()
            .unwrap();
        assert!(
            !model
                .lines(116, 24)
                .iter()
                .any(|line| line.to_string().contains("needs attention"))
        );
        assert!(matches!(
            model.take_selected(),
            Some(Destination::Workspace { .. })
        ));
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
        assert_eq!(branches, ["-", "main", "feature-branch"]);
    }

    #[test]
    fn missing_checkout_identity_is_not_inferred_from_a_label_or_pane_directory() {
        let mut sources = sources();
        let workspace = sources.snapshot.workspaces.get_mut(1).unwrap();
        workspace.worktree = None;
        workspace.label = "cafe\u{301} 👨‍👩‍👧".into();
        sources
            .worktrees
            .first_mut()
            .unwrap()
            .worktrees
            .first_mut()
            .unwrap()
            .open_workspace_id = None;
        let destinations = sources.destinations(Path::new("/home/dev"));
        for destination in &destinations {
            match destination {
                Destination::Workspace {
                    id,
                    repository,
                    branch,
                    ..
                } if id.as_str() == "w2" => {
                    assert!(repository.is_none());
                    assert_eq!(branch, "-");
                }
                Destination::Agent {
                    pane,
                    repository,
                    branch,
                    ..
                } if pane.as_str() == "w2:p1" => {
                    assert!(repository.is_none());
                    assert_eq!(branch, "-");
                }
                _ => {}
            }
        }
        let mut model = herdrkit::picker::Picker::new(destinations)
            .groups(crate::groups())
            .build()
            .unwrap();
        for ch in "'cafe\u{301}".chars() {
            model.apply(herdrkit::picker::Command::Insert(ch));
        }
        assert_eq!(model.matched(), 2, "workspace label also finds its agent");
        model.scroll_into_view(24);
        let rows = model.lines(116, 24);
        assert!(
            rows.iter()
                .any(|line| line.to_string().contains("workspace"))
        );
        assert!(
            rows.iter()
                .any(|line| line.to_string().contains("cafe\u{301} 👨‍👩‍👧"))
        );
        for width in 0..=120 {
            for line in model.lines(width, 24) {
                assert_eq!(line.width(), width);
            }
        }
    }

    fn worktree_reply(index: usize) -> herdrkit::testing::Step {
        use serde_json::json;
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("../tests/fixtures/sources.json")).unwrap();
        let mut result = fixture
            .get("worktrees")
            .unwrap()
            .get(index)
            .unwrap()
            .clone();
        result
            .as_object_mut()
            .unwrap()
            .insert("type".into(), json!("worktree_list"));
        herdrkit::testing::Step::reply(json!({"id": "{id}", "result": result}).to_string())
    }

    #[test]
    fn native_discovery_identifies_plain_workspaces_and_their_agents() {
        use herdrkit::testing::{RecordedRequest, Server, Step};
        use serde_json::json;

        let mut sources = sources();
        sources.snapshot.workspaces.get_mut(1).unwrap().worktree = None;
        let server = Server::start(vec![
            Step::reply(
                r#"{"id":"{id}","error":{"code":"not_git_worktree","message":"not a Git work tree"}}"#,
            ),
            worktree_reply(0),
            worktree_reply(1),
        ]);
        sources.worktrees = Sources::collect_worktrees(&server.client(), &sources.snapshot);
        assert_eq!(
            server.requests(),
            ["/home/dev", "/home/dev/src/dots", "/home/dev/src/service"].map(|cwd| {
                RecordedRequest {
                    method: "worktree.list".into(),
                    params: json!({"cwd": cwd}),
                }
            })
        );
        assert!(
            sources
                .snapshot
                .workspaces
                .get(1)
                .unwrap()
                .worktree
                .is_none()
        );
        let destinations = sources.destinations(Path::new("/home/dev"));
        let workspace = destinations
            .iter()
            .find(|d| matches!(d, Destination::Workspace { id, .. } if id.as_str() == "w2"))
            .unwrap();
        let agent = destinations
            .iter()
            .find(|d| matches!(d, Destination::Agent { pane, .. } if pane.as_str() == "w2:p1"))
            .unwrap();
        for destination in [workspace, agent] {
            let (Destination::Workspace {
                repository, branch, ..
            }
            | Destination::Agent {
                repository, branch, ..
            }) = destination
            else {
                unreachable!();
            };
            assert_eq!(repository.as_ref().unwrap().name, "dots");
            assert_eq!(branch, "main");
        }
        assert_eq!(
            destinations
                .iter()
                .filter(|d| matches!(d, Destination::Worktree { .. }))
                .count(),
            1,
            "the open checkout is not offered twice"
        );
    }

    #[test]
    fn native_discovery_deduplicates_repositories_reached_from_different_directories() {
        use herdrkit::testing::{RecordedRequest, Server};
        use serde_json::json;

        let mut sources = sources();
        sources.snapshot.panes.first_mut().unwrap().cwd = Some("/home/dev/src/dots/subdir".into());
        let server = Server::start(vec![
            worktree_reply(0),
            worktree_reply(0),
            worktree_reply(1),
        ]);
        let worktrees = Sources::collect_worktrees(&server.client(), &sources.snapshot);
        assert_eq!(worktrees.len(), 2);
        assert_eq!(
            server.requests(),
            [
                "/home/dev/src/dots",
                "/home/dev/src/dots/subdir",
                "/home/dev/src/service"
            ]
            .map(|cwd| RecordedRequest {
                method: "worktree.list".into(),
                params: json!({"cwd": cwd}),
            })
        );
    }

    #[test]
    fn a_missing_listing_never_falls_back_to_workspace_labels() {
        let mut sources = sources();
        sources.worktrees.clear();
        for destination in sources.destinations(Path::new("/home/dev")) {
            if let Destination::Workspace { branch, .. } | Destination::Agent { branch, .. } =
                destination
            {
                assert_eq!(branch, "-");
            }
        }
    }

    #[test]
    fn detached_checkouts_are_identified_for_workspaces_and_agents() {
        let mut sources = self::sources();
        let worktree = sources
            .worktrees
            .first_mut()
            .unwrap()
            .worktrees
            .first_mut()
            .unwrap();
        worktree.branch = None;
        worktree.is_detached = true;
        let destinations = sources.destinations(Path::new("/home/dev"));
        let Destination::Workspace { branch, .. } = destinations.get(1).unwrap() else {
            panic!("expected workspace");
        };
        assert_eq!(branch, "(detached)");
        let agent = destinations.iter().find(|destination| {
            matches!(destination, Destination::Agent { pane, .. } if pane.as_str() == "w2:p1")
        }).unwrap();
        let Destination::Agent { branch, .. } = agent else {
            unreachable!();
        };
        assert_eq!(branch, "(detached)");
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
                ("service".to_owned(), "feature-branch".to_owned()),
                ("dots".to_owned(), "main".to_owned())
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
        assert_eq!(optional_zoxide_dirs(missing).len(), 0);
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

    /// Content columns yield to the task on smaller terminals.
    #[test]
    fn an_agent_task_is_still_readable_at_the_popup_width() {
        let mut model = herdrkit::picker::Picker::new(built())
            .groups(crate::groups())
            .build()
            .unwrap();
        for ch in "plugin-dev".chars() {
            model.apply(herdrkit::picker::Command::Insert(ch));
        }
        for width in [60, 80, 116] {
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
    }

    #[test]
    fn scopes_cycle_without_losing_the_query_and_directory_sigil_wins() {
        use herdrkit::picker::{Command, Picker};
        let mut model = Picker::new(built())
            .groups(crate::groups())
            .build()
            .unwrap();
        for (label, count) in [
            ("needs attention", 1),
            ("workspaces", 3),
            ("agents", 1),
            ("worktrees", 1),
            ("directories", 2),
            ("all", 6),
        ] {
            model.apply(Command::NextScope);
            assert_eq!(model.scope_label(), label);
            assert_eq!(model.matched(), count);
        }
        model.apply(Command::PreviousScope);
        assert_eq!(model.scope_label(), "directories");
        model.apply(Command::NextScope);
        for ch in "/notes".chars() {
            model.apply(Command::Insert(ch));
        }
        assert_eq!(model.matched(), 1);
        model.apply(Command::PreviousScope);
        assert_eq!(model.scope_label(), "worktrees");
        assert_eq!(model.query(), "notes");
        assert_eq!(model.matched(), 0);
        model.apply(Command::NextScope);
        assert_eq!(model.matched(), 1);
    }

    #[test]
    fn headings_and_selection_remain_aligned_at_narrow_widths() {
        use herdrkit::picker::{Command, Picker};
        let mut model = Picker::new(built())
            .groups(crate::groups())
            .build()
            .unwrap();
        for width in 0..=120 {
            model.scroll_into_view(24);
            for line in model.lines(width, 24) {
                assert_eq!(line.width(), width);
            }
        }
        model.apply(Command::NextScope);
        model.apply(Command::NextScope);
        model.apply(Command::NextScope);
        let rows = model.lines(116, 24);
        assert!(rows.iter().any(|line| line.to_string().contains("task")));
        assert!(rows.iter().any(|line| line.to_string().starts_with("▎ ")));
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

    /// Kind and attention aliases remain searchable across presentation groups.
    ///
    /// Asserted with fzf exact atoms (`'word`). A fuzzy `space` also matches an
    /// agent, through `service` → `approval` → `force`, which is the matcher
    /// working correctly and not something to assert a count against.
    #[test]
    fn kinds_statuses_and_attention_aliases_are_searchable() {
        assert_eq!(matches("'workspace"), 3);
        assert_eq!(matches("'agent"), 2, "both agents");
        assert_eq!(matches("'worktree"), 1);
        assert_eq!(matches("'agent 'working"), 1, "only the working one");
        assert_eq!(matches("'agent 'blocked"), 1, "only the blocked one");
        assert_eq!(matches("'attention"), 1, "attention is searchable");
        assert_eq!(matches("'waiting"), 1, "waiting is searchable");
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
