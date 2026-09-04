# Manifests and invocation environment

The authority is `herdr api schema --json`, the current Herdr plugin docs, and
the checked-in manifests. Use this file for the repository's integration
contract; verify fields against the live schema when adding a capability.

## Repository manifest shape

```toml
id = "herdr-example"
name = "Example"
version = "0.1.0"
min_herdr_version = "0.8.2"
description = "..."
platforms = ["macos", "linux"]

[[build]]
command = ["make", "-C", "../..", "plugin-build-example"]

[[actions]]
id = "refresh"
title = "Refresh example state"
contexts = ["workspace"]
command = ["./bin/herdr-example"]

[[panes]]
id = "picker"
title = "Choose an example"
placement = "popup"
command = ["./bin/herdr-example"]
width = 80
height = 24
```

The package, plugin ID, staged executable, and Make target share the plugin
directory name. Manifest action and pane commands contain only
`./bin/herdr-<plugin>`: Herdr injects the entry ID, so argv must not repeat it.
The workspace `contracts` tests enforce these relationships.

`command` is argv, not shell syntax. Do not place pipes, globs, expansion, or
quoting in it. A third-party interactive program such as `gh dash` may be the
pane command when the pane is intentionally not implemented by the plugin.

## Entrypoints and dispatch

Every process begins with `Invocation::load()` and matches
`Invocation::kind()`:

- `InvocationKind::Action { id }` for `[[actions]]`;
- `InvocationKind::PaneEntrypoint { id }` for `[[panes]]`;
- `InvocationKind::Event { name, payload }` for `[[events]]`;
- `InvocationKind::Startup` only when no other manifest identity is present.

Conflicting identity variables are an error. Event identity requires a valid
`HERDR_PLUGIN_EVENT_JSON` payload. Reject unknown IDs or wrong entrypoint kinds;
do not silently choose a default action.

A `[[link_handlers]]` entry names an action in the same plugin. Its ID does not
replace the action invocation kind: read it with `Invocation::link_handler_id`
and the URL with `Invocation::clicked_url` when needed.

Current workspace contracts:

| plugin | actions | panes |
|---|---|---|
| find | none | `picker` |
| github | `ci-refresh`, `ci-fix`, `open-link` | `ci-arm`, `ci-brief`, `dashboard` |
| nav | `left`, `down`, `up`, `right` | none |
| nvim | `open` | `picker` |

The GitHub `dashboard` pane directly runs `gh dash`; it is intentionally absent
from the Rust binary's dispatch.

## Injected environment

Runtime commands receive the socket, plugin paths, and context, including:

- `HERDR_SOCKET_PATH`, `HERDR_PLUGIN_ID`, `HERDR_PLUGIN_ROOT`,
  `HERDR_PLUGIN_CONFIG_DIR`, `HERDR_PLUGIN_STATE_DIR`, and
  `HERDR_PLUGIN_CONTEXT_JSON`;
- applicable direct IDs such as `HERDR_WORKSPACE_ID`, `HERDR_TAB_ID`, and
  `HERDR_PANE_ID`;
- `HERDR_PLUGIN_ACTION_ID` for actions;
- `HERDR_PLUGIN_ENTRYPOINT_ID` for pane entrypoints;
- `HERDR_PLUGIN_EVENT` and `HERDR_PLUGIN_EVENT_JSON` for events;
- `HERDR_PLUGIN_LINK_HANDLER_ID` and `HERDR_PLUGIN_CLICKED_URL` for link
  actions.

Do not parse these independently in a plugin. `Invocation` normalizes direct
IDs with context fallbacks, including the underlying focused pane for a popup,
and rejects missing required base variables. Use `require_workspace_id`,
`require_tab_id`, `require_pane_id`, and `require_target_dir` at the boundary
where a command needs them.

`InvocationContext::target_dir` prefers the Herdr-managed worktree checkout,
then focused-pane cwd, then workspace cwd, ignoring empty paths. There is no
process-cwd fallback. `Snapshot::effective_workspace_dir` applies the same
checkout-or-pane policy to snapshot data.

`HERDR_PLUGIN_ROOT` is managed and may be replaced on reinstall. Persist user
configuration in `config_dir()` and runtime state in `state_dir()`.

## Placement and lifecycle

Confirm available placement strings in the schema. A popup is modal and does
not itself have a pane ID; use normalized invocation context to reach the pane
beneath it. Fixed numeric picker dimensions are terminal cells and are clamped
by Herdr, which keeps rows readable on large terminals.

Do not add a resident `[[startup]]` daemon. Herdr starts such hooks per session
but does not currently provide the supervised lifecycle this repository
requires for a durable resident process. Prefer:

- a one-shot action, as GitHub uses `ci-refresh`;
- a one-shot `[[events]]` hook when Herdr should schedule a reaction; or
- an explicitly designed supervised lifecycle if Herdr later exposes one.

## Config bindings and linking

A plugin action is globally qualified as `<plugin-id>.<action-id>`:

```toml
[[keys.command]]
key = "ctrl+h"
type = "plugin_action"
command = "herdr-nav.left"
```

When replacing a native chord, a missing plugin registration leaves the chord
dead. This is why plain `make` builds first and runs `plugins-link` last, and
why a link failure is fatal.

Plugin registration in `~/.config/herdr/plugins.json` is derived external
state, not a tracked file. Do not edit or commit it. Building or checking source
does not authorize linking, installing, or reloading the user's live server.
