# Editor integration and parked ownership work

The active target is navigation and review in Neovim instances opened by the
user. Registration/native RPC are the connection layer; review data belongs in
`review-core`. See [the configured review surface](../review/NEOVIM.md).

Persistent editors, controller leases/recovery ledgers and managed remote UIs
are **parked**, not prerequisites for review integration. Their implemented
boundaries and remaining gates are retained below for reference. There is no
persistent editor service.

## Three distinct lifetimes

1. **Editor endpoint**: one Neovim process, its private listener, random
   registration, and fixed Herdr socket-path association. It owns buffers,
   undo history, registers, windows and tabs. `dots.herdr.endpoint` serves local
   identity independently of metadata publication. Only its owner closes it.
2. **UI attachment**: a native remote-UI client and the corresponding Neovim UI
   channel. Attaching or detaching a UI does not create or destroy the editor.
   Managed attachment ownership is not implemented yet.
3. **Pane advertisement**: metadata telling Herdr callers where an editor is
   presented. `dots.herdr.attachment` currently publishes from the directly
   running editor's caller context. Its failure or removal does not revoke the
   editor endpoint. It is not yet a remote-UI attachment manager.

For a directly launched editor, the composition root owns both endpoint and
advertisement. On exit it revokes/closes the endpoint before metadata cleanup.
For a future managed headless editor, the editor endpoint must outlive every UI
attachment and advertisement. A headless editor must not use its own inherited
pane environment to advertise a remote UI's location.

The endpoint's `server` field is a socket **path**, not proof of a concrete Herdr
server instance. The future Rust controller must additionally bind ownership to
`Client::server_id()` and the plugin registration, as `herdrkit::service` does.
A replacement Herdr server at the same path is not authority to adopt an editor.

## What the native lifecycle tests establish

`tests/lua/test_ui.lua` runs real headless Neovim and native `--remote-ui` clients
in disposable PTYs. It does not mock UI APIs or operate the live Herdr session.

- Unexpected client termination leaves the editor running.
- Native `:detach` also leaves it running and permits reattachment.
- Visible and hidden unsaved buffers, cursor, undo sequence, registers, process
  identity and listener survive these transitions.
- Detaching through one UI leaves a second UI connected.
- Multiple UIs share editor focus; they are not independent file views.
- Ordinary quit refuses a dirty current buffer. Explicit `:qall!` destroys the
  editor, not just its UI. Never implement hide using quit or forced shutdown.

These are minimal-config checks on the installed Neovim version, not proof that
all user plugins tolerate headless startup or reattachment. No reliance on new
`:detach!` behavior or internal `nvim__*` APIs is needed for this headless model.

## Editor-side controller loss is fenced

[The ownership contract](OWNERSHIP.md) now implements a five-second monotonic
controller lease, orphaned state, generation-fenced navigation, and explicit
compare-and-recover. Expiry never destroys editor state. Renewals cannot revive
expired owners, and replaying recovery never extends the successor's lease.
This is opt-in endpoint behavior, not a launched controller or durable registry.

## Persistent ownership is still gated

### Controller loss must not become data loss

`herdrkit::service` is suitable for the Rust controller: singleton lock,
server/plugin identity, bounded readiness, cooldown and owner checks. It exits
on disablement, owner replacement, or prolonged owner lookup failure. It has no
editor-state disposition protocol. Do not weaken those service ownership rules
or place a dirty editor behind an unconditional kill-on-drop child guard.

Before launching persistent editors, implement and test an explicit disposition
and recovery contract for normal controller shutdown **and** abrupt controller
crash. Required properties:

- No timeout, missing health reply, closed pane, or missing Herdr server proves
  an editor is clean. None authorizes force-quitting it.
- A departing controller stops routing/publication. Preserving an editor must
  not leave a stale controller authoritative against a replacement server.
- Surviving editors need a private recovery record and a reachable control path;
  they must not become invisible abandoned processes. Record identity before
  exposing a newly launched editor, and reconcile interrupted launches.
- Recovery verifies endpoint identity. A saved PID or a socket found by scanning
  is not adoption or kill authority. Do not auto-restart over an unreachable
  editor whose ownership is unresolved.
- Preserved in-memory state is not crash durability. Swap files and `mksession`
  are not a complete checkpoint of a live editor. Do not advertise recovery from
  editor/host death without a separately tested persistence design.

The editor-side disposition is now preserve-and-fence. Private recovery records
and reserved listener identity are implemented and tested (see [OWNERSHIP.md](OWNERSHIP.md)).
The controller/launch side remains gated on wiring those primitives to durable
record discovery, fresh Herdr authority checks, independent process lifetime and
explicit operator adoption. Lease expiry is not durable storage or proof of a
concrete Herdr owner.

### A UI channel must be bound to a pane deliberately

Start with **one managed UI per editor**, not independent per-pane views.
A second live attachment should be rejected, not steal focus or silently detach
another UI. Handoff can be added once the simpler lifecycle is sound.

The attachment contract must identify the launched client, observed Neovim UI
channel, concrete Herdr owner and current pane, with an attachment generation.
Prove that correspondence before publishing metadata. Native `:detach` knows
its invoking UI; a separate control RPC must not guess the intended UI.

Pane movement changes routing, not editor identity. Reconcile current topology;
never use inherited pane IDs as ongoing authority. Late callbacks from a retired
attachment must not clear or replace its successor's advertisement. The current
caller-aware read-then-clear metadata adapter is not an atomic lease protocol
and must not be reused unchanged for concurrent UI handoffs.

### Close is not hide

Hide removes a verified UI attachment and its advertisement while keeping the
editor available to its owner. Close is a separate, explicit editor operation.
A clean-state observation is not a reservation: edits can arrive after it.
Any future close operation must recheck inside Neovim immediately before acting
and report refusal or uncertainty without an automatic force/retry fallback.

## Parked persistent-editor implementation order

1. **Done:** separate local endpoint readiness from pane advertisement; make
   native UI lifecycle assumptions executable tests.
2. **Lease/ledger contracts done:** tested expiry, explicit recovery, durable
   intent and reserved identity across interrupted launch/recovery. Next prove
   independent process lifetime and wire controller authority/discoverability.
   Extend shared service primitives only for a demonstrated need.
3. Bind a single UI attachment to its real Herdr pane, then publish/revoke its
   advertisement with generation-safe ownership.
4. Add persistent-owner launch/readiness and explicit show/hide/close operations.
5. Only then implement sidebar layout through typed Herdr operations. Never use
   `layout.apply` to rearrange a live editor: it replaces panes. Review state
   remains in `review-core`, independent of these lifetimes.
