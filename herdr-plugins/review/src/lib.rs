//! Herdr-facing coordination for repository review activity.

mod bridge;
mod catalog;
mod context;
mod inventory;
mod materializer;
mod open;
mod picker;
mod source;

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use anyhow::{Context as _, Result};
use herdrkit::api::{AgentRef, AgentStatus};
use review_core::{
    AgentAssignment, CapturedComparison, DispatchAttemptId, DispatchJob, DispatchOutcome,
    Repository, RuntimeBindingId, Store,
};
use serde::Serialize;

pub use bridge::serve_agent;
pub use bridge::serve_stdio;
pub use open::open_review;

const DELIVERY_TIMEOUT: Duration = Duration::from_secs(2 * 60 * 60);
const DELIVERY_SCAN_INTERVAL: Duration = Duration::from_secs(1);
const DELIVERY_RETRY_INITIAL: Duration = Duration::from_millis(250);
const DELIVERY_RETRY_MAX: Duration = Duration::from_secs(30);
const OUTCOME_RETRY_DELAYS: &[Duration] = &[
    Duration::ZERO,
    Duration::from_millis(100),
    Duration::from_millis(500),
];
const MAX_PROMPT_BYTES: usize = 512 * 1024;
#[cfg(not(test))]
const CLAIM_LEASE: Duration = Duration::from_secs(5 * 60);
#[cfg(test)]
const CLAIM_LEASE: Duration = Duration::from_secs(1);
#[cfg(not(test))]
const CLAIM_HEARTBEAT: Duration = Duration::from_secs(30);
#[cfg(test)]
const CLAIM_HEARTBEAT: Duration = Duration::from_millis(100);

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct AgentChoice {
    pub label: String,
    pub status: AgentStatus,
    pub assignment: AgentAssignment,
}

fn agents_with_client(
    repository: &Repository,
    binding: &review_core::RuntimeBinding,
    client: &herdrkit::Client,
) -> Result<Vec<AgentChoice>> {
    let snapshot = client.snapshot().context("reading the Herdr session")?;
    let directory = snapshot
        .effective_workspace_dir(&binding.workspace_id)
        .with_context(|| format!("Herdr workspace {} is no longer open", binding.workspace_id))?;
    if !checkout_matches(directory, repository, &binding.checkout_token)? {
        anyhow::bail!(
            "Herdr workspace {} now points at a different checkout",
            binding.workspace_id
        );
    }
    let server_id = client.server_id().context("identifying the Herdr server")?;

    let mut choices = Vec::new();
    for agent in snapshot
        .agents
        .iter()
        .filter(|agent| agent.workspace_id == binding.workspace_id)
    {
        match AgentAssignment::new(
            &server_id,
            agent.workspace_id.clone(),
            agent.pane_id.clone(),
            agent.name.clone(),
            agent.agent_kind.clone(),
        ) {
            Ok(assignment) => choices.push(AgentChoice {
                label: agent.display_name().to_owned(),
                status: agent.agent_status,
                assignment,
            }),
            Err(error) => eprintln!(
                "excluding invalid Herdr agent assignment for pane {}: {error}",
                agent.pane_id
            ),
        }
    }
    Ok(choices)
}

fn checkout_matches(directory: &Path, expected: &Repository, expected_token: &str) -> Result<bool> {
    match Repository::discover(directory) {
        Ok(actual) => {
            if actual.common_git_dir() != expected.common_git_dir() {
                return Ok(false);
            }
            match std::fs::read_to_string(actual.checkout_token_path()) {
                Ok(token) => Ok(token == expected_token),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
                Err(error) => Err(error).context("reading the workspace checkout identity"),
            }
        }
        Err(review_core::Error::GitFailed { .. }) => Ok(false),
        Err(error) => Err(error).context("discovering the workspace repository"),
    }
}

/// Owns one delivery worker for each binding used by this stdio bridge.
#[derive(Debug)]
struct DeliverySupervisors {
    repository: Repository,
    client: herdrkit::Client,
    workers: BTreeMap<RuntimeBindingId, DeliverySupervisor>,
    start: SupervisorStarter,
}

type SupervisorStarter =
    fn(&Repository, &RuntimeBindingId, &herdrkit::Client) -> Result<DeliverySupervisor>;

impl DeliverySupervisors {
    fn new(repository: Repository, client: herdrkit::Client) -> Self {
        Self {
            repository,
            client,
            workers: BTreeMap::new(),
            start: DeliverySupervisor::start,
        }
    }

    #[cfg(test)]
    fn with_starter(
        repository: Repository,
        client: herdrkit::Client,
        start: SupervisorStarter,
    ) -> Self {
        Self {
            repository,
            client,
            workers: BTreeMap::new(),
            start,
        }
    }

