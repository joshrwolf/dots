//! Bounded binding projections. Cursors identify stable rows, not vector offsets.

use super::*;
use crate::{BindingCursor, BindingPosition};

const PAGE_BYTES: usize = 256 * 1024;
const FRAGMENT_BYTES: usize = 96 * 1024;
const PAGE_ITEMS: usize = 128;

impl Store {
    /// Cheap change token for a bound editor; source projection is unnecessary when unchanged.
    pub fn binding_revision(&self, binding: &RuntimeBindingId) -> Result<(u64, ObservationId)> {
        let transaction = self.connection.unchecked_transaction().map_err(|source| {
            database_error("begin a binding revision snapshot", &self.path, source)
        })?;
        let binding = self.require_local_binding(binding)?;
        let revision = self
            .connection
            .query_row(
                "SELECT revision FROM review_contexts WHERE id = ?1",
                [binding.context_id.as_str()],
                |row| nonnegative_count(row, 0),
            )
            .map_err(|source| database_error("read binding revision", &self.path, source))?;
        transaction.commit().map_err(|source| {
            database_error("finish a binding revision snapshot", &self.path, source)
        })?;
        Ok((revision, binding.observation_id))
    }

    /// Reads a bounded page. Repeated thread/request IDs continue their messages/attempts.
    pub fn binding_state_page(
        &mut self,
        binding: &RuntimeBindingId,
        cursor: Option<&BindingCursor>,
    ) -> Result<BindingState> {
        let transaction = self.connection.unchecked_transaction().map_err(|source| {
            database_error("begin a binding page snapshot", &self.path, source)
        })?;
        let state = self.read_binding_page(binding, cursor)?;
        transaction.commit().map_err(|source| {
            database_error("finish a binding page snapshot", &self.path, source)
        })?;
        Ok(state)
    }

    fn read_binding_page(
        &self,
        binding: &RuntimeBindingId,
        cursor: Option<&BindingCursor>,
    ) -> Result<BindingState> {
        let binding = self.require_local_binding(binding)?;
        let revision = self
            .connection
            .query_row(
                "SELECT revision FROM review_contexts WHERE id = ?1",
                [binding.context_id.as_str()],
                |row| nonnegative_count(row, 0),
            )
            .map_err(|source| database_error("read binding page revision", &self.path, source))?;
        if cursor.is_some_and(|cursor| {
            cursor.revision != revision || cursor.observation_id != binding.observation_id
        }) {
            return Err(Error::StaleCursor);
        }
        let mut state = BindingState {
            revision,
            context: self.context(&binding.context_id)?,
            observation: self.observation(&binding.observation_id)?,
            binding,
            threads: Vec::new(),
            agent_requests: Vec::new(),
            next_cursor: None,
        };
        let mut next = match cursor {
            Some(cursor) => Some(cursor.position.clone()),
            None => self.first_thread_cursor(&state.context.id, None)?,
        };
        let mut bytes = encoded_size(&state)?;
        for _ in 0..PAGE_ITEMS {
            let Some(current) = next.clone() else { break };
            let (thread, request, following) = match &current {
                BindingPosition::Threads {
                    thread_id,
                    after_message,
                } => {
                    require_thread_scope(&self.connection, &state.binding, thread_id, &self.path)?;
                    let (thread, following) =
                        self.thread_fragment(&state.context.id, thread_id, *after_message)?;
                    (Some(thread), None, following)
                }
                BindingPosition::Requests {
                    request_id,
                    after_attempt,
                } => {
                    require_request_context(
                        &self.connection,
                        &state.context.id,
                        request_id,
                        &self.path,
                    )?;
                    let (request, following) =
                        self.request_fragment(&state.binding, request_id, *after_attempt)?;
                    (None, Some(request), following)
                }
            };
            let size = encoded_size(&thread)? + encoded_size(&request)?;
            if bytes + size > PAGE_BYTES
                && (!state.threads.is_empty() || !state.agent_requests.is_empty())
            {
                break;
            }
            bytes += size;
            state.threads.extend(thread);
            state.agent_requests.extend(request);
            next = following;
        }
        self.relocate_threads(&state.observation.comparison, &mut state.threads)?;
        state.next_cursor = next.map(|position| BindingCursor {
            revision,
            observation_id: state.observation.id.clone(),
            position,
        });
        if encoded_size(&state)? > 512 * 1024 {
            return Err(Error::InputTooLarge {
                field: "binding page",
                limit: 512 * 1024,
            });
        }
        Ok(state)
    }

    fn first_thread_cursor(
        &self,
        context: &ReviewContextId,
        after: Option<&ThreadId>,
    ) -> Result<Option<BindingPosition>> {
        let id = self.connection.query_row(
            "SELECT id FROM threads WHERE context_id = ?1 AND (?2 IS NULL OR id > ?2) ORDER BY id LIMIT 1",
            params![context.as_str(), after.map(ThreadId::as_str)],
            |row| row.get::<_, String>(0),
        ).optional().map_err(|source| database_error("page review threads", &self.path, source))?;
        match id {
            Some(id) => Ok(Some(BindingPosition::Threads {
                thread_id: ThreadId::from_stored(id),
                after_message: 0,
            })),
            None => self.first_request_cursor(context, None),
        }
    }

