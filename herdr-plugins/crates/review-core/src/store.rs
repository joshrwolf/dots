use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{ErrorKind, Write as _};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use herdrkit::{PaneId, WorkspaceId};
use rusqlite::{
    Connection, ErrorCode as SqliteErrorCode, OptionalExtension as _, Row, Transaction,
    TransactionBehavior, params,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use crate::model::{validate_input_size, validate_required};
use crate::{
    AgentAssignment, AgentRequest, AgentRequestId, AgentRequestState, AnchorContext,
    AnchorLocation, BindingState, CapturedComparison, CapturedEndpoint, CheckoutKind,
    ComparisonSpec, ContextListQuery, ContextListing, CreateAgentRequest, CreateBoundThread,
    DiffEndpoint, DiffSide, DispatchAttempt, DispatchAttemptId, DispatchAttemptState, DispatchJob,
    DispatchOutcome, Error, ExternalReference, Finding, Message, MessageId, MessageOrigin,
    NewFinding, NewMessage, NewThread, Observation, ObservationId, Repository, Result,
    ReviewAnchor, ReviewContext, ReviewContextId, RuntimeBinding, RuntimeBindingId, Thread,
    ThreadId, ThreadStatus,
};

mod reads;

const BUSY_TIMEOUT: Duration = Duration::from_secs(5);
const CONFIGURE_BUSY_TIMEOUT: Duration = Duration::from_millis(100);
const MAX_LEASE: Duration = Duration::from_secs(5 * 60);
const MAX_REQUEST_PROJECTION_BYTES: usize = 256 * 1024;

#[derive(Serialize, Deserialize)]
struct RequestThreadSnapshot {
    thread: Thread,
    anchor_source_id: Option<i64>,
}

#[derive(Serialize)]
struct RequestProjection<'a> {
    new_message_ids: &'a [MessageId],
    answered_thread_ids: &'a [ThreadId],
    history_policy: &'static str,
    threads: &'a [Thread],
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoreSummary {
    pub checkouts: u64,
    pub contexts: u64,
    pub observations: u64,
    pub runtime_bindings: u64,
    pub open_threads: u64,
}

#[derive(Debug)]
pub struct Store {
    connection: Connection,
    repository: Repository,
    checkout_id: i64,
    checkout_token: String,
    path: PathBuf,
}

impl Store {
    pub fn open(repository: &Repository) -> Result<Self> {
        let path = repository.database_path().to_path_buf();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|source| Error::CreateStateDirectory {
                path: parent.to_path_buf(),
                source,
            })?;
        }
        let mut connection = Connection::open(&path).map_err(|source| Error::OpenDatabase {
            path: path.clone(),
            source,
        })?;
        configure(&connection, &path)?;
        initialize_schema(&mut connection, &path)?;
        let (checkout_id, checkout_token) = register_checkout(&connection, repository, &path)?;
        reconcile_observation_refs(&mut connection, repository, &path)?;
        Ok(Self {
            connection,
            repository: repository.clone(),
            checkout_id,
            checkout_token,
            path,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
    pub fn checkout_token(&self) -> &str {
        &self.checkout_token
    }

    pub fn summary(&self) -> Result<StoreSummary> {
        Ok(StoreSummary {
            checkouts: self.count("checkouts", None)?,
            contexts: self.count("review_contexts", None)?,
            observations: self.count("observations", None)?,
            runtime_bindings: self.count("runtime_bindings", None)?,
            open_threads: self.count("threads", Some("status = 'open'"))?,
        })
    }

    fn count(&self, table: &'static str, filter: Option<&'static str>) -> Result<u64> {
        let sql = filter.map_or_else(
            || format!("SELECT count(*) FROM {table}"),
            |filter| format!("SELECT count(*) FROM {table} WHERE {filter}"),
        );
        let count = self
            .connection
            .query_row(&sql, [], |row| row.get::<_, i64>(0))
            .map_err(|source| database_error("summarize review store", &self.path, source))?;
        u64::try_from(count).map_err(|_| Error::InvalidCount {
            path: self.path.clone(),
            field: table,
            count,
        })
    }

    pub fn create_context(
        &mut self,
        title: &str,
        references: &[ExternalReference],
    ) -> Result<ReviewContext> {
        validate_context_input(title, references)?;
        let path = self.path.clone();
        let transaction = self.transaction("create a review context")?;
        let id = insert_context(&transaction, title, references, &path)?;
        transaction
            .commit()
            .map_err(|source| database_error("commit a review context", &path, source))?;
        context_on(&self.connection, &id, &path)
    }

    pub fn list_contexts(&self, query: ContextListQuery) -> Result<Vec<ContextListing>> {
        ContextListQuery::new(query.context_limit, query.bindings_per_context)?;
        let mut statement = self.connection.prepare("SELECT c.id, c.title, c.created_at, c.updated_at, (SELECT count(*) FROM threads t WHERE t.context_id = c.id AND t.status = 'open') FROM review_contexts c ORDER BY c.updated_at DESC, c.id LIMIT ?1").map_err(|source| database_error("list review contexts", &self.path, source))?;
        let mut listings = statement
            .query_map([query.context_limit], |row| {
                Ok(ContextListing {
                    context: ReviewContext {
                        id: ReviewContextId::from_stored(row.get(0)?),
                        title: row.get(1)?,
                        references: Vec::new(),
                        created_at: row.get(2)?,
                        updated_at: row.get(3)?,
                    },
                    runtime_bindings: Vec::new(),
                    open_threads: nonnegative_count(row, 4)?,
                })
            })
            .map_err(|source| database_error("list review contexts", &self.path, source))?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(|source| database_error("read review contexts", &self.path, source))?;
        if listings.is_empty() {
            return Ok(listings);
        }
        let positions = listings
            .iter()
            .enumerate()
            .map(|(position, listing)| (listing.context.id.to_string(), position))
            .collect::<HashMap<_, _>>();

        let selected = "SELECT id FROM review_contexts ORDER BY updated_at DESC, id LIMIT ?1";
        let mut references = self.connection.prepare(&format!("SELECT r.context_id, r.kind, r.locator FROM external_references r JOIN ({selected}) c ON c.id = r.context_id ORDER BY r.context_id, r.kind, r.locator")).map_err(|source| database_error("list context references", &self.path, source))?;
        let references = references
            .query_map([query.context_limit], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    ExternalReference {
                        kind: row.get(1)?,
                        locator: row.get(2)?,
                    },
                ))
            })
            .map_err(|source| database_error("list context references", &self.path, source))?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(|source| database_error("read context references", &self.path, source))?;
        for (context, reference) in references {
            if let Some(position) = positions.get(&context) {
                listings
                    .get_mut(*position)
                    .ok_or_else(|| invalid_stored(&self.path, "context list position", &context))?
                    .context
                    .references
                    .push(reference);
            }
        }

        let mut bindings = self.connection.prepare(&format!("WITH selected AS ({selected}), ranked AS (SELECT b.*, row_number() OVER (PARTITION BY b.context_id ORDER BY b.updated_at DESC, b.id) AS binding_rank FROM runtime_bindings b JOIN selected c ON c.id = b.context_id) SELECT b.context_id, b.id, b.observation_id, c.checkout_token, c.checkout_root, b.checkout_kind, b.herdr_server_id, b.workspace_id, b.pane_id, b.created_at, b.updated_at FROM ranked b JOIN checkouts c ON c.id = b.checkout_id WHERE b.binding_rank <= ?2 ORDER BY b.context_id, b.updated_at DESC, b.id")).map_err(|source| database_error("list context runtime bindings", &self.path, source))?;
        let bindings = bindings
            .query_map(
                params![query.context_limit, query.bindings_per_context],
                |row| {
                    let raw_kind = row.get::<_, String>(5)?;
                    let checkout_kind = CheckoutKind::from_str(&raw_kind).ok_or_else(|| {
                        rusqlite::Error::InvalidColumnType(
                            5,
                            "checkout_kind".to_owned(),
                            rusqlite::types::Type::Text,
                        )
                    })?;
                    Ok((
                        row.get::<_, String>(0)?,
                        RuntimeBinding {
                            id: RuntimeBindingId::from_stored(row.get(1)?),
                            context_id: ReviewContextId::from_stored(row.get(0)?),
                            observation_id: ObservationId::from_stored(row.get(2)?),
                            checkout_token: row.get(3)?,
                            checkout_root: row.get(4)?,
                            checkout_kind,
                            herdr_server_id: row.get(6)?,
                            workspace_id: WorkspaceId::new(row.get::<_, String>(7)?),
                            pane_id: row.get::<_, Option<String>>(8)?.map(PaneId::new),
                            created_at: row.get(9)?,
                            updated_at: row.get(10)?,
                        },
                    ))
                },
            )
            .map_err(|source| database_error("list context runtime bindings", &self.path, source))?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(|source| {
                database_error("read context runtime bindings", &self.path, source)
            })?;
        for (context, binding) in bindings {
            if let Some(position) = positions.get(&context) {
                listings
                    .get_mut(*position)
                    .ok_or_else(|| invalid_stored(&self.path, "context list position", &context))?
                    .runtime_bindings
                    .push(binding);
            }
        }
        Ok(listings)
    }

    pub fn context(&self, id: &ReviewContextId) -> Result<ReviewContext> {
        context_on(&self.connection, id, &self.path)
    }

    pub fn contexts_by_reference(
        &self,
        reference: &ExternalReference,
    ) -> Result<Vec<ReviewContext>> {
        reference.validate()?;
        let mut statement = self.connection.prepare("SELECT r.context_id FROM external_references r JOIN review_contexts c ON c.id = r.context_id WHERE r.kind = ?1 AND r.locator = ?2 ORDER BY c.updated_at DESC, r.context_id").map_err(|source| database_error("find review contexts by external reference", &self.path, source))?;
        let ids = statement
            .query_map(params![reference.kind, reference.locator], |row| {
                row.get::<_, String>(0)
            })
            .map_err(|source| {
                database_error(
                    "find review contexts by external reference",
                    &self.path,
                    source,
                )
            })?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(|source| {
                database_error("read referenced review contexts", &self.path, source)
            })?;
        ids.into_iter()
            .map(|id| {
                context_on(
                    &self.connection,
                    &ReviewContextId::from_stored(id),
                    &self.path,
                )
            })
            .collect()
    }

    pub fn add_external_reference(
        &mut self,
        context: &ReviewContextId,
        reference: &ExternalReference,
    ) -> Result<ReviewContext> {
        reference.validate()?;
        let path = self.path.clone();
        let transaction = self.transaction("add an external reference")?;
        require_context(&transaction, context, &path)?;
        transaction.execute("INSERT INTO external_references (context_id, kind, locator) VALUES (?1, ?2, ?3) ON CONFLICT(context_id, kind, locator) DO NOTHING", params![context.as_str(), reference.kind, reference.locator]).map_err(|source| database_error("add an external reference", &path, source))?;
        let updated = context_on(&transaction, context, &path)?;
        validate_context_input(&updated.title, &updated.references)?;
        touch_context(&transaction, context, &path)?;
        transaction
            .commit()
            .map_err(|source| database_error("commit an external reference", &path, source))?;
        context_on(&self.connection, context, &path)
    }

    pub fn set_context_title(
        &mut self,
        context: &ReviewContextId,
        title: &str,
    ) -> Result<ReviewContext> {
        validate_required("review title", title)?;
        let path = self.path.clone();
        let transaction = self.transaction("set a review context title")?;
        let changed = transaction
            .execute(
                "UPDATE review_contexts SET title = ?2, revision = revision + 1, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?1",
                params![context.as_str(), title],
            )
            .map_err(|source| database_error("set a review context title", &path, source))?;
        if changed != 1 {
            return Err(Error::ReviewContextNotFound {
                context: context.to_string(),
            });
        }
        let updated = context_on(&transaction, context, &path)?;
        validate_context_input(&updated.title, &updated.references)?;
        transaction
            .commit()
            .map_err(|source| database_error("commit a review context title", &path, source))?;
        context_on(&self.connection, context, &path)
    }

    pub fn record_observation(
        &mut self,
        context: &ReviewContextId,
        spec: &ComparisonSpec,
    ) -> Result<Observation> {
        require_context(&self.connection, context, &self.path)?;
        let proposed = new_id::<ObservationId>(&self.connection, &self.path)?;
        let comparison = self.repository.capture_comparison(spec)?;
        let repository = self.repository.clone();
        let path = self.path.clone();
        let transaction = self.transaction("record a review observation")?;
        let id = insert_observation(&transaction, context, proposed.clone(), &comparison, &path)?;
        touch_context(&transaction, context, &path)?;
        commit_observation(
            transaction,
            &repository,
            &id,
            &comparison,
            id == proposed,
            "commit a review observation",
            &path,
        )?;
        observation_on(&self.connection, &id, &self.path)
    }

    pub fn create_context_with_observation(
        &mut self,
        title: &str,
        references: &[ExternalReference],
        spec: &ComparisonSpec,
    ) -> Result<(ReviewContext, Observation)> {
        validate_context_input(title, references)?;
        let proposed = new_id::<ObservationId>(&self.connection, &self.path)?;
        let comparison = self.repository.capture_comparison(spec)?;
        let repository = self.repository.clone();
        let path = self.path.clone();
        let transaction = self.transaction("create a review context and observation")?;
        let context_id = insert_context(&transaction, title, references, &path)?;
        let observation_id = insert_observation(
            &transaction,
            &context_id,
            proposed.clone(),
            &comparison,
            &path,
        )?;
        commit_observation(
            transaction,
            &repository,
            &observation_id,
            &comparison,
            observation_id == proposed,
            "commit a review context and observation",
            &path,
        )?;
        Ok((
            context_on(&self.connection, &context_id, &path)?,
            observation_on(&self.connection, &observation_id, &path)?,
        ))
    }

    /// Creates a new context, its first immutable observation, one runtime
    /// binding, and the first discussion thread as one durable operation.
    pub fn create_bound_thread(&mut self, request: &CreateBoundThread) -> Result<BindingState> {
        request.validate()?;
        validate_context_input(&request.title, &request.references)?;

        let proposed_observation = new_id::<ObservationId>(&self.connection, &self.path)?;
        let comparison = self.repository.capture_comparison(&request.comparison)?;
        let repository = self.repository.clone();
        let source = self
            .repository
            .anchor_source(&comparison, &request.thread.anchor.original)?
            .ok_or_else(|| Error::AnchorSourceUnavailable {
                path: request.thread.anchor.original.path.clone(),
                endpoint: Repository::endpoint_description(
                    &comparison,
                    request.thread.anchor.original.side,
                ),
            })?;
        validate_anchor_source(&request.thread.anchor, &source)?;

        let path = self.path.clone();
        let checkout_id = self.checkout_id;
        let transaction = self.transaction("create a bound review thread")?;
        let context = insert_context(&transaction, &request.title, &request.references, &path)?;
        let observation = insert_observation(
            &transaction,
            &context,
            proposed_observation.clone(),
            &comparison,
            &path,
        )?;
        let binding = insert_runtime_binding(
            &transaction,
            &context,
            &observation,
            checkout_id,
            request.checkout_kind,
            &request.herdr_server_id,
            &request.workspace_id,
            request.pane_id.as_ref(),
            &path,
        )?;
        insert_thread(
            &transaction,
            &context,
            &observation,
            &request.thread,
            &source,
            &path,
        )?;
        commit_observation(
            transaction,
            &repository,
            &observation,
            &comparison,
            observation == proposed_observation,
            "commit a bound review thread",
            &path,
        )?;
        self.binding_state(&binding)
    }

    pub fn observations(&self, context: &ReviewContextId) -> Result<Vec<Observation>> {
        require_context(&self.connection, context, &self.path)?;
        let mut statement = self
            .connection
            .prepare("SELECT id FROM observations WHERE context_id = ?1 ORDER BY ordinal, id")
            .map_err(|source| database_error("list observations", &self.path, source))?;
        let ids = statement
            .query_map([context.as_str()], |row| row.get::<_, String>(0))
            .map_err(|source| database_error("list observations", &self.path, source))?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(|source| database_error("read observations", &self.path, source))?;
        ids.into_iter()
            .map(|id| {
                observation_on(
                    &self.connection,
                    &ObservationId::from_stored(id),
                    &self.path,
                )
            })
            .collect()
    }

    pub fn observation(&self, id: &ObservationId) -> Result<Observation> {
        observation_on(&self.connection, id, &self.path)
    }

    pub fn bind_runtime(
        &mut self,
        context: &ReviewContextId,
        observation: &ObservationId,
        herdr_server_id: &str,
        workspace: &WorkspaceId,
        pane: Option<&PaneId>,
        checkout_kind: CheckoutKind,
    ) -> Result<RuntimeBinding> {
        validate_runtime_input(herdr_server_id, workspace, pane)?;
        let path = self.path.clone();
        let checkout_id = self.checkout_id;
        let transaction = self.transaction("bind a review runtime")?;
        require_observation_context(&transaction, context, observation, &path)?;
        let id = insert_runtime_binding(
            &transaction,
            context,
            observation,
            checkout_id,
            checkout_kind,
            herdr_server_id,
            workspace,
            pane,
            &path,
        )?;
        touch_context(&transaction, context, &path)?;
        transaction
            .commit()
            .map_err(|source| database_error("commit a review runtime binding", &path, source))?;
        self.runtime_binding(&id)
    }

    pub fn runtime_binding(&self, binding: &RuntimeBindingId) -> Result<RuntimeBinding> {
        binding_on(&self.connection, binding, &self.path)
    }

    pub fn runtime_bindings(&self, context: &ReviewContextId) -> Result<Vec<RuntimeBinding>> {
        require_context(&self.connection, context, &self.path)?;
        let mut statement = self.connection.prepare("SELECT id FROM runtime_bindings WHERE context_id = ?1 ORDER BY updated_at DESC, id").map_err(|source| database_error("list runtime bindings", &self.path, source))?;
        let ids = statement
            .query_map([context.as_str()], |row| row.get::<_, String>(0))
            .map_err(|source| database_error("list runtime bindings", &self.path, source))?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(|source| database_error("read runtime bindings", &self.path, source))?;
        ids.into_iter()
            .map(|id| {
                binding_on(
                    &self.connection,
                    &RuntimeBindingId::from_stored(id),
                    &self.path,
                )
            })
            .collect()
    }

    /// Finds the most recently used binding for this checkout, Herdr view, and
    /// operational comparison. This lets an editor with no in-memory binding
    /// resume its existing local review instead of creating parallel lineage.
    pub fn matching_runtime_binding(
        &self,
        spec: &ComparisonSpec,
        herdr_server_id: &str,
        workspace: &WorkspaceId,
        pane: Option<&PaneId>,
    ) -> Result<Option<RuntimeBinding>> {
        spec.validate()?;
        validate_runtime_input(herdr_server_id, workspace, pane)?;
        let (base_kind, base_oid) = spec.base.columns();
        let (target_kind, target_oid) = spec.target.columns();
        let id = self
            .connection
            .query_row(
                "SELECT b.id
                 FROM runtime_bindings b
                 JOIN observations o ON o.id = b.observation_id
                 WHERE b.checkout_id = ?1
                   AND b.herdr_server_id = ?2
                   AND b.workspace_id = ?3
                   AND ((b.pane_id IS NULL AND ?4 IS NULL) OR b.pane_id = ?4)
                   AND o.base_source_kind = ?5
                   AND o.base_source_oid IS ?6
                   AND o.target_source_kind = ?7
                   AND o.target_source_oid IS ?8
                 ORDER BY b.updated_at DESC, b.id
                 LIMIT 1",
                params![
                    self.checkout_id,
                    herdr_server_id,
                    workspace.as_str(),
                    pane.map(PaneId::as_str),
                    base_kind,
                    base_oid,
                    target_kind,
                    target_oid,
                ],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(|source| {
                database_error("find a matching review runtime", &self.path, source)
            })?;
        id.map(|id| self.runtime_binding(&RuntimeBindingId::from_stored(id)))
            .transpose()
    }

    pub fn binding_state(&mut self, binding: &RuntimeBindingId) -> Result<BindingState> {
        self.binding_state_page(binding, None)
    }

    /// Captures a fresh immutable observation and projects this binding onto it.
    /// Other bindings of the same context are left untouched.
    pub fn observe_binding(
        &mut self,
        binding: &RuntimeBindingId,
        spec: &ComparisonSpec,
    ) -> Result<BindingState> {
        let binding_value = self.require_local_binding(binding)?;
        let proposed = new_id::<ObservationId>(&self.connection, &self.path)?;
        let comparison = self.repository.capture_comparison(spec)?;
        let repository = self.repository.clone();
        let path = self.path.clone();
        let checkout_id = self.checkout_id;
        let transaction = self.transaction("observe a review binding")?;
        let observation = insert_observation(
            &transaction,
            &binding_value.context_id,
            proposed.clone(),
            &comparison,
            &path,
        )?;
        let changed = transaction
            .execute(
                "UPDATE runtime_bindings SET observation_id = ?2, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?1 AND checkout_id = ?3",
                params![binding.as_str(), observation.as_str(), checkout_id],
            )
            .map_err(|source| database_error("project a binding onto an observation", &path, source))?;
        if changed != 1 {
            return Err(Error::RuntimeBindingNotFound {
                binding: binding.to_string(),
            });
        }
        touch_context(&transaction, &binding_value.context_id, &path)?;
        commit_observation(
            transaction,
            &repository,
            &observation,
            &comparison,
            observation == proposed,
            "commit a binding observation",
            &path,
        )?;
        self.binding_state(binding)
    }

    fn require_local_binding(&self, binding: &RuntimeBindingId) -> Result<RuntimeBinding> {
        let value = self.runtime_binding(binding)?;
        if value.checkout_token != self.checkout_token {
            return Err(Error::BindingCheckoutMismatch {
                binding: binding.to_string(),
            });
        }
        Ok(value)
    }

    fn transaction(&mut self, operation: &'static str) -> Result<Transaction<'_>> {
        self.connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|source| database_error(operation, &self.path, source))
    }
}

