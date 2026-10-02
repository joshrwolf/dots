# Controller lease and explicit recovery

**Parked:** the active workflow uses normally opened Neovim instances. This work
is retained for reference and is not a dependency of navigation or review.

This is the **lease and recovery-ledger foundation**, not a running controller service. The
normal Neovim configuration still creates unmanaged endpoints. There is no
persistent launch, automatic adoption, heartbeat worker, or recovery command yet.

## Policy

A managed endpoint is constructed with one `dots.herdr.ownership` instance and a
fresh 32-character lowercase hexadecimal controller-run ID. IDs are not PIDs and
must not be reused on controller restart. Its initial generation is 1.

The lease lasts five seconds, measured with Neovim's monotonic clock. Only a
valid, timely renewal extends it. Every management/navigation request observes
expiry; no timer callback must run first, so a paused/busy editor cannot accept
an old renewal simply because its expiry callback was delayed.

Expiry or explicit release makes the editor **orphaned**:

- bridge navigation stops, including rechecks after user hooks;
- late renewal cannot revive the owner;
- buffers, undo, UI connections, listeners and the editor process are untouched;
- local interactive editing and read-only identity/ownership inspection remain
  available. Hello proves endpoint readiness, not controller authority.

This fences cooperating bridge clients. It is not authentication or a sandbox
against same-user clients issuing arbitrary native Neovim RPC.

## Wire contract

The envelope and endpoint `context` are unchanged. `src/protocol.rs` defines
these strict request/reply shapes alongside the Lua decoder.

`open` may carry `lease: {owner, generation}`. It is required on managed
endpoints and rejected on unmanaged ones. No client silently acquires a lease
by probing identity. The existing directly launched editor navigation path
continues to omit it.

Management uses `operation: {method: "owner", control: ...}`:

| Control | Fields besides `action` | Effect |
| --- | --- | --- |
| `inspect` | none | Read current ownership; unmanaged returns null. |
| `renew` | `lease`, `not_after_ms` | Extend only the matching active lease. |
| `release` | `lease`, `not_after_ms` | Make that generation orphaned; repeat is harmless. |
| `recover` | `expected`, `owner`, `not_after_ms` | Replace the matching orphaned lease with a fresh owner and incremented generation. |

`expected` has the same shape as `lease`. Generations are positive u32 values and
never wrap. Recovery requires a different owner ID, refuses live-owner takeover,
and compares both expected ID and generation. Every mutating control has the
same wall-clock request-expiry protection as navigation.

Replies have `status: "ownership"`, observed `identity`, and `ownership`:

- null for an unmanaged editor;
- otherwise `{lease, phase, remaining_ms}`, where phase is `active` or `orphaned`.

Remaining time is an observation, not a reservation. Stable failures include
`owner_required`, `owner_mismatch`, `owner_expired`, `owner_active`, and
`not_managed`. An unmanaged editor cannot be converted through recovery, and a
closed endpoint cannot be reopened through control requests.

A lost recovery acknowledgement does not permit an automatic new takeover.
Inspection can reconcile it. A replay still requires an unexpired request
deadline. Repeating the same expected/new-owner pair from the last recovery returns its
successor's **current** state without renewing it; if that successor has already
expired or been released, the replay reports orphaned. After another generation
has taken over, the old recovery conflicts. Old opens, renewals and releases also
conflict with the new generation.

## Private recovery ledger

`src/recovery.rs` owns editor policy; `herdrkit::state_file` owns the filesystem
mechanics. No persistence files are created by normal editor startup yet.

One exclusively locked slot contains `record.json`, with schema version, fixed
endpoint identity/socket, concrete Herdr server fingerprint, plugin root,
workspace/cwd launch provenance, and one of these states:

| Recorded state | Meaning | Reconciliation |
| --- | --- | --- |
| `launch_pending` | Identity and initial owner were reserved before spawn. | Matching managed endpoint/lease becomes `observed`; no reply keeps the reservation. |
| `observed` | This generation was observed, not necessarily still alive or authoritative. | Only the same identity and lease confirm it. |
| `recovery_pending` | Exact expected/new-owner pair and request deadline were saved before RPC. | Exact successor becomes `observed`; old lease or no reply retains the intent. |

There are no saved PIDs, automatic restarts, implicit ownership upgrades, or
record deletion operations. Corrupt, unsafe, and unknown-schema files are errors,
not empty slots. A different owner, generation, server fingerprint, or plugin
root stops reconciliation. Workspace/cwd are provenance, not current pane routing.
The caller must obtain fresh Herdr authority/enablement checks; comparing a scope
copied from the record is not validation against the running server.

A lost acknowledgement may mean recovery already applied. Explicit retry keeps
the same expected/new-owner pair and compares the saved intent before updating
its deadline. It cannot replace an uncertain operation with a different recovery.
Observing the old owner does not cancel pending intent: an earlier queued RPC
may still execute. An already-orphaned exact successor can still settle which
generation applied; this does not renew it or authorize further work.

The listener accepts a pre-reserved 128-bit random registration, so identity can
be persisted **before** native process creation. It refuses any existing socket
pathname, including stale files, rather than unlinking it or silently generating
a different identity. This is a bootstrap primitive, not a launched controller.

### Storage boundary

