use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result, bail};
use herdrkit::api::{Snapshot, Worktree};
use herdrkit::{Invocation, WorkspaceId};
use review_core::{
    CapturedComparison, CapturedEndpoint, CheckoutKind, ComparisonSpec, DiffEndpoint, Observation,
    ObservationId, Repository, ReviewContext, ReviewContextId, RuntimeBinding, Store,
};

use crate::source::SourcePlan;

pub(crate) fn binding_is_live(
    source_repository: &Repository,
    snapshot: &Snapshot,
    herdr_server_id: &str,
    binding: &RuntimeBinding,
) -> Result<bool> {
    if binding.herdr_server_id != herdr_server_id {
        return Ok(false);
    }
    let Some(checkout) = snapshot.effective_workspace_dir(&binding.workspace_id) else {
        return Ok(false);
    };
    let repository = match Repository::discover(checkout) {
        Ok(repository) => repository,
        Err(review_core::Error::GitFailed { .. }) => return Ok(false),
        Err(error) => return Err(error).context("discovering a bound review checkout"),
    };
    if repository.common_git_dir() != source_repository.common_git_dir() {
        return Ok(false);
    }
    let store = Store::open(&repository).context("opening a bound review checkout")?;
    Ok(store.checkout_token() == binding.checkout_token)
}

/// Resumes a live workspace without prescribing its panes or tools.
pub(crate) fn focus_live(
    invocation: &Invocation,
    source_repository: &Repository,
    snapshot: &Snapshot,
    bindings: &[RuntimeBinding],
) -> Result<bool> {
    let herdr_server_id = invocation
        .client()
        .server_id()
        .context("identifying the Herdr server")?;
    for binding in bindings {
        if !binding_is_live(source_repository, snapshot, &herdr_server_id, binding)? {
            continue;
        }
        focus_binding(invocation, binding)?;
        return Ok(true);
    }
    Ok(false)
}

/// Materializes a freshly resolved provider-neutral plan. A matching inactive
/// checkout is reused; a changed observation follows the plan's strategy.
pub(crate) fn open_source(
    invocation: &Invocation,
    source_repository: &Repository,
    store: &mut Store,
    plan: &SourcePlan,
) -> Result<()> {
    let context = store
        .contexts_by_reference(&plan.external_reference)
        .context("looking up review contexts by external reference")?
        .into_iter()
        .next();
    let Some(mut context) = context else {
        return materialize_new_context(invocation, source_repository, plan);
    };
    if context.title != plan.label {
        context = store
            .set_context_title(&context.id, &plan.label)
            .context("refreshing the review context title")?;
    }

    open_context_plan(invocation, source_repository, store, &context, plan)
}

/// Refreshes the exact context selected by the reviewer. External references
/// are intentionally non-unique, so resolving a source again must not switch
/// to a newer context that happens to name the same source.
pub(crate) fn refresh_context(
    invocation: &Invocation,
    source_repository: &Repository,
    store: &mut Store,
    context: &ReviewContext,
    plan: &SourcePlan,
) -> Result<()> {
    if !context.references.contains(&plan.external_reference) {
        bail!(
            "source {} does not belong to review context {}",
            plan.external_reference.locator,
            context.id
        );
    }
    let context = if context.title == plan.label {
        context.clone()
    } else {
        store
            .set_context_title(&context.id, &plan.label)
            .context("refreshing the review context title")?
    };
    open_context_plan(invocation, source_repository, store, &context, plan)
}

fn open_context_plan(
    invocation: &Invocation,
    source_repository: &Repository,
    store: &mut Store,
    context: &ReviewContext,
    plan: &SourcePlan,
) -> Result<()> {
    let bindings = store
        .runtime_bindings(&context.id)
        .with_context(|| format!("reading runtime bindings for context {}", context.id))?;
    let observations = store
        .observations(&context.id)
        .with_context(|| format!("reading observations for context {}", context.id))?;
    let snapshot = invocation
        .client()
        .snapshot()
        .context("refreshing live Herdr workspaces")?;
    let herdr_server_id = invocation
        .client()
        .server_id()
        .context("identifying the Herdr server")?;
    if focus_matching_live(
        invocation,
        source_repository,
        &snapshot,
        &herdr_server_id,
        &bindings,
        &observations,
        &plan.comparison,
    )? {
        return Ok(());
    }
    if let Some((binding, observation)) = reusable_binding(
        source_repository,
        &bindings,
        &observations,
        &plan.comparison,
    )? {
        return open_existing_checkout(
            invocation,
            source_repository,
            context,
            &observation.id,
            Path::new(&binding.checkout_root),
            binding.checkout_kind,
        );
    }

    materialize_plan(invocation, source_repository, context, plan)
}