fn nonnegative_count(row: &Row<'_>, column: usize) -> rusqlite::Result<u64> {
    u64::try_from(row.get::<_, i64>(column)?).map_err(|source| {
        rusqlite::Error::FromSqlConversionFailure(
            column,
            rusqlite::types::Type::Integer,
            Box::new(source),
        )
    })
}

trait StoredId: Sized {
    fn from_stored(value: String) -> Self;
}
macro_rules! stored_id { ($($ty:ty),+ $(,)?) => { $(impl StoredId for $ty { fn from_stored(value: String) -> Self { <$ty>::from_stored(value) } })+ }; }
stored_id!(
    ReviewContextId,
    ObservationId,
    RuntimeBindingId,
    ThreadId,
    MessageId,
    AgentRequestId,
    DispatchAttemptId
);

fn new_id<T: StoredId>(connection: &Connection, path: &Path) -> Result<T> {
    connection
        .query_row("SELECT lower(hex(randomblob(16)))", [], |row| {
            row.get::<_, String>(0)
        })
        .map(T::from_stored)
        .map_err(|source| database_error("generate an identity", path, source))
}

fn validate_context_input(title: &str, references: &[ExternalReference]) -> Result<()> {
    validate_required("review title", title)?;
    for reference in references {
        reference.validate()?;
    }
    let mut unique = std::collections::HashSet::new();
    if references
        .iter()
        .any(|reference| !unique.insert((&reference.kind, &reference.locator)))
    {
        return Err(Error::DuplicateExternalReference);
    }
    validate_input_size("review context", &(title, references))
}

fn validate_runtime_input(
    herdr_server_id: &str,
    workspace: &WorkspaceId,
    pane: Option<&PaneId>,
) -> Result<()> {
    validate_required("Herdr server id", herdr_server_id)?;
    validate_required("review workspace id", workspace.as_str())?;
    if let Some(pane) = pane {
        validate_required("review editor pane id", pane.as_str())?;
    }
    validate_input_size("review runtime", &(herdr_server_id, workspace, pane))
}

fn insert_context(
    connection: &Connection,
    title: &str,
    references: &[ExternalReference],
    path: &Path,
) -> Result<ReviewContextId> {
    let id = connection.query_row("INSERT INTO review_contexts (id, title) VALUES (lower(hex(randomblob(16))), ?1) RETURNING id", [title], |row| row.get::<_, String>(0)).map(ReviewContextId::from_stored).map_err(|source| database_error("create a review context", path, source))?;
    for reference in references {
        connection
            .execute(
                "INSERT INTO external_references (context_id, kind, locator) VALUES (?1, ?2, ?3)",
                params![id.as_str(), reference.kind, reference.locator],
            )
            .map_err(|source| database_error("add an external reference", path, source))?;
    }
    Ok(id)
}

