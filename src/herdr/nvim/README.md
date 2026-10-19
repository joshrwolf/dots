# Herdr editor connection

The file picker and clicked-file action open targets in an explicitly registered
Neovim in the invocation's tab. They no longer inspect processes, guess Neovim's
socket names, or send cursor keystrokes. This is the connection foundation, not
sidebar/daemon management or another annotation system.

## Registration

`home/nvim/.config/nvim/lua/dots/herdr.lua` is loaded explicitly by our Neovim config.
Outside Herdr it does nothing. A UI-attached, non-nested editor starts a dedicated
Neovim listener and asynchronously publishes a random 32-character `nvim_editor`
metadata token. Headless processes and editors inheriting `NVIM` do not register.
Set `DOTS_HERDR_EDITOR=0` for transient `$EDITOR` invocations, or set
`vim.g.dots_herdr_editor = false` before setup. A standalone TUI cannot reliably
be classified as transient automatically; its launcher must opt out.
`:HerdrEditorRegister` retries publication after a startup failure. Exit cleanup
is best effort; stale registrations are expected and never prove liveness.

The listener is `/tmp/dots-nvim-<uid>/<token>.sock`, inside an owned, non-symlink,
0700 directory. Both halves verify directory ownership and permissions. The
short identifier fits Herdr's display-metadata limits; socket paths never pass
through metadata's normalization/truncation. Existing Neovim listeners remain
untouched. The per-user directory may remain empty after exit; crashed editors
may leave socket files, which are not discovered by directory scanning.

The caller's registered editor takes priority. Otherwise exactly one registered
editor must exist in the tab. Multiple registrations fail explicitly, including
stale markers: invoke from the intended editor or clear stale metadata rather
than serially probing potentially suspended editors. Navigation needs one RPC,
not a handshake followed by an open. An
unreachable caller registration does not fall back to a sibling. A future picker
can add explicit selection without changing the connection contract.

## Ownership

- `src/protocol.rs`: Rust wire DTOs and stable error codes.
- `src/recovery.rs`: private launch/recovery intent and exact-identity reconciliation.
- `herdrkit::state_file`: bounded, locked, atomically replaced private JSON records.
- `src/remote.rs`: discovery, target selection and revalidation policy.
- `src/transport.rs`: bounded subprocess/RPC encoding, with a fixed Lua entrypoint.
- `dots/herdr.lua`: Neovim composition and commands, not protocol or lifecycle logic.
- `dots/herdr/protocol.lua`: strict request decoding, LuaLS types and coded errors.
- `dots/herdr/endpoint.lua`: local editor identity and endpoint revocation.
- `dots/herdr/ownership.lua`: opt-in controller leases, orphaning and recovery.
- `dots/herdr/attachment.lua`: pane advertisement lifecycle, not editor ownership.
- `dots/herdr/host.lua`: the narrow Herdr CLI adapter; the only Lua code that
  knows Herdr response shapes. It can be replaced without changing editor logic.
- `dots/herdr/listener.lua`: private local Neovim listener ownership.
- `dots/herdr/editor.lua`: buffer/window operations and partial presentation.
- `dots/herdr/bridge.lua`: dispatch between these boundaries.

The Lua adapter still uses Herdr's bounded CLI calls. It is not a second raw
socket implementation. Moving that adapter behind the typed Rust client later
must not add another plugin subprocess to every navigation operation. The adapter
runs only for registration/cleanup; hello and open perform no Herdr calls.

## Protocol 1

Both halves ship together; there are no compatibility shims. `hello` and `open` are bounded JSON requests carried
as data in native Neovim `luaeval` RPC, using one `nvim --remote-expr` subprocess
per operation. `HERDR_NVIM_BIN` selects that executable. There is no shell/Ex
interpolation of paths and no custom MessagePack implementation.

Requests have exactly `protocol`, `context`, and `operation`. `context` contains
`registration` and `server`, independent of pane/tab/workspace lifetime.
The common operations are `{method: "hello"}` and
`{method: "open", not_after_ms, target: {path, line, column}}` for a directly
launched editor. Managed opens additionally require a current `lease` fence.
The strict `owner` control operation and `ownership` reply are documented in
[the ownership contract](OWNERSHIP.md); normal startup remains unmanaged.
Unknown fields and operations are rejected. A random per-process registration
identifies the editor; replies also report its observed PID. Rust selects and
re-reads the pane immediately before delivery; Lua checks its local registration
and server association without blocking I/O. Moves after the final Herdr read
cannot be atomically excluded across the two systems. Inherited pane coordinates
never become editor identity. The connection is trusted local IPC, not a sandbox
against same-user processes.

