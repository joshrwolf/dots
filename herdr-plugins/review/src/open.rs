use std::io::{self, Write as _};
use std::path::Path;

use anyhow::{Context as _, Result, bail};
use github_client::{Client as GitHub, GitHubRepository, Target as GitHubTarget};
use herdrkit::Invocation;
use review_core::{Repository, Store};

use crate::catalog::RepositoryCatalog;
use crate::inventory::{self, KnownRepository};
use crate::materializer;
use crate::picker::{self, Selection};
use crate::source;

/// Opens the Herdr-global review picker and activates the selected review.
/// Repository state is opened only after discovery finds an existing registry
/// or the reviewer explicitly starts work in that repository.
pub fn open_review(invocation: &Invocation) -> Result<()> {
    let snapshot = invocation
        .client()
        .snapshot()
        .context("reading live Herdr workspaces")?;
    let herdr_server_id = invocation
        .client()
        .server_id()
        .context("identifying the Herdr server")?;
    let catalog = RepositoryCatalog::open(invocation.state_dir())?;
    let current_repository = invocation.target_dir().and_then(discover_optional);
    let repositories = inventory::collect(
        &snapshot,
        current_repository.as_ref().map(Repository::checkout_root),
        &catalog,
    )?;

    if let Some(target) = std::env::var_os("HERDR_REVIEW_PR") {
        let target = target
            .to_str()
            .context("HERDR_REVIEW_PR is not valid UTF-8")?;
        let target =
            source::parse_target(target)?.context("HERDR_REVIEW_PR must name a pull request")?;
        return open_target(
            invocation,
            &catalog,
            &repositories,
            current_repository.as_ref(),
            &target,
        );
    }

    let Some(selection) = picker::choose(
        &repositories,
        current_repository.as_ref(),
        &snapshot,
        &herdr_server_id,
    )?
    else {
        return Ok(());
    };

    match selection {
        Selection::Source(action) => {
            let Some(target) = source::target_for_action(action)? else {
                return Ok(());
            };
            open_target(
                invocation,
                &catalog,
                &repositories,
                current_repository.as_ref(),
                &target,
            )
        }
        Selection::Review(review) => {
            let picker::SelectedReview {
                repository,
                context,
                bindings,
            } = *review;
            let mut store = Store::open(&repository.repository)
                .with_context(|| format!("opening reviews for {}", repository.name))?;
            if materializer::focus_live(invocation, &repository.repository, &snapshot, &bindings)? {
                return Ok(());
            }
            if let Some(plan) =
                source::refresh(repository.repository.checkout_root(), &context.references)?
            {
                materializer::refresh_context(
                    invocation,
                    &repository.repository,
                    &mut store,
                    &context,
                    &plan,
                )
            } else {
                materializer::reopen_stored(invocation, &repository.repository, &context, &bindings)
            }
        }
    }
}

fn open_target(
    invocation: &Invocation,
    catalog: &RepositoryCatalog,
    repositories: &[KnownRepository],
    current_repository: Option<&Repository>,
    target: &GitHubTarget,
) -> Result<()> {
    let repository = if let Some(identity) = source::target_repository(target)? {
        choose_target_repository(repositories, &identity)?
            .map_or_else(|| prompt_target_repository(&identity), Ok)?
    } else {
        let current = current_repository.context(
            "a pull request number or current-branch lookup needs a current Git repository; use a full pull request URL to open another repository",
        )?;
        repositories
            .iter()
            .find(|known| known.repository.common_git_dir() == current.common_git_dir())
            .cloned()
            .context("the current repository disappeared from the review inventory")?
    };
    catalog
        .remember(&repository.repository, &repository.name)
        .context("remembering the selected review repository")?;
    let plan = source::resolve(repository.repository.checkout_root(), target)?;
    let mut store = Store::open(&repository.repository)
        .with_context(|| format!("opening reviews for {}", repository.name))?;
    materializer::open_source(invocation, &repository.repository, &mut store, &plan)
}

fn choose_target_repository(
    repositories: &[KnownRepository],
    identity: &GitHubRepository,
) -> Result<Option<KnownRepository>> {
    let github = GitHub;
    let mut matches = Vec::new();
    for known in repositories {
        let identities = match github.repositories(known.repository.checkout_root()) {
            Ok(identities) => identities,
            Err(error) => {
                eprintln!(
                    "review: could not inspect remotes for {}: {error}",
                    known.repository.checkout_root().display()
                );
                continue;
            }
        };
        if identities
            .iter()
            .any(|candidate| candidate.same_repository(identity))
        {
            matches.push(known.clone());
        }
    }
    match matches.len() {
        0 => Ok(None),
        1 => Ok(matches.pop()),
        _ => picker::choose_repository(matches),
    }
}

fn prompt_target_repository(identity: &GitHubRepository) -> Result<KnownRepository> {
    print!(
        "Local checkout for {} (Herdr will not clone it): ",
        identity.display()
    );
    io::stdout()
        .flush()
        .context("drawing the checkout prompt")?;
    let mut value = String::new();
    io::stdin()
        .read_line(&mut value)
        .context("reading the checkout path")?;
    let value = value.trim();
    if value.is_empty() {
        bail!("no local checkout was selected for {}", identity.display());
    }
    let repository = Repository::discover(Path::new(value))
        .with_context(|| format!("discovering local checkout {value}"))?;
    let matches = GitHub
        .repositories(repository.checkout_root())
        .context("reading the selected checkout remotes")?
        .iter()
        .any(|candidate| candidate.same_repository(identity));
    if !matches {
        bail!(
            "{} has no Git remote for {}",
            repository.checkout_root().display(),
            identity.display()
        );
    }
    let name = repository
        .checkout_root()
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("repository")
        .to_owned();
    Ok(KnownRepository {
        repository,
        name,
        workspaces: Vec::new(),
    })
}

fn discover_optional(path: &Path) -> Option<Repository> {
    match Repository::discover(path) {
        Ok(repository) => Some(repository),
        Err(error) => {
            eprintln!(
                "review: current directory {} is not a Git checkout: {error}",
                path.display()
            );
            None
        }
    }
}
