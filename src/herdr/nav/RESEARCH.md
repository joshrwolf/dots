# Jumplist alternatives review

Reviewed 2026-09-28. No plugins were installed and no running session was
upgraded or changed during this review. Downloaded sources were read in `/tmp`.
Community implementations were inspected, not executed or benchmarked.

## Accepted scope

The user requires a dots-owned plugin without a Herdr fork or upstream changes.
The earlier native recommendation is rejected for this task. The implemented
contract is in [ARCHITECTURE.md](ARCHITECTURE.md).

## Conclusion

The previous proposal imposed a lossless, server-wide external history contract
before evaluating the existing ecosystem or the native client boundary. The
socket stress test establishes a limitation of `events.subscribe`; it does
not establish that every plugin approach must use a new journal API.

There are real community Back/Forward plugins. They use manifest hooks, which
bypass the subscription ring. None of the reviewed implementations supplies
the agreed combination of pane coverage, default-Vim branching, and robust
concurrent state ownership. They do establish a smaller plugin-only approach
for ordinary interactive use, with explicit limitations.

Use the existing Rust nav plugin and herdrkit service lifecycle on stock
Herdr 0.9.1+. Hooks wake a serialized snapshot reconciler; they are not replayed
as an ordered journal. The worker owns history, checkpoints and focus
confirmation. This avoids stale-event echo suppression and concurrent file
writers, while accepting the stock API's observation gaps and session-wide
focus. See the architecture for transient-surface and failure behavior.

## Search coverage

