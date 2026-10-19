# Dynamic picker sources

Providers supply remote results without making the picker own provider policy.

## Constraints

Provider construction must be inert. Exclusive scopes activate only through
their query sigil, not ordinary Tab navigation. Provider-ranked groups preserve
server ordering because matches may depend on fields absent from visible rows.

The picker owns query generations and rejects stale deliveries, including after
scope exit/reentry. Providers return complete bounded batches with stable IDs;
validation precedes replacement. Loading/error notices are inert chrome, while
actions are typed entries or shortcuts.

Providers own debounce, cache freshness, page size, and rate policy. Futures must
be nonblocking and cancellation-safe, with bounded bodies and no unmanaged tasks.
Do not use `spawn_blocking` on the picker runtime: shutdown waits for blocking
jobs. Dropping a future cannot revoke a request already received by a server.

Shutdown cancels providers and restores terminal modes before returning to the
caller. Provider panics terminate the picker because Ratatui's panic hook changes
global terminal state; they cannot masquerade as ordinary source errors.

See [the runnable example](../../examples/dynamic_picker.rs) and public Rust
types for construction, shortcut bindings, and terminal-free model tests.