    fn client(&self) -> &herdrkit::Client {
        &self.client
    }

    fn ensure(&mut self, binding: &RuntimeBindingId) -> Result<()> {
        let Self {
            repository,
            client,
            workers,
            start,
        } = self;
        ensure_worker(workers, binding, || start(repository, binding, client))
    }

    fn wake_after_commit(&mut self, binding: &RuntimeBindingId, attempt: &DispatchAttemptId) {
        if let Err(error) = self.ensure(binding) {
            eprintln!(
                "agent dispatch {attempt} was saved but immediate supervision is unavailable: {error:#}"
            );
        }
    }

    fn resume_best_effort(&mut self, binding: &RuntimeBindingId) {
        if let Err(error) = self.ensure(binding) {
            eprintln!(
                "could not resume agent dispatch supervision for binding {binding}: {error:#}"
            );
        }
    }
}

fn ensure_worker(
    workers: &mut BTreeMap<RuntimeBindingId, DeliverySupervisor>,
    binding: &RuntimeBindingId,
    start: impl FnOnce() -> Result<DeliverySupervisor>,
) -> Result<()> {
    if workers
        .get(binding)
        .is_some_and(|supervisor| supervisor.wake().is_ok())
    {
        return Ok(());
    }

    workers.remove(binding);
    let supervisor = start()?;
    supervisor.wake()?;
    workers.insert(binding.clone(), supervisor);
    Ok(())
}

#[derive(Debug)]
struct DeliverySupervisor {
    worker_signal: mpsc::Sender<WorkerSignal>,
}

#[derive(Debug, Clone, Copy)]
enum WorkerSignal {
    Wake,
    Stop,
}

impl DeliverySupervisor {
    fn start(
        repository: &Repository,
        binding: &RuntimeBindingId,
        client: &herdrkit::Client,
    ) -> Result<Self> {
        let worker_store = Store::open(repository).context("initializing review dispatch store")?;
        let claimant = format!("review-stdio-{}-{binding}", std::process::id());
        let (worker_signal, receiver) = mpsc::channel();
        let worker_repository = repository.clone();
        let worker_binding = binding.clone();
        let worker_client = client.clone();
        thread::Builder::new()
            .name("herdr-review-dispatch".to_owned())
            .spawn(move || {
                delivery_worker(
                    worker_store,
                    &worker_repository,
                    &worker_binding,
                    &claimant,
                    &worker_client,
                    &receiver,
                );
            })
            .context("spawning the review dispatch worker")?;
        Ok(Self { worker_signal })
    }

    fn wake(&self) -> Result<()> {
        self.worker_signal
            .send(WorkerSignal::Wake)
            .context("the review dispatch worker stopped")
    }
}

impl Drop for DeliverySupervisor {
    fn drop(&mut self) {
        let _ = self.worker_signal.send(WorkerSignal::Stop);
    }
}

fn delivery_worker(
    worker_store: Store,
    repository: &Repository,
    binding: &RuntimeBindingId,
    claimant: &str,
    client: &herdrkit::Client,
    signals: &mpsc::Receiver<WorkerSignal>,
) {
    let mut store = Some(worker_store);
    supervise_delivery_scans(signals, || {
        scan_deliveries_with_store(&mut store, repository, binding, claimant, client)
    });
}

fn scan_deliveries_with_store(
    store: &mut Option<Store>,
    repository: &Repository,
    binding: &RuntimeBindingId,
    claimant: &str,
    client: &herdrkit::Client,
) -> Result<()> {
    if store.is_none() {
        *store = Some(Store::open(repository).context("opening review dispatch store")?);
    }
    let result = scan_deliveries(
        store
            .as_mut()
            .context("review dispatch store was not initialized")?,
        repository,
        binding,
        claimant,
        client,
    );
    if result.is_err() {
        *store = None;
    }
    result
}

fn supervise_delivery_scans(
    signals: &mpsc::Receiver<WorkerSignal>,
    mut scan: impl FnMut() -> Result<()>,
) {
    let mut retry_delay = DELIVERY_RETRY_INITIAL;
    loop {
        let wait = match scan() {
            Ok(()) => {
                retry_delay = DELIVERY_RETRY_INITIAL;
                DELIVERY_SCAN_INTERVAL
            }
            Err(error) => {
                eprintln!("review dispatch supervision failed: {error:#}");
                let wait = retry_delay;
                retry_delay = retry_delay.saturating_mul(2).min(DELIVERY_RETRY_MAX);
                wait
            }
        };
        match signals.recv_timeout(wait) {
            Ok(WorkerSignal::Wake) | Err(mpsc::RecvTimeoutError::Timeout) => {}
            Ok(WorkerSignal::Stop) | Err(mpsc::RecvTimeoutError::Disconnected) => return,
        }
    }
}