Consulted the [official marketplace](https://herdr.dev/plugins/), GitHub
repository searches for `topic:herdr-plugin navigation`,
`topic:herdr-plugin history`, and `herdr jumplist`, and upstream issue/PR
searches for jumplist and focus history. The navigation query returned 29
repositories; the jumplist query returned one. Inspected the two dedicated
Back/Forward implementations, a daemon-based last-focus plugin, and Navigator.
Directional-only projects and previous-tab/workspace toggles were screened
but do not implement this behavior. Search results are not proof that no
other implementation exists.

## Community implementations

### FjedorGaede/herdr-focus-history

Inspected revision `6470a30a657c0fa25c90f8c5ced4267ba5727588`:
[manifest](https://github.com/FjedorGaede/herdr-focus-history/blob/6470a30a657c0fa25c90f8c5ced4267ba5727588/herdr-plugin.toml),
[policy](https://github.com/FjedorGaede/herdr-focus-history/blob/6470a30a657c0fa25c90f8c5ced4267ba5727588/src/lib/plugin.sh),
[history](https://github.com/FjedorGaede/herdr-focus-history/blob/6470a30a657c0fa25c90f8c5ced4267ba5727588/src/lib/history.sh),
[store](https://github.com/FjedorGaede/herdr-focus-history/blob/6470a30a657c0fa25c90f8c5ced4267ba5727588/src/lib/store.sh).

Uses workspace/pane focus hooks and close hooks, two 100-entry histories, and
shell scripts. Agent history excludes ordinary terminal panes. Fresh jumps
discard the forward branch, unlike the default-Vim policy agreed here.

It saves the cursor before issuing focus, then ignores an event matching the
selected entry. This avoids a blanket suppression timer, but a failed focus
does not roll the cursor back. State is written as separate history/cursor
files without locking in the inspected code, while hook processes can overlap.
The real-server tests wait for each visit to appear before issuing the next;
they do not establish behavior under overlapping hooks or actions.

Useful ideas: small pure history policy, close pruning, and isolated-server
tests. It is not a drop-in implementation of our semantics.

### jugyo/herdr-nav-history

Inspected revision `a40939eb4762da4c1da0fe84f105c3e35221bf92`:
[implementation](https://github.com/jugyo/herdr-nav-history/blob/a40939eb4762da4c1da0fe84f105c3e35221bf92/main.js).

Uses `pane.focused` hooks and a JSON history with Back/Forward. Fresh jumps
discard the forward branch. Ignores recording for one second after a jump,
which can also discard a real user jump during that interval. Its older
focus implementation walks directional pane geometry after focusing a tab;
the current socket API already supports direct `pane.focus`. Atomic file
replacement protects a single write, but does not serialize concurrent
read/modify/write operations.

Useful as evidence that hook-based pane history is practical. Do not copy
its time suppression, geometry walk, or obsolete API assumptions.

### pedrobarco/herdr-lastfocus

Inspected revision `b179cd7f96719be626bb3fe6b7979d4edff251bd`:
[daemon](https://github.com/pedrobarco/herdr-lastfocus/blob/b179cd7f96719be626bb3fe6b7979d4edff251bd/internal/daemon/daemon.go).

One Go daemon owns recording and persists per-scope current/previous pairs.
Actions use direct socket focus. It also keeps a bounded ring, but the user
actions are two-place toggles, not Back/Forward traversal. It seeds from a
snapshot before subscribing, leaving a bootstrap gap, and infers pane/tab
association from the order of different event kinds. That ordering reflects
its subscription arrangement, not a universal causal event order. It uses
the socket stream affected by the retention/polling limitation.

Useful ownership model; it does not remove the socket stream's limitations.

### thanhdat77/herdr-navigator

Inspected revision `313b753b1f98f802b7d861430a13cb95a86e8dd3`:
[navigation and jump-back](https://github.com/thanhdat77/herdr-navigator/blob/313b753b1f98f802b7d861430a13cb95a86e8dd3/src/app.rs).

Records the departed workspace after successful local navigation performed
through Navigator. Its jump-back action toggles current/previous workspace.
This is a smaller, valid model: own the jumps you initiate instead of observing
all navigation. Extending our `find`/`nav` actions similarly would work for
those routes, but would miss mouse/native navigation unless all such routes
were redirected. It is a deliberate coverage tradeoff, not a full substitute.

## Herdr source findings

Inspected installed-version tag v0.9.0, relevant stable v0.9.1 files, and current
upstream revision `267d47fb571c0a5e771f2e31bdf6459d625d7459`.

### Hooks bypass the socket retention ring

[`App::emit_event`](https://github.com/herdrdev/herdr/blob/v0.9.1/src/app/api.rs)
runs manifest hooks before pushing the event into `EventHub`. Therefore the
previous 240-change/174-notification socket experiment cannot be applied
directly to hooks.

The [hook runner](https://github.com/herdrdev/herdr/blob/v0.9.1/src/app/api/plugins/runtime.rs)
spawns asynchronous processes and limits all in-flight plugin commands to 32.
Hook spawn/execution/arrival order is not a guaranteed event order. A mutex in
our plugin would prevent concurrent state corruption, but would not recover
the original order of hooks arriving late. A single service is an improvement,
not proof of complete recording. Hook overload can also reject launches.

### Herdr 0.9.1 fixes a material 0.9.0 blind spot

In [v0.9.0 client navigation](https://github.com/herdrdev/herdr/blob/v0.9.0/src/server/headless/client_views.rs),
the server accepts focus without publishing the normal API focus events.
[v0.9.1](https://github.com/herdrdev/herdr/blob/v0.9.1/src/server/headless/client_views.rs)
explicitly compares that client's focus before/after and emits focus events
when it changes. The
[regression test](https://github.com/herdrdev/herdr/blob/267d47fb571c0a5e771f2e31bdf6459d625d7459/src/server/headless/tests/mod.rs)
`client_local_navigation_emits_pane_focused_only_when_that_client_moves`
checks two clients selecting the same destination independently, repeated
selection, and tab closure.

This was missed by the earlier headless tests, which issued public API focus
calls instead of exercising the actual client navigation route. Any observer
plugin should require at least the server-side 0.9.1 fix. Updating only an
on-disk executable does not change an already-running server's behavior.

v0.9.1 still has the
[512-event ring](https://github.com/herdrdev/herdr/blob/v0.9.1/src/api/event_hub.rs)
and [one-match-per-poll subscription implementation](https://github.com/herdrdev/herdr/blob/v0.9.1/src/api/subscriptions.rs).
The manual-navigation fix does not establish lossless delivery.

### Native navigation already has the infrastructure we were duplicating

The client stores `previous_pane_id` in
[`ClientShellState`](https://github.com/herdrdev/herdr/blob/v0.9.1/src/client/shell/state.rs).
Its [LastPane action and pending request machinery](https://github.com/herdrdev/herdr/blob/v0.9.1/src/client/shell/actions.rs)
already resolve a destination against current state and send a pane focus.
[`EndpointCommands`](https://github.com/herdrdev/herdr/blob/v0.9.1/src/client/endpoint_commands.rs)
serializes endpoint commands and tracks request ID, endpoint generation, boot
ID, completion, cancellation, and retired responses.

These IDs do not magically make snapshots a full history log. They do mean
native command handling would be the appropriate boundary for an upstream
implementation. That alternative is outside this plugin-only task.
Capture native navigation at accepted focus transitions; a snapshot-only
observer can still miss intermediate changes.

Focus is also client-local in ways the public socket API is not. Herdr keeps
[`ClientShellLocation`](https://github.com/herdrdev/herdr/blob/v0.9.1/src/server/clients.rs)
per client, while
[public explicit focus](https://github.com/herdrdev/herdr/blob/v0.9.1/src/server/headless/client_views.rs)
can move all shell clients to the default target. A server-wide plugin list
mixes clients' visits, and its Back action can affect other clients. The
previous design treated server-wide history as a necessity; it is instead a
consequence of choosing the public plugin boundary.

Some upstream layout code/tests also use the phrase focus history for pane
selection/close fallback. That is not an exposed cross-tab jumplist with a
Back/Forward cursor. The inspected current tree still has `LastPane`, not
native jumplist actions. Upstream search found no jumplist-titled issue/PR;
this is a search result, not a guarantee that no related work exists.

## Alternatives and decision

| Approach | What it buys | Constraint |
| --- | --- | --- |
| Adapt a community hook plugin into our Rust service | No new Herdr protocol; automatic recording on fixed server versions | Hook ordering/overload, command echoes, and mixed-client history remain best-effort |
| Record only jumps routed through our actions | Small, direct implementation with known intent | Misses mouse and native routes; changes the agreed coverage |
| Poll current focus/layout | Simple observer; works around missing events | Misses brief visits and offers no causal history |
| Observe private client IPC from a plugin | Access to richer projections | Private compatibility surface; projections are not a journal; unnecessary coupling |
| Native client-scoped jumplist | Fits actual user navigation and existing focus/command ownership | Requires a Herdr change; must cover native, API-induced, and lifecycle focus transitions |
| Public revisioned journal plus conditional focus | Strong external automation primitive | Much broader API and plugin machinery than this interactive feature needs |

Decision: implement the hook-woken service in our existing nav plugin. Use
snapshot observations rather than assuming event order, and retain the pure
100-entry Vim-style history policy. Public terminal IDs preserve identity
across pane moves. Reserve before focus, confirm afterward, and never replay
an uncertain focus mutation after recovery.

Native client history and a revisioned public journal remain evaluated
alternatives outside the accepted scope. The stock plugin cannot provide
lossless recording, per-client isolation, conditional focus, or reliable modal
classification. These limits are documented rather than hidden behind a timer
or inferred from labels. Nothing in this research authorizes upgrading the
live server or publishing an upstream issue/PR.
