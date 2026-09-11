# Review architecture

Review is a repository service with Herdr-owned runtime, provider-owned source
resolution, and Neovim-owned presentation. It has no CLI-shaped domain boundary
and no persisted open, closed, done, or phase state.

## Slow operations and recovery

Herdrkit allows ten minutes for worktree listing, creation, opening, and removal.
Set `HERDR_WORKTREE_TIMEOUT_SECS` in the Herdr server's launch environment to
override this (1–86400 seconds). Other immediate API calls retain their short
deadline. This is a client wait deadline, not cancellation of server-side work.

Materialization errors preserve the workspace, checkout, and any saved runtime
binding. The launcher never rolls back by removing
that checkout or closing its workspace/tab. Reopening the review can reuse the
preserved runtime; errors include its workspace ID and checkout path.

## Contexts and observations

Each Git repository clone has one SQLite registry at:

```text
<absolute Git common directory>/herdr-review/review.sqlite3
```

The path is inside Git's private metadata, never the worktree, so it cannot be
staged accidentally. Linked worktrees share the registry because they share a
Git common directory. A random token in each worktree's private Git directory
provides checkout identity that survives a move and changes when a pruned path
is reused.

The review plugin also keeps a small repository catalog in its Herdr-managed
plugin state directory. It contains only clone identity, a preferred checkout,
display name, and last-seen time. It never contains reviews, threads, anchors,
or runtime state, and therefore does not become a second review store. Live
Herdr workspaces refresh the catalog; catalog entries make reviews discoverable
again after their workspaces close.

A `ReviewContext` groups durable threads and may carry generic external
references. The core knows only each reference's kind and canonical locator; it
does not know GitHub concepts. An `Observation` records one exact ordered Git
comparison seen in that context. Commit sides use their object IDs; mutable
index and working-tree sides are captured as synthetic immutable commits.
Threads and agent requests retain the observation on which they were created.

Opening an external reference reuses the most recent matching context by
default. References are not unique database identities: generic tools may still
create independent contexts for the same source when the reviewer asks, and
checkout identity is never context identity.

Each runtime binding pins one observation to a checkout, concrete Herdr server,
Herdr workspace, and optional editor pane. A context may have multiple
simultaneous bindings, and multiple contexts may use one checkout. The server
identity and checkout token prevent recycled workspace IDs, path reuse, or an
unrelated workspace from being mistaken for a review runtime.

The schema is intentionally greenfield. There are no migrations or schema
versions. An empty database is initialized; a nonempty incompatible or corrupt
database fails loudly. Obsolete development databases are removed once during
development, never silently by normal runtime code.

Each editor bridge owns one store connection for its lifetime. Schema checking,
checkout registration, and orphan-ref reconciliation happen at open, not on each
interactive request. Observations and mutations remain ordered on that connection.
Agent discovery uses a separate bounded worker with no database writes; responses
may arrive out of order and are matched by request ID. Neovim starts discovery
alongside fresh review loading, shows the picker when discovery finishes, and only
queues a request after both inputs are ready and the editor still owns the same
view. Delivery independently revalidates the live agent and checkout identity.

## Source providers

Sources turn an explicit user choice or a stored external reference into a
provider-neutral plan: a label, exact comparison, and checkout strategy. The
GitHub adapter is the first source. It owns pull-request target parsing and
canonical URL construction; neither the picker nor the core understands GitHub
identity.

Picker population is completely local. It never lists pull requests, searches
GitHub, or queries for reviews awaiting the user. GitHub is contacted only after
the reviewer chooses a GitHub action or reopens a GitHub-backed context that
needs refreshing.

The shared GitHub client performs bounded explicit queries. A full pull-request
URL exposes its provider repository identity before a checkout is chosen. The
launcher matches that identity against every configured Git remote, including
fork/upstream and HTTPS/SSH layouts. It selects one matching clone, asks when
several match, or requests a local checkout when none match; it never clones
implicitly. The client then fetches only the named base and pull refs, verifies
the immutable PR head OID, and computes the merge base against the freshly
fetched base branch. The moving base tip is deliberately not compared with
earlier API metadata. The adapter emits a managed-worktree plan at that
verified head commit.