fn insert_observation(
    connection: &Connection,
    context: &ReviewContextId,
    proposed: ObservationId,
    comparison: &CapturedComparison,
    path: &Path,
) -> Result<ObservationId> {
    let fingerprint = comparison_fingerprint(comparison);
    if let Some(id) = connection
        .query_row(
            "SELECT id FROM observations WHERE context_id = ?1 AND fingerprint = ?2",
            params![context.as_str(), fingerprint],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(|source| database_error("find a matching observation", path, source))?
    {
        return Ok(ObservationId::from_stored(id));
    }
    let ordinal = next_ordinal(
        connection,
        "observations",
        "context_id",
        context.as_str(),
        path,
    )?;
    let (base_kind, base_source_oid) = comparison.base.source.columns();
    let (target_kind, target_source_oid) = comparison.target.source.columns();
    connection.execute("INSERT INTO observations (id, context_id, ordinal, fingerprint, base_source_kind, base_source_oid, base_oid, target_source_kind, target_source_oid, target_oid) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)", params![proposed.as_str(), context.as_str(), ordinal, fingerprint, base_kind, base_source_oid, comparison.base.oid, target_kind, target_source_oid, comparison.target.oid]).map_err(|source| database_error("record a review observation", path, source))?;
    Ok(proposed)
}

#[allow(clippy::too_many_arguments)]
fn commit_observation(
    transaction: Transaction<'_>,
    repository: &Repository,
    observation: &ObservationId,
    comparison: &CapturedComparison,
    is_new: bool,
    operation: &'static str,
    path: &Path,
) -> Result<()> {
    if is_new {
        repository.retain_comparison(observation, comparison)?;
    }
    if let Err(source) = transaction.commit() {
        if is_new {
            repository.release_observation(observation);
        }
        return Err(database_error(operation, path, source));
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn insert_runtime_binding(
    connection: &Connection,
    context: &ReviewContextId,
    observation: &ObservationId,
    checkout_id: i64,
    checkout_kind: CheckoutKind,
    herdr_server_id: &str,
    workspace: &WorkspaceId,
    pane: Option<&PaneId>,
    path: &Path,
) -> Result<RuntimeBindingId> {
    connection.query_row("INSERT INTO runtime_bindings (id, context_id, observation_id, checkout_id, checkout_kind, herdr_server_id, workspace_id, pane_id) VALUES (lower(hex(randomblob(16))), ?1, ?2, ?3, ?4, ?5, ?6, ?7) RETURNING id", params![context.as_str(), observation.as_str(), checkout_id, checkout_kind.as_str(), herdr_server_id, workspace.as_str(), pane.map(PaneId::as_str)], |row| row.get::<_, String>(0)).map(RuntimeBindingId::from_stored).map_err(|source| database_error("bind a review runtime", path, source))
}

fn comparison_fingerprint(comparison: &CapturedComparison) -> String {
    let mut digest = Sha256::new();
    for endpoint in [&comparison.base, &comparison.target] {
        let (kind, source_oid) = endpoint.source.columns();
        for value in [
            kind.as_bytes(),
            source_oid.unwrap_or("").as_bytes(),
            endpoint.oid.as_bytes(),
        ] {
            digest.update(value.len().to_be_bytes());
            digest.update(value);
        }
    }
    format!("{:x}", digest.finalize())
}

fn next_ordinal(
    connection: &Connection,
    table: &'static str,
    parent: &'static str,
    id: &str,
    path: &Path,
) -> Result<u32> {
    let value = connection
        .query_row(
            &format!("SELECT coalesce(max(ordinal), 0) + 1 FROM {table} WHERE {parent} = ?1"),
            [id],
            |row| row.get::<_, i64>(0),
        )
        .map_err(|source| database_error("allocate an ordinal", path, source))?;
    u32::try_from(value).map_err(|_| Error::InvalidCount {
        path: path.to_path_buf(),
        field: "ordinal",
        count: value,
    })
}

impl Store {
    /// Resolves this checkout's binding without borrowing the focused workspace.
    pub fn current_runtime_binding(
        &self,
        server: &str,
        workspace: &WorkspaceId,
    ) -> Result<Option<RuntimeBinding>> {
        let mut statement = self.connection.prepare("SELECT id FROM runtime_bindings WHERE checkout_id = ?1 AND herdr_server_id = ?2 AND workspace_id = ?3 ORDER BY updated_at DESC, id DESC")
            .map_err(|source| database_error("find current runtime bindings", &self.path, source))?;
        let ids = statement
            .query_map(
                params![self.checkout_id, server, workspace.as_str()],
                |row| row.get::<_, String>(0),
            )
            .map_err(|source| database_error("query current runtime bindings", &self.path, source))?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(|source| {
                database_error("decode current runtime bindings", &self.path, source)
            })?;
        let mut selected: Option<RuntimeBinding> = None;
        for id in ids {
            let binding = binding_on(
                &self.connection,
                &RuntimeBindingId::from_stored(id),
                &self.path,
            )?;
            if let Some(current) = &selected {
                if current.context_id != binding.context_id
                    || current.observation_id != binding.observation_id
                {
                    return Err(Error::AmbiguousRuntimeBinding {
                        workspace: workspace.to_string(),
                    });
                }
            } else {
                selected = Some(binding);
            }
        }
        Ok(selected)
    }

    /// Saves or updates a finding by its stable context-scoped key.
    pub fn save_finding(
        &mut self,
        binding: &RuntimeBindingId,
        expected: &ObservationId,
        request: &NewFinding,
    ) -> Result<BindingState> {
        request.validate()?;
        let value = self.require_local_binding(binding)?;
        let observation = self.observation(expected)?;
        if value.observation_id != *expected {
            return Err(Error::StaleObservation {
                binding: binding.to_string(),
            });
        }
        let source = request
            .location
            .as_ref()
            .map(|location| {
                self.repository
                    .anchor_source(&observation.comparison, location)?
                    .ok_or_else(|| Error::AnchorSourceUnavailable {
                        path: location.path.clone(),
                        endpoint: Repository::endpoint_description(
                            &observation.comparison,
                            location.side,
                        ),
                    })
            })
            .transpose()?;
        if let (Some(location), Some(source)) = (&request.location, &source)
            && usize::try_from(location.end_line)
                .ok()
                .is_none_or(|end| end > source.lines().count())
        {
            return Err(Error::AnchorSourceMismatch {
                path: location.path.clone(),
                start_line: location.start_line,
                end_line: location.end_line,
            });
        }
        for related in &request.finding.related_locations {
            let source = self
                .repository
                .anchor_source(&observation.comparison, related)?;
            if source.as_ref().is_none_or(|source| {
                usize::try_from(related.end_line)
                    .ok()
                    .is_none_or(|end| end > source.lines().count())
            }) {
                return Err(Error::AnchorSourceMismatch {
                    path: related.path.clone(),
                    start_line: related.start_line,
                    end_line: related.end_line,
                });
            }
        }
        let json = serde_json::to_string(&request.finding).map_err(Error::EncodeFinding)?;
        let path = self.path.clone();
        let checkout_id = self.checkout_id;
        let transaction = self.transaction("save a finding")?;
        let current = binding_on(&transaction, binding, &path)?;
        let valid = transaction
            .query_row(
                "SELECT checkout_id = ?2 FROM runtime_bindings WHERE id = ?1",
                params![binding.as_str(), checkout_id],
                |row| row.get::<_, bool>(0),
            )
            .map_err(|source| database_error("validate finding checkout", &path, source))?;
        if !valid {
            return Err(Error::BindingCheckoutMismatch {
                binding: binding.to_string(),
            });
        }
        if current.observation_id != *expected {
            return Err(Error::StaleObservation {
                binding: binding.to_string(),
            });
        }
        let source_id = source
            .as_ref()
            .map(|source| {
                save_anchor_source(
                    &transaction,
                    Sha256::digest(source.as_bytes()).as_ref(),
                    source,
                    &path,
                )
            })
            .transpose()?;
        let location = request.location.as_ref();
        let thread = transaction.query_row(
            "INSERT INTO threads (id, context_id, observation_id, path, side, start_line, end_line, anchor_source_id, finding_key, finding_json, status) VALUES (lower(hex(randomblob(16))), ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 'open') ON CONFLICT(context_id, finding_key) DO UPDATE SET observation_id = excluded.observation_id, path = excluded.path, side = excluded.side, start_line = excluded.start_line, end_line = excluded.end_line, anchor_source_id = excluded.anchor_source_id, context_json = NULL, finding_json = excluded.finding_json, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') RETURNING id",
            params![current.context_id.as_str(), expected.as_str(), location.map(|loc| loc.path.as_str()), location.map(|loc| loc.side.as_str()), location.map(|loc| loc.start_line), location.map(|loc| loc.end_line), source_id, request.finding.key, json], |row| row.get::<_, String>(0))
            .map(ThreadId::from_stored).map_err(|source| database_error("save finding", &path, source))?;
        transaction.execute("INSERT INTO messages (id, thread_id, position, author, body, origin) VALUES (lower(hex(randomblob(16))), ?1, 1, ?2, ?3, 'agent') ON CONFLICT(thread_id, position) DO UPDATE SET body = excluded.body, author = excluded.author", params![thread.as_str(), request.author, request.body])
            .map_err(|source| database_error("save finding message", &path, source))?;
        touch_context(&transaction, &current.context_id, &path)?;
        transaction
            .commit()
            .map_err(|source| database_error("commit finding", &path, source))?;
        self.binding_state(binding)
    }

    pub fn create_thread(
        &mut self,
        binding: &RuntimeBindingId,
        request: &NewThread,
    ) -> Result<BindingState> {
        request.validate()?;
        let binding = self.require_local_binding(binding)?;
        let observation = self.observation(&binding.observation_id)?;
        let source = self
            .repository
            .anchor_source(&observation.comparison, &request.anchor.original)?
            .ok_or_else(|| Error::AnchorSourceUnavailable {
                path: request.anchor.original.path.clone(),
                endpoint: Repository::endpoint_description(
                    &observation.comparison,
                    request.anchor.original.side,
                ),
            })?;
        validate_anchor_source(&request.anchor, &source)?;
        let path = self.path.clone();
        let transaction = self.transaction("create a review thread")?;
        insert_thread(
            &transaction,
            &binding.context_id,
            &binding.observation_id,
            request,
            &source,
            &path,
        )?;
        touch_context(&transaction, &binding.context_id, &path)?;
        transaction
            .commit()
            .map_err(|source| database_error("commit a review thread", &path, source))?;
        self.binding_state(&binding.id)
    }

    /// Captures a comparison and creates a thread on it without exposing an
    /// intermediate binding projection if thread validation fails.
    pub fn create_thread_on_comparison(
        &mut self,
        binding: &RuntimeBindingId,
        spec: &ComparisonSpec,
        request: &NewThread,
    ) -> Result<BindingState> {
        request.validate()?;
        let binding_value = self.require_local_binding(binding)?;
        let proposed = new_id::<ObservationId>(&self.connection, &self.path)?;
        let comparison = self.repository.capture_comparison(spec)?;
        let repository = self.repository.clone();
        let source = self
            .repository
            .anchor_source(&comparison, &request.anchor.original)?
            .ok_or_else(|| Error::AnchorSourceUnavailable {
                path: request.anchor.original.path.clone(),
                endpoint: Repository::endpoint_description(
                    &comparison,
                    request.anchor.original.side,
                ),
            })?;
        validate_anchor_source(&request.anchor, &source)?;

        let path = self.path.clone();
        let checkout_id = self.checkout_id;
        let transaction = self.transaction("create a thread on a comparison")?;
        let observation = insert_observation(
            &transaction,
            &binding_value.context_id,
            proposed.clone(),
            &comparison,
            &path,
        )?;
        let changed = transaction
            .execute(
                "UPDATE runtime_bindings SET observation_id = ?2, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?1 AND checkout_id = ?3",
                params![binding.as_str(), observation.as_str(), checkout_id],
            )
            .map_err(|source| database_error("project a thread binding", &path, source))?;
        if changed != 1 {
            return Err(Error::RuntimeBindingNotFound {
                binding: binding.to_string(),
            });
        }
        insert_thread(
            &transaction,
            &binding_value.context_id,
            &observation,
            request,
            &source,
            &path,
        )?;
        touch_context(&transaction, &binding_value.context_id, &path)?;
        commit_observation(
            transaction,
            &repository,
            &observation,
            &comparison,
            observation == proposed,
            "commit a thread comparison",
            &path,
        )?;
        self.binding_state(binding)
    }

    pub fn add_message(
        &mut self,
        binding: &RuntimeBindingId,
        thread: &ThreadId,
        request: &NewMessage,
    ) -> Result<Message> {
        request.validate()?;
        let binding = self.require_local_binding(binding)?;
        let path = self.path.clone();
        let transaction = self.transaction("add a review message")?;
        require_thread_scope(&transaction, &binding, thread, &path)?;
        let message = insert_message(&transaction, thread, request, None, &path)?;
        transaction.execute("UPDATE threads SET updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?1", [thread.as_str()]).map_err(|source| database_error("touch a review thread", &path, source))?;
        touch_context(&transaction, &binding.context_id, &path)?;
        transaction
            .commit()
            .map_err(|source| database_error("commit a review message", &path, source))?;
        message_on(&self.connection, &message, &path)
    }

    pub fn resolve_thread(&mut self, binding: &RuntimeBindingId, thread: &ThreadId) -> Result<()> {
        self.set_thread_status(binding, thread, ThreadStatus::Resolved)
    }

    /// A reply belongs to one dispatched request, not to the currently
    /// focused editor. Repeating an identical save is harmless.
    pub fn reply_to_request(
        &mut self,
        binding: &RuntimeBindingId,
        request: &AgentRequestId,
        thread: &ThreadId,
        expected_attempt: &DispatchAttemptId,
        caller: &AgentAssignment,
        message: &NewMessage,
    ) -> Result<Message> {
        message.validate()?;
        let binding_value = self.require_local_binding(binding)?;
        let path = self.path.clone();
        let transaction = self.transaction("reply to a review request")?;
        require_thread_scope(&transaction, &binding_value, thread, &path)?;
        let request_value = request_without_attempts_on(&transaction, request, &path)?;
        let attempt = latest_dispatch_on(&transaction, request, &path)?;
        if request_value.context_id != binding_value.context_id
            || !request_value.thread_ids.contains(thread)
            || attempt.id != *expected_attempt
            || attempt.runtime_binding_id != *binding
            || !attempt.assignment.matches_assignment(caller)
            || !matches!(
                attempt.state,
                DispatchAttemptState::Dispatching
                    | DispatchAttemptState::Returned
                    | DispatchAttemptState::Unknown
                    | DispatchAttemptState::Blocked
            )
        {
            return Err(Error::InvalidAgentReply);
        }
        let existing = transaction
            .query_row(
                "SELECT id FROM messages WHERE thread_id = ?1 AND reply_to_request = ?2",
                params![thread.as_str(), request.as_str()],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(|source| database_error("find an existing agent reply", &path, source))?;
        if let Some(existing) = existing {
            let existing = message_on(&transaction, &MessageId::from_stored(existing), &path)?;
            if existing.body != message.body || existing.author != message.author {
                return Err(Error::ConflictingAgentReply);
            }
            return Ok(existing);
        }
        let id = insert_message(&transaction, thread, message, Some(request), &path)?;
        transaction.execute("UPDATE threads SET updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?1", [thread.as_str()])
            .map_err(|source| database_error("touch a review thread", &path, source))?;
        touch_context(&transaction, &binding_value.context_id, &path)?;
        transaction
            .commit()
            .map_err(|source| database_error("commit an agent reply", &path, source))?;
        message_on(&self.connection, &id, &path)
    }

    pub fn reopen_thread(&mut self, binding: &RuntimeBindingId, thread: &ThreadId) -> Result<()> {
        self.set_thread_status(binding, thread, ThreadStatus::Open)
    }

    fn set_thread_status(
        &mut self,
        binding: &RuntimeBindingId,
        thread: &ThreadId,
        status: ThreadStatus,
    ) -> Result<()> {
        let binding = self.require_local_binding(binding)?;
        let path = self.path.clone();
        let transaction = self.transaction("set a review thread status")?;
        require_thread_scope(&transaction, &binding, thread, &path)?;
        transaction.execute("UPDATE threads SET status = ?2, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?1", params![thread.as_str(), status.as_str()]).map_err(|source| database_error("set a review thread status", &path, source))?;
        touch_context(&transaction, &binding.context_id, &path)?;
        transaction
            .commit()
            .map_err(|source| database_error("commit a review thread status", &path, source))?;
        Ok(())
    }

    pub fn create_agent_request(
        &mut self,
        binding: &RuntimeBindingId,
        request: &CreateAgentRequest,
    ) -> Result<AgentRequest> {
        request.validate()?;
        let binding_value = self.require_local_binding(binding)?;
        let path = self.path.clone();
        let repository = self.repository.clone();
        let transaction = self.transaction("create an agent request")?;
        for thread in &request.thread_ids {
            require_thread_scope(&transaction, &binding_value, thread, &path)?;
            let status = thread_status_on(&transaction, thread, &path)?;
            if status != ThreadStatus::Open {
                return Err(Error::ThreadNotOpen {
                    thread: thread.to_string(),
                });
            }
        }
        let mut threads = Vec::new();
        let mut pending_messages = Vec::new();
        let mut mandatory_bytes = 0;
        let mut context_bytes = 0;
        for thread in &request.thread_ids {
            let pending = unreserved_reviewer_messages_on(&transaction, thread, &path)?;
            if !pending.is_empty() {
                if threads.len() >= 256 || pending_messages.len() + pending.len() > 1024 {
                    return Err(Error::RequestTooLarge);
                }
                threads.push(request_thread_on(
                    &transaction,
                    thread,
                    &pending,
                    &mut mandatory_bytes,
                    &mut context_bytes,
                    &path,
                )?);
                pending_messages.extend(pending);
            }
        }
        if threads.is_empty() {
            return Err(Error::NoPendingMessages);
        }
        let observation = observation_on(&transaction, &binding_value.observation_id, &path)?;
        relocate_threads_on(
            &transaction,
            &repository,
            &observation.comparison,
            &mut threads,
            &path,
        )?;
        // Feasibility is checked while the reservation transaction is still
        // reversible. New messages and finding metadata are never truncated.
        prepare_projection(&mut threads, &pending_messages, &[])?;
        let ordinal = next_ordinal(
            &transaction,
            "agent_requests",
            "context_id",
            binding_value.context_id.as_str(),
            &path,
        )?;
        let id = transaction.query_row("INSERT INTO agent_requests (id, context_id, observation_id, runtime_binding_id, ordinal) VALUES (lower(hex(randomblob(16))), ?1, ?2, ?3, ?4) RETURNING id", params![binding_value.context_id.as_str(), binding_value.observation_id.as_str(), binding.as_str(), ordinal], |row| row.get::<_, String>(0)).map(AgentRequestId::from_stored).map_err(|source| database_error("create an agent request", &path, source))?;
        for (index, thread) in threads.iter().enumerate() {
            let position = u32::try_from(index + 1).map_err(|_| Error::InvalidCount {
                path: path.clone(),
                field: "agent request thread position",
                count: i64::MAX,
            })?;
            let snapshot = serde_json::to_string(&RequestThreadSnapshot {
                thread: thread.clone(),
                anchor_source_id: thread.anchor_source_id,
            })
            .map_err(Error::EncodeRequestProjection)?;
            transaction.execute("INSERT INTO agent_request_threads (request_id, thread_id, context_id, position, snapshot_json) VALUES (?1, ?2, ?3, ?4, ?5)", params![id.as_str(), thread.id.as_str(), binding_value.context_id.as_str(), position, snapshot]).map_err(|source| database_error("attach a thread to an agent request", &path, source))?;
            for message in thread
                .messages
                .iter()
                .filter(|message| pending_messages.contains(&message.id))
            {
                transaction.execute("INSERT INTO agent_request_messages (message_id, request_id) VALUES (?1, ?2)", params![message.id.as_str(), id.as_str()]).map_err(|source| database_error("reserve a reviewer message", &path, source))?;
            }
        }
        insert_dispatch(
            &transaction,
            &id,
            &binding_value.context_id,
            binding,
            1,
            &request.assignment,
            &path,
        )?;
        touch_context(&transaction, &binding_value.context_id, &path)?;
        transaction
            .commit()
            .map_err(|source| database_error("commit an agent request", &path, source))?;
        request_on(&self.connection, self.checkout_id, &id, &self.path)
    }

    pub fn agent_request(
        &self,
        binding: &RuntimeBindingId,
        request: &AgentRequestId,
    ) -> Result<AgentRequest> {
        let binding_value = self.require_local_binding(binding)?;
        require_request_context(
            &self.connection,
            &binding_value.context_id,
            request,
            &self.path,
        )?;
        request_on_any(&self.connection, request, &self.path)
    }

    pub fn retry_dispatch(
        &mut self,
        binding: &RuntimeBindingId,
        request: &AgentRequestId,
        assignment: &AgentAssignment,
    ) -> Result<DispatchAttempt> {
        assignment.validate()?;
        let binding_value = self.require_local_binding(binding)?;
        let path = self.path.clone();
        let transaction = self.transaction("retry a dispatch")?;
        require_request_context(&transaction, &binding_value.context_id, request, &path)?;
        let previous = latest_dispatch_on(&transaction, request, &path)?;
        let mut request_value = request_without_attempts_on(&transaction, request, &path)?;
        request_value.attempts.push(previous.clone());
        if request_value.answered_thread_ids.len() == request_value.thread_ids.len()
            || (request_value.recovery_for(binding).is_none()
                && previous.state != DispatchAttemptState::Dispatching)
        {
            return Err(invalid_transition(
                "agent request",
                "retry dispatch",
                request_state_from_dispatch(previous.state).as_str(),
            ));
        }
        match previous.state {
            DispatchAttemptState::Pending if previous.runtime_binding_id != *binding => {
                let detail = format!("superseded by runtime binding {binding}");
                let changed = transaction.execute("UPDATE dispatch_attempts SET state = 'superseded', detail = ?2, finished_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?1 AND state = 'pending' AND claimant IS NULL", params![previous.id.as_str(), detail]).map_err(|source| database_error("supersede a pending dispatch", &path, source))?;
                if changed != 1 {
                    return Err(invalid_transition(
                        "dispatch attempt",
                        "supersede",
                        "claimed concurrently",
                    ));
                }
            }
            DispatchAttemptState::Returned
            | DispatchAttemptState::Blocked
            | DispatchAttemptState::Rejected
            | DispatchAttemptState::Unknown
            | DispatchAttemptState::Superseded => {}
            DispatchAttemptState::Dispatching => {
                let detail = format!("claim expired before retry from runtime binding {binding}");
                let changed = transaction.execute("UPDATE dispatch_attempts SET state = 'unknown', detail = ?2, claim_expires_at = NULL, finished_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?1 AND state = 'dispatching' AND claim_expires_at <= strftime('%Y-%m-%dT%H:%M:%fZ', 'now')", params![previous.id.as_str(), detail]).map_err(|source| database_error("recover an expired dispatch before retry", &path, source))?;
                if changed != 1 {
                    return Err(invalid_transition(
                        "agent request",
                        "retry dispatch",
                        AgentRequestState::Dispatching.as_str(),
                    ));
                }
            }
            state @ DispatchAttemptState::Pending => {
                return Err(invalid_transition(
                    "agent request",
                    "retry dispatch",
                    request_state_from_dispatch(state).as_str(),
                ));
            }
        }
        let ordinal = next_ordinal(
            &transaction,
            "dispatch_attempts",
            "request_id",
            request.as_str(),
            &path,
        )?;
        let id = insert_dispatch(
            &transaction,
            request,
            &binding_value.context_id,
            binding,
            ordinal,
            assignment,
            &path,
        )?;
        transaction
            .commit()
            .map_err(|source| database_error("commit a dispatch retry", &path, source))?;
        dispatch_on(&self.connection, self.checkout_id, &id, &self.path)
    }

    pub fn claim_next_dispatch(
        &mut self,
        binding: &RuntimeBindingId,
        claimant: &str,
        lease: Duration,
    ) -> Result<Option<DispatchJob>> {
        validate_required("dispatch claimant", claimant)?;
        self.require_local_binding(binding)?;
        let lease_modifier = lease_modifier(lease)?;
        let path = self.path.clone();
        let checkout_id = self.checkout_id;
        let transaction = self.transaction("claim a dispatch")?;
        let id = transaction.query_row("SELECT id FROM dispatch_attempts WHERE runtime_binding_id = ?1 AND state = 'pending' ORDER BY created_at, id LIMIT 1", [binding.as_str()], |row| row.get::<_, String>(0)).optional().map_err(|source| database_error("find pending dispatch", &path, source))?;
        let Some(id) = id.map(DispatchAttemptId::from_stored) else {
            transaction
                .commit()
                .map_err(|source| database_error("finish empty dispatch claim", &path, source))?;
            return Ok(None);
        };
        let changed = transaction.execute("UPDATE dispatch_attempts SET state = 'dispatching', claimant = ?2, claimed_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), claim_expires_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now', ?3) WHERE id = ?1 AND state = 'pending' AND runtime_binding_id = ?4", params![id.as_str(), claimant, lease_modifier, binding.as_str()]).map_err(|source| database_error("claim a dispatch", &path, source))?;
        if changed != 1 {
            transaction
                .commit()
                .map_err(|source| database_error("finish lost dispatch claim", &path, source))?;
            return Ok(None);
        }
        let attempt = dispatch_on(&transaction, checkout_id, &id, &path)?;
        let request = request_on_any(&transaction, &attempt.request_id, &path)?;
        let observation = observation_on(&transaction, &request.observation_id, &path)?;
        let mut threads = request_threads_on(&transaction, &request.id, &path)?;
        let projection_json = prepare_projection(
            &mut threads,
            &request.message_ids,
            &request.answered_thread_ids,
        )?;
        transaction
            .commit()
            .map_err(|source| database_error("commit a dispatch claim", &path, source))?;
        Ok(Some(DispatchJob {
            comparison: observation.comparison,
            request,
            attempt,
            threads,
            projection_json,
        }))
    }

    pub fn renew_dispatch_claim(
        &mut self,
        binding: &RuntimeBindingId,
        attempt: &DispatchAttemptId,
        claimant: &str,
        lease: Duration,
    ) -> Result<DispatchAttempt> {
        self.require_local_binding(binding)?;
        validate_required("dispatch claimant", claimant)?;
        let modifier = lease_modifier(lease)?;
        let changed = self.connection.execute("UPDATE dispatch_attempts SET claim_expires_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now', ?3) WHERE id = ?1 AND claimant = ?2 AND state = 'dispatching' AND runtime_binding_id = ?4", params![attempt.as_str(), claimant, modifier, binding.as_str()]).map_err(|source| database_error("renew a dispatch claim", &self.path, source))?;
        if changed != 1 {
            return Err(claim_lost(attempt, claimant));
        }
        dispatch_on(&self.connection, self.checkout_id, attempt, &self.path)
    }

    pub fn release_dispatch(
        &mut self,
        binding: &RuntimeBindingId,
        attempt: &DispatchAttemptId,
        claimant: &str,
    ) -> Result<DispatchAttempt> {
        self.require_local_binding(binding)?;
        let path = self.path.clone();
        let transaction = self.transaction("release a dispatch")?;
        claimed_request(&transaction, binding, attempt, claimant, &path)?;
        transaction.execute("UPDATE dispatch_attempts SET state = 'pending', claimant = NULL, claimed_at = NULL, claim_expires_at = NULL WHERE id = ?1", [attempt.as_str()]).map_err(|source| database_error("release a dispatch", &path, source))?;
        transaction
            .commit()
            .map_err(|source| database_error("commit a released dispatch", &path, source))?;
        dispatch_on(&self.connection, self.checkout_id, attempt, &self.path)
    }

    pub fn finish_dispatch(
        &mut self,
        binding: &RuntimeBindingId,
        attempt: &DispatchAttemptId,
        claimant: &str,
        outcome: &DispatchOutcome,
    ) -> Result<DispatchAttempt> {
        self.require_local_binding(binding)?;
        outcome.validate()?;
        let (state, detail) = outcome.columns();
        let path = self.path.clone();
        let transaction = self.transaction("finish a dispatch")?;
        claimed_request(&transaction, binding, attempt, claimant, &path)?;
        transaction.execute("UPDATE dispatch_attempts SET state = ?3, detail = ?4, claim_expires_at = NULL, finished_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?1 AND claimant = ?2 AND state = 'dispatching'", params![attempt.as_str(), claimant, state.as_str(), detail]).map_err(|source| database_error("finish a dispatch", &path, source))?;
        transaction
            .commit()
            .map_err(|source| database_error("commit a dispatch outcome", &path, source))?;
        dispatch_on(&self.connection, self.checkout_id, attempt, &self.path)
    }

    pub fn recover_expired_dispatches(
        &mut self,
        binding: &RuntimeBindingId,
        detail: &str,
    ) -> Result<Vec<DispatchAttempt>> {
        self.require_local_binding(binding)?;
        validate_required("recovery detail", detail)?;
        let path = self.path.clone();
        let checkout_id = self.checkout_id;
        let transaction = self.transaction("recover expired dispatches")?;
        let ids = {
            let mut statement = transaction.prepare("SELECT id FROM dispatch_attempts WHERE runtime_binding_id = ?1 AND state = 'dispatching' AND claim_expires_at <= strftime('%Y-%m-%dT%H:%M:%fZ', 'now')").map_err(|source| database_error("find expired dispatches", &path, source))?;
            statement
                .query_map([binding.as_str()], |row| row.get::<_, String>(0))
                .map_err(|source| database_error("find expired dispatches", &path, source))?
                .collect::<rusqlite::Result<Vec<_>>>()
                .map_err(|source| database_error("read expired dispatches", &path, source))?
        };
        let mut recovered_ids = Vec::new();
        for id in ids {
            let id = DispatchAttemptId::from_stored(id);
            let changed = transaction.execute("UPDATE dispatch_attempts SET state = 'unknown', detail = ?2, claim_expires_at = NULL, finished_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?1 AND runtime_binding_id = ?3 AND state = 'dispatching' AND claim_expires_at <= strftime('%Y-%m-%dT%H:%M:%fZ', 'now')", params![id.as_str(), detail, binding.as_str()]).map_err(|source| database_error("recover an expired dispatch", &path, source))?;
            if changed == 1 {
                recovered_ids.push(id);
            }
        }
        transaction
            .commit()
            .map_err(|source| database_error("commit expired dispatch recovery", &path, source))?;
        recovered_ids
            .into_iter()
            .map(|id| dispatch_on(&self.connection, checkout_id, &id, &path))
            .collect()
    }

    pub fn pending_dispatches(&self, binding: &RuntimeBindingId) -> Result<Vec<DispatchAttempt>> {
        self.require_local_binding(binding)?;
        let mut statement = self.connection.prepare("SELECT id FROM dispatch_attempts WHERE runtime_binding_id = ?1 AND state = 'pending' ORDER BY created_at, id").map_err(|source| database_error("list pending dispatches", &self.path, source))?;
        let ids = statement
            .query_map([binding.as_str()], |row| row.get::<_, String>(0))
            .map_err(|source| database_error("list pending dispatches", &self.path, source))?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(|source| database_error("read pending dispatches", &self.path, source))?;
        ids.into_iter()
            .map(|id| {
                dispatch_on(
                    &self.connection,
                    self.checkout_id,
                    &DispatchAttemptId::from_stored(id),
                    &self.path,
                )
            })
            .collect()
    }

    pub fn dispatch_attempt(
        &self,
        binding: &RuntimeBindingId,
        attempt: &DispatchAttemptId,
    ) -> Result<DispatchAttempt> {
        self.require_local_binding(binding)?;
        let belongs = self
            .connection
            .query_row(
                "SELECT 1 FROM dispatch_attempts WHERE id = ?1 AND runtime_binding_id = ?2",
                params![attempt.as_str(), binding.as_str()],
                |_| Ok(()),
            )
            .optional()
            .map_err(|source| database_error("scope a dispatch attempt", &self.path, source))?;
        if belongs.is_none() {
            return Err(Error::DispatchAttemptNotFound {
                attempt: attempt.to_string(),
            });
        }
        dispatch_on(&self.connection, self.checkout_id, attempt, &self.path)
    }

    fn relocate_threads(
        &self,
        comparison: &CapturedComparison,
        threads: &mut [Thread],
    ) -> Result<()> {
        relocate_threads_on(
            &self.connection,
            &self.repository,
            comparison,
            threads,
            &self.path,
        )
    }
}

fn relocate_threads_on(
    connection: &Connection,
    repository: &Repository,
    comparison: &CapturedComparison,
    threads: &mut [Thread],
    database_path: &Path,
) -> Result<()> {
    for thread in threads {
        let (Some(anchor), Some(source_id)) = (&thread.anchor, thread.anchor_source_id) else {
            continue;
        };
        let source = anchor_source_on(connection, source_id, database_path)?;
        let current = repository.relocated_anchor_source(comparison, &anchor.original)?;
        let (path, content) = current.as_ref().map_or((None, None), |(path, content)| {
            (Some(path.as_str()), Some(content.as_str()))
        });
        thread.resolution = Some(crate::anchor::resolve(
            &anchor.original,
            &source,
            path,
            content,
        ));
    }
    Ok(())
}

fn insert_thread(
    connection: &Connection,
    context: &ReviewContextId,
    observation: &ObservationId,
    request: &NewThread,
    source: &str,
    path: &Path,
) -> Result<ThreadId> {
    let digest = Sha256::digest(source.as_bytes());
    let context_json = request
        .anchor
        .context
        .as_ref()
        .map(serde_json::to_string)
        .transpose()
        .map_err(Error::EncodeAnchorContext)?;
    let anchor_source_id = save_anchor_source(connection, digest.as_ref(), source, path)?;
    let thread = connection.query_row("INSERT INTO threads (id, context_id, observation_id, path, side, start_line, end_line, anchor_source_id, context_json, status) VALUES (lower(hex(randomblob(16))), ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'open') RETURNING id", params![context.as_str(), observation.as_str(), request.anchor.original.path, request.anchor.original.side.as_str(), request.anchor.original.start_line, request.anchor.original.end_line, anchor_source_id, context_json], |row| row.get::<_, String>(0)).map(ThreadId::from_stored).map_err(|source| database_error("create a review thread", path, source))?;
    insert_message(
        connection,
        &thread,
        &NewMessage::new(request.body.clone(), request.author.clone())?,
        None,
        path,
    )?;
    Ok(thread)
}

fn insert_message(
    connection: &Connection,
    thread: &ThreadId,
    request: &NewMessage,
    reply_to_request: Option<&AgentRequestId>,
    path: &Path,
) -> Result<MessageId> {
    let position = next_position(connection, thread, path)?;
    let origin = if reply_to_request.is_some() {
        "agent"
    } else {
        "reviewer"
    };
    connection.query_row("INSERT INTO messages (id, thread_id, position, author, body, origin, reply_to_request) VALUES (lower(hex(randomblob(16))), ?1, ?2, ?3, ?4, ?5, ?6) RETURNING id", params![thread.as_str(), position, request.author, request.body, origin, reply_to_request.map(AgentRequestId::as_str)], |row| row.get::<_, String>(0)).map(MessageId::from_stored).map_err(|source| database_error("add a review message", path, source))
}

fn next_position(connection: &Connection, thread: &ThreadId, path: &Path) -> Result<u32> {
    let value = connection
        .query_row(
            "SELECT coalesce(max(position), 0) + 1 FROM messages WHERE thread_id = ?1",
            [thread.as_str()],
            |row| row.get::<_, i64>(0),
        )
        .map_err(|source| database_error("allocate a message position", path, source))?;
    u32::try_from(value).map_err(|_| Error::InvalidCount {
        path: path.to_path_buf(),
        field: "message position",
        count: value,
    })
}

fn insert_dispatch(
    connection: &Connection,
    request: &AgentRequestId,
    context: &ReviewContextId,
    binding: &RuntimeBindingId,
    ordinal: u32,
    assignment: &AgentAssignment,
    path: &Path,
) -> Result<DispatchAttemptId> {
    connection.query_row("INSERT INTO dispatch_attempts (id, request_id, context_id, runtime_binding_id, ordinal, herdr_server_id, workspace_id, pane_id, expected_agent_name, expected_agent_kind, state) VALUES (lower(hex(randomblob(16))), ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 'pending') RETURNING id", params![request.as_str(), context.as_str(), binding.as_str(), ordinal, assignment.herdr_server_id, assignment.workspace_id.as_str(), assignment.pane_id.as_str(), assignment.expected_agent_name, assignment.expected_agent_kind], |row| row.get::<_, String>(0)).map(DispatchAttemptId::from_stored).map_err(|source| database_error("create a dispatch attempt", path, source))
}

fn validate_anchor_source(anchor: &ReviewAnchor, source: &str) -> Result<()> {
    let Some(context) = &anchor.context else {
        return Ok(());
    };
    let lines = if source.is_empty() {
        vec![""]
    } else {
        source.lines().collect::<Vec<_>>()
    };
    let selected = usize::try_from(anchor.original.start_line)
        .ok()
        .and_then(|line| line.checked_sub(1))
        .zip(usize::try_from(anchor.original.end_line).ok())
        .and_then(|(start, end)| lines.get(start..end));
    if selected.is_some_and(|selected| {
        selected
            .iter()
            .copied()
            .eq(context.selected.iter().map(String::as_str))
    }) {
        Ok(())
    } else {
        Err(Error::AnchorSourceMismatch {
            path: anchor.original.path.clone(),
            start_line: anchor.original.start_line,
            end_line: anchor.original.end_line,
        })
    }
}

fn context_on(connection: &Connection, id: &ReviewContextId, path: &Path) -> Result<ReviewContext> {
    let (title, created_at, updated_at) = connection
        .query_row(
            "SELECT title, created_at, updated_at FROM review_contexts WHERE id = ?1",
            [id.as_str()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()
        .map_err(|source| database_error("read a review context", path, source))?
        .ok_or_else(|| Error::ReviewContextNotFound {
            context: id.to_string(),
        })?;
    let mut statement = connection.prepare("SELECT kind, locator FROM external_references WHERE context_id = ?1 ORDER BY kind, locator").map_err(|source| database_error("read external references", path, source))?;
    let references = statement
        .query_map([id.as_str()], |row| {
            Ok(ExternalReference {
                kind: row.get(0)?,
                locator: row.get(1)?,
            })
        })
        .map_err(|source| database_error("read external references", path, source))?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|source| database_error("decode external references", path, source))?;
    Ok(ReviewContext {
        id: id.clone(),
        title,
        references,
        created_at,
        updated_at,
    })
}

fn observation_on(connection: &Connection, id: &ObservationId, path: &Path) -> Result<Observation> {
    connection.query_row("SELECT context_id, ordinal, base_source_kind, base_source_oid, base_oid, target_source_kind, target_source_oid, target_oid, created_at FROM observations WHERE id = ?1", [id.as_str()], |row| {
        let base_source = endpoint_source(row, 2, 3)?;
        let target_source = endpoint_source(row, 5, 6)?;
        Ok(Observation { id: id.clone(), context_id: ReviewContextId::from_stored(row.get(0)?), ordinal: row.get(1)?, comparison: CapturedComparison { base: CapturedEndpoint { source: base_source, oid: row.get(4)? }, target: CapturedEndpoint { source: target_source, oid: row.get(7)? } }, created_at: row.get(8)? })
    }).optional().map_err(|source| database_error("read an observation", path, source))?.ok_or_else(|| Error::ObservationNotFound { observation: id.to_string() })
}

fn endpoint_source(
    row: &Row<'_>,
    kind_column: usize,
    oid: usize,
) -> rusqlite::Result<DiffEndpoint> {
    let kind = row.get::<_, String>(kind_column)?;
    let oid = row.get::<_, Option<String>>(oid)?;
    DiffEndpoint::from_columns(&kind, oid).ok_or_else(|| {
        rusqlite::Error::InvalidColumnType(
            kind_column,
            "endpoint source".to_owned(),
            rusqlite::types::Type::Text,
        )
    })
}

fn binding_on(
    connection: &Connection,
    id: &RuntimeBindingId,
    path: &Path,
) -> Result<RuntimeBinding> {
    connection.query_row("SELECT b.context_id, b.observation_id, c.checkout_token, c.checkout_root, b.checkout_kind, b.herdr_server_id, b.workspace_id, b.pane_id, b.created_at, b.updated_at FROM runtime_bindings b JOIN checkouts c ON c.id = b.checkout_id WHERE b.id = ?1", [id.as_str()], |row| {
        let raw_kind = row.get::<_, String>(4)?;
        let checkout_kind = CheckoutKind::from_str(&raw_kind).ok_or_else(|| rusqlite::Error::InvalidColumnType(4, "checkout_kind".to_owned(), rusqlite::types::Type::Text))?;
        Ok(RuntimeBinding { id: id.clone(), context_id: ReviewContextId::from_stored(row.get(0)?), observation_id: ObservationId::from_stored(row.get(1)?), checkout_token: row.get(2)?, checkout_root: row.get(3)?, checkout_kind, herdr_server_id: row.get(5)?, workspace_id: WorkspaceId::new(row.get::<_, String>(6)?), pane_id: row.get::<_, Option<String>>(7)?.map(PaneId::new), created_at: row.get(8)?, updated_at: row.get(9)? })
    }).optional().map_err(|source| database_error("read a runtime binding", path, source))?.ok_or_else(|| Error::RuntimeBindingNotFound { binding: id.to_string() })
}

fn thread_header_on(connection: &Connection, id: &ThreadId, path: &Path) -> Result<Thread> {
    connection.query_row("SELECT context_id, observation_id, path, side, start_line, end_line, anchor_source_id, context_json, status, created_at, updated_at, finding_json FROM threads WHERE id = ?1", [id.as_str()], |row| raw_thread(row, id.clone())).optional().map_err(|source| database_error("read a review thread", path, source))?.ok_or_else(|| Error::ThreadNotFound { thread: id.to_string() })
}

fn request_thread_on(
    connection: &Connection,
    id: &ThreadId,
    pending: &[MessageId],
    mandatory_bytes: &mut usize,
    context_bytes: &mut usize,
    path: &Path,
) -> Result<Thread> {
    let mut thread = thread_header_on(connection, id, path)?;
    *mandatory_bytes += serde_json::to_vec(&thread)
        .map_err(Error::EncodeRequestProjection)?
        .len();
    if *mandatory_bytes > MAX_REQUEST_PROJECTION_BYTES {
        return Err(Error::RequestTooLarge);
    }
    let context_limit = if *context_bytes < 64 * 1024 { 32 } else { 0 };
    let mut statement = connection.prepare("SELECT id FROM messages WHERE thread_id = ?1 AND (position IN (SELECT position FROM messages WHERE thread_id = ?1 ORDER BY position DESC LIMIT ?2) OR (origin = 'reviewer' AND NOT EXISTS (SELECT 1 FROM agent_request_messages WHERE message_id = messages.id))) ORDER BY position")
        .map_err(|source| database_error("prepare bounded request history", path, source))?;
    let ids = statement
        .query_map(params![id.as_str(), context_limit], |row| {
            row.get::<_, String>(0).map(MessageId::from_stored)
        })
        .map_err(|source| database_error("list bounded request history", path, source))?;
    for id in ids {
        let id =
            id.map_err(|source| database_error("read bounded request history", path, source))?;
        let message = message_on(connection, &id, path)?;
        let size = serde_json::to_vec(&message)
            .map_err(Error::EncodeRequestProjection)?
            .len();
        if pending.contains(&id) {
            *mandatory_bytes += size;
            if *mandatory_bytes > MAX_REQUEST_PROJECTION_BYTES {
                return Err(Error::RequestTooLarge);
            }
        } else {
            *context_bytes += size;
            if *context_bytes > 64 * 1024 {
                continue;
            }
        }
        thread.messages.push(message);
    }
    Ok(thread)
}

fn raw_thread(row: &Row<'_>, id: ThreadId) -> rusqlite::Result<Thread> {
    let side = row.get::<_, Option<String>>(3)?;
    let side = side
        .map(|side| {
            DiffSide::from_str(&side).ok_or_else(|| {
                rusqlite::Error::InvalidColumnType(
                    3,
                    "side".to_owned(),
                    rusqlite::types::Type::Text,
                )
            })
        })
        .transpose()?;
    let raw_context = row.get::<_, Option<String>>(7)?;
    let context = raw_context
        .map(|value| {
            serde_json::from_str::<AnchorContext>(&value).map_err(|error| {
                rusqlite::Error::FromSqlConversionFailure(
                    value.len(),
                    rusqlite::types::Type::Text,
                    Box::new(error),
                )
            })
        })
        .transpose()?;
    let raw_status = row.get::<_, String>(8)?;
    let status = ThreadStatus::from_str(&raw_status).ok_or_else(|| {
        rusqlite::Error::InvalidColumnType(8, "status".to_owned(), rusqlite::types::Type::Text)
    })?;
    Ok(Thread {
        id,
        context_id: ReviewContextId::from_stored(row.get(0)?),
        observation_id: ObservationId::from_stored(row.get(1)?),
        anchor: side
            .map(|side| -> rusqlite::Result<ReviewAnchor> {
                Ok(ReviewAnchor {
                    original: AnchorLocation {
                        path: row.get(2)?,
                        side,
                        start_line: row.get(4)?,
                        end_line: row.get(5)?,
                    },
                    context,
                })
            })
            .transpose()?,
        finding: row
            .get::<_, Option<String>>(11)?
            .map(|value| {
                serde_json::from_str::<Finding>(&value).map_err(|error| {
                    rusqlite::Error::FromSqlConversionFailure(
                        11,
                        rusqlite::types::Type::Text,
                        Box::new(error),
                    )
                })
            })
            .transpose()?,
        status,
        messages: Vec::new(),
        resolution: None,
        anchor_source_id: row.get(6)?,
        created_at: row.get(9)?,
        updated_at: row.get(10)?,
    })
}

fn message_on(connection: &Connection, id: &MessageId, path: &Path) -> Result<Message> {
    connection
        .query_row(
            "SELECT thread_id, author, body, created_at, origin = 'agent', reply_to_request FROM messages WHERE id = ?1",
            [id.as_str()],
            |row| {
                Ok(Message {
                    id: id.clone(),
                    thread_id: ThreadId::from_stored(row.get(0)?),
                    author: row.get(1)?,
                    body: row.get(2)?,
                    created_at: row.get(3)?,
                    origin: if row.get::<_, bool>(4)? { MessageOrigin::Agent } else { MessageOrigin::Reviewer },
                    reply_to_request: row.get::<_, Option<String>>(5)?.map(AgentRequestId::from_stored),
                })
            },
        )
        .optional()
        .map_err(|source| database_error("read a review message", path, source))?
        .ok_or_else(|| Error::MessageNotFound {
            message: id.to_string(),
        })
}

fn request_on(
    connection: &Connection,
    checkout_id: i64,
    id: &AgentRequestId,
    path: &Path,
) -> Result<AgentRequest> {
    let exists = connection.query_row("SELECT 1 FROM agent_requests r JOIN runtime_bindings b ON b.id = r.runtime_binding_id WHERE r.id = ?1 AND b.checkout_id = ?2", params![id.as_str(), checkout_id], |_| Ok(())).optional().map_err(|source| database_error("scope an agent request", path, source))?;
    if exists.is_none() {
        return Err(Error::AgentRequestNotFound {
            request: id.to_string(),
        });
    }
    request_on_any(connection, id, path)
}

fn request_on_any(
    connection: &Connection,
    id: &AgentRequestId,
    path: &Path,
) -> Result<AgentRequest> {
    let mut request = request_without_attempts_on(connection, id, path)?;
    request.attempts = dispatches_on(connection, id, path)?;
    set_request_outcome(&mut request, path)?;
    Ok(request)
}

fn request_without_attempts_on(
    connection: &Connection,
    id: &AgentRequestId,
    path: &Path,
) -> Result<AgentRequest> {
    let mut request = connection.query_row("SELECT context_id, observation_id, runtime_binding_id, ordinal, created_at FROM agent_requests WHERE id = ?1", [id.as_str()], |row| {
        Ok(AgentRequest { id: id.clone(), context_id: ReviewContextId::from_stored(row.get(0)?), observation_id: ObservationId::from_stored(row.get(1)?), runtime_binding_id: RuntimeBindingId::from_stored(row.get(2)?), ordinal: row.get(3)?, state: AgentRequestState::AwaitingDispatch, thread_ids: Vec::new(), message_ids: Vec::new(), answered_thread_ids: Vec::new(), recovery: None, attempts: Vec::new(), created_at: row.get(4)?, returned_at: None })
    }).optional().map_err(|source| database_error("read an agent request", path, source))?.ok_or_else(|| Error::AgentRequestNotFound { request: id.to_string() })?;
    request.thread_ids = request_thread_ids_on(connection, id, path)?;
    let mut answers = connection
        .prepare("SELECT thread_id FROM messages WHERE reply_to_request = ?1 ORDER BY thread_id")
        .map_err(|source| database_error("list answered request threads", path, source))?;
    request.answered_thread_ids = answers
        .query_map([id.as_str()], |row| {
            row.get::<_, String>(0).map(ThreadId::from_stored)
        })
        .map_err(|source| database_error("list answered request threads", path, source))?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|source| database_error("read answered request threads", path, source))?;
    let mut statement = connection.prepare("SELECT message_id FROM agent_request_messages WHERE request_id = ?1 ORDER BY message_id")
        .map_err(|source| database_error("list requested messages", path, source))?;
    request.message_ids = statement
        .query_map([id.as_str()], |row| {
            row.get::<_, String>(0).map(MessageId::from_stored)
        })
        .map_err(|source| database_error("list requested messages", path, source))?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|source| database_error("read requested messages", path, source))?;
    Ok(request)
}

fn set_request_outcome(request: &mut AgentRequest, path: &Path) -> Result<()> {
    let latest = request
        .attempts
        .last()
        .ok_or_else(|| invalid_stored(path, "agent request dispatch attempts", "none"))?;
    request.state = request_state_from_dispatch(latest.state);
    request.returned_at = (latest.state == DispatchAttemptState::Returned)
        .then(|| latest.finished_at.clone())
        .flatten();
    request.recovery = request.recovery_for(&request.runtime_binding_id);
    Ok(())
}

fn request_thread_ids_on(
    connection: &Connection,
    request: &AgentRequestId,
    path: &Path,
) -> Result<Vec<ThreadId>> {
    let mut statement = connection
        .prepare(
            "SELECT thread_id FROM agent_request_threads WHERE request_id = ?1 ORDER BY position",
        )
        .map_err(|source| database_error("list agent request threads", path, source))?;
    statement
        .query_map([request.as_str()], |row| {
            row.get::<_, String>(0).map(ThreadId::from_stored)
        })
        .map_err(|source| database_error("list agent request threads", path, source))?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|source| database_error("read agent request threads", path, source))
}

fn request_threads_on(
    connection: &Connection,
    request: &AgentRequestId,
    path: &Path,
) -> Result<Vec<Thread>> {
    let mut statement = connection.prepare("SELECT snapshot_json FROM agent_request_threads WHERE request_id = ?1 ORDER BY position")
        .map_err(|source| database_error("read request snapshots", path, source))?;
    let snapshots = statement
        .query_map([request.as_str()], |row| row.get::<_, String>(0))
        .map_err(|source| database_error("read request snapshots", path, source))?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|source| database_error("read request snapshots", path, source))?;
    snapshots
        .into_iter()
        .map(|snapshot| {
            let RequestThreadSnapshot {
                mut thread,
                anchor_source_id,
            } = serde_json::from_str(&snapshot).map_err(Error::DecodeRequestSnapshot)?;
            thread.anchor_source_id = anchor_source_id;
            let reply = connection
                .query_row(
                    "SELECT id FROM messages WHERE thread_id = ?1 AND reply_to_request = ?2",
                    params![thread.id.as_str(), request.as_str()],
                    |row| row.get::<_, String>(0),
                )
                .optional()
                .map_err(|source| database_error("find saved request answer", path, source))?;
            if let Some(reply) = reply {
                thread.messages.push(message_on(
                    connection,
                    &MessageId::from_stored(reply),
                    path,
                )?);
            }
            Ok(thread)
        })
        .collect()
}

/// Trim only old context, never the requested messages or structured findings.
/// Retry projections carry saved replies where budget permits and always carry
/// the authoritative answered-thread identities even when old text is omitted.
fn prepare_projection(
    threads: &mut [Thread],
    new_message_ids: &[MessageId],
    answered_thread_ids: &[ThreadId],
) -> Result<String> {
    let encode = |threads: &[Thread]| {
        serde_json::to_string(&RequestProjection {
            new_message_ids,
            answered_thread_ids,
            history_policy: "Older context may be omitted. New messages and finding metadata are complete. Answered thread IDs are authoritative; do not answer them again.",
            threads,
        }).map_err(Error::EncodeRequestProjection)
    };
    let json = encode(threads)?;
    // Leave room for reply membership added by subsequent attempts.
    let remaining_answers = threads.len().saturating_sub(answered_thread_ids.len());
    let budget = MAX_REQUEST_PROJECTION_BYTES.saturating_sub(remaining_answers * 40);
    if json.len() <= budget {
        return Ok(json);
    }
    let mut estimated_bytes = json.len();
    for thread in threads.iter_mut() {
        let mut retained = Vec::new();
        for message in std::mem::take(&mut thread.messages) {
            if estimated_bytes > budget && !new_message_ids.contains(&message.id) {
                // Ignore the removed comma: conservative by one byte, without
                // repeatedly encoding the entire transcript.
                estimated_bytes = estimated_bytes.saturating_sub(
                    serde_json::to_vec(&message)
                        .map_err(Error::EncodeRequestProjection)?
                        .len(),
                );
            } else {
                retained.push(message);
            }
        }
        thread.messages = retained;
    }
    let json = encode(threads)?;
    if json.len() > budget {
        return Err(Error::RequestTooLarge);
    }
    Ok(json)
}

fn unreserved_reviewer_messages_on(
    connection: &Connection,
    thread: &ThreadId,
    path: &Path,
) -> Result<Vec<MessageId>> {
    let mut statement = connection.prepare("SELECT id FROM messages WHERE thread_id = ?1 AND origin = 'reviewer' AND NOT EXISTS (SELECT 1 FROM agent_request_messages WHERE message_id = messages.id) ORDER BY position LIMIT 1025")
        .map_err(|source| database_error("find new reviewer messages", path, source))?;
    let messages = statement
        .query_map([thread.as_str()], |row| {
            row.get::<_, String>(0).map(MessageId::from_stored)
        })
        .map_err(|source| database_error("find new reviewer messages", path, source))?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|source| database_error("read new reviewer messages", path, source))?;
    if messages.len() > 1024 {
        return Err(Error::RequestTooLarge);
    }
    Ok(messages)
}