fn scan_deliveries(
    store: &mut Store,
    repository: &Repository,
    binding: &RuntimeBindingId,
    claimant: &str,
    client: &herdrkit::Client,
) -> Result<()> {
    store
        .recover_expired_dispatches(binding, "the dispatch process lease expired")
        .context("recovering expired agent dispatches")?;
    while let Some(job) = store
        .claim_next_dispatch(binding, claimant, CLAIM_LEASE)
        .context("claiming the next agent dispatch")?
    {
        deliver_job(
            repository,
            binding,
            &job,
            claimant,
            client,
            DELIVERY_TIMEOUT,
        )?;
    }
    Ok(())
}

fn deliver_job(
    repository: &Repository,
    binding: &RuntimeBindingId,
    job: &DispatchJob,
    claimant: &str,
    client: &herdrkit::Client,
    timeout: Duration,
) -> Result<()> {
    let outcome = match validate_assignment(repository, binding, job, client) {
        Ok(status) => unavailable_agent_outcome(status),
        Err(error) => Some(DispatchOutcome::Rejected {
            detail: format!("{error:#}"),
        }),
    };
    if let Some(outcome) = outcome {
        return finish_attempt(repository, binding, &job.attempt.id, claimant, &outcome)
            .context("recording an unavailable agent dispatch");
    }

    let prompt = match delivery_prompt(job, repository.checkout_root()) {
        Ok(prompt) => prompt,
        Err(error) => {
            return finish_attempt(
                repository,
                binding,
                &job.attempt.id,
                claimant,
                &DispatchOutcome::Rejected {
                    detail: format!("agent request rejected before dispatch: {error:#}"),
                },
            )
            .context("recording a rejected agent request");
        }
    };
    let heartbeat = match Heartbeat::start(
        repository.clone(),
        binding.clone(),
        job.attempt.id.clone(),
        claimant.to_owned(),
    ) {
        Ok(heartbeat) => heartbeat,
        Err(error) => {
            return finish_attempt(
                repository,
                binding,
                &job.attempt.id,
                claimant,
                &DispatchOutcome::Rejected {
                    detail: format!("could not supervise dispatch before prompting: {error:#}"),
                },
            )
            .context("recording an unsupervised agent dispatch");
        }
    };
    let result = client.agent_prompt_and_wait(
        AgentRef::Pane(&job.attempt.assignment.pane_id),
        &prompt,
        &[AgentStatus::Idle, AgentStatus::Done, AgentStatus::Blocked],
        Some(timeout),
    );
    heartbeat.stop();

    let outcome = match result {
        Ok(prompted) => returned_outcome(job, &prompted.agent),
        Err(error) => DispatchOutcome::Unknown {
            detail: format!("agent.prompt may have been accepted before it failed: {error}"),
        },
    };
    finish_attempt(repository, binding, &job.attempt.id, claimant, &outcome)
        .context("recording the agent dispatch outcome")
}

fn unavailable_agent_outcome(status: AgentStatus) -> Option<DispatchOutcome> {
    match status {
        AgentStatus::Idle | AgentStatus::Done => None,
        AgentStatus::Blocked => Some(DispatchOutcome::Blocked {
            detail: "selected agent is already blocked".to_owned(),
        }),
        AgentStatus::Working | AgentStatus::Unknown => Some(DispatchOutcome::Rejected {
            detail: format!(
                "selected agent is {}, not ready for a request",
                status.as_str()
            ),
        }),
    }
}

fn finish_attempt(
    repository: &Repository,
    binding: &RuntimeBindingId,
    attempt: &DispatchAttemptId,
    claimant: &str,
    outcome: &DispatchOutcome,
) -> Result<()> {
    let mut last_error = None;
    for delay in OUTCOME_RETRY_DELAYS {
        if !delay.is_zero() {
            thread::sleep(*delay);
        }
        let result = Store::open(repository)
            .and_then(|mut store| store.finish_dispatch(binding, attempt, claimant, outcome));
        match result {
            Ok(_) => return Ok(()),
            Err(error) => last_error = Some(error),
        }
    }
    Err(last_error.map_or_else(
        || anyhow::anyhow!("dispatch outcome retry schedule is empty"),
        anyhow::Error::from,
    ))
}

