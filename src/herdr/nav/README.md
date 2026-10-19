# Herdr navigation

Workspace toggling and pane history complement application-aware directional
navigation.

## Setup

Requires stock Herdr 0.9.1 or newer, including the running server.

```sh
mise run //src/herdr/nav:install
```

Configure the navigation bindings from `home/herdr/.config/herdr/config.toml`
and the matching Ghostty bindings, then reload those configurations when ready.
Do not copy bindings for uninstalled plugins. The aggregate Herdr installation
also includes GitHub integration, which needs the [patched host](../host/README.md).

## Use

- Cmd+O toggles current/previous workspaces. Tab and split changes within one
  workspace do not change the pair; Herdr restores the selected tab and pane.
- Ctrl+S then lowercase O walks backward through visited panes.
- Cmd+I walks forward.
- Ctrl+H/J/K/L routes directional navigation to the foreground application.

Pane history crosses tabs and workspaces, holds 100 distinct terminal destinations,
and stops at either end. New navigation preserves other destinations and renews
the departure point, like Vim's default jumplist:

```text
A, B, (C) → Back → A, (B), C → visit D → A, C, B, (D)
```

Closed panes disappear; moved panes retain their identity. History survives a
worker restart within the same server, but not a new server. Neovim owns its
internal split/cursor navigation.

The plugin's status action reports history, pending targets, queue depth, and
the latest worker error in the command log. Background errors go to
`service.log` in the plugin state directory; failed starts have a launch cooldown.

## Constraints

History is shared per server; a jump can move other attached clients. Hooks wake
one worker to read current focus, not replay an ordered journal. The fallback
read repairs missed hooks and closures but cannot reconstruct visits that begin
and end between observations.

Popups are absent from pane inventory. Overlays exposed as panes participate
until they close because the public API has no reliable modal classification.

The worker saves a reservation before focusing and confirms with a fresh
snapshot. Concurrent focus can interrupt a jump; an uncertain result stays
pending for reconciliation and is never automatically replayed. Checkpoint
failure halts mutations rather than risking duplicate or inconsistent jumps.

## Checks

`mise run //src/herdr/nav:check` checks the feature without activation.
An opt-in integration test uses a disposable stock server and PTY client:

```sh
HERDR_NAV_TEST_BINARY=/path/to/stock/herdr \
  cargo test --manifest-path Cargo.toml -p herdr-nav \
  --test acceptance stock_server_jumplist -- --ignored --nocapture
```