fn dispatches_on(
    connection: &Connection,
    request: &AgentRequestId,
    path: &Path,
) -> Result<Vec<DispatchAttempt>> {
    let mut statement = connection
        .prepare("SELECT id FROM dispatch_attempts WHERE request_id = ?1 ORDER BY ordinal")
        .map_err(|source| database_error("list dispatch attempts", path, source))?;
    let ids = statement
        .query_map([request.as_str()], |row| row.get::<_, String>(0))
        .map_err(|source| database_error("list dispatch attempts", path, source))?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|source| database_error("read dispatch attempts", path, source))?;
    ids.into_iter()
        .map(|id| dispatch_on_any(connection, &DispatchAttemptId::from_stored(id), path))
        .collect()
}

fn dispatch_on_any(
    connection: &Connection,
    id: &DispatchAttemptId,
    path: &Path,
) -> Result<DispatchAttempt> {
    connection.query_row("SELECT request_id, runtime_binding_id, ordinal, herdr_server_id, workspace_id, pane_id, expected_agent_name, expected_agent_kind, state, claimant, detail, created_at, claimed_at, claim_expires_at, finished_at FROM dispatch_attempts WHERE id = ?1", [id.as_str()], |row| {
        let raw = row.get::<_, String>(8)?;
        let state = DispatchAttemptState::from_str(&raw).ok_or_else(|| rusqlite::Error::InvalidColumnType(8, "state".to_owned(), rusqlite::types::Type::Text))?;
        Ok(DispatchAttempt { id: id.clone(), request_id: AgentRequestId::from_stored(row.get(0)?), runtime_binding_id: RuntimeBindingId::from_stored(row.get(1)?), ordinal: row.get(2)?, assignment: AgentAssignment { herdr_server_id: row.get(3)?, workspace_id: WorkspaceId::new(row.get::<_, String>(4)?), pane_id: PaneId::new(row.get::<_, String>(5)?), expected_agent_name: row.get(6)?, expected_agent_kind: row.get(7)? }, state, claimant: row.get(9)?, detail: row.get(10)?, created_at: row.get(11)?, claimed_at: row.get(12)?, claim_expires_at: row.get(13)?, finished_at: row.get(14)? })
    }).optional().map_err(|source| database_error("read a dispatch attempt", path, source))?.ok_or_else(|| Error::DispatchAttemptNotFound { attempt: id.to_string() })
}