fn validate_assignment(
    repository: &Repository,
    binding: &RuntimeBindingId,
    job: &DispatchJob,
    client: &herdrkit::Client,
) -> Result<AgentStatus> {
    if job.attempt.runtime_binding_id != *binding {
        anyhow::bail!(
            "dispatch attempt belongs to binding {}, not {binding}",
            job.attempt.runtime_binding_id
        );
    }
    let assignment = &job.attempt.assignment;
    let store = Store::open(repository).context("opening the review registry")?;
    let runtime = store
        .runtime_binding(binding)
        .context("loading the request runtime binding")?;
    if runtime.workspace_id != assignment.workspace_id {
        anyhow::bail!(
            "agent target workspace {} is not binding workspace {}",
            assignment.workspace_id,
            runtime.workspace_id
        );
    }
    let server_id = client.server_id().context("identifying the Herdr server")?;
    if server_id != assignment.herdr_server_id {
        anyhow::bail!(
            "agent request belongs to Herdr server {:?}, not {:?}",
            assignment.herdr_server_id,
            server_id
        );
    }

    let snapshot = client
        .snapshot()
        .context("revalidating the Herdr session before prompting")?;
    let directory = snapshot
        .effective_workspace_dir(&assignment.workspace_id)
        .with_context(|| {
            format!(
                "workspace {} has no effective directory",
                assignment.workspace_id
            )
        })?;
    if !checkout_matches(directory, repository, &runtime.checkout_token)? {
        anyhow::bail!(
            "workspace {} no longer resolves to {}",
            assignment.workspace_id,
            repository.checkout_root().display()
        );
    }
    let agent = snapshot
        .agents
        .iter()
        .find(|agent| {
            agent.pane_id == assignment.pane_id && agent.workspace_id == assignment.workspace_id
        })
        .with_context(|| {
            format!(
                "pane {} no longer contains an agent in workspace {}",
                assignment.pane_id, assignment.workspace_id
            )
        })?;
    if !assignment.matches_agent(agent) {
        anyhow::bail!(
            "pane {} is occupied by a different agent than the selected assignment",
            assignment.pane_id
        );
    }
    Ok(agent.agent_status)
}

fn returned_outcome(job: &DispatchJob, agent: &herdrkit::api::Agent) -> DispatchOutcome {
    if !job.attempt.assignment.matches_agent(agent) {
        return DispatchOutcome::Unknown {
            detail: "Herdr returned a different agent identity after accepting the prompt"
                .to_owned(),
        };
    }
    match agent.agent_status {
        AgentStatus::Blocked => DispatchOutcome::Blocked {
            detail: "agent reported that it is blocked while handling the request".to_owned(),
        },
        AgentStatus::Idle | AgentStatus::Done => DispatchOutcome::Returned {
            detail: Some("agent turn returned; threads remain open for inspection".to_owned()),
        },
        AgentStatus::Working | AgentStatus::Unknown => DispatchOutcome::Unknown {
            detail: format!(
                "Herdr returned unexpected terminal agent state {}",
                agent.agent_status.as_str()
            ),
        },
    }
}

#[derive(Debug)]
struct Heartbeat {
    stop: mpsc::Sender<()>,
    worker: thread::JoinHandle<()>,
}

impl Heartbeat {
    fn start(
        repository: Repository,
        binding: RuntimeBindingId,
        attempt: DispatchAttemptId,
        claimant: String,
    ) -> Result<Self> {
        let (stop, stopped) = mpsc::channel();
        let worker = thread::Builder::new()
            .name("herdr-review-dispatch-heartbeat".to_owned())
            .spawn(move || {
                loop {
                    match stopped.recv_timeout(CLAIM_HEARTBEAT) {
                        Ok(()) | Err(mpsc::RecvTimeoutError::Disconnected) => return,
                        Err(mpsc::RecvTimeoutError::Timeout) => {
                            let result = Store::open(&repository).and_then(|mut store| {
                                store.renew_dispatch_claim(
                                    &binding,
                                    &attempt,
                                    &claimant,
                                    CLAIM_LEASE,
                                )
                            });
                            if let Err(error) = result {
                                eprintln!("could not renew agent dispatch {attempt}: {error}");
                            }
                        }
                    }
                }
            })
            .context("spawning the agent dispatch heartbeat")?;
        Ok(Self { stop, worker })
    }

    fn stop(self) {
        let _ = self.stop.send(());
        if self.worker.join().is_err() {
            eprintln!("agent dispatch heartbeat thread panicked");
        }
    }
}