fn focus_matching_live(
    invocation: &Invocation,
    source_repository: &Repository,
    snapshot: &Snapshot,
    herdr_server_id: &str,
    bindings: &[RuntimeBinding],
    observations: &[Observation],
    comparison: &ComparisonSpec,
) -> Result<bool> {
    for binding in bindings {
        let Some(observation) = observations
            .iter()
            .find(|observation| observation.id == binding.observation_id)
        else {
            continue;
        };
        if comparison_matches(&observation.comparison, comparison)
            && binding_is_live(source_repository, snapshot, herdr_server_id, binding)?
        {
            focus_binding(invocation, binding)?;
            return Ok(true);
        }
    }
    Ok(false)
}

/// Reopens a context with no refreshable source using its latest valid stored
/// binding. The binding's immutable observation remains the comparison.
pub(crate) fn reopen_stored(
    invocation: &Invocation,
    source_repository: &Repository,
    context: &ReviewContext,
    bindings: &[RuntimeBinding],
) -> Result<()> {
    for binding in bindings {
        let checkout = Path::new(&binding.checkout_root);
        if checkout_is_valid(source_repository, checkout, &binding.checkout_token)? {
            return open_existing_checkout(
                invocation,
                source_repository,
                context,
                &binding.observation_id,
                checkout,
                binding.checkout_kind,
            );
        }
    }
    bail!(
        "review context {} has no source provider or reusable checkout",
        context.id
    )
}

fn materialize_plan(
    invocation: &Invocation,
    source_repository: &Repository,
    context: &ReviewContext,
    plan: &SourcePlan,
) -> Result<()> {
    let runtime = create_managed_runtime(invocation, source_repository, plan)?;
    finish_materialization(
        invocation,
        source_repository,
        context,
        &plan.comparison,
        &runtime,
    )
}

fn materialize_new_context(
    invocation: &Invocation,
    source_repository: &Repository,
    plan: &SourcePlan,
) -> Result<()> {
    let runtime = create_managed_runtime(invocation, source_repository, plan)?;
    let result = (|| {
        let repository = Repository::discover(&runtime.checkout)
            .context("discovering the materialized review checkout")?;
        if repository.common_git_dir() != source_repository.common_git_dir() {
            bail!("Herdr materialized the review in a different repository");
        }
        let mut store = Store::open(&repository)
            .context("opening the registry from the materialized checkout")?;
        let (context, observation) = store
            .create_context_with_observation(
                &plan.label,
                std::slice::from_ref(&plan.external_reference),
                &plan.comparison,
            )
            .context("creating the review context and first observation")?;
        bind_runtime(
            invocation,
            &mut store,
            &context.id,
            &observation.id,
            &runtime,
        )
    })();
    preserve_runtime_on_error(&runtime, result)
}

fn create_managed_runtime(
    invocation: &Invocation,
    source_repository: &Repository,
    plan: &SourcePlan,
) -> Result<CreatedRuntime> {
    if let Some(runtime) = recover_managed_runtime(invocation, source_repository, plan)? {
        return Ok(runtime);
    }

    let created = match invocation.client().worktree_create(
        None,
        Some(source_repository.checkout_root()),
        Some(&plan.checkout.branch),
        Some(&plan.checkout.commit),
        None,
        Some(&plan.checkout.label),
        false,
    ) {
        Ok(created) => created,
        Err(error) => {
            let error = anyhow::Error::from(error)
                .context(format!("creating a worktree for {}", plan.checkout.label));
            return match recover_managed_runtime(invocation, source_repository, plan) {
                Ok(Some(runtime)) => Ok(runtime),
                Ok(None) => Err(error),
                Err(recovery) => Err(error.context(format!(
                    "also failed to reconcile the worktree creation: {recovery:#}"
                ))),
            };
        }
    };
    let runtime = CreatedRuntime {
        checkout: created.worktree.checkout_path,
        workspace: created.workspace.workspace_id,
        kind: CheckoutKind::ManagedWorktree,
    };
    preserve_runtime_on_error(
        &runtime,
        validate_managed_checkout(source_repository, &runtime.checkout, plan),
    )?;
    Ok(runtime)
}

