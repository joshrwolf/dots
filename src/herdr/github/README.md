# Herdr PR status

Registered PRs appear beneath their workspace with separate review and CI status;
selecting one opens it in the browser.

## Setup

Both client and server need the [workspace-resources patch](../host/README.md).
Then install the CLI and plugins:

```sh
mise run //src/dots:install
mise run //src/herdr:install
```

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

CI brief and watcher panes offer a PR picker. Direct fix requires exactly one
registered PR. The **GitHub status service health** action reports watcher failures.

## Constraints

Associations live in
`${XDG_STATE_HOME:-$HOME/.local/state}/dots/herdr-prs/registry.sqlite3`, keyed by
endpoint and persistent workspace UUID. Closing a workspace retains its records;
a new workspace needs fresh registration.

One pane-less service shares fetches across workspaces. Pending CI refreshes
more often than settled or completed PRs; rate limits and failures delay updates.
Errors retain cached status and its age, never become a no-PR result.
Expired sidebar projections disappear if their producer stops.

Hooks wake the service rather than querying GitHub themselves. Crash recovery
depends on the next startup/event/action hook, not Herdr supervision.
The status watcher neither invokes an agent nor mutates GitHub.