fn dispatch_on(
    connection: &Connection,
    checkout_id: i64,
    id: &DispatchAttemptId,
    path: &Path,
) -> Result<DispatchAttempt> {
    let exists = connection.query_row("SELECT 1 FROM dispatch_attempts a JOIN runtime_bindings b ON b.id = a.runtime_binding_id WHERE a.id = ?1 AND b.checkout_id = ?2", params![id.as_str(), checkout_id], |_| Ok(())).optional()
        .map_err(|source| database_error("scope a dispatch attempt", path, source))?;
    if exists.is_none() {
        return Err(Error::DispatchAttemptNotFound {
            attempt: id.to_string(),
        });
    }
    dispatch_on_any(connection, id, path)
}

fn require_context(connection: &Connection, id: &ReviewContextId, path: &Path) -> Result<()> {
    if connection
        .query_row(
            "SELECT 1 FROM review_contexts WHERE id = ?1",
            [id.as_str()],
            |_| Ok(()),
        )
        .optional()
        .map_err(|source| database_error("find a review context", path, source))?
        .is_some()
    {
        Ok(())
    } else {
        Err(Error::ReviewContextNotFound {
            context: id.to_string(),
        })
    }
}

fn require_observation_context(
    connection: &Connection,
    context: &ReviewContextId,
    observation: &ObservationId,
    path: &Path,
) -> Result<()> {
    if connection
        .query_row(
            "SELECT 1 FROM observations WHERE id = ?1 AND context_id = ?2",
            params![observation.as_str(), context.as_str()],
            |_| Ok(()),
        )
        .optional()
        .map_err(|source| database_error("find a review observation", path, source))?
        .is_some()
    {
        Ok(())
    } else {
        Err(Error::ObservationNotInContext {
            observation: observation.to_string(),
            context: context.to_string(),
        })
    }
}

fn require_thread_scope(
    connection: &Connection,
    binding: &RuntimeBinding,
    thread: &ThreadId,
    path: &Path,
) -> Result<()> {
    if connection
        .query_row(
            "SELECT 1 FROM threads WHERE id = ?1 AND context_id = ?2",
            params![thread.as_str(), binding.context_id.as_str()],
            |_| Ok(()),
        )
        .optional()
        .map_err(|source| database_error("find a review thread", path, source))?
        .is_some()
    {
        Ok(())
    } else {
        Err(Error::ThreadNotFound {
            thread: thread.to_string(),
        })
    }
}

fn thread_status_on(
    connection: &Connection,
    thread: &ThreadId,
    path: &Path,
) -> Result<ThreadStatus> {
    let raw = connection
        .query_row(
            "SELECT status FROM threads WHERE id = ?1",
            [thread.as_str()],
            |row| row.get::<_, String>(0),
        )
        .map_err(|source| database_error("read review thread status", path, source))?;
    ThreadStatus::from_str(&raw).ok_or_else(|| invalid_stored(path, "thread status", &raw))
}

fn require_request_context(
    connection: &Connection,
    context: &ReviewContextId,
    request: &AgentRequestId,
    path: &Path,
) -> Result<()> {
    let exists = connection
        .query_row(
            "SELECT 1 FROM agent_requests WHERE id = ?1 AND context_id = ?2",
            params![request.as_str(), context.as_str()],
            |_| Ok(()),
        )
        .optional()
        .map_err(|source| database_error("scope an agent request", path, source))?;
    if exists.is_some() {
        Ok(())
    } else {
        Err(Error::AgentRequestNotFound {
            request: request.to_string(),
        })
    }
}