## Runtime ownership

Openness is derived by validating runtime bindings against one live Herdr
snapshot; it is never stored as context state. Closing a workspace therefore
makes its binding inactive without another command or cleanup step. Durable
contexts, observations, threads, requests, and reusable checkouts remain
available to reopen later.

`prefix+r` opens one Herdr-global review picker from any workspace, including a
workspace whose current directory is not a Git checkout:

- active reviews identify their repository and Herdr workspace, then focus the
  workspace without changing its tab layout;
- recent reviews remain grouped by repository and refresh from a supported
  source or reopen a valid checkout;
- Current branch pull request resolves only that branch's PR;
- Open GitHub pull request accepts a number for the current repository or a
  globally routable URL for any known local clone.

Starting a PR review creates its dedicated managed Herdr worktree/workspace.
It is not nested under the caller and there is no universal review workspace.
Closing that workspace is the entire close operation: openness is derived from
the live Herdr snapshot, while durable review content remains available in the
repository registry. Each exact source observation has a deterministic
`review-pr-<number>-<short-identity>` branch. Herdr can therefore reopen the same checkout after its
workspace closes, and a retry can reconcile a completed worktree creation even
when the original socket response was lost. Worktree creation uses a bounded
long-operation deadline rather than the immediate control-plane deadline.

Ordinary CodeDiff tabs need no picker action and remain ephemeral until the
reviewer saves a thread. The first saved thread atomically creates its context,
immutable observation, runtime binding, initial message, and anchor source.
If the same editor/workspace later has no in-memory binding, the exact checkout
and operational comparison resume that binding instead of creating duplicate
local review lineage.

One generic materializer handles both existing checkouts and managed
worktrees. It records the observation and binds the workspace before returning
to an ordinary shell. Failed preparation preserves the runtime for recovery.
The plugin owns the picker popup, not editor tabs. Starting agents and editors
is the reviewer's choice; no comparison or binding is injected into editor
environment variables.

## Agent surface and shared skill

`herdr-review --agent <checkout>` accepts one bounded JSON request and returns
one response, using the same context/finding handlers as the editor bridge.
It exposes only `context.load`, `finding.save`, and `thread.reply`; it cannot
publish GitHub feedback or send agent prompts. The explicit checkout must match the calling
Herdr workspace. Lookup selects bindings by checkout identity, server identity,
and workspace, and rejects ambiguity rather than guessing from focus or branch.

`context.load` returns the binding state, repository paths, current HEAD and
working-tree status. State pages carry an opaque continuation cursor; callers
merge repeated thread/request fragments by message/attempt ID. The cursor pins
the context's mutation revision and observation, and concurrent changes require
restarting the read rather than mixing snapshots. Each page is read in one
SQLite snapshot. PR references and the captured comparison come from the
saved context. GitHub freshness is a separate explicit query, never an implicit
checkout update. `finding.save` requires the loaded binding and observation IDs
and captures primary/related locations from that immutable comparison.

Findings are ordinary threads with finding metadata and agent authorship.
A null anchor means General; it has no fabricated code position. Stable
context-scoped finding keys update the same thread without discarding later
discussion or resolution. Findings and reviewer-created threads are the same
local conversation primitive: only new reviewer messages enter dispatch batches.
The existing `pr-review` skill is shared by Claude and Codex and routes review,
orientation and discussion according to the user's prompt. It documents the
agent surface. GitHub publication is a separate explicit user instruction, not
a state or stage of the local conversation.

## Neovim surface

Neovim is an adapter over the repository registry. It starts a private
`herdr-review --stdio <checkout>` child and uses newline-framed JSON only as
process IPC. `:ReviewOpen` explicitly opens the saved comparison in CodeDiff;
opening a workspace never launches it automatically. `<leader>fr` discovers the
bound context and lists reviewer comments, agent findings and General threads.
SQLite is the structured metadata store; immutable comparison
content lives in repository-local Git objects and private refs. Every bridge
operation carries the immutable binding ID supplied by Herdr or selected at
the comparison surface. The bridge exposes exact context, thread, request,
agent, and dispatch operations; it does not start, complete, open, or close
review runtime.

