# Workspace toggle and pane jumplist

Status: plugin-only implementation contract. Supersedes the journal and native
proposals. No Herdr fork, private IPC, new API, or upstream change is required.

## Behavior

herdr-nav.previous-workspace (Cmd+O) keeps a current/previous workspace pair,
independent of the pane jumplist cursor. Every observed workspace change,
including one caused by pane traversal, updates this pair. Same-workspace tab
and split changes do not. Repeated toggles alternate the pair. Visiting C from
B makes B the alternate even if A was previously the alternate. Closed
workspaces are pruned; no alternate means no operation.

Use the public workspace_id and workspace.focus operation, letting Herdr keep
each workspace's selected tab and pane. Never derive workspace identity from
pane IDs or select a historical pane when toggling a workspace.

herdr-nav.back (Ctrl+S then lowercase O) and herdr-nav.forward (Cmd+I) traverse a shared
per-server list of at most 100 terminal destinations, across panes, tabs and
workspaces. Ends do not wrap. Traversal changes only the cursor. Ordinary
observed S -> T removes S and T, appends S then T, and selects T. Thus
A,B,(C) -> Back -> A,(B),C -> visit D -> A,C,B,(D): forward destinations
survive, as in Vim's default jumplist. Repeating the current destination is a
no-op. Neovim owns navigation within its own terminal.

Identity is the public terminal_id, resolved to the current pane_id in each
fresh session.snapshot. This follows workspace moves without parsing opaque
pane IDs. Missing terminals are pruned. Restarts of the same plugin worker
restore its checkpoint; a new Herdr server gets a separate history.

## Stock Herdr boundary

Require stock Herdr 0.9.1 or newer for its native focus-event fix. Startup,
workspace focus, pane focus/creation/closure/move and tab/workspace closure hooks wake the
service. A one-second reconciliation repairs missed hooks and lifecycle
changes. Every command also reconciles immediately before choosing a target.

Hooks are notifications, never an ordered event journal: payloads may arrive
late, out of order or not at all. Always read current session.snapshot. This
records observed visits, not every intermediate focus under arbitrary load.
Delayed traversal hooks cannot rewind history because they read current focus;
no time-based suppression or pending-event matching is used.

Public focus is session-wide. Multiple clients share history and a jump can
move other clients. Neither per-client isolation nor atomic compare-and-focus
is available. A concurrent external change can race the last read and focus
call. Read back afterward; if another destination won, record it and report
the interrupted traversal rather than claim success.

Popups are absent from public inventory and never become entries. Herdr does
not expose modal state or reliable overlay placement in snapshots. Therefore
there is no SurfaceBusy promise or overlay classification: an overlay exposed
as a pane participates until it closes and is pruned. Native modal key handling
determines whether a key invokes the plugin. Persistent plugin panes participate
normally. No label/process heuristics. These are explicit API constraints, not
TODOs or reasons to patch Herdr.

## Ownership and execution

Use herdrkit::service: one server-scoped worker, OS lock, readiness, bounded
messages, launch cooldown, owner/enablement checks. The control handler stays
responsive. A dedicated thread owns history and serializes socket/checkpoint
I/O. Wake requests coalesce; traversal requests enter a bounded FIFO with a
five-second start deadline. Reject overload and expired requests without
focusing. Actions receive tickets and poll for completion with a bounded wait.
Never retry focus after an ambiguous transport failure. Rapid presses resolve
their target when processed, not from stale invocation context.

Persist a versioned checkpoint with herdrkit::state_file. Validate capacity,
uniqueness, cursor and pending target before use. Before focus, durably reserve
the target while keeping the old cursor. A successful focus response plus a
confirming snapshot commits the cursor. Rejection clears the reservation;
transport/readback uncertainty keeps it for reconciliation. On restart a fresh
snapshot resolves it: target current means commit traversal; otherwise record
actual focus. Never replay a reserved focus request. Checkpoint errors halt
mutations until the file can be reread and reconciled.

The same checkpoint stores the workspace pair and its pending target. A
workspace switch reserves the alternate before focus and confirms via a fresh
snapshot. Ordinary observation updates the pair, so confirmed toggles need no
special suppression. An uncertain request is never replayed. Version-one pane
checkpoints gain an empty workspace pair on first read without changing pane
history. Upgrade the plugin worker before sending the new control requests;
the Herdr server does not need to restart.

Directional ctrl+hjkl stays on its existing foreground-process-aware fast
path; history observes the result through the same hooks. No second daemon,
CLI subprocess per key, raw socket code in nav, or competing subscription.

## Validation

Pure tests: branching, both directions, boundaries, duplicates, closure,
capacity and persisted-state validation. Scripted Herdr tests: confirmation,
rejection, ambiguous failure, recovery, moved identities and stale wake-ups.
Worker tests: ordering and admission bounds. Contract tests: manifest/dispatch
and bindings. An opt-in integration test runs the actual plugin against an
isolated stock server with neutral temporary data, including hooks, rapid
traversal, closure, restart and plugin surfaces. Run make herdr-check.
Never install or reload the live server as a validation step.
