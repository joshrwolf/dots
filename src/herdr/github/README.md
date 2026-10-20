# Herdr PR status

Each workspace's Space row in the sidebar summarizes its registered PRs, such as
`#42 ✓✗ · #43 ◷✓`: the PR number, then its review and CI glyphs. Ctrl+S then U
opens a picker of the workspace's PRs with their status in words; choosing one
opens it in the browser.

## Setup

Requires stock Herdr 0.9.0 or newer. Install the CLI and plugins:

```sh
mise run //src/dots:install
mise run //src/herdr:install
```

The Space row and keybinding live in `home/herdr/.config/herdr/config.toml`;
the row shows the `$prs` workspace metadata token.

## Use

```sh
dots herdr prs register https://github.com/example/project/pull/42 --workspace VERIFIED_ID --json
dots herdr prs list --workspace VERIFIED_ID --json
dots herdr prs unregister https://github.com/example/project/pull/42 --workspace VERIFIED_ID --json
```

Verify the workspace against the task and live inventory. Inherited environment
IDs, UI focus, and matching checkout paths do not establish ownership; shared
agent daemons can carry stale IDs.

Registration is idempotent and keeps earlier associations. Only HTTPS github.com
PR URLs are supported. Ship-it and managed review creation/resume register PRs;
other existing PRs need explicit registration. Unregistering removes only the
local association, not the PR.

Glyphs: review `✓` approved, `!` changes requested, `◷` awaiting review, `○`
draft or unreviewed, `?` unknown; CI `✓` passing, `✗` failing, `◷` pending,
`?` cancelled or unknown, `—` none. Merged and closed PRs show `◆` and `×`.
A trailing `…` marks a missing or delayed observation.

CI brief and watcher panes use the same PR picker. Direct fix requires exactly one
registered PR. The **GitHub status service health** action reports watcher failures.

## Constraints

Associations live in
`${XDG_STATE_HOME:-$HOME/.local/state}/dots/herdr-prs/registry.sqlite3`, keyed by
Herdr endpoint and workspace ID. Herdr reuses the IDs of closed workspaces, so
the watcher discards a workspace's associations once it closes; a reopened
workspace needs fresh registration.

One pane-less service shares fetches across workspaces. Pending CI refreshes
more often than settled or completed PRs; rate limits and failures delay updates.
Errors retain cached status and its age, never become a no-PR result.
Tokens expire two minutes after the service stops renewing them.

Hooks wake the service rather than querying GitHub themselves. Crash recovery
depends on the next startup/event/action hook, not Herdr supervision.
The status watcher neither invokes an agent nor mutates GitHub.