/// Wraps the core's bounded request projection in execution instructions.
fn delivery_prompt(job: &DispatchJob, checkout: &Path) -> Result<String> {
    let mut prompt = PromptBuilder::default();
    prompt.push("Continue local review request ")?;
    prompt.push(job.request.id.as_str())?;
    prompt.push(" (dispatch attempt ")?;
    prompt.push(job.attempt.id.as_str())?;
    prompt.push(") in ")?;
    prompt.push(&checkout.display().to_string())?;
    prompt.push(". Respond to the NEW reviewer messages below. Earlier messages and findings are context, not new instructions. Answer questions and discuss disagreements; do not edit code unless the new messages explicitly request changes. Do not post to GitHub unless explicitly asked. Request and message IDs are stable: do not repeat actions if this request already ran.\nScope: ")?;
    prompt.push(&scope_description(&job.comparison))?;
    prompt.push("\n")?;
    prompt.push(
        "The JSON projection below contains complete findings and NEW reviewer message IDs, bounded earlier context, and answers already saved for this request. Answer only threads without a saved answer. Do not repeat actions on answered threads. For any uncertain previous execution, inspect the checkout and existing answers before taking action; a retry is not permission to repeat side effects. Save one response per unanswered thread through the local review tool so it appears in Neovim. Run ",
    )?;
    prompt.push(
        &std::env::current_exe()
            .context("locating the review reply tool")?
            .display()
            .to_string(),
    )?;
    prompt.push(" with arguments --agent and the checkout path above. Pass one JSON document on stdin (use a JSON encoder, not shell interpolation):\n")?;
    let example = serde_json::json!({
        "id": 1, "method": "thread.reply", "params": {
            "binding_id": job.attempt.runtime_binding_id,
            "request_id": job.request.id,
            "attempt_id": job.attempt.id,
            "thread_id": "<thread-id-from-below>",
            "message": { "author": "<your-agent-name>", "body": "<your-answer>" }
        }
    });
    prompt.push(&example.to_string())?;
    prompt.push("\nAn identical reply retry is safe; a different reply for the same request/thread is rejected. Do not overwrite the original finding to answer a question. If saving fails, report that explicitly and include your answer in chat. Finish with a concise summary.\n")?;
    prompt.push(&job.projection_json)?;
    Ok(prompt.finish())
}

fn scope_description(scope: &CapturedComparison) -> String {
    format!(
        "captured commits {} to {}",
        scope.base.oid, scope.target.oid
    )
}

#[derive(Debug, Default)]
struct PromptBuilder {
    text: String,
}

impl PromptBuilder {
    fn push(&mut self, value: &str) -> Result<()> {
        let length = self
            .text
            .len()
            .checked_add(value.len())
            .context("agent request prompt length overflowed")?;
        if length > MAX_PROMPT_BYTES {
            anyhow::bail!("agent request prompt exceeds the {MAX_PROMPT_BYTES}-byte limit");
        }
        self.text.push_str(value);
        Ok(())
    }

    fn finish(self) -> String {
        self.text
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn binding_id() -> RuntimeBindingId {
        RuntimeBindingId::parse("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa").unwrap()
    }

    #[test]
    fn dead_delivery_worker_is_evicted_and_restartable() {
        let binding = binding_id();
        let (dead_signal, dead_receiver) = mpsc::channel();
        drop(dead_receiver);
        let mut workers = BTreeMap::from([(
            binding.clone(),
            DeliverySupervisor {
                worker_signal: dead_signal,
            },
        )]);
        let (replacement_signal, replacement_receiver) = mpsc::channel();
        let mut starts = 0;

        ensure_worker(&mut workers, &binding, || {
            starts += 1;
            Ok(DeliverySupervisor {
                worker_signal: replacement_signal,
            })
        })
        .unwrap();

        assert_eq!(starts, 1);
        assert!(workers.contains_key(&binding));
        assert!(matches!(
            replacement_receiver.recv().unwrap(),
            WorkerSignal::Wake
        ));
    }

    #[test]
    fn failed_replacement_does_not_retain_a_dead_worker() {
        let binding = binding_id();
        let (dead_signal, dead_receiver) = mpsc::channel();
        drop(dead_receiver);
        let mut workers = BTreeMap::from([(
            binding.clone(),
            DeliverySupervisor {
                worker_signal: dead_signal,
            },
        )]);

        let error = ensure_worker(&mut workers, &binding, || {
            anyhow::bail!("replacement unavailable")
        })
        .unwrap_err();

        assert!(error.to_string().contains("replacement unavailable"));
        assert!(!workers.contains_key(&binding));
    }

    #[test]
    fn prompt_builder_rejects_oversized_content() {
        let mut prompt = PromptBuilder::default();
        assert!(prompt.push(&"x".repeat(MAX_PROMPT_BYTES)).is_ok());
        assert!(prompt.push("x").is_err());
    }
}
