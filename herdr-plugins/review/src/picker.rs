use std::borrow::Cow;
use std::path::Path;

use anyhow::{Context as _, Result};
use herdrkit::Theme;
use herdrkit::api::Snapshot;
use herdrkit::picker::{Cell, Column, Entry, Group, Picker};
use review_core::{ContextListQuery, Repository, ReviewContext, RuntimeBinding, Store};

use crate::inventory::KnownRepository;
use crate::materializer::binding_is_live;
use crate::source::SourceAction;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CandidateGroup {
    Active,
    Recent,
    Start,
}

#[derive(Debug, Clone)]
struct ReviewActivity {
    repository: KnownRepository,
    context: ReviewContext,
    bindings: Vec<RuntimeBinding>,
    open_threads: u64,
    live_workspaces: Vec<String>,
}

#[derive(Debug, Clone)]
enum Candidate {
    Review(Box<ReviewActivity>),
    Source {
        action: SourceAction,
        repository: String,
    },
}

#[derive(Debug, Clone)]
pub(crate) enum Selection {
    Review(Box<SelectedReview>),
    Source(SourceAction),
}

#[derive(Debug, Clone)]
pub(crate) struct SelectedReview {
    pub repository: KnownRepository,
    pub context: ReviewContext,
    pub bindings: Vec<RuntimeBinding>,
}

impl Candidate {
    fn group_key(&self) -> CandidateGroup {
        match self {
            Self::Review(activity) if !activity.live_workspaces.is_empty() => {
                CandidateGroup::Active
            }
            Self::Review(_) => CandidateGroup::Recent,
            Self::Source { .. } => CandidateGroup::Start,
        }
    }
}

impl Entry for Candidate {
    type Group = CandidateGroup;

    fn group(&self) -> Self::Group {
        self.group_key()
    }

    fn cells(&self, theme: &Theme) -> Vec<Cell> {
        match self {
            Self::Review(activity) => {
                let active = !activity.live_workspaces.is_empty();
                let runtime = if active {
                    activity.live_workspaces.join(", ")
                } else {
                    activity
                        .bindings
                        .first()
                        .map_or_else(|| "not open".to_owned(), binding_label)
                };
                vec![
                    Cell::tag(if active { "◆" } else { "" }, theme.green),
                    Cell::new(activity.repository.display_name(), theme.accent),
                    Cell::new(activity.context.title.clone(), theme.strong),
                    Cell::new(reference_label(&activity.context), theme.blue),
                    Cell::new(runtime, if active { theme.green } else { theme.muted }),
                    Cell::tag(thread_label(activity.open_threads), theme.muted),
                ]
            }
            Self::Source {
                action: SourceAction::CurrentBranchPullRequest,
                repository,
            } => new_cells(
                theme,
                repository,
                "Current branch pull request",
                "resolve with GitHub",
            ),
            Self::Source {
                action: SourceAction::ExplicitPullRequest,
                ..
            } => new_cells(
                theme,
                "any repository",
                "Open GitHub pull request…",
                "number or URL",
            ),
        }
    }

    fn hidden_terms(&self) -> Option<Cow<'_, str>> {
        Some(match self {
            Self::Review(activity) => format!(
                "{} {} {} {} {}",
                activity.context.id,
                activity.repository.repository.checkout_root().display(),
                activity.repository.repository.common_git_dir().display(),
                activity
                    .context
                    .references
                    .iter()
                    .map(|reference| reference.locator.as_str())
                    .collect::<Vec<_>>()
                    .join(" "),
                activity.live_workspaces.join(" ")
            )
            .into(),
            Self::Source {
                action: SourceAction::CurrentBranchPullRequest,
                repository,
            } => format!("new github pr branch {repository}").into(),
            Self::Source {
                action: SourceAction::ExplicitPullRequest,
                ..
            } => Cow::Borrowed("new github pr number url repository"),
        })
    }
}

pub(crate) fn choose(
    repositories: &[KnownRepository],
    current_repository: Option<&Repository>,
    snapshot: &Snapshot,
    herdr_server_id: &str,
) -> Result<Option<Selection>> {
    let mut candidates = Vec::new();
    for known in repositories {
        if !known.repository.database_path().is_file() {
            continue;
        }
        let store = match Store::open(&known.repository) {
            Ok(store) => store,
            Err(error) => {
                eprintln!(
                    "review: leaving out review state for {}: {error}",
                    known.repository.database_path().display()
                );
                continue;
            }
        };
        for listing in store
            .list_contexts(ContextListQuery::default())
            .with_context(|| format!("reading reviews for {}", known.display_name()))?
        {
            let live_workspaces = live_workspace_labels(
                &known.repository,
                snapshot,
                herdr_server_id,
                &listing.runtime_bindings,
            )?;
            candidates.push(Candidate::Review(Box::new(ReviewActivity {
                repository: known.clone(),
                context: listing.context,
                bindings: listing.runtime_bindings,
                open_threads: listing.open_threads,
                live_workspaces,
            })));
        }
    }
    if let Some(current) = current_repository
        && let Some(known) = repositories
            .iter()
            .find(|known| known.repository.common_git_dir() == current.common_git_dir())
    {
        candidates.push(Candidate::Source {
            action: SourceAction::CurrentBranchPullRequest,
            repository: known.display_name(),
        });
    }
    candidates.push(Candidate::Source {
        action: SourceAction::ExplicitPullRequest,
        repository: String::new(),
    });

    Picker::new(candidates)
        .groups(groups())
        .prompt("review  ")
        .match_paths()
        .run()
        .context("running the review picker")
        .map(|selected| {
            selected.map(|candidate| match candidate {
                Candidate::Review(activity) => Selection::Review(Box::new(SelectedReview {
                    repository: activity.repository,
                    context: activity.context,
                    bindings: activity.bindings,
                })),
                Candidate::Source { action, .. } => Selection::Source(action),
            })
        })
}

