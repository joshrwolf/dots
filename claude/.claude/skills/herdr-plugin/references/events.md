# Events, waits, and metadata

Use this reference when reacting to Herdr changes, waiting for agent or pane
state, publishing UI metadata, or considering a background process.

## Lifecycle first

Use `herdrkit::service` for long-lived background work. Startup/event hooks
ensure a singleton worker and return; the worker owns its timer and subscribes
to topology changes. OS-held locks, private control sockets, readiness checks,
launch backoff and owner/enablement checks are shared lifecycle policy.
Hooks recover a crashed service on subsequent interaction, not immediately in
an idle session. GitHub status runs one such worker per server.

## Subscription semantics

`Events::subscribe` is the exception to the one-request-per-connection rule: it
keeps the connection and reads unsolicited newline-delimited frames. Subscribe
to the coarsest set that covers the state you will re-read.

```rust
let subscriptions = Subscription::workspace_topology();
let mut events = Events::subscribe(client, &subscriptions)?;
reconcile(client)?;
while let Some(_changes) = events.changes(Duration::from_secs(60))? {
    reconcile(client)?;
}
```

An event is a wake-up, not an authoritative record. Herdr may replay a backlog,
and frames do not carry enough ordering information to distinguish old history
from a live change. Reconcile once before waiting, then re-read current state
when a batch arrives. Do not perform irreversible work once per raw frame.

`Events::changes` coalesces bursts after 250 ms of quiet and caps a batch at 2
seconds. `None` means the requested quiet interval elapsed, not failure.
`Subscription::workspace_topology()` and `Subscription::panes()` deliberately
exclude noisy focus subscriptions; read current focus from `Snapshot` instead.

Event data is required and decoded into `Change`. `EventKind::Unknown` absorbs
future event kinds, while known object IDs are normalized into `_id` fields.
Do not recreate the wire-shape branching in plugins.

## Blocking operations

Use server-side waits rather than polling:

- `Client::agent_wait` for an agent status transition;
- `Client::agent_prompt_and_wait` to submit a prompt and wait atomically;
- `Client::pane_wait_for_output` for server-side substring or regex matching.

The client gives a finite server wait its requested timeout plus 2 seconds of
transport slack. A `None` timeout is explicitly indefinite and has no socket
read deadline. Do not wrap these in the ordinary 3-second timeout or emulate
them with status polling.

An empty `until` list uses Herdr's settled-state default. `AgentWaitResult`
contains the event `kind`, pane ID, workspace ID, and resulting agent status.

## Metadata tokens

Construct metadata through `MetadataReporter`, which binds a client, source,
and retention policy. Use `retained` for last-known results or `new` with a TTL
for temporary signals:

```rust
let reporter = MetadataReporter::new(client, "herdr-ci", Duration::from_secs(90))?;
let tokens = Tokens::new().set("ci", "passing")?;
reporter.report_workspace(workspace_id, &tokens)?;
```

Rules enforced by `herdrkit`:

- TTL rounds to milliseconds, must be at least 1 ms, and may not exceed 24
  hours;
- a source may send at most 16 tokens;
- token names are 1-32 ASCII letters, digits, `_`, or `-`;
- `Tokens::clear(name)` serializes an explicit `null`; omitting a token leaves
  its old value untouched.

Use `report_workspace` or `report_pane` for sequential refreshes. Use the
`*_sequenced` variants only when refreshes can overlap and a caller-owned
monotonic sequence is needed to prevent an older result overwriting a newer
one.

Retain last-known status with its fetch timestamp; do not erase results merely
because a refresh failed. TTLs are for temporary signals, not scheduling.
Retained metadata lasts until cleared/replaced or the workspace closes; it
does not survive a server restart and must be republished from cache.

`Client::notify` returning `Ok` does not prove a notification appeared. Inspect
`Notification.shown` and `Notification.reason` when the notification is the
only failure surface.