    fn first_request_cursor(
        &self,
        context: &ReviewContextId,
        after: Option<&AgentRequestId>,
    ) -> Result<Option<BindingPosition>> {
        self.connection.query_row(
            "SELECT id FROM agent_requests WHERE context_id = ?1 AND ordinal > COALESCE((SELECT ordinal FROM agent_requests WHERE id = ?2), 0) ORDER BY ordinal LIMIT 1",
            params![context.as_str(), after.map(AgentRequestId::as_str)],
            |row| row.get::<_, String>(0),
        ).optional().map(|id| id.map(|id| BindingPosition::Requests { request_id: AgentRequestId::from_stored(id), after_attempt: 0 }))
            .map_err(|source| database_error("page agent requests", &self.path, source))
    }

    fn thread_fragment(
        &self,
        context: &ReviewContextId,
        id: &ThreadId,
        mut position: u32,
    ) -> Result<(Thread, Option<BindingPosition>)> {
        let mut thread = self.connection.query_row(
            "SELECT context_id, observation_id, path, side, start_line, end_line, anchor_source_id, context_json, status, created_at, updated_at, finding_json FROM threads WHERE id = ?1",
            [id.as_str()], |row| raw_thread(row, id.clone()),
        ).map_err(|source| database_error("page a review thread", &self.path, source))?;
        let mut bytes = encoded_size(&thread)?;
        for _ in 0..PAGE_ITEMS {
            let next = self.connection.query_row(
                "SELECT id, position FROM messages WHERE thread_id = ?1 AND position > ?2 ORDER BY position LIMIT 1",
                params![id.as_str(), position],
                |row| Ok((MessageId::from_stored(row.get(0)?), row.get::<_, u32>(1)?)),
            ).optional().map_err(|source| database_error("page review messages", &self.path, source))?;
            let Some((message_id, next_position)) = next else {
                return Ok((thread, self.first_thread_cursor(context, Some(id))?));
            };
            if bytes >= FRAGMENT_BYTES && !thread.messages.is_empty() {
                break;
            }
            let message = message_on(&self.connection, &message_id, &self.path)?;
            bytes += encoded_size(&message)?;
            thread.messages.push(message);
            position = next_position;
        }
        Ok((
            thread,
            Some(BindingPosition::Threads {
                thread_id: id.clone(),
                after_message: position,
            }),
        ))
    }

    fn request_fragment(
        &self,
        binding: &RuntimeBinding,
        id: &AgentRequestId,
        mut ordinal: u32,
    ) -> Result<(AgentRequest, Option<BindingPosition>)> {
        let mut request = request_without_attempts_on(&self.connection, id, &self.path)?;
        let latest = self.connection.query_row(
            "SELECT id FROM dispatch_attempts WHERE request_id = ?1 ORDER BY ordinal DESC LIMIT 1", [id.as_str()],
            |row| row.get::<_, String>(0).map(DispatchAttemptId::from_stored),
        ).map_err(|source| database_error("read latest dispatch attempt", &self.path, source))?;
        request
            .attempts
            .push(dispatch_on_any(&self.connection, &latest, &self.path)?);
        set_request_outcome(&mut request, &self.path)?;
        request.recovery = request.recovery_for(&binding.id);
        request.attempts.clear();
        let mut bytes = encoded_size(&request)?;
        for _ in 0..PAGE_ITEMS {
            let next = self.connection.query_row(
                "SELECT id, ordinal FROM dispatch_attempts WHERE request_id = ?1 AND ordinal > ?2 ORDER BY ordinal LIMIT 1",
                params![id.as_str(), ordinal],
                |row| Ok((DispatchAttemptId::from_stored(row.get(0)?), row.get::<_, u32>(1)?)),
            ).optional().map_err(|source| database_error("page dispatch attempts", &self.path, source))?;
            let Some((attempt_id, next_ordinal)) = next else {
                return Ok((
                    request,
                    self.first_request_cursor(&binding.context_id, Some(id))?,
                ));
            };
            if bytes >= FRAGMENT_BYTES && !request.attempts.is_empty() {
                break;
            }
            let attempt = dispatch_on_any(&self.connection, &attempt_id, &self.path)?;
            bytes += encoded_size(&attempt)?;
            request.attempts.push(attempt);
            ordinal = next_ordinal;
        }
        Ok((
            request,
            Some(BindingPosition::Requests {
                request_id: id.clone(),
                after_attempt: ordinal,
            }),
        ))
    }
}

fn encoded_size(value: &impl Serialize) -> Result<usize> {
    serde_json::to_vec(value)
        .map(|bytes| bytes.len())
        .map_err(Error::EncodeBindingPage)
}