fn latest_dispatch_on(
    connection: &Connection,
    request: &AgentRequestId,
    path: &Path,
) -> Result<DispatchAttempt> {
    let id = connection
        .query_row(
            "SELECT id FROM dispatch_attempts WHERE request_id = ?1 ORDER BY ordinal DESC LIMIT 1",
            [request.as_str()],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(|source| database_error("read latest dispatch attempt", path, source))?
        .ok_or_else(|| invalid_stored(path, "agent request dispatch attempts", "none"))?;
    dispatch_on_any(connection, &DispatchAttemptId::from_stored(id), path)
}

fn request_state_from_dispatch(state: DispatchAttemptState) -> AgentRequestState {
    match state {
        DispatchAttemptState::Pending => AgentRequestState::AwaitingDispatch,
        DispatchAttemptState::Dispatching => AgentRequestState::Dispatching,
        DispatchAttemptState::Returned => AgentRequestState::Returned,
        DispatchAttemptState::Blocked
        | DispatchAttemptState::Rejected
        | DispatchAttemptState::Unknown
        | DispatchAttemptState::Superseded => AgentRequestState::AwaitingRetry,
    }
}

fn claimed_request(
    connection: &Connection,
    binding: &RuntimeBindingId,
    attempt: &DispatchAttemptId,
    claimant: &str,
    path: &Path,
) -> Result<AgentRequestId> {
    connection.query_row("SELECT request_id FROM dispatch_attempts WHERE id = ?1 AND claimant = ?2 AND state = 'dispatching' AND runtime_binding_id = ?3", params![attempt.as_str(), claimant, binding.as_str()], |row| row.get::<_, String>(0)).optional().map_err(|source| database_error("validate a dispatch claim", path, source))?.map(AgentRequestId::from_stored).ok_or_else(|| claim_lost(attempt, claimant))
}

fn anchor_source_on(connection: &Connection, id: i64, path: &Path) -> Result<String> {
    connection
        .query_row(
            "SELECT content FROM anchor_sources WHERE id = ?1",
            [id],
            |row| row.get(0),
        )
        .map_err(|source| database_error("read anchor source", path, source))
}

fn save_anchor_source(
    connection: &Connection,
    digest: &[u8],
    content: &str,
    path: &Path,
) -> Result<i64> {
    connection.execute("INSERT INTO anchor_sources (digest, content) VALUES (?1, ?2) ON CONFLICT(digest) DO NOTHING", params![digest, content]).map_err(|source| database_error("save anchor source", path, source))?;
    connection
        .query_row(
            "SELECT id FROM anchor_sources WHERE digest = ?1",
            [digest],
            |row| row.get(0),
        )
        .map_err(|source| database_error("read saved anchor source", path, source))
}

fn touch_context(connection: &Connection, context: &ReviewContextId, path: &Path) -> Result<()> {
    connection.execute("UPDATE review_contexts SET revision = revision + 1, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?1", [context.as_str()]).map(|_| ()).map_err(|source| database_error("touch a review context", path, source))
}

fn lease_modifier(lease: Duration) -> Result<String> {
    if lease.is_zero() || lease > MAX_LEASE {
        return Err(Error::InvalidDispatchLease);
    }
    Ok(format!("+{:.3} seconds", lease.as_secs_f64()))
}

fn claim_lost(attempt: &DispatchAttemptId, claimant: &str) -> Error {
    Error::DispatchClaimLost {
        attempt: attempt.to_string(),
        claimant: claimant.to_owned(),
    }
}
fn invalid_transition(entity: &'static str, action: &'static str, state: &str) -> Error {
    Error::InvalidTransition {
        entity,
        action,
        state: state.to_owned(),
    }
}
fn invalid_stored(path: &Path, field: &'static str, value: &str) -> Error {
    Error::InvalidStoredValue {
        path: path.to_path_buf(),
        field,
        value: value.to_owned(),
    }
}
fn database_error(operation: &'static str, path: &Path, source: rusqlite::Error) -> Error {
    Error::DatabaseOperation {
        operation,
        path: path.to_path_buf(),
        source,
    }
}

fn configure(connection: &Connection, path: &Path) -> Result<()> {
    connection
        .busy_timeout(CONFIGURE_BUSY_TIMEOUT)
        .and_then(|()| connection.pragma_update(None, "foreign_keys", true))
        .map_err(|source| Error::ConfigureDatabase {
            path: path.to_path_buf(),
            source,
        })?;
    retry_sqlite_contention(|| connection.pragma_update(None, "journal_mode", "WAL"))
        .and_then(|()| connection.busy_timeout(BUSY_TIMEOUT))
        .map_err(|source| Error::ConfigureDatabase {
            path: path.to_path_buf(),
            source,
        })
}

fn retry_sqlite_contention(
    mut operation: impl FnMut() -> rusqlite::Result<()>,
) -> rusqlite::Result<()> {
    let deadline = Instant::now() + BUSY_TIMEOUT;
    let mut delay = Duration::from_millis(5);
    loop {
        match operation() {
            Ok(()) => return Ok(()),
            Err(error) if matches!(&error, rusqlite::Error::SqliteFailure(failure, _) if matches!(failure.code, SqliteErrorCode::DatabaseBusy | SqliteErrorCode::DatabaseLocked)) =>
            {
                let remaining = deadline.saturating_duration_since(Instant::now());
                if remaining.is_zero() {
                    return Err(error);
                }
                std::thread::sleep(delay.min(remaining));
                delay = delay.saturating_mul(2).min(Duration::from_millis(100));
            }
            Err(error) => return Err(error),
        }
    }
}

fn initialize_schema(connection: &mut Connection, path: &Path) -> Result<()> {
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|source| Error::InitializeDatabase {
            path: path.to_path_buf(),
            source,
        })?;
    let tables = transaction
        .query_row(
            "SELECT count(*) FROM sqlite_schema WHERE type = 'table' AND name NOT LIKE 'sqlite_%'",
            [],
            |row| row.get::<_, u32>(0),
        )
        .map_err(|source| Error::InitializeDatabase {
            path: path.to_path_buf(),
            source,
        })?;
    if tables == 0 {
        return transaction
            .execute_batch(include_str!("schema.sql"))
            .and_then(|()| transaction.commit())
            .map_err(|source| Error::InitializeDatabase {
                path: path.to_path_buf(),
                source,
            });
    }
    validate_schema(&transaction, path)?;
    transaction
        .commit()
        .map_err(|source| Error::InitializeDatabase {
            path: path.to_path_buf(),
            source,
        })
}

fn reconcile_observation_refs(
    connection: &mut Connection,
    repository: &Repository,
    path: &Path,
) -> Result<()> {
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|source| database_error("begin observation ref reconciliation", path, source))?;
    for observation in repository.retained_observations()? {
        let exists = transaction
            .query_row(
                "SELECT 1 FROM observations WHERE id = ?1",
                [observation.as_str()],
                |_| Ok(()),
            )
            .optional()
            .map_err(|source| database_error("check a retained observation", path, source))?;
        if exists.is_none() {
            repository.delete_observation_refs(&observation)?;
        }
    }
    transaction
        .commit()
        .map_err(|source| database_error("commit observation ref reconciliation", path, source))
}

fn validate_schema(connection: &Connection, path: &Path) -> Result<()> {
    let expected = Connection::open_in_memory()
        .and_then(|expected| {
            expected.execute_batch(include_str!("schema.sql"))?;
            schema_signature(&expected)
        })
        .map_err(|source| Error::InitializeDatabase {
            path: path.to_path_buf(),
            source,
        })?;
    let actual = schema_signature(connection).map_err(|source| Error::InitializeDatabase {
        path: path.to_path_buf(),
        source,
    })?;
    if actual != expected {
        return Err(Error::IncompatibleDatabase {
            path: path.to_path_buf(),
            reason: "stored schema differs from the schema expected by this build".to_owned(),
        });
    }
    let integrity = connection
        .query_row("PRAGMA quick_check", [], |row| row.get::<_, String>(0))
        .map_err(|source| Error::IncompatibleDatabase {
            path: path.to_path_buf(),
            reason: source.to_string(),
        })?;
    if integrity != "ok" {
        return Err(Error::IncompatibleDatabase {
            path: path.to_path_buf(),
            reason: integrity,
        });
    }
    let violation = connection
        .query_row("SELECT 1 FROM pragma_foreign_key_check LIMIT 1", [], |_| {
            Ok(())
        })
        .optional()
        .map_err(|source| Error::IncompatibleDatabase {
            path: path.to_path_buf(),
            reason: source.to_string(),
        })?;
    if violation.is_some() {
        return Err(Error::IncompatibleDatabase {
            path: path.to_path_buf(),
            reason: "foreign key check failed".to_owned(),
        });
    }
    Ok(())
}

fn schema_signature(connection: &Connection) -> rusqlite::Result<String> {
    connection.query_row(
        "SELECT group_concat(type || ':' || name || ':' || sql, char(10)) FROM (SELECT type, name, sql FROM sqlite_schema WHERE type IN ('table', 'index', 'trigger') AND sql IS NOT NULL AND name NOT LIKE 'sqlite_%' ORDER BY type, name)",
        [],
        |row| row.get(0),
    )
}

fn register_checkout(
    connection: &Connection,
    repository: &Repository,
    database: &Path,
) -> Result<(i64, String)> {
    let checkout_root = utf8(repository.checkout_root())?;
    let worktree_git_dir = utf8(repository.worktree_git_dir())?;
    let checkout_token = checkout_token(connection, repository)?;
    let id = connection.query_row("INSERT INTO checkouts (checkout_token, worktree_git_dir, checkout_root, is_linked_worktree) VALUES (?1, ?2, ?3, ?4) ON CONFLICT(checkout_token) DO UPDATE SET checkout_root = excluded.checkout_root, worktree_git_dir = excluded.worktree_git_dir, is_linked_worktree = excluded.is_linked_worktree, last_seen_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') RETURNING id", params![checkout_token, worktree_git_dir, checkout_root, repository.is_linked_worktree()], |row| row.get(0)).map_err(|source| Error::RegisterCheckout { checkout: repository.checkout_root().to_path_buf(), database: database.to_path_buf(), source })?;
    Ok((id, checkout_token))
}

fn checkout_token(connection: &Connection, repository: &Repository) -> Result<String> {
    let path = repository.checkout_token_path();
    match fs::read_to_string(path) {
        Ok(value) => validate_checkout_token(path, value),
        Err(source) if source.kind() == ErrorKind::NotFound => {
            let token = connection
                .query_row("SELECT lower(hex(randomblob(16)))", [], |row| {
                    row.get::<_, String>(0)
                })
                .map_err(|source| {
                    database_error(
                        "generate a checkout identity",
                        repository.database_path(),
                        source,
                    )
                })?;
            publish_checkout_token(repository, token)
        }
        Err(source) => Err(Error::CheckoutTokenIo {
            path: path.to_path_buf(),
            source,
        }),
    }
}

fn publish_checkout_token(repository: &Repository, token: String) -> Result<String> {
    let path = repository.checkout_token_path();
    let temporary = repository
        .worktree_git_dir()
        .join(format!(".herdr-review-checkout-id-{token}.tmp"));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(|source| Error::CheckoutTokenIo {
            path: temporary.clone(),
            source,
        })?;
    if let Err(source) = file
        .write_all(token.as_bytes())
        .and_then(|()| file.sync_all())
    {
        drop(file);
        let _ = fs::remove_file(&temporary);
        return Err(Error::CheckoutTokenIo {
            path: temporary,
            source,
        });
    }
    drop(file);
    match fs::hard_link(&temporary, path) {
        Ok(()) => {
            let _ = fs::remove_file(&temporary);
            sync_directory(repository.worktree_git_dir())?;
            Ok(token)
        }
        Err(source) if source.kind() == ErrorKind::AlreadyExists => {
            let _ = fs::remove_file(&temporary);
            fs::read_to_string(path)
                .map_err(|source| Error::CheckoutTokenIo {
                    path: path.to_path_buf(),
                    source,
                })
                .and_then(|value| validate_checkout_token(path, value))
        }
        Err(source) => {
            let _ = fs::remove_file(&temporary);
            Err(Error::CheckoutTokenIo {
                path: temporary,
                source,
            })
        }
    }
}