CodeDiff supplies comparison and file-side context. Durable anchors contain the
original side, path, range, nearby context, and a content-addressed full source
snapshot. Refresh resolves that immutable anchor against the current file,
including insertions, edits, deletion, ambiguity, and Git renames. Neovim
extmarks are only the live-buffer projection: their explicit gravity moves
range highlights and navigation with edits, and a refresh recreates them from
the durable anchor.

Threads are always rendered inline in visible review files. The gutter marks
the full range, focused threads expand beside their anchor, the CodeDiff file
tree shows per-file counts, `[r` and `]r` navigate unresolved threads, and
`<leader>fr` opens their Snacks picker.

Review-local bindings are:

- `<leader>ra`: add a thread at the cursor or visual range
- `<leader>rp`: reply to an open thread at the cursor
- `<leader>rr`: resolve the thread at the cursor
- `<leader>rS`: send new reviewer messages in open threads to a Herdr agent
- `<leader>rR`: refresh persisted state
- `<leader>rs`: show activity and dispatch status
- `[r` / `]r`: previous or next unresolved thread
- `<leader>fr`: find unresolved threads

The equivalent commands are `:ReviewThread`, `:ReviewReply`, `:ReviewResolve`,
`:ReviewSend`, `:ReviewRetryDispatch`, `:ReviewRefresh`, `:ReviewStatus`, and
`:ReviewThreads`.

## Agent dispatch

The stdio bridge owns one lazy dispatch supervisor per binding because it
already has the editor lifetime needed for dispatch. SQLite is the durable
queue; an in-process channel only wakes a binding-scoped scan early. Claims
have leases and heartbeats, expired claims become `unknown`, and duplicate
workers cannot claim one attempt. Before the external prompt side effect,
dispatch revalidates the concrete Herdr server, exact bound workspace and
checkout token, pane occupant, stable agent identity, and eligibility.

The core prepares a bounded request projection before reserving any messages.
It retains the new reviewer messages, finding metadata, and anchor context,
with explicit limits on earlier history. A request that cannot fit is rejected
before reservation, so its messages remain available for a smaller request.
History reads are bounded too: a growing context must not produce an unbounded
bridge response.

An immutable agent request reserves exact reviewer message IDs and stores its
prepared thread snapshots. A unique message reservation prevents repeat sends,
including uncertain delivery; reservation is not proof of delivery. Messages
added later remain for the next request. Agent-origin messages are context,
never new outgoing messages. Each dispatch attempt records the runtime binding
and agent assignment that own its delivery. A later binding may safely
supersede an unclaimed attempt or retry a terminal or expired one, but cannot
steal a live claim.

Dispatch outcome and saved answers are separate facts. `blocked`, `rejected`,
`unknown`, and `returned` remain distinct dispatch outcomes. Agent return does
not resolve threads or imply that every answer was saved. Recovery selects an
outstanding request explicitly, including an older request or a returned turn
with missing answers, and appends a new dispatch attempt. Retries retain the
original request and new-message IDs, include answers already saved for it,
and ask only for missing answers. They never make reserved messages new again.

The agent-only `thread.reply` operation carries the expected dispatch attempt
ID. The bridge validates the observed caller's workspace, pane, and agent; the
store atomically validates the attempt and assignment before accepting one
answer per request/thread. An old historical binding does not authorize a
reply after reassignment. Identical saves are idempotent; conflicting duplicates
are rejected with a typed error. The prompt identifies new messages separately
from history and includes finding evidence, severity, and related locations.
Neither history nor findings authorize edits or GitHub posting unless the
reviewer asks.

Neovim refreshes binding state independently of dispatch activity, so findings
and replies saved by another process become visible even when no request is
running. Conditional refresh checks the binding revision first, avoiding history
reads and anchor resolution when persisted state is unchanged. Lease heartbeats
do not invalidate an otherwise unchanged history scan. Dispatch notifications
report saved replies separately from the agent's
turn status; editor freshness does not depend on those notifications.

Moving dispatch supervision into a workspace-owned resident process is a
possible later seam. It does not change the durable primitives or the Neovim
protocol.