The caller supplies a trusted, already-durable parent tree. A slot is owned and
0700; records/locks are owned private regular files, without symlinks or hard-link
aliases. The stable `record.lock` uses nonblocking OS locking and must never be
removed while another process could hold it. Mutable Rust borrows also prevent
concurrent writes through the same handle. Hold the ledger across reservation,
bounded RPC and reconciliation; process exit releases its lock.

JSON reads and serialization are bounded to 64 KiB. Writes serialize first, write
and sync `record.next`, atomically rename it, then sync the directory. Lock setup
also syncs the slot's parent entry. Interrupted scratch files are not authority;
only a verified private regular scratch file may be discarded under the lock.
Existing unsafe paths are rejected, not chmodded or adopted.

A write error never authorizes the next external effect. If rename succeeded but
directory sync failed, the API reports uncertain commitment; reopen/read and
reconcile instead of assuming the write failed to publish. Durability relies on
local filesystem/OS sync semantics and the established parent tree. Fault tests
exercise interruption boundaries, not physical power-loss guarantees. These
records contain identity and intent, **not editor buffer contents**.

## What this does not authorize

The editor does not query Herdr while renewing. The future Rust controller must
bind its run ID to a concrete `Client::server_id()`, plugin registration and
exclusive ownership record. It must stop renewal/routing when that authority is
lost. Renewal alone is not evidence that a Herdr owner is still valid.

The endpoint's socket-path association remains fixed. Recovery does not silently
reassociate it with a different Herdr socket path, and an identical path does
not prove an identical Herdr server instance. Cross-instance adoption requires
explicit controller-side validation and policy.

The caller-aware Lua pane publisher rejects managed endpoints. A remote UI
needs its own verified pane binding and generation-safe advertisement; the
headless editor's inherited pane environment is not that binding.

## Process-lifetime boundary

An isolated native-process probe confirmed that Neovim can survive both normal
controller exit and termination of the controller's entire process group when
it starts in a **separate POSIX session**. Visible/hidden dirty buffers, cursor,
undo sequence and registers remained unchanged. Advancing the fixture's lease
clock orphaned the editor without affecting state or endpoint reachability.
Removing isolation made the group-kill case lose the editor, as
expected. This was a disposable, minimal-config probe—not a deployed service or
proof of a production Rust bootstrap.

The launch design must preserve these boundaries:

- Persist identity before spawning; enter the independent session **before**
  exec/startup can expose an editable endpoint. Do not merely detach a UI or
  redirect output and assume process-group independence.
- Use a fresh bootstrap process for session creation, rather than adding an
  unsafe post-fork callback to the multithreaded controller. Do not launch it
  through `process::output`, whose process-group ownership and failure cleanup
  are intentionally for bounded one-shot commands.
- Close controller input/control pipes and locks across exec; use null stdin
  and explicitly owned log files. A session boundary does not close inherited
  descriptors. A log/pipe must not keep the controller's capture pending.
- Retain non-killing child handles while the controller lives and reap exited
  children. Once bootstrap may have exec'd Neovim, readiness timeout, cancellation
  or controller teardown must not run a kill-on-drop guard. Retain unresolved
  launch intent and arrange reconciliation instead.
- On controller loss, the OS reparents surviving editors; the lease fences
  automation. Recovery uses the recorded endpoint, never a reparented/saved PID.
  The normal service owner/disable exit rules remain unchanged.

Still needed: a production bootstrap/stdio/descriptor contract, retained-child
reaping tests, and normal/crash disposition through the actual controller path.
The process probe alone does not close those gates.

## Remaining launch gates

Before connecting this to `herdrkit::service`:

1. Wire the tested ledger/reserved-listener primitives into a controller with
   a durable private parent, deliberate record enumeration, and fresh Herdr
   owner/enablement checks. Never discover or kill editors by scanning sockets
   or trusting a saved PID.
2. Keep editor process lifetime independent of controller exit/process-group
   cleanup. A controller crash must leave its editor reachable for explicit
   recovery, not unrecorded or forcibly destroyed.
3. Give recovery an explicit operator-facing action with expected-generation
   comparison; never auto-adopt after a timeout or auto-restart over an
   unreachable editor.
4. Bind one native UI to a real Herdr pane before publishing it. Revalidate
   controller authority and topology before external Herdr mutations as well as
   before editor RPC; an editor lease cannot fence mutations in another system.

This preserves live memory across controller loss. It does **not** provide
crash durability across Neovim or host death. Swap and session files are not a
complete durable editor checkpoint.

## Evidence

`test_ownership.lua` covers release, real-time and controlled-clock expiry,
renewal, competing recovery, stale-generation fencing, replay without revival,
expiry inside buffer hooks, unmanaged/closed endpoints, and expired control
requests. The clock seam is local to ownership tests, not a patched Neovim API.

Rust sends production DTOs through native Neovim RPC, validates ownership
replies, tests malformed nested controls, and checks that rejected stale
navigation leaves the editor unchanged. Existing native UI tests separately
prove that detaching a UI preserves editor state.

Ledger unit tests retain interrupted intent, reject foreign/stale observations,
and fence conflicting retries. Storage tests cover bounded I/O, unsafe paths,
interrupted writes, cross-process exclusion, and OS lock release after abrupt
process exit. A native RPC test persists the reservation before spawning Neovim,
reopens the ledger before readiness acknowledgement, and reconciles a remotely
applied recovery whose acknowledgement was not recorded. `test_listener.lua`
checks exact reserved identity and refusal to replace existing paths.
