# Review boundaries

Local review content outlives its Herdr workspace and editor.

## Durable identity

The registry lives at `<Git common directory>/herdr-review/review.sqlite3`,
shared by linked worktrees. Checkout tokens distinguish moved checkouts from
reused paths. Runtime bindings include the concrete server and workspace;
neither path similarity nor current focus establishes ownership.

Contexts group threads. Observations pin exact ordered comparisons, including
immutable captures of index/working-tree content. Saved anchors refer to those
observations, not necessarily today's files. Git objects hold comparison content;
SQLite holds structured metadata. The plugin's repository catalog is discovery
metadata, not a second review store.

Openness is derived from live bindings, not persisted review phases. Closing a
workspace leaves review content and reusable checkouts intact. Failed preparation
also preserves the runtime; retry reconciles it rather than deleting user work.
Worktree deadlines bound the client's wait, not server-side cancellation.

Nonempty incompatible or corrupt databases fail loudly; runtime code does not
silently reset them.

## External effects

GitHub source resolution follows an explicit choice and verifies the fetched PR
head. It matches local clones rather than cloning implicitly. Local review,
thread resolution, agent delivery, code edits, and GitHub publication are
separate operations; none implies authorization for the others.

Agent requests freeze inputs and reserve reviewer messages before delivery.
Reservations are not proof of delivery. An uncertain attempt is not automatically
replayed, and agent return neither resolves threads nor proves answers were saved.
Retries retain the request and saved answers; replies are accepted against the
current expected attempt and observed caller.

Adapters use core policy rather than recreating reservation or recovery rules.
Paged reads pin revision and observation; restart a stale read without repeating
mutations. Editor IPC is private transport, not a second source of truth.

See [the enabled Neovim surface](NEOVIM.md) for current user operations and
[contributor rules](../AGENTS.md#local-review-ownership) for changes.