`not_after_ms` is a same-host Unix epoch deadline. Rust allows two seconds; Lua
rejects expired requests before buffer work and rechecks before presentation and
cursor positioning. A request queued while Neovim is suspended cannot navigate
later just because its transport client has already timed out. Wall-clock
adjustments and arbitrary running autocommands are not a transactional boundary.

`open` uses absolute UTF-8 paths and one-based line/byte-column coordinates. It
requires an existing regular file and valid position. It prefers the current
ordinary nonfloating window, then the alternate/another ordinary window in the
current Neovim tab. Oil/help/quickfix/terminal/floating focus does not prevent
navigation when a suitable window exists; no suitable window is a rejection.
Opened buffers are listed. Unsaved buffers are preserved when `hidden`/`bufhidden`
permits that; otherwise navigation refuses to abandon them.

A scoped `SwapExists` handler chooses quit, never recovery, deletion or editing
anyway. Conflicts are remembered even if Neovim leaves a loaded buffer: repeated
opens remain `swap_conflict`. Resolve the conflict manually and wipe/reopen the
buffer before retrying. BufRead/BufEnter hooks still run, so this is not a
transaction. Replies distinguish:

- `ready`: verified endpoint identity, not proof of an active controller lease;
- `ownership`: observed managed lease state (or null for an unmanaged editor);
- `opened`: verified identity plus observed buffer name and cursor, checked by Rust;
- `rejected`: request validation/preparation failed;
- `partial`: presentation began but did not complete.

Failures carry `error: {code, message}`. Codes are the `ErrorCode` enum in
`protocol.rs`; messages are diagnostics, never a compatibility or testing API.
A pane advertisement is ready only after publication completes. The editor
endpoint is independently ready once listening, including while unadvertised.
Withdrawing an advertisement invalidates its pending callbacks, settles a report
already started, and clears its token; it does not close the editor endpoint.
The directly launched editor's composition root revokes/closes its endpoint on
exit before metadata cleanup, even if that cleanup fails.

Loading a buffer may itself run user autocommands, even for a rejected request.
No failure automatically retries. A timeout or lost reply means delivery is
uncertain, not that nothing happened. Focus uses `pane.focus` directly and is
best effort after an acknowledged open.

The Rust subprocess deadline is three seconds, request limit 16 KiB, and output
limit 64 KiB per stream. `herdrkit::process` drains both streams, bounds capture,
and kills the process group and reaps the child on failure. Lua's Herdr calls
have 1.5-second deadlines but are absent from navigation. Slow autocommands can
still produce an uncertain/partial open; they cannot be cancelled or rolled back.
Inspect the editor before retrying.

The process helper never signals a group after reaping its leader. It drains
pipes before reaping so timeout/limit cleanup still owns the PID. Successful
commands are not supervisors for detached descendants that closed both pipes.
Older bounded runners in `github-client/src/lib.rs` and
`review-core/src/repository.rs` remain a separate consolidation task: migrate
their caller-specific limits/errors and cleanup tests, rather than copying their
process handling into new consumers.

## Tests and activation

Run `mise run //src/herdr:deps` once to fetch the exact mini.test revision locked in
`tests/nvim/nvim-pack-lock.json`. Neovim's native `vim.pack` owns installation
under ignored `target/test-deps`, isolated from your editor's config and data.
`mise run rust-check` verifies the pinned checkout offline (excluding
Neovim's generated help index), then runs Rust tests, the mini.test behavior suite, and independent
cross-language contract tests. Checks never fetch dependencies.

Neovim with `vim.system`, `vim.uv`, and `--remote-ui` (for UI eligibility tests)
is required. Tests do not contact a live
Herdr server or load the user's editor configuration. See
[the testing guide](tests/README.md) for isolation, ownership and focused runs.

Run `mise run //src/herdr:install` to build, restow, and link; then restart Neovim to load both
matching halves. Never activate a live session merely to validate an edit.

## Next layers

See [the ownership foundation](ARCHITECTURE.md) for the implemented lifetime
boundaries, native UI tests, and prerequisites before launching a daemon.
Persistent headless-editor ownership and remote-UI attachments should build on
this request/acknowledgement boundary. They still need explicit daemon and
attachment lifecycle policy. Editor identity is now independent of routing, but
registration metadata still points at a live pane; a detached daemon does not
pretend to have one. Automatic headless registration remains disabled.

Add typed layout operations only when implementing the layout controller.
Herdr 0.9 `layout.apply` replaces panes: it must not be used to rearrange a live
editor. A full-height sidebar needs an interruption-safe pane-move plan (or a new
upstream primitive), based on `layout.export`, not reconstructed rectangles.
Review/annotation state remains the responsibility of the existing review stack.
