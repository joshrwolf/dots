# PRs in spaces

Agents register every PR they work on; the plugin watches GitHub and displays
two-line PR items under the existing workspace: number and review status above,
CI below, with separate colors. Completed PRs collapse into a group. Clicking
either line or pressing Enter opens the PR in your browser.

```sh
dots herdr prs register https://github.com/example/project/pull/42 --workspace <verified-id> --json
dots herdr prs list --workspace <verified-id> --json
dots herdr prs unregister https://github.com/example/project/pull/42 --workspace <verified-id> --json
```

Every command requires an explicit `--workspace <id>`. Verify the target against
the task and live workspace inventory. Environment IDs, UI focus and checkout
similarity do not establish ownership; shared agent daemons can inherit stale IDs.
Registration is idempotent and does not replace earlier
PRs. Only HTTPS github.com PR URLs are supported. Ship-it and managed review
creation/resume register PRs automatically. Existing PRs need explicit registration.

Associations live in
`${XDG_STATE_HOME:-$HOME/.local/state}/dots/herdr-prs/registry.sqlite3`, keyed by
endpoint and persistent workspace UUID. Closing a workspace retains its records;
a new workspace requires fresh registration. Unregistering only removes the local
association.

The watcher reconciles registrations every 15 seconds and shares fetches across
workspaces. Pending checks refresh every 45 seconds, open PRs every three minutes,
and completed PRs every 30 minutes. Errors retain cached status with its age.
The **GitHub status service health** action reports watcher failures.

CI brief and watcher panes offer a PR picker. Direct fix requires exactly one
registered PR; with several, select through the watcher pane.

Install the [patched host](../host/README.md), then run `make cli` and `make herdr`.
Both client and server need the patch. Validate with `make herdr-check`.