/// Reopens the exact review-owned branch left behind when a workspace closed
/// or Herdr finished creating a worktree after the caller lost its response.
fn recover_managed_runtime(
    invocation: &Invocation,
    source_repository: &Repository,
    plan: &SourcePlan,
) -> Result<Option<CreatedRuntime>> {
    let worktrees = invocation
        .client()
        .worktree_list(source_repository.checkout_root())
        .context("reading repository worktrees while reconciling worktree creation")?;
    let Some(worktree) = review_worktree(&worktrees.worktrees, &plan.checkout.branch)? else {
        return Ok(None);
    };
    validate_managed_checkout(source_repository, &worktree.checkout_path, plan)?;

    if let Some(workspace) = worktree
        .open_workspace_id
        .as_ref()
        .filter(|workspace| !workspace.as_str().is_empty())
        .cloned()
    {
        return Ok(Some(CreatedRuntime {
            checkout: worktree.checkout_path.clone(),
            workspace,
            kind: CheckoutKind::ManagedWorktree,
        }));
    }

    if !worktree.is_openable() {
        bail!(
            "review branch {} belongs to a worktree Herdr cannot open at {}",
            plan.checkout.branch,
            worktree.checkout_path.display()
        );
    }
    let opened = invocation
        .client()
        .worktree_open(
            source_repository.checkout_root(),
            &worktree.checkout_path,
            Some(&plan.checkout.label),
            false,
        )
        .with_context(|| {
            format!(
                "reopening review worktree {}",
                worktree.checkout_path.display()
            )
        })?;
    Ok(Some(CreatedRuntime {
        checkout: opened.worktree.checkout_path,
        workspace: opened.workspace.workspace_id,
        kind: CheckoutKind::ManagedWorktree,
    }))
}

fn review_worktree<'a>(worktrees: &'a [Worktree], branch: &str) -> Result<Option<&'a Worktree>> {
    let mut matches = worktrees.iter().filter(|worktree| {
        worktree.is_linked_worktree && worktree.branch.as_deref() == Some(branch)
    });
    let Some(worktree) = matches.next() else {
        return Ok(None);
    };
    if matches.next().is_some() {
        bail!("more than one worktree uses review branch {branch}");
    }
    Ok(Some(worktree))
}

fn validate_managed_checkout(
    source_repository: &Repository,
    checkout: &Path,
    plan: &SourcePlan,
) -> Result<()> {
    let repository = Repository::discover(checkout).context("discovering the review worktree")?;
    if repository.common_git_dir() != source_repository.common_git_dir() {
        bail!("Herdr materialized the review in a different repository");
    }
    let head = repository
        .head_oid()
        .context("reading the review worktree HEAD")?;
    if head != plan.checkout.commit {
        bail!(
            "review branch {} is at {}, expected {}",
            plan.checkout.branch,
            head,
            plan.checkout.commit
        );
    }
    Ok(())
}

fn open_existing_checkout(
    invocation: &Invocation,
    source_repository: &Repository,
    context: &ReviewContext,
    observation: &ObservationId,
    checkout: &Path,
    checkout_kind: CheckoutKind,
) -> Result<()> {
    let runtime = match checkout_kind {
        CheckoutKind::Existing => {
            let created = invocation
                .client()
                .workspace_create(checkout, Some(&format!("Review: {}", context.title)), false)
                .with_context(|| {
                    format!("creating a review workspace at {}", checkout.display())
                })?;
            CreatedRuntime {
                checkout: checkout.to_path_buf(),
                workspace: created.workspace.workspace_id,
                kind: CheckoutKind::Existing,
            }
        }
        CheckoutKind::ManagedWorktree => {
            let opened = invocation
                .client()
                .worktree_open(
                    source_repository.checkout_root(),
                    checkout,
                    Some(&format!("Review: {}", context.title)),
                    false,
                )
                .with_context(|| format!("opening review worktree {}", checkout.display()))?;
            CreatedRuntime {
                checkout: checkout.to_path_buf(),
                workspace: opened.workspace.workspace_id,
                kind: CheckoutKind::ManagedWorktree,
            }
        }
    };
    finish_binding(
        invocation,
        source_repository,
        &context.id,
        observation,
        &runtime,
    )
}

