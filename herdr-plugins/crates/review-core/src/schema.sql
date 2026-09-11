CREATE TABLE checkouts (
    id INTEGER PRIMARY KEY,
    checkout_token TEXT NOT NULL UNIQUE CHECK (
        length(checkout_token) = 32 AND checkout_token NOT GLOB '*[^0-9a-f]*'
    ),
    worktree_git_dir TEXT NOT NULL CHECK (length(worktree_git_dir) > 0),
    checkout_root TEXT NOT NULL CHECK (length(checkout_root) > 0),
    is_linked_worktree INTEGER NOT NULL CHECK (is_linked_worktree IN (0, 1)),
    last_seen_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;

CREATE TABLE review_contexts (
    id TEXT PRIMARY KEY CHECK (length(id) = 32 AND id NOT GLOB '*[^0-9a-f]*'),
    title TEXT NOT NULL CHECK (length(trim(title)) > 0),
    revision INTEGER NOT NULL DEFAULT 0 CHECK (revision >= 0),
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;

CREATE TABLE external_references (
    context_id TEXT NOT NULL REFERENCES review_contexts(id) ON DELETE CASCADE,
    kind TEXT NOT NULL CHECK (length(trim(kind)) > 0),
    locator TEXT NOT NULL CHECK (length(trim(locator)) > 0),
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    PRIMARY KEY (context_id, kind, locator)
) STRICT, WITHOUT ROWID;

CREATE INDEX external_references_by_locator ON external_references(kind, locator);

CREATE TABLE observations (
    id TEXT PRIMARY KEY CHECK (length(id) = 32 AND id NOT GLOB '*[^0-9a-f]*'),
    context_id TEXT NOT NULL REFERENCES review_contexts(id) ON DELETE CASCADE,
    ordinal INTEGER NOT NULL CHECK (ordinal > 0),
    fingerprint TEXT NOT NULL CHECK (
        length(fingerprint) = 64 AND fingerprint NOT GLOB '*[^0-9a-f]*'
    ),
    base_source_kind TEXT NOT NULL CHECK (base_source_kind IN ('commit', 'index', 'working_tree')),
    base_source_oid TEXT,
    base_oid TEXT NOT NULL CHECK (length(base_oid) IN (40, 64) AND base_oid NOT GLOB '*[^0-9a-f]*'),
    target_source_kind TEXT NOT NULL CHECK (target_source_kind IN ('commit', 'index', 'working_tree')),
    target_source_oid TEXT,
    target_oid TEXT NOT NULL CHECK (length(target_oid) IN (40, 64) AND target_oid NOT GLOB '*[^0-9a-f]*'),
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    UNIQUE (context_id, ordinal),
    UNIQUE (context_id, fingerprint),
    UNIQUE (id, context_id),
    CHECK ((base_source_kind = 'commit') = (base_source_oid IS NOT NULL)),
    CHECK ((target_source_kind = 'commit') = (target_source_oid IS NOT NULL)),
    CHECK (base_source_oid IS NULL OR (length(base_source_oid) IN (40, 64) AND base_source_oid NOT GLOB '*[^0-9a-f]*')),
    CHECK (target_source_oid IS NULL OR (length(target_source_oid) IN (40, 64) AND target_source_oid NOT GLOB '*[^0-9a-f]*')),
    CHECK (base_oid != target_oid)
) STRICT;

CREATE TABLE runtime_bindings (
    id TEXT PRIMARY KEY CHECK (length(id) = 32 AND id NOT GLOB '*[^0-9a-f]*'),
    context_id TEXT NOT NULL REFERENCES review_contexts(id) ON DELETE CASCADE,
    observation_id TEXT NOT NULL,
    checkout_id INTEGER NOT NULL REFERENCES checkouts(id) ON DELETE CASCADE,
    checkout_kind TEXT NOT NULL CHECK (checkout_kind IN ('existing', 'managed_worktree')),
    herdr_server_id TEXT NOT NULL CHECK (length(trim(herdr_server_id)) > 0),
    workspace_id TEXT NOT NULL CHECK (length(trim(workspace_id)) > 0),
    pane_id TEXT CHECK (pane_id IS NULL OR length(trim(pane_id)) > 0),
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    UNIQUE (id, context_id),
    FOREIGN KEY (observation_id, context_id) REFERENCES observations(id, context_id)
) STRICT;

CREATE INDEX runtime_bindings_by_context ON runtime_bindings(context_id, updated_at);

CREATE TABLE anchor_sources (
    id INTEGER PRIMARY KEY,
    digest BLOB NOT NULL UNIQUE CHECK (length(digest) = 32),
    content TEXT NOT NULL
) STRICT;

CREATE TABLE threads (
    id TEXT PRIMARY KEY CHECK (length(id) = 32 AND id NOT GLOB '*[^0-9a-f]*'),
    context_id TEXT NOT NULL REFERENCES review_contexts(id) ON DELETE CASCADE,
    observation_id TEXT NOT NULL,
    path TEXT CHECK (length(trim(path)) > 0),
    side TEXT CHECK (side IN ('base', 'target')),
    start_line INTEGER CHECK (start_line > 0),
    end_line INTEGER CHECK (end_line >= start_line),
    anchor_source_id INTEGER REFERENCES anchor_sources(id),
    context_json TEXT,
    finding_key TEXT,
    finding_json TEXT,
    status TEXT NOT NULL CHECK (status IN ('open', 'resolved')),
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    UNIQUE (id, context_id),
    UNIQUE (context_id, finding_key),
    CHECK ((finding_key IS NULL) = (finding_json IS NULL)),
    CHECK (path IS NOT NULL OR finding_key IS NOT NULL),
    CHECK ((path IS NULL AND side IS NULL AND start_line IS NULL AND end_line IS NULL AND anchor_source_id IS NULL AND context_json IS NULL) OR
           (path IS NOT NULL AND side IS NOT NULL AND start_line IS NOT NULL AND end_line IS NOT NULL AND anchor_source_id IS NOT NULL)),
    FOREIGN KEY (observation_id, context_id) REFERENCES observations(id, context_id)
) STRICT;

CREATE INDEX threads_by_context_status ON threads(context_id, status);

CREATE TABLE messages (
    id TEXT PRIMARY KEY CHECK (length(id) = 32 AND id NOT GLOB '*[^0-9a-f]*'),
    thread_id TEXT NOT NULL REFERENCES threads(id) ON DELETE CASCADE,
    position INTEGER NOT NULL CHECK (position > 0),
    author TEXT NOT NULL CHECK (length(trim(author)) > 0),
    body TEXT NOT NULL CHECK (length(trim(body)) > 0),
    origin TEXT NOT NULL DEFAULT 'reviewer' CHECK (origin IN ('reviewer', 'agent')),
    reply_to_request TEXT REFERENCES agent_requests(id),
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    UNIQUE (thread_id, position),
    UNIQUE (thread_id, reply_to_request)
) STRICT;

CREATE TABLE agent_requests (
    id TEXT PRIMARY KEY CHECK (length(id) = 32 AND id NOT GLOB '*[^0-9a-f]*'),
    context_id TEXT NOT NULL REFERENCES review_contexts(id) ON DELETE CASCADE,
    observation_id TEXT NOT NULL,
    runtime_binding_id TEXT NOT NULL,
    ordinal INTEGER NOT NULL CHECK (ordinal > 0),
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    UNIQUE (context_id, ordinal),
    UNIQUE (id, context_id),
    FOREIGN KEY (observation_id, context_id) REFERENCES observations(id, context_id),
    FOREIGN KEY (runtime_binding_id, context_id) REFERENCES runtime_bindings(id, context_id)
) STRICT;

CREATE TABLE agent_request_threads (
    request_id TEXT NOT NULL,
    thread_id TEXT NOT NULL,
    context_id TEXT NOT NULL,
    position INTEGER NOT NULL CHECK (position > 0),
    snapshot_json TEXT NOT NULL,
    PRIMARY KEY (request_id, thread_id),
    UNIQUE (request_id, position),
    FOREIGN KEY (request_id, context_id) REFERENCES agent_requests(id, context_id) ON DELETE CASCADE,
    FOREIGN KEY (thread_id, context_id) REFERENCES threads(id, context_id) ON DELETE CASCADE
) STRICT, WITHOUT ROWID;

-- Reserving a reviewer message is permanent, including unknown deliveries.
-- An explicit retry reuses its request rather than generating another one.
CREATE TABLE agent_request_messages (
    message_id TEXT PRIMARY KEY REFERENCES messages(id),
    request_id TEXT NOT NULL REFERENCES agent_requests(id) ON DELETE CASCADE
) STRICT, WITHOUT ROWID;

CREATE TABLE dispatch_attempts (
    id TEXT PRIMARY KEY CHECK (length(id) = 32 AND id NOT GLOB '*[^0-9a-f]*'),
    request_id TEXT NOT NULL,
    context_id TEXT NOT NULL,
    runtime_binding_id TEXT NOT NULL,
    ordinal INTEGER NOT NULL CHECK (ordinal > 0),
    herdr_server_id TEXT NOT NULL CHECK (length(trim(herdr_server_id)) > 0),
    workspace_id TEXT NOT NULL CHECK (length(trim(workspace_id)) > 0),
    pane_id TEXT NOT NULL CHECK (length(trim(pane_id)) > 0),
    expected_agent_name TEXT CHECK (expected_agent_name IS NULL OR length(trim(expected_agent_name)) > 0),
    expected_agent_kind TEXT CHECK (expected_agent_kind IS NULL OR length(trim(expected_agent_kind)) > 0),
    state TEXT NOT NULL CHECK (state IN ('pending', 'dispatching', 'returned', 'blocked', 'rejected', 'unknown', 'superseded')),
    claimant TEXT,
    detail TEXT,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    claimed_at TEXT,
    claim_expires_at TEXT,
    finished_at TEXT,
    UNIQUE (request_id, ordinal),
    FOREIGN KEY (request_id, context_id) REFERENCES agent_requests(id, context_id) ON DELETE CASCADE,
    FOREIGN KEY (runtime_binding_id, context_id) REFERENCES runtime_bindings(id, context_id),
    CHECK (
        (state = 'pending' AND claimant IS NULL AND claimed_at IS NULL AND claim_expires_at IS NULL AND finished_at IS NULL AND detail IS NULL)
        OR (state = 'dispatching' AND length(trim(claimant)) > 0 AND claimed_at IS NOT NULL AND claim_expires_at IS NOT NULL AND finished_at IS NULL AND detail IS NULL)
        OR (state IN ('returned', 'blocked', 'rejected', 'unknown') AND length(trim(claimant)) > 0 AND claimed_at IS NOT NULL AND claim_expires_at IS NULL AND finished_at IS NOT NULL)
        OR (state = 'superseded' AND claimant IS NULL AND claimed_at IS NULL AND claim_expires_at IS NULL AND finished_at IS NOT NULL AND length(trim(detail)) > 0)
    ),
    CHECK (expected_agent_name IS NOT NULL OR expected_agent_kind IS NOT NULL),
    CHECK (state IN ('pending', 'dispatching', 'returned') OR length(trim(detail)) > 0)
) STRICT;

CREATE UNIQUE INDEX one_live_dispatch_per_request
    ON dispatch_attempts(request_id)
    WHERE state IN ('pending', 'dispatching');

CREATE INDEX pending_dispatches ON dispatch_attempts(state, created_at) WHERE state = 'pending';

CREATE TRIGGER dispatch_insert_revision AFTER INSERT ON dispatch_attempts BEGIN
    UPDATE review_contexts SET revision = revision + 1 WHERE id = NEW.context_id;
END;

CREATE TRIGGER dispatch_update_revision AFTER UPDATE OF state, detail, finished_at ON dispatch_attempts BEGIN
    UPDATE review_contexts SET revision = revision + 1 WHERE id = NEW.context_id;
END;
