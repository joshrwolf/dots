---
name: herdr-plugin
description: Build or change the Rust plugins, shared herdrkit crate, manifests, or Herdr-facing config in this dots repository. Load for work under herdr-plugins, herdr-plugin.toml files, plugin actions or panes, Herdr keybindings, metadata, events, or socket API behavior. Use the separate herdr skill when the task is only to operate a running Herdr session.
---

# Herdr plugin development

Extend Herdr through the unified Rust workspace in `herdr-plugins/`. The local
[`herdr-plugins/CLAUDE.md`](../../../../herdr-plugins/CLAUDE.md) is the design
constitution; read it before editing. Treat the current workspace source and
`herdr api schema --json` as authoritative when this skill and the code differ.

The separate `herdr` skill is for controlling a live session. Load it as well
only when the requested work requires inspecting or operating Herdr, not merely
because plugin code talks to Herdr.

## Route the task

- Read [references/manifest.md](references/manifest.md) for a manifest,
  entrypoint, link handler, event hook, keybinding, or injected environment.
- Read [references/cargo.md](references/cargo.md) for workspace membership,
  dependencies, build staging, linking, or validation.
- Read [references/picker.md](references/picker.md) when a plugin presents an
  interactive choice.
- Read [references/events.md](references/events.md) for subscriptions, waits,
  metadata tokens, or any proposed background process.

Inspect the closest existing plugin and the relevant `herdrkit` module before
designing a new abstraction. Add shared behavior to `herdrkit`; leave only the
plugin's domain policy in its crate.

## Architecture

The workspace contains `contracts`, `crates/herdrkit`, and one package per
plugin: `find`, `github`, `nav`, and `nvim`. A plugin is normally one
`herdr-<name>` Rust binary plus `herdr-plugin.toml`. Keep `main.rs` thin and put
testable domain logic in the library.

Use `herdrkit` for:

- typed IDs, protocol DTOs, client operations, transport, and errors;
- normalized invocation context and manifest-entry identity;
- event subscriptions, metadata tokens, and blocking waits;
- shared terminal application detection, themes, Unicode-safe columns, popup
  failure handling, and picker behavior;
- the scripted fake server used by plugin tests.

Do not duplicate wire DTOs, socket code, environment parsing, target-directory
fallbacks, terminal-width logic, or test fakes in plugins. Model plugin domain
states with enums and newtypes, and keep raw third-party DTOs private at their
integration boundary.

## Invocation dispatch

Herdr supplies action and pane identity in the environment. Manifest commands
therefore contain only the binary:

```toml
[[actions]]
id = "refresh"
command = ["./bin/herdr-example"]
```

Load the environment once and dispatch exhaustively on `InvocationKind`:

```rust
let invocation = Invocation::load()?;
match invocation.kind() {
    InvocationKind::Action { id } if id == "refresh" => refresh(&invocation),
    InvocationKind::PaneEntrypoint { id } if id == "picker" => pick(&invocation),
    other => anyhow::bail!("unsupported example invocation: {other:?}"),
}
```

The variants are `Action`, `PaneEntrypoint`, `Startup`, and `Event`. A link
handler invokes an ordinary action; its identity and clicked URL are
orthogonal fields on `Invocation`. Never pass action, pane-entrypoint, event,
or link-handler IDs again in argv.

Use the typed `Invocation` accessors and `require_*` methods. Resolve a working
directory through `Invocation::target_dir` or
`Snapshot::effective_workspace_dir`; never fall back to process cwd, because a
runtime command starts in the plugin directory.

## Herdr protocol invariants

`herdrkit::api::PROTOCOL` records protocol 22. The client uses one Unix-socket
connection per ordinary request and a retained connection for event streams.
Do not bypass these invariants:

- newline-delimited JSON with an 8 MiB response/event frame limit;
- response request-ID and result-tag checks;
- unknown response fields tolerated, malformed required fields rejected;
- a 3-second read deadline for ordinary calls;
- server wait timeout plus 2 seconds of transport slack;
- no read deadline for an explicitly indefinite wait;
- `agent.start` defaults to 30 seconds; an explicit timeout must be greater
  than 3 seconds and at most 5 minutes;
- expiring metadata TTL is at least 1 millisecond and at most 24 hours; retained metadata omits TTL. A source may
  publish at most 16 validly named tokens.

Use `AgentRef` for agent targets. When submitting a prompt and waiting for a
settled state are one operation, use `Client::agent_prompt_and_wait` rather
than composing `agent_prompt` and `agent_wait` across a race window.

Validate external data before side effects. Revalidate mutable state
immediately before a mutation even while holding a local lock. For workflows
that reserve an attempt before external effects, persist the reservation
durably and make recovery identity-based. External commands need wall-clock
deadlines and must be drained, killed, and reaped on timeout unless they are
deliberately interactive.

## Process and UI rules

Keep latency-sensitive logic in process. Do not interpose shell, `jq`, a text
record format, or a second invocation of the same plugin between indexing and
acting. A picker returns the typed value it was given.

Render error chains at the binary boundary and add context at meaningful
operation boundaries. Use `runtime::finish` with the appropriate
`FailureSurface` so popup failures remain readable and background/action
failures reach the plugin log.

Use `herdrkit::service` for pane-less background features. Startup and event
hooks ensure the service is ready and exit; the service owns its timer and
provider work. Require server-scoped singleton ownership, bounded readiness and
control messages, launch backoff, and exit on owner replacement or plugin
disablement. Herdr does not supervise these workers: crash recovery occurs on
the next hook. Manual GitHub refresh queues work in the periodic scheduler.

## Tests and activation

Use neutral, hand-authored fixtures shaped from the live schema. Never commit
real repository paths, employer data, agent tasks, or session captures. Enable
`herdrkit`'s `testing` feature only as a dev dependency, script replies with
owned `Step::reply`/`Step::bytes`, and assert the exact requests the plugin
sent. Manifest-to-dispatch and build invariants belong in `contracts`.

Run the complete gate from the repository root:

```sh
make check
```

It checks per-plugin Make targets, formatting, strict all-feature/all-target
Clippy, the full workspace through `cargo nextest`, doctests, and rustdoc with
warnings denied.

Do not link, install, or reload a live Herdr session merely to validate an
edit. `make plugins-build` stages all release binaries; plain `make` builds,
restows, and runs `plugins-link` last. If live activation was not explicitly
requested, report the command the user must run.