fn finish_materialization(
    invocation: &Invocation,
    source_repository: &Repository,
    context: &ReviewContext,
    comparison: &ComparisonSpec,
    runtime: &CreatedRuntime,
) -> Result<()> {
    let result = (|| {
        let repository = Repository::discover(&runtime.checkout)
            .context("discovering the materialized review checkout")?;
        if repository.common_git_dir() != source_repository.common_git_dir() {
            bail!("Herdr materialized the review in a different repository");
        }
        let mut store = Store::open(&repository)
            .context("opening the registry from the materialized checkout")?;
        let observation = store
            .record_observation(&context.id, comparison)
            .context("recording the review observation")?;
        bind_runtime(
            invocation,
            &mut store,
            &context.id,
            &observation.id,
            runtime,
        )
    })();
    preserve_runtime_on_error(runtime, result)
}

fn finish_binding(
    invocation: &Invocation,
    source_repository: &Repository,
    context: &ReviewContextId,
    observation: &ObservationId,
    runtime: &CreatedRuntime,
) -> Result<()> {
    let result = (|| {
        let repository =
            Repository::discover(&runtime.checkout).context("discovering the review checkout")?;
        if repository.common_git_dir() != source_repository.common_git_dir() {
            bail!("the stored review checkout belongs to a different repository");
        }
        let mut store = Store::open(&repository).context("opening the review registry")?;
        bind_runtime(invocation, &mut store, context, observation, runtime)
    })();
    preserve_runtime_on_error(runtime, result)
}

fn bind_runtime(
    invocation: &Invocation,
    store: &mut Store,
    context: &ReviewContextId,
    observation: &ObservationId,
    runtime: &CreatedRuntime,
) -> Result<()> {
    let binding = store
        .bind_runtime(
            context,
            observation,
            &invocation
                .client()
                .server_id()
                .context("identifying the Herdr server")?,
            &runtime.workspace,
            None,
            runtime.kind,
        )
        .context("binding the review runtime")?;
    focus_binding(invocation, &binding)
}

fn focus_binding(invocation: &Invocation, binding: &RuntimeBinding) -> Result<()> {
    invocation
        .client()
        .workspace_focus(&binding.workspace_id)
        .with_context(|| format!("focusing review workspace {}", binding.workspace_id))
}

fn reusable_binding<'a>(
    source_repository: &Repository,
    bindings: &'a [RuntimeBinding],
    observations: &'a [Observation],
    comparison: &ComparisonSpec,
) -> Result<Option<(&'a RuntimeBinding, &'a Observation)>> {
    for binding in bindings {
        let Some(observation) = observations
            .iter()
            .find(|observation| observation.id == binding.observation_id)
        else {
            continue;
        };
        if comparison_matches(&observation.comparison, comparison)
            && checkout_is_valid(
                source_repository,
                Path::new(&binding.checkout_root),
                &binding.checkout_token,
            )?
        {
            return Ok(Some((binding, observation)));
        }
    }
    Ok(None)
}

fn comparison_matches(captured: &CapturedComparison, requested: &ComparisonSpec) -> bool {
    endpoint_matches(&captured.base, &requested.base)
        && endpoint_matches(&captured.target, &requested.target)
}

fn endpoint_matches(captured: &CapturedEndpoint, requested: &DiffEndpoint) -> bool {
    match requested {
        DiffEndpoint::Commit { oid } => {
            matches!(captured.source, DiffEndpoint::Commit { .. }) && captured.oid == *oid
        }
        DiffEndpoint::Index | DiffEndpoint::WorkingTree => false,
    }
}

