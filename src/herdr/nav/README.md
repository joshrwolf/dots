# nav

Cmd+O toggles between the current and previous Herdr **workspace**. Repeated
presses alternate those two workspaces; changing tabs or splits within a
workspace does not change the pair. Visiting a third workspace makes the one
you just left its alternate. Herdr restores each workspace's selected tab and
pane. A closed alternate is cleared; with no alternate, Cmd+O does nothing.

The pane jumplist remains separate: Ctrl+S then O (lowercase, without Shift)
walks backward through visited panes, and Cmd+I walks forward.
Both cross tabs and workspaces. New navigation preserves the other
destinations and renews the departure point, like Vim's default jumplist:

    A, B, (C) -> Back -> A, (B), C -> visit D -> A, C, B, (D)

The list holds 100 distinct terminal destinations. Ends stop without wrapping.
Closed panes disappear; moving a pane between workspaces preserves its entry.
History survives a plugin worker restart within the same Herdr server. A new
server starts a new history. Neovim owns its internal split/cursor navigation.

Ctrl+H/J/K/L retains the existing application-aware directional routing.

## Requirements and activation

Requires **stock Herdr 0.9.1+**, including the running server, for native focus
notifications. No fork, patch, extra daemon installation, or upstream API is
needed. Version 0.5.0 adds the independent workspace toggle.

With the running server on a supported version, run from the dots root:

    mise run //src/herdr/nav:install

This builds and registers only navigation. Configure its bindings from
`home/herdr/.config/herdr/config.toml` and the matching Ghostty bindings, then
reload those configurations when ready. Do not copy unrelated plugin bindings
without installing their plugins. The aggregate `mise run //src/herdr:install`
also stows the complete configuration and installs GitHub integration, which
requires the [patched Herdr client and server](../host/README.md). When upgrading from 0.4.0, disable
herdr-nav, wait for its worker to exit (at most ten seconds), then enable it
again. This restarts only the plugin, preserving the server session and pane
history. The workspace pair starts recording from activation. Development checks do not
install, link, reload, or upgrade the live session.

The manifest's previous-workspace, back, forward and status actions are available through
Herdr's plugin action interface. The status action prints the current terminal
history, cursor, current/previous workspace pair, pending targets, queue depth and last worker error into
the plugin command log. Background errors go to service.log under the plugin's
state directory. A 30-second launch cooldown protects failed starts; inspect
the log before retrying.

## Boundaries

History is **shared per server**, because the public plugin API exposes
session-wide focus. A jump can move other attached clients.

Hooks wake a single worker, which reads actual current focus. Late events
cannot replay old destinations or turn Back into a new jump. However, a visit
that begins and ends between observations can be missed. A one-second fallback
read repairs missed hooks and closures; it cannot reconstruct unseen visits.

Popups do not appear in pane inventory and never become entries. Stock Herdr
does not expose reliable overlay/modal classification: overlays that appear as
panes participate until they close. Native modal key handling controls whether
a key reaches the plugin.

External focus can race a jump because Herdr has no conditional focus API.
The worker reads back focus and reports an interruption if a different pane
won. Lost responses remain pending until a fresh observation resolves them;
the plugin never automatically repeats an uncertain focus request.

See [ARCHITECTURE.md](ARCHITECTURE.md) for persistence and ordering, and
[RESEARCH.md](RESEARCH.md) for the reviewed alternatives.

## Validation

From the dots root:

    mise run rust-check

Opt-in real-server coverage, using an unmodified downloaded Herdr executable
without installing it:

    HERDR_NAV_TEST_BINARY=/path/to/stock/herdr \
      cargo test --manifest-path Cargo.toml -p herdr-nav \
      --test stock stock_server_jumplist -- --ignored --nocapture

The Rust integration test creates temporary configuration, state, plugin
registration, panes and a PTY client. It covers hooks, branching, concurrent
Back presses, workspace toggles with remembered tabs, late hooks, pane moves, closure, popup/overlay/split behavior,
service restart, and native key dispatch. It stops its client/server and waits
for its worker to exit. It can take about two minutes because it respects
service restart and owner-loss grace periods.
