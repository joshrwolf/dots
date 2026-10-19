# Dynamic picker sources

## Shortcut actions

`Picker::run_with_shortcuts(vec![Shortcut::control('r', "review URL", action)])`
returns `Selection::Entry`, `Selection::Shortcut`, or `Selection::Cancelled`.
Shortcut values need not implement `Entry`: actions are exits from the list,
not fake destinations. They work in every scope, including with zero matches.
The footer derives its key hint from the binding. Reserved editing/navigation
keys and duplicate bindings are rejected before entering terminal mode.
The picker stops source workers and restores terminal modes before returning,
so the caller can safely show another interactive prompt in the same popup.

## Sources

`Picker::source(group, NonZeroUsize, provider)` attaches one `Source<T>` to a
`Group::exclusive(sigil)`. The provider constructor must be inert. The picker
calls `query` only when the query actually starts with that sigil; ordinary
Tab navigation never enters an exclusive scope. `Picker::query` supplies a
sanitized initial query when reopening a popup after an action.

`Group::scoped` controls visibility for local rows and retains ordinary Tab
navigation. `Group::matching(Matching::ProviderRanked)` preserves remote order
and disables local filtering and highlighting. This matters when a server
matched information absent from the row's visible cells.

A source returns a complete `SourceResult<T>`: typed `DynamicEntry` rows and a
`SourceStatus`. Stable row IDs preserve selection during replacement. The model
validates the complete batch (limit, group, IDs and cell arity) before changing
anything. Loading, notice, empty and error feedback is inert chrome. `Notice`
explains ready results (for example a page limit); `Empty` describes absence of
domain results even when connection actions remain. Actionable states
such as connecting an account are ordinary typed entries supplied by a plugin.
The provider owns debounce, cache freshness, page size and rate policy.

The terminal owns query generations. Every edit and scope exit invalidates the
previous generation, including leaving and returning to identical query text.
Old rows are cleared when starting a new query so they cannot be accepted for
the wrong query. A watch slot coalesces pending edits; there is no task or thread
per keystroke. One runtime thread hosts one task per provider, and the bounded
result queue has capacity equal to the number of providers. Oversized batches
are rejected before entering that queue. Model validation remains independent.

The event loop polls at 25 ms while idle and redraws only for input or applied
updates. It processes bounded updates per iteration. Popup shutdown closes all
request slots, drops in-flight futures, drains task shutdown, joins the runtime
thread and restores terminal modes on all outcomes. Provider panics terminate
the picker with an error; they cannot be treated as ordinary source errors
because Ratatui's panic hook restores terminal state globally.

Providers must use nonblocking, cancellation-safe futures and bounded response
bodies. Native blocking operations need a separately owned bounded adapter;
`spawn_blocking` on this runtime is unsuitable because runtime shutdown waits
for blocking jobs. Dropping a future cannot revoke a request already received
by a server. Providers must not perform uncancellable blocking work or detach
unmanaged tasks, and destructors must not block or panic.

`Model::scoped_query`, `replace_group`, and `set_status` support terminal-free
provider/model tests. Internal lifecycle tests exercise cancellation, coalescing,
stale delivery and scope reentry. `examples/dynamic_picker.rs` is a complete
runnable provider with cancellable simulated latency and Unicode result rows.