fn checkout_is_valid(
    source_repository: &Repository,
    checkout: &Path,
    expected_token: &str,
) -> Result<bool> {
    let repository = match Repository::discover(checkout) {
        Ok(repository) => repository,
        Err(review_core::Error::GitFailed { .. }) => return Ok(false),
        Err(error) => return Err(error).context("discovering a stored review checkout"),
    };
    if repository.common_git_dir() != source_repository.common_git_dir() {
        return Ok(false);
    }
    let store = Store::open(&repository).context("opening a stored review checkout")?;
    Ok(store.checkout_token() == expected_token)
}

fn preserve_runtime_on_error(runtime: &CreatedRuntime, result: Result<()>) -> Result<()> {
    result.with_context(|| {
        format!(
            "review workspace {} at {} has been preserved; reopen the review to resume",
            runtime.workspace,
            runtime.checkout.display()
        )
    })
}

#[derive(Debug)]
struct CreatedRuntime {
    checkout: PathBuf,
    workspace: WorkspaceId,
    kind: CheckoutKind,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn materialization_failure_preserves_runtime_and_explains_recovery() {
        let runtime = CreatedRuntime {
            checkout: PathBuf::from("/review-checkout"),
            workspace: WorkspaceId::new("w3"),
            kind: CheckoutKind::ManagedWorktree,
        };
        let error =
            preserve_runtime_on_error(&runtime, Err(anyhow::anyhow!("editor response lost")))
                .unwrap_err();
        let message = format!("{error:#}");
        assert!(message.contains("w3 at /review-checkout has been preserved"));
        assert!(message.contains("editor response lost"));
        assert!(preserve_runtime_on_error(&runtime, Ok(())).is_ok());
    }

    fn worktree(branch: &str, linked: bool) -> Worktree {
        serde_json::from_value(serde_json::json!({
            "path": format!("/worktrees/{}", branch.replace('/', "-")),
            "label": "repo",
            "is_bare": false,
            "is_detached": false,
            "is_prunable": false,
            "is_linked_worktree": linked,
            "branch": branch,
        }))
        .unwrap()
    }

    #[test]
    fn review_worktree_identity_ignores_unrelated_and_primary_checkouts() {
        let worktrees = [
            worktree("main", false),
            worktree("worktree/unrelated", true),
            worktree("herdr-review/exact", true),
        ];

        let matched = review_worktree(&worktrees, "herdr-review/exact")
            .unwrap()
            .unwrap();
        assert_eq!(matched.branch.as_deref(), Some("herdr-review/exact"));
        assert!(
            review_worktree(&worktrees, "herdr-review/missing")
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn duplicate_review_worktree_identity_fails_loudly() {
        let worktrees = [
            worktree("herdr-review/exact", true),
            worktree("herdr-review/exact", true),
        ];
        assert!(review_worktree(&worktrees, "herdr-review/exact").is_err());
    }

    #[test]
    fn only_exact_immutable_comparisons_reuse_a_checkout() {
        let captured = CapturedComparison {
            base: CapturedEndpoint {
                source: DiffEndpoint::Commit {
                    oid: "a".repeat(40),
                },
                oid: "a".repeat(40),
            },
            target: CapturedEndpoint {
                source: DiffEndpoint::Commit {
                    oid: "b".repeat(40),
                },
                oid: "b".repeat(40),
            },
        };
        let same = ComparisonSpec::new(
            DiffEndpoint::Commit {
                oid: "a".repeat(40),
            },
            DiffEndpoint::Commit {
                oid: "b".repeat(40),
            },
        )
        .unwrap();
        let changed = ComparisonSpec::new(
            DiffEndpoint::Commit {
                oid: "a".repeat(40),
            },
            DiffEndpoint::Commit {
                oid: "c".repeat(40),
            },
        )
        .unwrap();
        let mutable = ComparisonSpec::new(
            DiffEndpoint::Commit {
                oid: "a".repeat(40),
            },
            DiffEndpoint::WorkingTree,
        )
        .unwrap();

        assert!(comparison_matches(&captured, &same));
        assert!(!comparison_matches(&captured, &changed));
        assert!(!comparison_matches(&captured, &mutable));
    }
}
