use std::io::{BufRead, Read as _, Write};
use std::path::Path;
use std::sync::{Mutex, mpsc};
use std::thread;

use anyhow::{Context as _, Result};
use herdrkit::{ExecutionContext, PaneId, WorkspaceId};
use review_core::{
    AgentAssignment, AgentRequestId, CheckoutKind, ComparisonSpec, CreateAgentRequest,
    CreateBoundThread, DiffEndpoint, NewMessage, NewThread, Repository, RuntimeBindingId, Store,
    ThreadId,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::DeliverySupervisors;
use crate::catalog::RepositoryCatalog;

const PROTOCOL: u32 = 1;
const MAX_FRAME_BYTES: usize = 1024 * 1024;
const CAPABILITIES: &[&str] = &[
    "context.load",
    "finding.save",
    "binding.load",
    "binding.observe",
    "thread.create",
    "thread.add_message",
    "thread.resolve",
    "agents.list",
    "agent_request.create",
    "dispatch.retry",
];
const CATALOG_ENV: &str = "HERDR_REVIEW_CATALOG_DIR";

/// Handles one bounded request from a local agent. No editor or dispatch worker
/// is required, and this surface cannot send prompts or publish GitHub feedback.
pub fn serve_agent(checkout: &Path, input: impl BufRead, mut output: impl Write) -> Result<()> {
    let mut frame = Vec::new();
    input
        .take(u64::try_from(MAX_FRAME_BYTES + 1)?)
        .read_to_end(&mut frame)?;
    let parsed = if frame.len() > MAX_FRAME_BYTES {
        Err(Failure::new(
            None,
            ErrorCode::FrameTooLarge,
            "agent request exceeds size limit".to_owned(),
        ))
    } else {
        parse_request(&frame)
    };
    let request = match parsed {
        Ok(request) => request,
        Err(failure) => {
            write_response(&mut output, &Response::from_failure(failure))?;
            anyhow::bail!("invalid agent review request (see JSON error)");
        }
    };
    let id = request.id;
    let result = (|| {
        anyhow::ensure!(
            matches!(
                request.method.as_str(),
                "context.load" | "finding.save" | "thread.reply"
            ),
            "unsupported agent review operation"
        );
        let repository = Repository::discover(checkout)?;
        let execution = ExecutionContext::load()?;
        let workspace = execution.require_workspace_id()?;
        let snapshot = execution.client().snapshot()?;
        let directory = snapshot
            .effective_workspace_dir(workspace)
            .context("calling workspace no longer exists or has no checkout")?;
        let live_repository = Repository::discover(directory)?;
        anyhow::ensure!(
            live_repository.checkout_root() == repository.checkout_root(),
            "explicit checkout does not belong to the calling Herdr workspace"
        );
        let runtime = BridgeRuntime {
            server: execution.client().server_id()?,
            workspace: workspace.clone(),
            pane: execution.pane_id().cloned(),
        };
        if !repository.database_path().is_file() {
            anyhow::ensure!(
                request.method == "context.load",
                "no review is bound to this checkout"
            );
            return Ok(Value::Null);
        }
        let mut store = Store::open(&repository)?;
        if request.method == "thread.reply" {
            let params: ReplyToRequest = decode(id, request.params).map_err(anyhow::Error::new)?;
            return save_agent_reply(&mut store, &runtime, &snapshot.agents, &params);
        }
        context_operation(request, &mut store, &runtime).map_err(anyhow::Error::new)
    })();
    let failed = result.is_err();
    let response = match result {
        Ok(value) => Response::success(id, value),
        Err(error) => {
            let code = if let Some(failure) = error.downcast_ref::<Failure>() {
                failure.code
            } else if let Some(source) = error.downcast_ref::<review_core::Error>() {
                store_error(id, source).code
            } else {
                ErrorCode::OperationFailed
            };
            Response::failure(Some(id), code, format!("{error:#}"))
        }
    };
    write_response(&mut output, &response)?;
    anyhow::ensure!(!failed, "agent review operation failed (see JSON error)");
    Ok(())
}

fn save_agent_reply(
    store: &mut Store,
    runtime: &BridgeRuntime,
    agents: &[herdrkit::api::Agent],
    params: &ReplyToRequest,
) -> Result<Value> {
    require_runtime_scope(0, store, runtime, &params.binding_id).map_err(anyhow::Error::new)?;
    let agent = agents
        .iter()
        .find(|agent| Some(&agent.pane_id) == runtime.pane.as_ref())
        .context("calling pane no longer contains the assigned agent")?;
    anyhow::ensure!(
        agent.workspace_id == runtime.workspace,
        "calling agent belongs to another workspace"
    );
    let caller = AgentAssignment::new(
        runtime.server.clone(),
        agent.workspace_id.clone(),
        agent.pane_id.clone(),
        agent.name.clone(),
        agent.agent_kind.clone(),
    )?;
    Ok(serde_json::to_value(store.reply_to_request(
        &params.binding_id,
        &params.request_id,
        &params.thread_id,
        &params.attempt_id,
        &caller,
        &params.message,
    )?)?)
}

fn context_operation(
    request: Request,
    store: &mut Store,
    runtime: &BridgeRuntime,
) -> std::result::Result<Value, Failure> {
    match request.method.as_str() {
        "context.load" => {
            let params: LoadContext = decode(request.id, request.params)?;
            encode_result(
                request.id,
                crate::context::load(
                    store,
                    &runtime.server,
                    &runtime.workspace,
                    params.cursor.as_ref(),
                ),
            )
        }
        "finding.save" => {
            let params: SaveFinding = decode(request.id, request.params)?;
            require_runtime_scope(request.id, store, runtime, &params.binding_id)?;
            encode_store(
                request.id,
                store.save_finding(&params.binding_id, &params.observation_id, &params.finding),
            )
        }
        _ => Err(Failure::for_request(
            request.id,
            ErrorCode::UnsupportedMethod,
            "unsupported context operation",
        )),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LoadContext {
    cursor: Option<review_core::BindingCursor>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SaveFinding {
    binding_id: RuntimeBindingId,
    observation_id: review_core::ObservationId,
    finding: review_core::NewFinding,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReplyToRequest {
    binding_id: RuntimeBindingId,
    request_id: AgentRequestId,
    attempt_id: review_core::DispatchAttemptId,
    thread_id: ThreadId,
    message: NewMessage,
}

/// Serves one Neovim process until stdin closes.
pub fn serve_stdio(checkout: &Path, input: impl BufRead, output: impl Write + Send) -> Result<()> {
    let repository = Repository::discover(checkout).context("discovering review repository")?;
    remember_repository(&repository);
    let execution = ExecutionContext::load().context("loading the Herdr execution context")?;
    let runtime = BridgeRuntime {
        server: execution
            .client()
            .server_id()
            .context("identifying the Herdr server")?,
        workspace: execution
            .require_workspace_id()
            .context("review bridge requires a Herdr workspace")?
            .clone(),
        pane: execution.pane_id().cloned(),
    };
    let mut supervisors = DeliverySupervisors::new(repository.clone(), execution.client().clone());
    serve(input, output, &repository, &runtime, &mut supervisors)
}

fn remember_repository(repository: &Repository) {
    let Some(state_dir) = std::env::var_os(CATALOG_ENV) else {
        return;
    };
    let name = repository
        .checkout_root()
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("repository");
    if let Err(error) = RepositoryCatalog::open(Path::new(&state_dir))
        .and_then(|catalog| catalog.remember(repository, name))
    {
        eprintln!("review: could not update the repository catalog: {error:#}");
    }
}

#[derive(Debug, Clone)]
struct BridgeRuntime {
    server: String,
    workspace: WorkspaceId,
    pane: Option<PaneId>,
}

fn serve(
    mut input: impl BufRead,
    output: impl Write + Send,
    repository: &Repository,
    runtime: &BridgeRuntime,
    supervisors: &mut DeliverySupervisors,
) -> Result<()> {
    let mut store = Store::open(repository).context("opening the review bridge store")?;
    let output = Mutex::new(output);
    let write = |response: &Response| {
        let mut output = output
            .lock()
            .map_err(|_| anyhow::anyhow!("review output lock poisoned"))?;
        write_response(&mut *output, response)
    };
    // Only discovery runs concurrently. One worker and one queued lookup bound
    // the work; all observations and mutations keep their input ordering.
    thread::scope(|scope| {
        let (sender, receiver) = mpsc::sync_channel::<(u64, review_core::RuntimeBinding)>(1);
        let client = supervisors.client().clone();
        let write = &write;
        let worker = scope.spawn(move || -> Result<()> {
            for (id, binding) in receiver {
                let response = match encode_result(
                    id,
                    crate::agents_with_client(repository, &binding, &client),
                ) {
                    Ok(value) => Response::success(id, value),
                    Err(failure) => Response::from_failure(failure),
                };
                write(&response)?;
            }
            Ok(())
        });
        let result = (|| {
            let mut negotiated = false;
            let mut frame = Vec::new();
            loop {
                frame.clear();
                let read = read_frame(&mut input, &mut frame)?;
                if read == FrameRead::End {
                    return Ok(());
                }
                let response = match read {
                    FrameRead::TooLarge => Response::failure(
                        None,
                        ErrorCode::FrameTooLarge,
                        format!("request exceeds the {MAX_FRAME_BYTES}-byte frame limit"),
                    ),
                    FrameRead::Frame => match parse_request(&frame) {
                        Ok(request) if request.method == "agents.list" => {
                            let id = request.id;
                            match agent_lookup(request, &store, runtime, negotiated) {
                                Ok(binding) => match sender.try_send((id, binding)) {
                                    Ok(()) => continue,
                                    Err(error) => Response::failure(
                                        Some(id),
                                        ErrorCode::OperationFailed,
                                        format!("agent discovery unavailable: {error}"),
                                    ),
                                },
                                Err(failure) => Response::from_failure(failure),
                            }
                        }
                        Ok(request) => {
                            respond(request, &mut store, runtime, supervisors, &mut negotiated)
                        }
                        Err(failure) => Response::from_failure(failure),
                    },
                    FrameRead::End => unreachable!("end of input returned above"),
                };
                write(&response)?;
            }
        })();
        drop(sender);
        let worker_result = worker
            .join()
            .map_err(|_| anyhow::anyhow!("agent discovery worker panicked"))?;
        result.and(worker_result)
    })
}

fn agent_lookup(
    request: Request,
    store: &Store,
    runtime: &BridgeRuntime,
    negotiated: bool,
) -> std::result::Result<review_core::RuntimeBinding, Failure> {
    require_handshake(request.id, negotiated)?;
    let params = decode::<SelectBinding>(request.id, request.params)?;
    require_runtime_scope(request.id, store, runtime, &params.binding_id)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FrameRead {
    Frame,
    TooLarge,
    End,
}

fn read_frame(input: &mut impl BufRead, frame: &mut Vec<u8>) -> Result<FrameRead> {
    let read = input
        .by_ref()
        .take(u64::try_from(MAX_FRAME_BYTES).unwrap_or(u64::MAX) + 1)
        .read_until(b'\n', frame)
        .context("reading a review bridge request")?;
    if read == 0 {
        return Ok(FrameRead::End);
    }
    if frame.len() <= MAX_FRAME_BYTES {
        return Ok(FrameRead::Frame);
    }
    if frame.last() != Some(&b'\n') {
        discard_through_newline(input).context("discarding an oversized review request")?;
    }
    Ok(FrameRead::TooLarge)
}

fn discard_through_newline(input: &mut impl BufRead) -> std::io::Result<()> {
    loop {
        let available = input.fill_buf()?;
        if available.is_empty() {
            return Ok(());
        }
        let consumed = available
            .iter()
            .position(|byte| *byte == b'\n')
            .map_or(available.len(), |position| position + 1);
        let found_newline = available.get(consumed.saturating_sub(1)) == Some(&b'\n');
        input.consume(consumed);
        if found_newline {
            return Ok(());
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    id: u64,
    method: String,
    params: Value,
}

fn parse_request(frame: &[u8]) -> std::result::Result<Request, Failure> {
    let value = serde_json::from_slice::<Value>(frame).map_err(|error| {
        Failure::new(
            None,
            ErrorCode::InvalidJson,
            format!("request is not valid JSON: {error}"),
        )
    })?;
    let id = value.get("id").and_then(Value::as_u64);
    serde_json::from_value(value).map_err(|error| {
        Failure::new(
            id,
            ErrorCode::InvalidRequest,
            format!("request envelope is invalid: {error}"),
        )
    })
}

#[derive(Debug, Serialize)]
struct Response {
    id: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<WireError>,
}

impl Response {
    fn success(id: u64, result: Value) -> Self {
        Self {
            id: Some(id),
            result: Some(result),
            error: None,
        }
    }

    fn failure(id: Option<u64>, code: ErrorCode, message: String) -> Self {
        Self {
            id,
            result: None,
            error: Some(WireError { code, message }),
        }
    }

    fn from_failure(failure: Failure) -> Self {
        Self::failure(failure.id, failure.code, failure.message)
    }
}

#[derive(Debug, Serialize)]
struct WireError {
    code: ErrorCode,
    message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum ErrorCode {
    InvalidJson,
    InvalidRequest,
    FrameTooLarge,
    ResponseTooLarge,
    HandshakeRequired,
    IncompatibleProtocol,
    UnsupportedMethod,
    InvalidArgument,
    NotFound,
    InvalidState,
    Conflict,
    StaleCursor,
    ClaimLost,
    OperationFailed,
    DispatchUnavailable,
}

#[derive(Debug)]
struct Failure {
    id: Option<u64>,
    code: ErrorCode,
    message: String,
}

impl std::fmt::Display for Failure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for Failure {}

impl Failure {
    fn new(id: Option<u64>, code: ErrorCode, message: String) -> Self {
        Self { id, code, message }
    }

    fn for_request(id: u64, code: ErrorCode, message: impl Into<String>) -> Self {
        Self::new(Some(id), code, message.into())
    }
}

fn write_response(output: &mut impl Write, response: &Response) -> Result<()> {
    let id = response.id;
    let mut encoded = serde_json::to_vec(response).context("encoding a review bridge response")?;
    if encoded.len() + 1 > MAX_FRAME_BYTES {
        encoded = serde_json::to_vec(&Response::failure(
            id,
            ErrorCode::ResponseTooLarge,
            format!("response exceeds the {MAX_FRAME_BYTES}-byte frame limit"),
        ))
        .context("encoding a bounded review bridge error")?;
    }
    encoded.push(b'\n');
    output
        .write_all(&encoded)
        .and_then(|()| output.flush())
        .context("writing a review bridge response")
}

fn respond(
    request: Request,
    store: &mut Store,
    runtime: &BridgeRuntime,
    supervisors: &mut DeliverySupervisors,
    negotiated: &mut bool,
) -> Response {
    let id = request.id;
    match dispatch(request, store, runtime, supervisors, negotiated) {
        Ok(result) => Response::success(id, result),
        Err(failure) => Response::from_failure(failure),
    }
}

fn dispatch(
    request: Request,
    store: &mut Store,
    runtime: &BridgeRuntime,
    supervisors: &mut DeliverySupervisors,
    negotiated: &mut bool,
) -> std::result::Result<Value, Failure> {
    if request.method == "hello" {
        return negotiate(request.id, request.params, negotiated);
    }
    require_handshake(request.id, *negotiated)?;

    if matches!(request.method.as_str(), "context.load" | "finding.save") {
        return context_operation(request, store, runtime);
    }

    match request.method.as_str() {
        "binding.load" => {
            let params = decode::<SelectBinding>(request.id, request.params)?;
            require_runtime_scope(request.id, store, runtime, &params.binding_id)?;
            match (params.if_revision, params.if_observation_id.as_ref()) {
                (Some(revision), Some(observation)) if params.cursor.is_none() => {
                    let current = store
                        .binding_revision(&params.binding_id)
                        .map_err(|error| store_error(request.id, &error))?;
                    if current == (revision, observation.clone()) {
                        supervisors.resume_best_effort(&params.binding_id);
                        return Ok(serde_json::json!({"unchanged": true}));
                    }
                }
                (None, None) => {}
                _ => {
                    return Err(Failure::for_request(
                        request.id,
                        ErrorCode::InvalidArgument,
                        "conditional binding reads require both revision and observation and no cursor",
                    ));
                }
            }
            let state = store
                .binding_state_page(&params.binding_id, params.cursor.as_ref())
                .map_err(|error| store_error(request.id, &error))?;
            supervisors.resume_best_effort(&params.binding_id);
            encode(request.id, &state)
        }
        "binding.observe" => {
            let params = decode::<ObserveBinding>(request.id, request.params)?;
            require_runtime_scope(request.id, store, runtime, &params.binding_id)?;
            let state = store
                .observe_binding(&params.binding_id, &params.comparison)
                .map_err(|error| store_error(request.id, &error))?;
            supervisors.resume_best_effort(&params.binding_id);
            encode(request.id, &state)
        }
        "thread.create" => {
            let params = decode::<CreateThread>(request.id, request.params)?;
            let state = create_thread(request.id, store, runtime, params)?;
            supervisors.resume_best_effort(&state.binding.id);
            encode(request.id, &state)
        }
        "thread.add_message" => {
            let params = decode::<AddMessage>(request.id, request.params)?;
            require_runtime_scope(request.id, store, runtime, &params.binding_id)?;
            encode_store(
                request.id,
                store.add_message(&params.binding_id, &params.thread_id, &params.message),
            )
        }
        "thread.resolve" => {
            let params = decode::<SelectThread>(request.id, request.params)?;
            require_runtime_scope(request.id, store, runtime, &params.binding_id)?;
            encode_store(
                request.id,
                store.resolve_thread(&params.binding_id, &params.thread_id),
            )
        }
        "agent_request.create" => {
            let params = decode::<CreateAgentRequestParams>(request.id, request.params)?;
            require_runtime_scope(request.id, store, runtime, &params.binding_id)?;
            let agent_request = create_agent_request(request.id, store, supervisors, params)?;
            encode(request.id, &agent_request)
        }
        "dispatch.retry" => {
            let params = decode::<RetryDispatch>(request.id, request.params)?;
            require_runtime_scope(request.id, store, runtime, &params.binding_id)?;
            let attempt = retry_dispatch(request.id, store, supervisors, &params)?;
            encode(request.id, &attempt)
        }
        _ => Err(Failure::for_request(
            request.id,
            ErrorCode::UnsupportedMethod,
            format!("unknown review bridge method {:?}", request.method),
        )),
    }
}

fn create_thread(
    request_id: u64,
    store: &mut Store,
    runtime: &BridgeRuntime,
    params: CreateThread,
) -> std::result::Result<review_core::BindingState, Failure> {
    if let Some(binding) = params.binding_id {
        require_runtime_scope(request_id, store, runtime, &binding)?;
        return store
            .create_thread_on_comparison(&binding, &params.comparison, &params.thread)
            .map_err(|error| store_error(request_id, &error));
    }
    if let Some(binding) = store
        .matching_runtime_binding(
            &params.comparison,
            &runtime.server,
            &runtime.workspace,
            runtime.pane.as_ref(),
        )
        .map_err(|error| store_error(request_id, &error))?
    {
        return store
            .create_thread_on_comparison(&binding.id, &params.comparison, &params.thread)
            .map_err(|error| store_error(request_id, &error));
    }
    let create = CreateBoundThread::new(
        comparison_title(&params.comparison),
        Vec::new(),
        params.comparison,
        runtime.server.clone(),
        runtime.workspace.clone(),
        runtime.pane.clone(),
        CheckoutKind::Existing,
        params.thread,
    )
    .map_err(|error| store_error(request_id, &error))?;
    store
        .create_bound_thread(&create)
        .map_err(|error| store_error(request_id, &error))
}

fn require_runtime_scope(
    request_id: u64,
    store: &Store,
    runtime: &BridgeRuntime,
    binding: &RuntimeBindingId,
) -> std::result::Result<review_core::RuntimeBinding, Failure> {
    let binding = store
        .runtime_binding(binding)
        .map_err(|error| store_error(request_id, &error))?;
    if binding.herdr_server_id != runtime.server || binding.workspace_id != runtime.workspace {
        return Err(Failure::for_request(
            request_id,
            ErrorCode::InvalidState,
            format!(
                "runtime binding {} belongs to Herdr server {:?}, workspace {}, not server {:?}, workspace {}",
                binding.id,
                binding.herdr_server_id,
                binding.workspace_id,
                runtime.server,
                runtime.workspace
            ),
        ));
    }
    Ok(binding)
}

fn create_agent_request(
    request_id: u64,
    store: &mut Store,
    supervisors: &mut DeliverySupervisors,
    params: CreateAgentRequestParams,
) -> std::result::Result<review_core::AgentRequest, Failure> {
    let create = CreateAgentRequest::new(params.thread_ids, params.assignment)
        .map_err(|error| store_error(request_id, &error))?;
    ensure_supervision(request_id, supervisors, &params.binding_id)?;
    let agent_request = store
        .create_agent_request(&params.binding_id, &create)
        .map_err(|error| store_error(request_id, &error))?;
    if let Some(attempt) = agent_request.attempts.last() {
        supervisors.wake_after_commit(&params.binding_id, &attempt.id);
    } else {
        eprintln!(
            "agent request {} was saved without a dispatch attempt",
            agent_request.id
        );
    }
    Ok(agent_request)
}

fn retry_dispatch(
    request_id: u64,
    store: &mut Store,
    supervisors: &mut DeliverySupervisors,
    params: &RetryDispatch,
) -> std::result::Result<review_core::DispatchAttempt, Failure> {
    ensure_supervision(request_id, supervisors, &params.binding_id)?;
    let attempt = store
        .retry_dispatch(&params.binding_id, &params.request_id, &params.assignment)
        .map_err(|error| store_error(request_id, &error))?;
    supervisors.wake_after_commit(&params.binding_id, &attempt.id);
    Ok(attempt)
}

fn comparison_title(comparison: &ComparisonSpec) -> String {
    format!(
        "{} → {}",
        endpoint_label(&comparison.base),
        endpoint_label(&comparison.target)
    )
}

fn endpoint_label(endpoint: &DiffEndpoint) -> String {
    match endpoint {
        DiffEndpoint::Commit { oid } => {
            format!("commit {}", oid.get(..8).unwrap_or(oid))
        }
        DiffEndpoint::Index => "index".to_owned(),
        DiffEndpoint::WorkingTree => "working tree".to_owned(),
    }
}

fn ensure_supervision(
    request_id: u64,
    supervisors: &mut DeliverySupervisors,
    binding: &RuntimeBindingId,
) -> std::result::Result<(), Failure> {
    supervisors.ensure(binding).map_err(|error| {
        Failure::for_request(
            request_id,
            ErrorCode::DispatchUnavailable,
            format!("could not supervise agent dispatches: {error:#}"),
        )
    })
}

fn require_handshake(id: u64, negotiated: bool) -> std::result::Result<(), Failure> {
    if negotiated {
        Ok(())
    } else {
        Err(Failure::for_request(
            id,
            ErrorCode::HandshakeRequired,
            "hello must negotiate the bridge protocol before other operations",
        ))
    }
}

fn negotiate(id: u64, params: Value, negotiated: &mut bool) -> std::result::Result<Value, Failure> {
    let hello = decode::<Hello>(id, params)?;
    if hello.protocol != PROTOCOL {
        return Err(Failure::for_request(
            id,
            ErrorCode::IncompatibleProtocol,
            format!(
                "bridge protocol {} is unsupported; this process speaks {PROTOCOL}",
                hello.protocol
            ),
        ));
    }
    *negotiated = true;
    encode(
        id,
        &HelloResult {
            protocol: PROTOCOL,
            server_version: env!("CARGO_PKG_VERSION"),
            capabilities: CAPABILITIES,
        },
    )
}

fn decode<T: for<'de> Deserialize<'de>>(id: u64, params: Value) -> std::result::Result<T, Failure> {
    serde_json::from_value(params).map_err(|error| {
        Failure::for_request(
            id,
            ErrorCode::InvalidRequest,
            format!("request parameters are invalid: {error}"),
        )
    })
}

fn encode(id: u64, value: &impl Serialize) -> std::result::Result<Value, Failure> {
    serde_json::to_value(value).map_err(|error| {
        Failure::for_request(
            id,
            ErrorCode::OperationFailed,
            format!("could not encode operation result: {error}"),
        )
    })
}

fn encode_store<T: Serialize>(
    id: u64,
    result: review_core::Result<T>,
) -> std::result::Result<Value, Failure> {
    let value = result.map_err(|error| store_error(id, &error))?;
    encode(id, &value)
}

fn encode_result<T: Serialize>(id: u64, result: Result<T>) -> std::result::Result<Value, Failure> {
    let value = result.map_err(|error| {
        if let Some(source) = error.downcast_ref::<review_core::Error>() {
            store_error(id, source)
        } else {
            Failure::for_request(id, ErrorCode::OperationFailed, format!("{error:#}"))
        }
    })?;
    encode(id, &value)
}

fn store_error(id: u64, error: &review_core::Error) -> Failure {
    use review_core::Error;

    let code = match error {
        Error::EmptyField { .. }
        | Error::InvalidIdentifier { .. }
        | Error::DuplicateExternalReference
        | Error::InvalidLineRange
        | Error::AnchorSpanMismatch { .. }
        | Error::AnchorSourceMismatch { .. }
        | Error::InvalidAnchorPath { .. }
        | Error::EmptyThreadBatch
        | Error::DuplicateThreadInBatch
        | Error::InvalidComparison
        | Error::EmptyComparison
        | Error::RequestTooLarge
        | Error::InputTooLarge { .. }
        | Error::InvalidCommitOid
        | Error::InvalidDispatchLease
        | Error::MissingExpectedAgentIdentity => ErrorCode::InvalidArgument,
        Error::ReviewContextNotFound { .. }
        | Error::ObservationNotFound { .. }
        | Error::RuntimeBindingNotFound { .. }
        | Error::ThreadNotFound { .. }
        | Error::MessageNotFound { .. }
        | Error::AgentRequestNotFound { .. }
        | Error::DispatchAttemptNotFound { .. } => ErrorCode::NotFound,
        Error::ObservationNotInContext { .. }
        | Error::BindingCheckoutMismatch { .. }
        | Error::ThreadNotOpen { .. }
        | Error::NoPendingMessages
        | Error::InvalidAgentReply
        | Error::InvalidTransition { .. } => ErrorCode::InvalidState,
        Error::ConflictingAgentReply => ErrorCode::Conflict,
        Error::StaleCursor => ErrorCode::StaleCursor,
        Error::DispatchClaimLost { .. } => ErrorCode::ClaimLost,
        _ => ErrorCode::OperationFailed,
    };
    Failure::for_request(id, code, error.to_string())
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Hello {
    protocol: u32,
}

#[derive(Debug, Serialize)]
struct HelloResult<'a> {
    protocol: u32,
    server_version: &'a str,
    capabilities: &'a [&'a str],
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SelectBinding {
    binding_id: RuntimeBindingId,
    cursor: Option<review_core::BindingCursor>,
    if_revision: Option<u64>,
    if_observation_id: Option<review_core::ObservationId>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ObserveBinding {
    binding_id: RuntimeBindingId,
    comparison: ComparisonSpec,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateThread {
    binding_id: Option<RuntimeBindingId>,
    comparison: ComparisonSpec,
    thread: NewThread,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AddMessage {
    binding_id: RuntimeBindingId,
    thread_id: ThreadId,
    message: NewMessage,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SelectThread {
    binding_id: RuntimeBindingId,
    thread_id: ThreadId,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateAgentRequestParams {
    binding_id: RuntimeBindingId,
    thread_ids: Vec<ThreadId>,
    assignment: AgentAssignment,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RetryDispatch {
    binding_id: RuntimeBindingId,
    request_id: AgentRequestId,
    assignment: AgentAssignment,
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::process::Command;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use serde_json::json;

    use super::*;

    static NEXT_TEMP: AtomicUsize = AtomicUsize::new(0);

    struct TestRepository {
        root: std::path::PathBuf,
    }

    impl TestRepository {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!(
                "herdr-review-bridge-test-{}-{}",
                std::process::id(),
                NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
            ));
            let _ = fs::remove_dir_all(&root);
            fs::create_dir_all(&root).unwrap();
            git(&root, &["init", "--initial-branch=main"]);
            git(&root, &["config", "user.name", "Review Test"]);
            git(&root, &["config", "user.email", "review@example.invalid"]);
            git(&root, &["config", "commit.gpgsign", "false"]);
            git(&root, &["config", "core.hooksPath", "/dev/null"]);
            fs::write(root.join("lib.rs"), "fn original() {}\n").unwrap();
            git(&root, &["add", "lib.rs"]);
            git(&root, &["commit", "-m", "initial"]);
            fs::write(root.join("lib.rs"), "fn changed() {}\n").unwrap();
            Self { root }
        }

        fn repository(&self) -> Repository {
            Repository::discover(&self.root).unwrap()
        }

        fn head(&self) -> String {
            git(&self.root, &["rev-parse", "HEAD"])
        }
    }

    impl Drop for TestRepository {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    fn git(root: &Path, args: &[&str]) -> String {
        let output = Command::new("git")
            .arg("-C")
            .arg(root)
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap().trim().to_owned()
    }

    fn local_thread_params(fixture: &TestRepository) -> Value {
        json!({
            "comparison": {
                "base": { "kind": "commit", "oid": fixture.head() },
                "target": { "kind": "working_tree" }
            },
            "thread": {
                "anchor": {
                    "original": {
                        "path": "lib.rs",
                        "side": "target",
                        "start_line": 1,
                        "end_line": 1
                    },
                    "context": { "selected": ["fn changed() {}"] }
                },
                "body": "Explain this change",
                "author": "reviewer"
            }
        })
    }

    fn unavailable_supervisor(
        _repository: &Repository,
        _binding: &RuntimeBindingId,
        _client: &herdrkit::Client,
    ) -> Result<crate::DeliverySupervisor> {
        anyhow::bail!("test supervisor unavailable")
    }

    #[test]
    fn conversation_replies_cross_tabs_but_not_agent_or_workspace_boundaries() {
        let fixture = TestRepository::new();
        let mut store = Store::open(&fixture.repository()).unwrap();
        let editor = BridgeRuntime {
            server: "test-server".into(),
            workspace: WorkspaceId::new("w1"),
            pane: Some(PaneId::new("w1:p2")),
        };
        let state = create_thread(
            1,
            &mut store,
            &editor,
            serde_json::from_value(local_thread_params(&fixture)).unwrap(),
        )
        .unwrap();
        let agent: herdrkit::api::Agent = serde_json::from_value(json!({
            "workspace_id":"w1", "tab_id":"w1:t2", "pane_id":"w1:p3",
            "agent":"codex", "agent_status":"working"
        }))
        .unwrap();
        let assignment = AgentAssignment::new(
            "test-server",
            agent.workspace_id.clone(),
            agent.pane_id.clone(),
            None,
            Some("codex".into()),
        )
        .unwrap();
        let thread = state.threads.first().unwrap();
        let request = store
            .create_agent_request(
                &state.binding.id,
                &CreateAgentRequest::new(vec![thread.id.clone()], assignment).unwrap(),
            )
            .unwrap();
        let params = ReplyToRequest {
            binding_id: state.binding.id.clone(),
            attempt_id: request.attempts.last().unwrap().id.clone(),
            request_id: request.id,
            thread_id: thread.id.clone(),
            message: NewMessage::new("Here is the explanation.", "codex").unwrap(),
        };
        let caller = BridgeRuntime {
            pane: Some(agent.pane_id.clone()),
            ..editor.clone()
        };
        assert!(
            save_agent_reply(&mut store, &caller, std::slice::from_ref(&agent), &params).is_err(),
            "unclaimed requests cannot be answered"
        );
        let job = store
            .claim_next_dispatch(
                &state.binding.id,
                "test",
                std::time::Duration::from_secs(30),
            )
            .unwrap()
            .unwrap();
        let prompt = crate::delivery_prompt(&job, &fixture.root).unwrap();
        assert!(prompt.contains("NEW reviewer message"));
        assert!(prompt.contains("do not edit code unless"));
        assert!(prompt.contains("thread.reply") && prompt.contains(params.request_id.as_str()));
        let saved =
            save_agent_reply(&mut store, &caller, std::slice::from_ref(&agent), &params).unwrap();
        assert_eq!(saved.get("origin"), Some(&json!("agent")));
        assert_eq!(
            saved,
            save_agent_reply(&mut store, &caller, std::slice::from_ref(&agent), &params).unwrap()
        );
        assert!(
            save_agent_reply(&mut store, &editor, std::slice::from_ref(&agent), &params).is_err()
        );
        let foreign = BridgeRuntime {
            workspace: WorkspaceId::new("w2"),
            ..caller.clone()
        };
        assert!(
            save_agent_reply(&mut store, &foreign, std::slice::from_ref(&agent), &params).is_err()
        );
        let replaced = herdrkit::api::Agent {
            agent_kind: Some("claude".into()),
            ..agent
        };
        assert!(save_agent_reply(&mut store, &caller, &[replaced], &params).is_err());
        let refreshed = store.binding_state(&state.binding.id).unwrap();
        assert_eq!(refreshed.threads.first().unwrap().messages.len(), 2);
    }

    #[test]
    fn dispatch_includes_the_finding_evidence_and_attempt_bound_reply_instructions() {
        let fixture = TestRepository::new();
        let mut store = Store::open(&fixture.repository()).unwrap();
        let runtime = BridgeRuntime {
            server: "test-server".into(),
            workspace: WorkspaceId::new("w1"),
            pane: None,
        };
        let state = create_thread(
            1,
            &mut store,
            &runtime,
            serde_json::from_value(local_thread_params(&fixture)).unwrap(),
        )
        .unwrap();
        let finding = serde_json::from_value(json!({
            "finding": { "key":"race", "kind":"finding", "severity":"blocking",
                "title":"Persist before cleanup", "evidence":"Two releases follow one cleanup failure. cafe\u{301} 👩‍💻",
                "related_locations":[{"path":"lib.rs","side":"target","start_line":1,"end_line":1}] },
            "location":null, "body":"Persist independently", "author":"agent"
        })).unwrap();
        let saved = store
            .save_finding(&state.binding.id, &state.observation.id, &finding)
            .unwrap();
        let thread = saved
            .threads
            .iter()
            .find(|thread| thread.finding.is_some())
            .unwrap();
        store
            .add_message(
                &state.binding.id,
                &thread.id,
                &NewMessage::new("Why is this blocking?", "reviewer").unwrap(),
            )
            .unwrap();
        let assignment = AgentAssignment::new(
            "test-server",
            WorkspaceId::new("w1"),
            PaneId::new("w1:p3"),
            None,
            Some("codex".into()),
        )
        .unwrap();
        store
            .create_agent_request(
                &state.binding.id,
                &CreateAgentRequest::new(vec![thread.id.clone()], assignment).unwrap(),
            )
            .unwrap();
        let job = store
            .claim_next_dispatch(
                &state.binding.id,
                "test",
                std::time::Duration::from_secs(30),
            )
            .unwrap()
            .unwrap();
        let prompt = crate::delivery_prompt(&job, &fixture.root).unwrap();
        for required in [
            "blocking",
            "Persist before cleanup",
            "Two releases follow one cleanup failure. cafe\u{301} 👩‍💻",
            "related_locations",
            "Why is this blocking?",
            "attempt_id",
            job.attempt.id.as_str(),
        ] {
            assert!(prompt.contains(required), "missing {required}");
        }
        assert!(prompt.len() <= crate::MAX_PROMPT_BYTES);
    }

    #[test]
    fn reply_errors_preserve_domain_categories() {
        for (error, expected) in [
            (
                review_core::Error::NoPendingMessages,
                ErrorCode::InvalidState,
            ),
            (
                review_core::Error::InvalidAgentReply,
                ErrorCode::InvalidState,
            ),
            (
                review_core::Error::ConflictingAgentReply,
                ErrorCode::Conflict,
            ),
            (
                review_core::Error::RequestTooLarge,
                ErrorCode::InvalidArgument,
            ),
        ] {
            let failure = store_error(7, &error);
            assert_eq!(failure.id, Some(7));
            assert_eq!(failure.code, expected);
        }
    }

    #[test]
    fn conditional_binding_reads_refresh_after_a_message_and_validate_tokens() {
        let fixture = TestRepository::new();
        let repository = fixture.repository();
        let mut store = Store::open(&repository).unwrap();
        let runtime = BridgeRuntime {
            server: "test-server".into(),
            workspace: WorkspaceId::new("w1"),
            pane: None,
        };
        let state = create_thread(
            1,
            &mut store,
            &runtime,
            serde_json::from_value(local_thread_params(&fixture)).unwrap(),
        )
        .unwrap();
        let mut supervisors = DeliverySupervisors::with_starter(
            repository,
            herdrkit::Client::new(fixture.root.join("missing.sock")),
            unavailable_supervisor,
        );
        let params = json!({ "binding_id":state.binding.id, "if_revision":state.revision,
            "if_observation_id":state.observation.id });
        let request = || Request {
            id: 2,
            method: "binding.load".into(),
            params: params.clone(),
        };
        assert_eq!(
            dispatch(request(), &mut store, &runtime, &mut supervisors, &mut true).unwrap(),
            json!({"unchanged":true})
        );
        store
            .add_message(
                &state.binding.id,
                &state.threads.first().unwrap().id,
                &NewMessage::new("A new question", "reviewer").unwrap(),
            )
            .unwrap();
        let refreshed =
            dispatch(request(), &mut store, &runtime, &mut supervisors, &mut true).unwrap();
        assert!(refreshed.get("revision").unwrap().as_u64().unwrap() > state.revision);
        let mut invalid = request();
        invalid
            .params
            .as_object_mut()
            .unwrap()
            .remove("if_observation_id");
        assert_eq!(
            dispatch(invalid, &mut store, &runtime, &mut supervisors, &mut true)
                .unwrap_err()
                .code,
            ErrorCode::InvalidArgument
        );
    }

    #[test]
    fn agent_input_is_a_bounded_json_document_not_a_line_protocol() {
        let request = b"{\n  \"id\": 1,\n  \"method\": \"unsupported\",\n  \"params\": {}\n}\n";
        let mut output = Vec::new();
        assert!(serve_agent(Path::new("/unused"), request.as_slice(), &mut output).is_err());
        let response: Value = serde_json::from_slice(&output).unwrap();
        assert_eq!(response.get("id"), Some(&json!(1)));
        assert!(
            response
                .to_string()
                .contains("unsupported agent review operation")
        );
        for request in [Vec::new(), vec![b'x'; MAX_FRAME_BYTES + 1]] {
            let mut output = Vec::new();
            assert!(serve_agent(Path::new("/unused"), request.as_slice(), &mut output).is_err());
            assert!(
                serde_json::from_slice::<Value>(&output)
                    .unwrap()
                    .get("error")
                    .is_some()
            );
        }
    }

    #[test]
    fn agent_findings_and_editor_context_share_one_binding_and_thread_store() {
        let fixture = TestRepository::new();
        let mut store = Store::open(&fixture.repository()).unwrap();
        let runtime = BridgeRuntime {
            server: "test-server".to_owned(),
            workspace: WorkspaceId::new("workspace-1"),
            pane: None,
        };
        let load = || Request {
            id: 1,
            method: "context.load".to_owned(),
            params: json!({}),
        };
        assert!(
            context_operation(load(), &mut store, &runtime)
                .unwrap()
                .is_null()
        );
        let state = create_thread(
            2,
            &mut store,
            &runtime,
            serde_json::from_value(local_thread_params(&fixture)).unwrap(),
        )
        .unwrap();
        for (key, location) in [
            (
                "inline-concern",
                json!({"path":"lib.rs","side":"target","start_line":1,"end_line":1}),
            ),
            ("general-concern", Value::Null),
        ] {
            let save = || Request {
                id: 3,
                method: "finding.save".to_owned(),
                params: json!({"binding_id":state.binding.id,"observation_id":state.observation.id,
                    "finding":{"finding":{"key":key,"kind":"finding","severity":"non_blocking","title":"Concern","evidence":"Observed behavior","related_locations":[]},"location":location,"body":"Suggested improvement","author":"agent"}}),
            };
            context_operation(save(), &mut store, &runtime).unwrap();
            context_operation(save(), &mut store, &runtime).unwrap();
            let foreign = BridgeRuntime {
                workspace: WorkspaceId::new("workspace-2"),
                ..runtime.clone()
            };
            assert!(context_operation(save(), &mut store, &foreign).is_err());
        }
        let context = context_operation(load(), &mut store, &runtime).unwrap();
        let loaded: review_core::BindingState =
            serde_json::from_value(context.get("state").unwrap().clone()).unwrap();
        assert_eq!(loaded.binding.id, state.binding.id);
        assert_eq!(loaded.threads.len(), 3);
        assert_eq!(
            loaded
                .threads
                .iter()
                .filter(|thread| thread.finding.is_some())
                .count(),
            2
        );
        assert_eq!(
            loaded
                .threads
                .iter()
                .filter(|thread| thread.anchor.is_none())
                .count(),
            1
        );
        assert_eq!(context.get("working_tree_dirty"), Some(&json!(true)));
    }

    #[test]
    fn handshake_advertises_only_the_new_primitives() {
        let mut negotiated = false;
        let result = negotiate(1, json!({ "protocol": 1 }), &mut negotiated).unwrap();
        assert!(negotiated);
        let capabilities = result.get("capabilities").unwrap();
        assert_eq!(capabilities, &json!(CAPABILITIES));
        assert!(
            capabilities
                .as_array()
                .unwrap()
                .iter()
                .all(|capability| !capability.as_str().unwrap().contains("comment"))
        );
    }

    #[test]
    fn request_before_handshake_is_rejected() {
        let failure = require_handshake(7, false).unwrap_err();
        assert_eq!(failure.id, Some(7));
        assert_eq!(failure.code, ErrorCode::HandshakeRequired);
    }

    #[test]
    fn discovery_does_not_block_ordered_requests_and_keeps_scope_checks() {
        use herdrkit::testing::{Server, Step};
        use std::time::Duration;

        let fixture = TestRepository::new();
        let repository = fixture.repository();
        let server = Server::start(vec![Step::Wait(Duration::from_millis(300)), Step::Close]);
        let runtime = BridgeRuntime {
            server: server.client().server_id().unwrap(),
            workspace: WorkspaceId::new("workspace-1"),
            pane: Some(PaneId::new("pane-1")),
        };
        let mut store = Store::open(&repository).unwrap();
        let state = create_thread(
            1,
            &mut store,
            &runtime,
            serde_json::from_value(local_thread_params(&fixture)).unwrap(),
        )
        .unwrap();
        let lookup = || Request {
            id: 2,
            method: "agents.list".to_owned(),
            params: json!({ "binding_id": state.binding.id }),
        };
        assert_eq!(
            agent_lookup(lookup(), &store, &runtime, false)
                .unwrap_err()
                .code,
            ErrorCode::HandshakeRequired
        );
        let foreign = BridgeRuntime {
            workspace: WorkspaceId::new("workspace-2"),
            ..runtime.clone()
        };
        assert_eq!(
            agent_lookup(lookup(), &store, &foreign, true)
                .unwrap_err()
                .code,
            ErrorCode::InvalidState
        );
        drop(store);
        let input = [
            json!({ "id": 1, "method": "hello", "params": { "protocol": 1 } }),
            json!({ "id": 2, "method": "agents.list", "params": { "binding_id": state.binding.id } }),
            json!({ "id": 3, "method": "hello", "params": { "protocol": 1 } }),
        ].map(|frame| format!("{frame}\n")).concat();
        let mut output = Vec::new();
        let mut supervisors = DeliverySupervisors::new(repository.clone(), server.client());
        serve(
            input.as_bytes(),
            &mut output,
            &repository,
            &runtime,
            &mut supervisors,
        )
        .unwrap();
        let responses: Vec<Value> = output
            .split(|byte| *byte == b'\n')
            .filter(|line| !line.is_empty())
            .map(|line| serde_json::from_slice(line).unwrap())
            .collect();
        assert_eq!(
            responses
                .iter()
                .map(|frame| frame.get("id").and_then(Value::as_u64).unwrap())
                .collect::<Vec<_>>(),
            [1, 3, 2]
        );
        let [hello, next, discovery] = responses.as_slice() else {
            panic!("expected three complete response frames");
        };
        assert!(hello.get("result").unwrap().is_object());
        assert!(next.get("result").unwrap().is_object());
        assert_eq!(
            discovery.pointer("/error/code").unwrap(),
            "operation_failed"
        );
    }

    #[test]
    fn local_context_titles_describe_the_comparison() {
        let comparison = ComparisonSpec::new(
            DiffEndpoint::Commit {
                oid: "a".repeat(40),
            },
            DiffEndpoint::WorkingTree,
        )
        .unwrap();
        assert_eq!(
            comparison_title(&comparison),
            "commit aaaaaaaa → working tree"
        );
    }

    #[test]
    fn first_threads_resume_the_same_review_in_one_herdr_pane() {
        let fixture = TestRepository::new();
        let repository = fixture.repository();
        let runtime = BridgeRuntime {
            server: "server-1".to_owned(),
            workspace: WorkspaceId::new("workspace-1"),
            pane: Some(PaneId::new("pane-1")),
        };
        let mut supervisors = DeliverySupervisors::new(
            repository.clone(),
            herdrkit::Client::new(fixture.root.join("missing.sock")),
        );

        let mut store = Store::open(&repository).unwrap();
        let first = dispatch(
            Request {
                id: 1,
                method: "thread.create".to_owned(),
                params: local_thread_params(&fixture),
            },
            &mut store,
            &runtime,
            &mut supervisors,
            &mut true,
        )
        .unwrap();
        let second = dispatch(
            Request {
                id: 2,
                method: "thread.create".to_owned(),
                params: local_thread_params(&fixture),
            },
            &mut store,
            &runtime,
            &mut supervisors,
            &mut true,
        )
        .unwrap();

        assert_eq!(first.pointer("/binding/id"), second.pointer("/binding/id"));
        let store = Store::open(&repository).unwrap();
        let summary = store.summary().unwrap();
        assert_eq!(summary.contexts, 1);
        assert_eq!(summary.runtime_bindings, 1);
        assert_eq!(summary.open_threads, 2);
    }

    #[test]
    fn supervision_failure_prevents_agent_request_commit() {
        let fixture = TestRepository::new();
        let repository = fixture.repository();
        let runtime = BridgeRuntime {
            server: "server-1".to_owned(),
            workspace: WorkspaceId::new("workspace-1"),
            pane: Some(PaneId::new("pane-1")),
        };
        let mut supervisors = DeliverySupervisors::with_starter(
            repository.clone(),
            herdrkit::Client::new(fixture.root.join("missing.sock")),
            unavailable_supervisor,
        );
        let state = create_thread(
            1,
            &mut Store::open(&repository).unwrap(),
            &runtime,
            serde_json::from_value(local_thread_params(&fixture)).unwrap(),
        )
        .unwrap();
        let assignment = AgentAssignment::new(
            runtime.server.clone(),
            runtime.workspace.clone(),
            runtime.pane.clone().unwrap(),
            Some("agent".to_owned()),
            None,
        )
        .unwrap();
        let Some(thread) = state.threads.first() else {
            panic!("thread creation returned no thread");
        };
        let mut store = Store::open(&repository).unwrap();

        let failure = create_agent_request(
            2,
            &mut store,
            &mut supervisors,
            CreateAgentRequestParams {
                binding_id: state.binding.id.clone(),
                thread_ids: vec![thread.id.clone()],
                assignment,
            },
        )
        .unwrap_err();

        assert_eq!(failure.code, ErrorCode::DispatchUnavailable);
        assert!(
            store
                .binding_state(&state.binding.id)
                .unwrap()
                .agent_requests
                .is_empty()
        );
    }

    #[test]
    fn post_commit_wake_failure_preserves_successful_request() {
        let fixture = TestRepository::new();
        let repository = fixture.repository();
        let runtime = BridgeRuntime {
            server: "server-1".to_owned(),
            workspace: WorkspaceId::new("workspace-1"),
            pane: Some(PaneId::new("pane-1")),
        };
        let state = create_thread(
            1,
            &mut Store::open(&repository).unwrap(),
            &runtime,
            serde_json::from_value(local_thread_params(&fixture)).unwrap(),
        )
        .unwrap();
        let assignment = AgentAssignment::new(
            runtime.server.clone(),
            runtime.workspace.clone(),
            runtime.pane.clone().unwrap(),
            Some("agent".to_owned()),
            None,
        )
        .unwrap();
        let Some(thread) = state.threads.first() else {
            panic!("thread creation returned no thread");
        };
        let mut store = Store::open(&repository).unwrap();
        let saved = store
            .create_agent_request(
                &state.binding.id,
                &CreateAgentRequest::new(vec![thread.id.clone()], assignment).unwrap(),
            )
            .unwrap();
        let attempt = saved.attempts.last().unwrap();
        let mut supervisors = DeliverySupervisors::with_starter(
            repository,
            herdrkit::Client::new(fixture.root.join("missing.sock")),
            unavailable_supervisor,
        );

        supervisors.wake_after_commit(&state.binding.id, &attempt.id);

        let reloaded = store.binding_state(&state.binding.id).unwrap();
        assert_eq!(reloaded.agent_requests.len(), 1);
        let Some(request) = reloaded.agent_requests.first() else {
            panic!("persisted request was not returned");
        };
        assert_eq!(request.id, saved.id);
    }
}