fn sync_directory(path: &Path) -> Result<()> {
    File::open(path)
        .and_then(|directory| directory.sync_all())
        .map_err(|source| Error::CheckoutTokenIo {
            path: path.to_path_buf(),
            source,
        })
}
fn validate_checkout_token(path: &Path, value: String) -> Result<String> {
    if value.len() == 32
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        Ok(value)
    } else {
        Err(Error::InvalidCheckoutToken {
            path: path.to_path_buf(),
        })
    }
}
fn utf8(path: &Path) -> Result<&str> {
    path.to_str().ok_or_else(|| Error::NonUtf8StatePath {
        path: path.to_path_buf(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

    struct Fixture {
        root: PathBuf,
    }
    impl Fixture {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!(
                "review-store-{}-{}",
                std::process::id(),
                NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed)
            ));
            let _ = fs::remove_dir_all(&root);
            fs::create_dir_all(&root).unwrap();
            run(&root, &["init", "--initial-branch=main"]);
            run(&root, &["config", "user.name", "Test"]);
            run(&root, &["config", "user.email", "test@example.invalid"]);
            fs::write(root.join("a"), "one\n").unwrap();
            run(&root, &["add", "a"]);
            run(&root, &["commit", "-m", "one"]);
            Self { root }
        }
        fn repository(&self) -> Repository {
            Repository::discover(&self.root).unwrap()
        }
        fn spec(&self) -> ComparisonSpec {
            ComparisonSpec::new(
                DiffEndpoint::Commit {
                    oid: run(&self.root, &["rev-parse", "HEAD"]),
                },
                DiffEndpoint::WorkingTree,
            )
            .unwrap()
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }
    fn run(cwd: &Path, args: &[&str]) -> String {
        let output = Command::new("git")
            .arg("-C")
            .arg(cwd)
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap().trim().to_owned()
    }

    fn ref_exists(cwd: &Path, reference: &str) -> bool {
        Command::new("git")
            .arg("-C")
            .arg(cwd)
            .args(["show-ref", "--verify", "--quiet", reference])
            .status()
            .unwrap()
            .success()
    }

    fn pending_request(
        fixture: &Fixture,
        store: &mut Store,
    ) -> (ReviewContext, Observation, RuntimeBinding, AgentRequest) {
        fs::write(fixture.root.join("a"), "two\n").unwrap();
        let (context, observation) = store
            .create_context_with_observation("review", &[], &fixture.spec())
            .unwrap();
        let binding = store
            .bind_runtime(
                &context.id,
                &observation.id,
                "server",
                &WorkspaceId::new("first"),
                None,
                CheckoutKind::Existing,
            )
            .unwrap();
        let anchor = ReviewAnchor::new(
            AnchorLocation::new("a", DiffSide::Target, 1, 1).unwrap(),
            Some(AnchorContext {
                before: vec![],
                selected: vec!["two".to_owned()],
                after: vec![],
            }),
        )
        .unwrap();
        let state = store
            .create_thread(&binding.id, &NewThread::new(anchor, "fix", "wolf").unwrap())
            .unwrap();
        let request = store
            .create_agent_request(
                &binding.id,
                &CreateAgentRequest::new(
                    vec![state.threads.first().unwrap().id.clone()],
                    AgentAssignment::new(
                        "socket",
                        WorkspaceId::new("agents"),
                        PaneId::new("agent"),
                        Some("codex".to_owned()),
                        None,
                    )
                    .unwrap(),
                )
                .unwrap(),
            )
            .unwrap();
        (context, observation, binding, request)
    }

    fn retry_assignment() -> AgentAssignment {
        AgentAssignment::new(
            "socket",
            WorkspaceId::new("agents"),
            PaneId::new("replacement-agent"),
            Some("codex".to_owned()),
            None,
        )
        .unwrap()
    }

    #[test]
    #[expect(
        clippy::too_many_lines,
        reason = "one complete two-exchange conversation exercises reservations and reply idempotency"
    )]
    fn conversations_reserve_messages_and_replies_are_idempotent() {
        let fixture = Fixture::new();
        let mut store = Store::open(&fixture.repository()).unwrap();
        let (_, observation, binding, _) = pending_request(&fixture, &mut store);
        let finding = NewFinding {
            finding: Finding {
                key: "conversation".into(),
                kind: crate::FindingKind::Question,
                severity: None,
                title: "Where does state belong?".into(),
                evidence: "Two owners".into(),
                related_locations: vec![],
            },
            location: None,
            body: "Consider the state owner".into(),
            author: "agent".into(),
        };
        let state = store
            .save_finding(&binding.id, &observation.id, &finding)
            .unwrap();
        let thread = state
            .threads
            .iter()
            .find(|thread| thread.finding.is_some())
            .unwrap()
            .id
            .clone();
        let batch = CreateAgentRequest::new(vec![thread.clone()], retry_assignment()).unwrap();
        assert!(matches!(
            store.create_agent_request(&binding.id, &batch),
            Err(Error::NoPendingMessages)
        ));
        let question = store
            .add_message(
                &binding.id,
                &thread,
                &NewMessage::new("Where exactly?", "reviewer").unwrap(),
            )
            .unwrap();
        let request = store.create_agent_request(&binding.id, &batch).unwrap();
        assert_eq!(request.message_ids, vec![question.id.clone()]);
        assert!(matches!(
            store.create_agent_request(&binding.id, &batch),
            Err(Error::NoPendingMessages)
        ));
        let mut other = Store::open(&fixture.repository()).unwrap();
        assert!(matches!(
            other.create_agent_request(&binding.id, &batch),
            Err(Error::NoPendingMessages)
        ));
        let followup = store
            .add_message(
                &binding.id,
                &thread,
                &NewMessage::new("And why?", "reviewer").unwrap(),
            )
            .unwrap();
        let frozen = request_threads_on(&store.connection, &request.id, &store.path).unwrap();
        assert_eq!(
            frozen.first().unwrap().messages.len(),
            2,
            "later messages must not leak into a queued request"
        );
        let answer = NewMessage::new("In the existing state owner.", "agent").unwrap();
        let attempt = request.attempts.last().unwrap();
        assert!(matches!(
            store.reply_to_request(
                &binding.id,
                &request.id,
                &thread,
                &attempt.id,
                &attempt.assignment,
                &answer
            ),
            Err(Error::InvalidAgentReply)
        ));
        while store
            .claim_next_dispatch(&binding.id, "test", Duration::from_secs(60))
            .unwrap()
            .is_some()
        {}
        let saved = store
            .reply_to_request(
                &binding.id,
                &request.id,
                &thread,
                &attempt.id,
                &attempt.assignment,
                &answer,
            )
            .unwrap();
        assert_eq!(saved.origin, MessageOrigin::Agent);
        assert_eq!(
            store
                .reply_to_request(
                    &binding.id,
                    &request.id,
                    &thread,
                    &attempt.id,
                    &attempt.assignment,
                    &answer
                )
                .unwrap()
                .id,
            saved.id
        );
        assert!(matches!(
            store.reply_to_request(
                &binding.id,
                &request.id,
                &thread,
                &attempt.id,
                &attempt.assignment,
                &NewMessage::new("Different", "agent").unwrap()
            ),
            Err(Error::ConflictingAgentReply)
        ));
        let second = store.create_agent_request(&binding.id, &batch).unwrap();
        assert_eq!(second.message_ids, vec![followup.id]);
        let context = request_threads_on(&store.connection, &second.id, &store.path).unwrap();
        assert_eq!(
            context.first().unwrap().messages.len(),
            4,
            "prior replies accompany the next exchange as context"
        );
        assert!(matches!(
            store.create_agent_request(&binding.id, &batch),
            Err(Error::NoPendingMessages)
        ));
        let human = state
            .threads
            .iter()
            .find(|thread| thread.finding.is_none())
            .unwrap();
        assert!(matches!(
            store.reply_to_request(
                &binding.id,
                &request.id,
                &human.id,
                &attempt.id,
                &attempt.assignment,
                &answer
            ),
            Err(Error::InvalidAgentReply)
        ));
    }

    #[test]
    fn returned_requests_recover_missing_answers_and_reject_stale_attempts() {
        let fixture = Fixture::new();
        let mut store = Store::open(&fixture.repository()).unwrap();
        let (_, _, binding, request) = pending_request(&fixture, &mut store);
        let thread = request.thread_ids.first().unwrap();
        let original = store
            .claim_next_dispatch(&binding.id, "original", Duration::from_secs(60))
            .unwrap()
            .unwrap();
        store
            .finish_dispatch(
                &binding.id,
                &original.attempt.id,
                "original",
                &DispatchOutcome::Returned { detail: None },
            )
            .unwrap();
        let assignment = retry_assignment();
        let retry = store
            .retry_dispatch(&binding.id, &request.id, &assignment)
            .unwrap();
        let answer = NewMessage::new("Answer", "agent").unwrap();
        assert!(matches!(
            store.reply_to_request(
                &binding.id,
                &request.id,
                thread,
                &original.attempt.id,
                &original.attempt.assignment,
                &answer
            ),
            Err(Error::InvalidAgentReply)
        ));
        let mut other = Store::open(&fixture.repository()).unwrap();
        assert!(matches!(
            other.reply_to_request(
                &binding.id,
                &request.id,
                thread,
                &retry.id,
                &assignment,
                &answer
            ),
            Err(Error::InvalidAgentReply)
        ));
        store
            .claim_next_dispatch(&binding.id, "retry", Duration::from_secs(60))
            .unwrap()
            .unwrap();
        assert!(matches!(
            other.reply_to_request(
                &binding.id,
                &request.id,
                thread,
                &retry.id,
                &original.attempt.assignment,
                &answer
            ),
            Err(Error::InvalidAgentReply)
        ));
        let saved = other
            .reply_to_request(
                &binding.id,
                &request.id,
                thread,
                &retry.id,
                &assignment,
                &answer,
            )
            .unwrap();
        let refreshed = store.agent_request(&binding.id, &request.id).unwrap();
        assert_eq!(refreshed.answered_thread_ids, vec![thread.clone()]);
        let threads = request_threads_on(&store.connection, &request.id, &store.path).unwrap();
        assert!(
            threads
                .first()
                .unwrap()
                .messages
                .iter()
                .any(|message| message.id == saved.id)
        );
        store
            .finish_dispatch(
                &binding.id,
                &retry.id,
                "retry",
                &DispatchOutcome::Returned { detail: None },
            )
            .unwrap();
        assert!(matches!(
            store.retry_dispatch(&binding.id, &request.id, &assignment),
            Err(Error::InvalidTransition { .. })
        ));
    }

    #[test]
    fn candidate_limit_applies_after_filtering_threads_without_new_messages() {
        let fixture = Fixture::new();
        let mut store = Store::open(&fixture.repository()).unwrap();
        let (_, _, binding, original) = pending_request(&fixture, &mut store);
        let thread = original.thread_ids.first().unwrap();
        store.connection.execute("WITH RECURSIVE n(x) AS (VALUES(1) UNION ALL SELECT x + 1 FROM n WHERE x < 256) INSERT INTO threads (id, context_id, observation_id, path, side, start_line, end_line, anchor_source_id, context_json, status) SELECT lower(hex(randomblob(16))), context_id, observation_id, path, side, start_line, end_line, anchor_source_id, context_json, status FROM threads, n WHERE id = ?1", [thread.as_str()]).unwrap();
        let candidates = store
            .connection
            .prepare("SELECT id FROM threads ORDER BY id")
            .unwrap()
            .query_map([], |row| row.get::<_, String>(0).map(ThreadId::from_stored))
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap();
        assert_eq!(candidates.len(), 257);
        store
            .add_message(
                &binding.id,
                thread,
                &NewMessage::new("A new question", "reviewer").unwrap(),
            )
            .unwrap();
        let request = store
            .create_agent_request(
                &binding.id,
                &CreateAgentRequest::new(candidates, retry_assignment()).unwrap(),
            )
            .unwrap();
        assert_eq!(request.thread_ids, vec![thread.clone()]);
    }

    #[test]
    fn retry_projection_stays_bounded_with_large_saved_answers() {
        let fixture = Fixture::new();
        let mut store = Store::open(&fixture.repository()).unwrap();
        let (_, observation, binding, _) = pending_request(&fixture, &mut store);
        let mut thread_ids = Vec::new();
        for index in 0..6 {
            let finding = NewFinding {
                finding: Finding {
                    key: format!("finding-{index}"),
                    kind: crate::FindingKind::Question,
                    severity: None,
                    title: "Title".into(),
                    evidence: "Evidence".into(),
                    related_locations: vec![],
                },
                location: None,
                body: "Context".into(),
                author: "agent".into(),
            };
            store
                .save_finding(&binding.id, &observation.id, &finding)
                .unwrap();
            let thread = store
                .connection
                .query_row(
                    "SELECT id FROM threads WHERE finding_key = ?1",
                    [&finding.finding.key],
                    |row| row.get::<_, String>(0).map(ThreadId::from_stored),
                )
                .unwrap();
            store
                .add_message(
                    &binding.id,
                    &thread,
                    &NewMessage::new("Explain?", "reviewer").unwrap(),
                )
                .unwrap();
            thread_ids.push(thread);
        }
        let assignment = retry_assignment();
        let request = store
            .create_agent_request(
                &binding.id,
                &CreateAgentRequest::new(thread_ids.clone(), assignment.clone()).unwrap(),
            )
            .unwrap();
        let job = loop {
            let job = store
                .claim_next_dispatch(&binding.id, "worker", Duration::from_secs(60))
                .unwrap()
                .unwrap();
            if job.request.id == request.id {
                break job;
            }
        };
        let mut observed = assignment.clone();
        observed.expected_agent_kind = Some("codex".into());
        for thread in thread_ids.iter().take(5) {
            store
                .reply_to_request(
                    &binding.id,
                    &request.id,
                    thread,
                    &job.attempt.id,
                    &observed,
                    &NewMessage::new("x".repeat(60 * 1024), "agent").unwrap(),
                )
                .unwrap();
        }
        store
            .finish_dispatch(
                &binding.id,
                &job.attempt.id,
                "worker",
                &DispatchOutcome::Unknown {
                    detail: "transport lost".into(),
                },
            )
            .unwrap();
        assert_eq!(
            store
                .agent_request(&binding.id, &request.id)
                .unwrap()
                .recovery,
            Some(crate::RequestRecovery::InspectBeforeRetry)
        );
        store
            .retry_dispatch(&binding.id, &request.id, &assignment)
            .unwrap();
        let retried = store
            .claim_next_dispatch(&binding.id, "retry", Duration::from_secs(60))
            .unwrap()
            .unwrap();
        assert_eq!(retried.request.answered_thread_ids.len(), 5);
        assert!(retried.projection_json.len() <= MAX_REQUEST_PROJECTION_BYTES);
        assert!(retried.projection_json.contains("Evidence"));
        for id in &request.message_ids {
            assert!(retried.projection_json.contains(id.as_str()));
        }
    }

    #[test]
    fn oversized_new_messages_are_not_reserved_and_old_context_is_bounded() {
        let fixture = Fixture::new();
        let mut store = Store::open(&fixture.repository()).unwrap();
        let (_, _, binding, first) = pending_request(&fixture, &mut store);
        let thread = first.thread_ids.first().unwrap().clone();
        let batch = CreateAgentRequest::new(vec![thread.clone()], retry_assignment()).unwrap();
        assert!(matches!(
            NewMessage::new("x".repeat(64 * 1024), "reviewer"),
            Err(Error::InputTooLarge { .. })
        ));
        // Each exchange fits independently; accumulated history exceeds the
        // transport limit, but only a bounded projection accompanies a send.
        for _ in 0..20 {
            store
                .add_message(
                    &binding.id,
                    &thread,
                    &NewMessage::new("cafe\u{301} 👨‍👩‍👧 ".repeat(1500), "reviewer").unwrap(),
                )
                .unwrap();
            store.create_agent_request(&binding.id, &batch).unwrap();
        }
        let mut claims = 0;
        while let Some(job) = store
            .claim_next_dispatch(&binding.id, "test", Duration::from_secs(60))
            .unwrap()
        {
            assert!(job.projection_json.len() <= MAX_REQUEST_PROJECTION_BYTES);
            for id in &job.request.message_ids {
                assert!(
                    job.threads
                        .iter()
                        .flat_map(|thread| &thread.messages)
                        .any(|message| message.id == *id)
                );
            }
            claims += 1;
        }
        assert_eq!(claims, 21);
        for _ in 0..6 {
            store
                .add_message(
                    &binding.id,
                    &thread,
                    &NewMessage::new("x".repeat(60 * 1024), "reviewer").unwrap(),
                )
                .unwrap();
        }
        for _ in 0..2 {
            assert!(matches!(
                store.create_agent_request(&binding.id, &batch),
                Err(Error::RequestTooLarge)
            ));
            assert_eq!(
                unreserved_reviewer_messages_on(&store.connection, &thread, &store.path)
                    .unwrap()
                    .len(),
                6
            );
        }
    }

    #[test]
    fn findings_are_idempotent_anchored_or_general_and_not_new_reviewer_messages() {
        let fixture = Fixture::new();
        let mut store = Store::open(&fixture.repository()).unwrap();
        let (_, observation, binding, _) = pending_request(&fixture, &mut store);
        let human = store.binding_state(&binding.id).unwrap().threads.remove(0);
        let mut request = NewFinding {
            finding: Finding {
                key: "release-order".into(),
                kind: crate::FindingKind::Finding,
                severity: Some(crate::FindingSeverity::NonBlocking),
                title: "Record success first".into(),
                evidence: "A cleanup failure retries publication".into(),
                related_locations: vec![],
            },
            location: Some(AnchorLocation::new("a", DiffSide::Target, 1, 1).unwrap()),
            body: "Persist success before cleanup".into(),
            author: "codex".into(),
        };
        let state = store
            .save_finding(&binding.id, &observation.id, &request)
            .unwrap();
        let finding = state
            .threads
            .iter()
            .find(|thread| thread.finding.is_some())
            .unwrap();
        let id = finding.id.clone();
        assert_eq!(
            finding.resolution.as_ref().unwrap().status,
            crate::AnchorStatus::Exact
        );
        assert!(matches!(
            store.create_agent_request(
                &binding.id,
                &CreateAgentRequest::new(vec![id.clone()], retry_assignment()).unwrap()
            ),
            Err(Error::NoPendingMessages)
        ));
        request.body = "Updated assessment".into();
        request.location = None;
        let state = store
            .save_finding(&binding.id, &observation.id, &request)
            .unwrap();
        assert_eq!(state.threads.len(), 2);
        let updated = state.threads.iter().find(|thread| thread.id == id).unwrap();
        assert!(updated.anchor.is_none());
        assert!(updated.resolution.is_none());
        assert_eq!(updated.messages.len(), 1);
        assert_eq!(updated.messages.first().unwrap().body, "Updated assessment");
        assert_eq!(
            state
                .threads
                .iter()
                .find(|thread| thread.id == human.id)
                .unwrap()
                .messages,
            human.messages
        );
        request.location = Some(AnchorLocation::new("a", DiffSide::Target, 9, 9).unwrap());
        assert!(matches!(
            store.save_finding(&binding.id, &observation.id, &request),
            Err(Error::AnchorSourceMismatch { .. })
        ));
    }

    #[test]
    fn runtime_lookup_is_exact_and_findings_reject_advanced_observations() {
        let fixture = Fixture::new();
        let mut store = Store::open(&fixture.repository()).unwrap();
        let (context, observation, binding, _) = pending_request(&fixture, &mut store);
        assert_eq!(
            store
                .current_runtime_binding("server", &WorkspaceId::new("first"))
                .unwrap()
                .unwrap()
                .id,
            binding.id
        );
        assert!(
            store
                .current_runtime_binding("other-server", &WorkspaceId::new("first"))
                .unwrap()
                .is_none()
        );
        fs::write(fixture.root.join("a"), "three\n").unwrap();
        let next = store
            .record_observation(&context.id, &fixture.spec())
            .unwrap();
        store
            .bind_runtime(
                &context.id,
                &next.id,
                "server",
                &WorkspaceId::new("first"),
                None,
                CheckoutKind::Existing,
            )
            .unwrap();
        assert!(matches!(
            store.current_runtime_binding("server", &WorkspaceId::new("first")),
            Err(Error::AmbiguousRuntimeBinding { .. })
        ));
        let request = NewFinding {
            finding: Finding {
                key: "general".into(),
                kind: crate::FindingKind::Question,
                severity: None,
                title: "Why?".into(),
                evidence: "Two paths differ".into(),
                related_locations: vec![],
            },
            location: None,
            body: "Explain the difference".into(),
            author: "claude".into(),
        };
        assert!(matches!(
            store.save_finding(&binding.id, &next.id, &request),
            Err(Error::StaleObservation { .. })
        ));
        assert!(
            store
                .save_finding(&binding.id, &observation.id, &request)
                .is_ok()
        );
    }

    #[test]
    fn contexts_references_observations_and_bindings_are_independent() {
        let fixture = Fixture::new();
        fs::write(fixture.root.join("a"), "two\n").unwrap();
        let repository = fixture.repository();
        let mut store = Store::open(&repository).unwrap();
        let reference = ExternalReference::new("change_request", "forge.example/o/r#1").unwrap();
        let (first, observation) = store
            .create_context_with_observation(
                "one",
                std::slice::from_ref(&reference),
                &fixture.spec(),
            )
            .unwrap();
        let second = store
            .create_context("two", std::slice::from_ref(&reference))
            .unwrap();
        assert_eq!(store.contexts_by_reference(&reference).unwrap().len(), 2);
        let first_binding = store
            .bind_runtime(
                &first.id,
                &observation.id,
                "server",
                &WorkspaceId::new("w"),
                Some(&PaneId::new("p")),
                CheckoutKind::Existing,
            )
            .unwrap();
        let second_binding = store
            .bind_runtime(
                &first.id,
                &observation.id,
                "server",
                &WorkspaceId::new("w"),
                Some(&PaneId::new("p")),
                CheckoutKind::Existing,
            )
            .unwrap();
        assert_ne!(first_binding.id, second_binding.id);
        assert_ne!(first.id, second.id);
        assert_eq!(first_binding.herdr_server_id, "server");

        let listing = store
            .list_contexts(ContextListQuery::new(2, 1).unwrap())
            .unwrap();
        assert_eq!(listing.len(), 2);
        assert_eq!(
            listing
                .iter()
                .find(|listing| listing.context.id == first.id)
                .unwrap()
                .runtime_bindings
                .len(),
            1
        );
    }

    #[test]
    fn context_title_can_be_refreshed() {
        let fixture = Fixture::new();
        let repository = fixture.repository();
        let mut store = Store::open(&repository).unwrap();
        let context = store.create_context("old title", &[]).unwrap();

        let updated = store.set_context_title(&context.id, "new title").unwrap();

        assert_eq!(updated.title, "new title");
        assert_eq!(store.context(&context.id).unwrap().title, "new title");
        assert!(matches!(
            store.set_context_title(&context.id, " "),
            Err(Error::EmptyField { .. })
        ));
    }

    #[test]
    fn thread_messages_and_dispatch_are_scoped_to_binding() {
        let fixture = Fixture::new();
        fs::write(fixture.root.join("a"), "two\n").unwrap();
        let repository = fixture.repository();
        let mut store = Store::open(&repository).unwrap();
        let (context, observation) = store
            .create_context_with_observation("review", &[], &fixture.spec())
            .unwrap();
        let binding = store
            .bind_runtime(
                &context.id,
                &observation.id,
                "server",
                &WorkspaceId::new("review"),
                None,
                CheckoutKind::Existing,
            )
            .unwrap();
        let anchor = ReviewAnchor::new(
            AnchorLocation::new("a", DiffSide::Target, 1, 1).unwrap(),
            Some(AnchorContext {
                before: vec![],
                selected: vec!["two".to_owned()],
                after: vec![],
            }),
        )
        .unwrap();
        let state = store
            .create_thread(&binding.id, &NewThread::new(anchor, "fix", "wolf").unwrap())
            .unwrap();
        let thread = state.threads.last().unwrap().clone();
        store
            .add_message(
                &binding.id,
                &thread.id,
                &NewMessage::new("more", "wolf").unwrap(),
            )
            .unwrap();
        let assignment = AgentAssignment::new(
            "socket",
            WorkspaceId::new("agents"),
            PaneId::new("agent"),
            Some("codex".to_owned()),
            None,
        )
        .unwrap();
        let request = store
            .create_agent_request(
                &binding.id,
                &CreateAgentRequest::new(vec![thread.id.clone()], assignment).unwrap(),
            )
            .unwrap();
        let job = store
            .claim_next_dispatch(&binding.id, "worker", Duration::from_secs(10))
            .unwrap()
            .unwrap();
        assert_eq!(job.request.id, request.id);
        assert_eq!(job.threads.first().unwrap().messages.len(), 2);
        store
            .finish_dispatch(
                &binding.id,
                &job.attempt.id,
                "worker",
                &DispatchOutcome::Returned { detail: None },
            )
            .unwrap();
        let state = store.binding_state(&binding.id).unwrap();
        assert_eq!(
            state.agent_requests.first().unwrap().state,
            AgentRequestState::Returned
        );
        assert_eq!(state.threads.first().unwrap().status, ThreadStatus::Open);

        let rebound = store
            .bind_runtime(
                &context.id,
                &observation.id,
                "server",
                &WorkspaceId::new("later-review"),
                Some(&PaneId::new("later-editor")),
                CheckoutKind::Existing,
            )
            .unwrap();
        let rebound_state = store.binding_state(&rebound.id).unwrap();
        assert_eq!(rebound_state.agent_requests.len(), 1);
        assert_eq!(rebound_state.agent_requests.first().unwrap().id, request.id);
        assert_eq!(
            rebound_state
                .agent_requests
                .first()
                .unwrap()
                .runtime_binding_id,
            binding.id
        );
    }

    #[test]
    fn retry_from_a_rebound_workspace_supersedes_the_old_pending_attempt() {
        let fixture = Fixture::new();
        let repository = fixture.repository();
        let mut store = Store::open(&repository).unwrap();
        let (context, observation, origin, request) = pending_request(&fixture, &mut store);
        let rebound = store
            .bind_runtime(
                &context.id,
                &observation.id,
                "server",
                &WorkspaceId::new("rebound"),
                None,
                CheckoutKind::Existing,
            )
            .unwrap();

        let replacement = store
            .retry_dispatch(&rebound.id, &request.id, &retry_assignment())
            .unwrap();
        let history = store.agent_request(&rebound.id, &request.id).unwrap();

        assert_eq!(request.runtime_binding_id, origin.id);
        assert_eq!(replacement.runtime_binding_id, rebound.id);
        assert_eq!(replacement.state, DispatchAttemptState::Pending);
        assert_eq!(history.attempts.len(), 2);
        assert_eq!(
            history.attempts.first().unwrap().state,
            DispatchAttemptState::Superseded
        );
        assert!(store.pending_dispatches(&origin.id).unwrap().is_empty());
        assert_eq!(store.pending_dispatches(&rebound.id).unwrap().len(), 1);
        assert!(
            store
                .claim_next_dispatch(&origin.id, "old-worker", Duration::from_secs(10))
                .unwrap()
                .is_none()
        );
        assert_eq!(
            store
                .claim_next_dispatch(&rebound.id, "new-worker", Duration::from_secs(10))
                .unwrap()
                .unwrap()
                .attempt
                .id,
            replacement.id
        );
    }

    #[test]
    fn retry_never_overwrites_an_active_foreign_claim_but_recovers_it_when_expired() {
        let fixture = Fixture::new();
        let repository = fixture.repository();
        let mut store = Store::open(&repository).unwrap();
        let (context, observation, origin, request) = pending_request(&fixture, &mut store);
        let rebound = store
            .bind_runtime(
                &context.id,
                &observation.id,
                "server",
                &WorkspaceId::new("rebound"),
                None,
                CheckoutKind::Existing,
            )
            .unwrap();
        let claimed = store
            .claim_next_dispatch(&origin.id, "old-worker", Duration::from_secs(10))
            .unwrap()
            .unwrap()
            .attempt;

        assert!(matches!(
            store.retry_dispatch(&rebound.id, &request.id, &retry_assignment()),
            Err(Error::InvalidTransition { .. })
        ));
        assert_eq!(
            store
                .dispatch_attempt(&origin.id, &claimed.id)
                .unwrap()
                .state,
            DispatchAttemptState::Dispatching
        );

        store
            .connection
            .execute(
                "UPDATE dispatch_attempts SET claim_expires_at = '1970-01-01T00:00:00.000Z' WHERE id = ?1",
                [claimed.id.as_str()],
            )
            .unwrap();
        let replacement = store
            .retry_dispatch(&rebound.id, &request.id, &retry_assignment())
            .unwrap();

        assert_eq!(replacement.runtime_binding_id, rebound.id);
        assert_eq!(
            store
                .dispatch_attempt(&origin.id, &claimed.id)
                .unwrap()
                .state,
            DispatchAttemptState::Unknown
        );
    }

    #[test]
    fn dispatch_uses_its_request_observation_after_the_binding_advances() {
        let fixture = Fixture::new();
        fs::write(fixture.root.join("a"), "two\n").unwrap();
        let repository = fixture.repository();
        let mut store = Store::open(&repository).unwrap();
        let (context, observation) = store
            .create_context_with_observation("review", &[], &fixture.spec())
            .unwrap();
        let binding = store
            .bind_runtime(
                &context.id,
                &observation.id,
                "server",
                &WorkspaceId::new("review"),
                None,
                CheckoutKind::Existing,
            )
            .unwrap();
        let anchor = ReviewAnchor::new(
            AnchorLocation::new("a", DiffSide::Target, 1, 1).unwrap(),
            Some(AnchorContext {
                before: vec![],
                selected: vec!["two".to_owned()],
                after: vec![],
            }),
        )
        .unwrap();
        let state = store
            .create_thread(&binding.id, &NewThread::new(anchor, "fix", "wolf").unwrap())
            .unwrap();
        let thread = state.threads.first().unwrap();
        let assignment = AgentAssignment::new(
            "socket",
            WorkspaceId::new("agents"),
            PaneId::new("agent"),
            Some("codex".to_owned()),
            None,
        )
        .unwrap();
        let request = store
            .create_agent_request(
                &binding.id,
                &CreateAgentRequest::new(vec![thread.id.clone()], assignment).unwrap(),
            )
            .unwrap();
        let requested_observation = store.observation(&request.observation_id).unwrap();

        fs::write(fixture.root.join("a"), "three\n").unwrap();
        let advanced = store
            .observe_binding(&binding.id, &repository.working_tree_comparison().unwrap())
            .unwrap();
        assert_ne!(advanced.observation.id, request.observation_id);

        let job = store
            .claim_next_dispatch(&binding.id, "worker", Duration::from_secs(10))
            .unwrap()
            .unwrap();
        assert_eq!(job.request.observation_id, requested_observation.id);
        assert_eq!(job.comparison, requested_observation.comparison);
        assert_eq!(
            job.threads
                .first()
                .unwrap()
                .resolution
                .as_ref()
                .unwrap()
                .status,
            crate::AnchorStatus::Exact
        );
    }

    #[test]
    fn creates_first_context_observation_binding_and_thread_atomically() {
        let fixture = Fixture::new();
        fs::write(fixture.root.join("a"), "two\n").unwrap();
        let repository = fixture.repository();
        let mut store = Store::open(&repository).unwrap();
        let anchor = ReviewAnchor::new(
            AnchorLocation::new("a", DiffSide::Target, 1, 1).unwrap(),
            Some(AnchorContext {
                before: vec![],
                selected: vec!["two".to_owned()],
                after: vec![],
            }),
        )
        .unwrap();
        let request = CreateBoundThread::new(
            "review",
            vec![],
            fixture.spec(),
            "server",
            WorkspaceId::new("review"),
            Some(PaneId::new("editor")),
            CheckoutKind::ManagedWorktree,
            NewThread::new(anchor, "fix this", "wolf").unwrap(),
        )
        .unwrap();

        let state = store.create_bound_thread(&request).unwrap();

        assert_eq!(state.context.title, "review");
        assert_eq!(state.binding.context_id, state.context.id);
        assert_eq!(state.binding.observation_id, state.observation.id);
        assert_eq!(state.threads.len(), 1);
        assert_eq!(
            state
                .threads
                .first()
                .unwrap()
                .messages
                .first()
                .unwrap()
                .body,
            "fix this"
        );
        assert_eq!(store.summary().unwrap().contexts, 1);
    }

    #[test]
    fn observations_advance_one_binding_without_hiding_context_threads() {
        let fixture = Fixture::new();
        fs::write(fixture.root.join("a"), "two\n").unwrap();
        let repository = fixture.repository();
        let mut store = Store::open(&repository).unwrap();
        let (context, observation) = store
            .create_context_with_observation("review", &[], &fixture.spec())
            .unwrap();
        let first = store
            .bind_runtime(
                &context.id,
                &observation.id,
                "server",
                &WorkspaceId::new("shared"),
                Some(&PaneId::new("editor")),
                CheckoutKind::Existing,
            )
            .unwrap();
        let second = store
            .bind_runtime(
                &context.id,
                &observation.id,
                "server",
                &WorkspaceId::new("shared"),
                Some(&PaneId::new("editor")),
                CheckoutKind::Existing,
            )
            .unwrap();
        let anchor = ReviewAnchor::new(
            AnchorLocation::new("a", DiffSide::Target, 1, 1).unwrap(),
            Some(AnchorContext {
                before: vec![],
                selected: vec!["two".to_owned()],
                after: vec![],
            }),
        )
        .unwrap();
        store
            .create_thread(
                &first.id,
                &NewThread::new(anchor, "keep this thread", "wolf").unwrap(),
            )
            .unwrap();

        fs::write(fixture.root.join("a"), "zero\ntwo\n").unwrap();
        let advanced = store
            .observe_binding(&first.id, &repository.working_tree_comparison().unwrap())
            .unwrap();
        let unchanged = store.binding_state(&second.id).unwrap();

        assert_ne!(advanced.observation.id, observation.id);
        assert_eq!(unchanged.observation.id, observation.id);
        assert_eq!(advanced.threads.len(), 1);
        let resolution = advanced
            .threads
            .first()
            .unwrap()
            .resolution
            .as_ref()
            .unwrap();
        assert_eq!(resolution.status, crate::AnchorStatus::Shifted);
        assert_eq!(resolution.current_location.as_ref().unwrap().start_line, 2);
    }

    #[test]
    fn invalid_thread_does_not_advance_its_binding_observation() {
        let fixture = Fixture::new();
        fs::write(fixture.root.join("a"), "two\n").unwrap();
        let repository = fixture.repository();
        let mut store = Store::open(&repository).unwrap();
        let (context, observation) = store
            .create_context_with_observation("review", &[], &fixture.spec())
            .unwrap();
        let binding = store
            .bind_runtime(
                &context.id,
                &observation.id,
                "server",
                &WorkspaceId::new("review"),
                None,
                CheckoutKind::Existing,
            )
            .unwrap();

        fs::write(fixture.root.join("a"), "three\n").unwrap();
        let anchor = ReviewAnchor::new(
            AnchorLocation::new("a", DiffSide::Target, 1, 1).unwrap(),
            Some(AnchorContext {
                before: vec![],
                selected: vec!["not three".to_owned()],
                after: vec![],
            }),
        )
        .unwrap();
        let result = store.create_thread_on_comparison(
            &binding.id,
            &repository.working_tree_comparison().unwrap(),
            &NewThread::new(anchor, "invalid anchor", "wolf").unwrap(),
        );

        assert!(matches!(result, Err(Error::AnchorSourceMismatch { .. })));
        assert_eq!(
            store.runtime_binding(&binding.id).unwrap().observation_id,
            observation.id
        );
        assert_eq!(store.observations(&context.id).unwrap().len(), 1);
        assert!(store.binding_state(&binding.id).unwrap().threads.is_empty());
    }

    #[test]
    fn repeated_identical_observations_do_not_leak_git_refs() {
        let fixture = Fixture::new();
        fs::write(fixture.root.join("a"), "two\n").unwrap();
        let repository = fixture.repository();
        let mut store = Store::open(&repository).unwrap();
        let spec = fixture.spec();
        let (context, observation) = store
            .create_context_with_observation("review", &[], &spec)
            .unwrap();
        let binding = store
            .bind_runtime(
                &context.id,
                &observation.id,
                "server",
                &WorkspaceId::new("review"),
                None,
                CheckoutKind::Existing,
            )
            .unwrap();

        for _ in 0..3 {
            let state = store.observe_binding(&binding.id, &spec).unwrap();
            assert_eq!(state.observation.id, observation.id);
        }

        let refs = run(
            &fixture.root,
            &[
                "for-each-ref",
                "--format=%(refname)",
                "refs/herdr-review/observations",
            ],
        );
        assert_eq!(refs.lines().count(), 2);
    }

    #[test]
    fn opening_store_removes_only_well_formed_orphan_observation_refs() {
        let fixture = Fixture::new();
        fs::write(fixture.root.join("a"), "two\n").unwrap();
        let repository = fixture.repository();
        let mut store = Store::open(&repository).unwrap();
        let (_, observation) = store
            .create_context_with_observation("review", &[], &fixture.spec())
            .unwrap();
        drop(store);

        let orphan = "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";
        let orphan_base = format!("refs/herdr-review/observations/{orphan}/base");
        let orphan_note = format!("refs/herdr-review/observations/{orphan}/note");
        let malformed = "refs/herdr-review/observations/not-an-id/base";
        let unrelated = "refs/herdr-review/unrelated";
        for reference in [&orphan_base, &orphan_note] {
            run(&fixture.root, &["update-ref", reference, "HEAD"]);
        }
        run(&fixture.root, &["update-ref", malformed, "HEAD"]);
        run(&fixture.root, &["update-ref", unrelated, "HEAD"]);

        drop(Store::open(&repository).unwrap());

        assert!(!ref_exists(&fixture.root, &orphan_base));
        assert!(ref_exists(&fixture.root, &orphan_note));
        assert!(ref_exists(&fixture.root, malformed));
        assert!(ref_exists(&fixture.root, unrelated));
        assert!(ref_exists(
            &fixture.root,
            &format!("refs/herdr-review/observations/{}/base", observation.id)
        ));
    }

    #[test]
    fn incompatible_nonempty_database_fails_loudly() {
        let fixture = Fixture::new();
        let repository = fixture.repository();
        fs::create_dir_all(repository.database_path().parent().unwrap()).unwrap();
        let connection = Connection::open(repository.database_path()).unwrap();
        connection
            .execute("CREATE TABLE legacy(value TEXT)", [])
            .unwrap();
        drop(connection);
        assert!(matches!(
            Store::open(&repository),
            Err(Error::IncompatibleDatabase { .. })
        ));
    }

    #[test]
    fn same_table_names_with_a_different_shape_are_incompatible() {
        let fixture = Fixture::new();
        let repository = fixture.repository();
        drop(Store::open(&repository).unwrap());
        let connection = Connection::open(repository.database_path()).unwrap();
        connection
            .execute("ALTER TABLE threads ADD COLUMN legacy TEXT", [])
            .unwrap();
        drop(connection);

        assert!(matches!(
            Store::open(&repository),
            Err(Error::IncompatibleDatabase { .. })
        ));
    }

    #[test]
    fn binding_pages_preserve_long_histories_and_reject_changed_snapshots() {
        let fixture = Fixture::new();
        let mut store = Store::open(&fixture.repository()).unwrap();
        let (_, _, binding, request) = pending_request(&fixture, &mut store);
        let thread = request.thread_ids.first().unwrap();
        let first_state = store.binding_state(&binding.id).unwrap();
        let mut expected = vec![
            first_state
                .threads
                .first()
                .unwrap()
                .messages
                .first()
                .unwrap()
                .id
                .clone(),
        ];
        for _ in 0..40 {
            expected.push(
                store
                    .add_message(
                        &binding.id,
                        thread,
                        &NewMessage::new("cafe\u{301} 👨‍👩‍👧 ".repeat(1_000), "reviewer").unwrap(),
                    )
                    .unwrap()
                    .id,
            );
        }
        let first = store.binding_state(&binding.id).unwrap();
        assert!(first.next_cursor.is_some());
        let stale = first.next_cursor.clone().unwrap();
        let mut found = Vec::new();
        let mut page = first;
        let mut request_ids = Vec::new();
        loop {
            assert!(serde_json::to_vec(&page).unwrap().len() <= 512 * 1024);
            for fragment in &page.threads {
                found.extend(fragment.messages.iter().map(|message| message.id.clone()));
            }
            request_ids.extend(page.agent_requests.iter().map(|request| request.id.clone()));
            let Some(cursor) = page.next_cursor else {
                break;
            };
            page = store
                .binding_state_page(&binding.id, Some(&cursor))
                .unwrap();
        }
        assert_eq!(found, expected);
        assert_eq!(request_ids, vec![request.id.clone()]);
        store
            .add_message(
                &binding.id,
                thread,
                &NewMessage::new("new question", "reviewer").unwrap(),
            )
            .unwrap();
        assert!(matches!(
            store.binding_state_page(&binding.id, Some(&stale)),
            Err(Error::StaleCursor)
        ));
        let fresh = store
            .binding_state(&binding.id)
            .unwrap()
            .next_cursor
            .unwrap();
        store
            .claim_next_dispatch(&binding.id, "worker", Duration::from_secs(30))
            .unwrap()
            .unwrap();
        assert!(matches!(
            store.binding_state_page(&binding.id, Some(&fresh)),
            Err(Error::StaleCursor)
        ));
    }

    #[test]
    fn binding_pages_include_every_dispatch_attempt() {
        let fixture = Fixture::new();
        let mut store = Store::open(&fixture.repository()).unwrap();
        let (_, _, binding, request) = pending_request(&fixture, &mut store);
        for _ in 0..140 {
            let job = store
                .claim_next_dispatch(&binding.id, "worker", Duration::from_secs(30))
                .unwrap()
                .unwrap();
            store
                .finish_dispatch(
                    &binding.id,
                    &job.attempt.id,
                    "worker",
                    &DispatchOutcome::Returned { detail: None },
                )
                .unwrap();
            store
                .retry_dispatch(&binding.id, &request.id, &retry_assignment())
                .unwrap();
        }
        let mut page = store.binding_state(&binding.id).unwrap();
        let mut attempts = Vec::new();
        loop {
            for request in &page.agent_requests {
                attempts.extend(request.attempts.iter().map(|attempt| attempt.ordinal));
            }
            let Some(cursor) = page.next_cursor else {
                break;
            };
            page = store
                .binding_state_page(&binding.id, Some(&cursor))
                .unwrap();
        }
        assert_eq!(attempts, (1..=141).collect::<Vec<_>>());
    }

    #[test]
    fn context_header_limits_are_atomic_and_revision_tracks_changes() {
        let fixture = Fixture::new();
        let mut store = Store::open(&fixture.repository()).unwrap();
        let (context, _, binding, _) = pending_request(&fixture, &mut store);
        let initial = store.binding_revision(&binding.id).unwrap();
        assert!(matches!(
            store.set_context_title(&context.id, &"x".repeat(70_000)),
            Err(Error::InputTooLarge { .. })
        ));
        assert_eq!(store.context(&context.id).unwrap().title, context.title);
        assert_eq!(store.binding_revision(&binding.id).unwrap(), initial);
        let reference = ExternalReference::new("example", "x".repeat(70_000)).unwrap();
        assert!(matches!(
            store.add_external_reference(&context.id, &reference),
            Err(Error::InputTooLarge { .. })
        ));
        assert!(store.context(&context.id).unwrap().references.is_empty());
        assert_eq!(store.binding_revision(&binding.id).unwrap(), initial);
        store.set_context_title(&context.id, "renamed").unwrap();
        assert!(store.binding_revision(&binding.id).unwrap().0 > initial.0);
    }
}