#[derive(Debug, Clone)]
struct RepositoryCandidate(KnownRepository);

impl Entry for RepositoryCandidate {
    type Group = ();

    fn group(&self) -> Self::Group {}

    fn cells(&self, theme: &Theme) -> Vec<Cell> {
        let workspaces = if self.0.workspaces.is_empty() {
            "not open".to_owned()
        } else {
            self.0
                .workspaces
                .iter()
                .map(|workspace| format!("{} ({})", workspace.label, workspace.id))
                .collect::<Vec<_>>()
                .join(", ")
        };
        vec![
            Cell::new(self.0.display_name(), theme.strong),
            Cell::new(
                self.0.repository.checkout_root().display().to_string(),
                theme.muted,
            ),
            Cell::new(workspaces, theme.green),
        ]
    }

    fn hidden_terms(&self) -> Option<Cow<'_, str>> {
        Some(
            self.0
                .repository
                .common_git_dir()
                .display()
                .to_string()
                .into(),
        )
    }
}

pub(crate) fn choose_repository(
    repositories: Vec<KnownRepository>,
) -> Result<Option<KnownRepository>> {
    Picker::new(repositories.into_iter().map(RepositoryCandidate).collect())
        .group(Group::new(
            (),
            "matching clones",
            vec![Column::new(22), Column::fill(), Column::new(24)],
        ))
        .prompt("clone  ")
        .match_paths()
        .run()
        .context("choosing a repository clone")
        .map(|selected| selected.map(|candidate| candidate.0))
}

fn live_workspace_labels(
    repository: &Repository,
    snapshot: &Snapshot,
    herdr_server_id: &str,
    bindings: &[RuntimeBinding],
) -> Result<Vec<String>> {
    let mut labels = Vec::new();
    for binding in bindings {
        if !binding_is_live(repository, snapshot, herdr_server_id, binding)? {
            continue;
        }
        let Some(workspace) = snapshot.workspace(&binding.workspace_id) else {
            continue;
        };
        let label = format!("{} ({})", workspace.label, workspace.workspace_id);
        if !labels.contains(&label) {
            labels.push(label);
        }
    }
    Ok(labels)
}

fn new_cells(theme: &Theme, repository: &str, title: &str, detail: &str) -> Vec<Cell> {
    vec![
        Cell::tag("+", theme.accent),
        Cell::new(repository, theme.accent),
        Cell::new(title, theme.strong),
        Cell::new(detail, theme.blue),
        Cell::tag("GitHub", theme.muted),
        Cell::tag("", theme.muted),
    ]
}

fn groups() -> Vec<Group<CandidateGroup>> {
    let group = |key, label| {
        Group::new(
            key,
            label,
            vec![
                Column::new(2),
                Column::new(18),
                Column::fill(),
                Column::new(24),
                Column::new(22),
                Column::new(10),
            ],
        )
    };
    vec![
        group(CandidateGroup::Active, "active reviews"),
        group(CandidateGroup::Recent, "recent reviews"),
        group(CandidateGroup::Start, "start review"),
    ]
}

fn reference_label(context: &ReviewContext) -> String {
    context.references.first().map_or_else(
        || "local".to_owned(),
        |reference| short_reference(&reference.locator),
    )
}

fn short_reference(locator: &str) -> String {
    let Some((prefix, number)) = locator.rsplit_once("/pull/") else {
        return locator.to_owned();
    };
    let repository = prefix
        .trim_end_matches('/')
        .rsplit_once('/')
        .map_or(prefix, |(_, repository)| repository);
    format!("{repository}#{number}")
}

fn thread_label(count: u64) -> String {
    match count {
        1 => "1 thread".to_owned(),
        count => format!("{count} threads"),
    }
}

fn binding_label(binding: &RuntimeBinding) -> String {
    let path = Path::new(&binding.checkout_root);
    path.file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(&binding.checkout_root)
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_actions_are_grouped_after_reviews() {
        assert_eq!(
            Candidate::Source {
                action: SourceAction::CurrentBranchPullRequest,
                repository: "repo".to_owned(),
            }
            .group(),
            CandidateGroup::Start
        );
    }

    #[test]
    fn github_reference_is_compact_but_still_specific() {
        assert_eq!(
            short_reference("https://github.com/example/project/pull/42"),
            "project#42"
        );
    }
}
