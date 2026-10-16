# Shared tasks

`dots task` is a machine-local work list for one project, shared by every
session and linked worktree of that checkout. It coordinates multi-PR efforts:
which slices remain, who holds one, what it depends on, and which PR it
produced. It is not a knowledge base and not evidence about code; Git and GitHub
stay authoritative, so reconcile against them when resuming.

Storage is one SQLite file under `$XDG_STATE_HOME/dots/tasks/` (default
`~/.local/state`). Claims and status changes are single transactions, so two
sessions cannot take the same slice. There is no daemon, sync, or network use.

```sh
dots task add "migrate network policies" --body "pattern: docs/net-policy.md" --json
dots task add "service a" --parent t-1a2b3c --priority 2 --json
dots task add "service b" --parent t-1a2b3c --blocked-by t-4d5e6f --json
dots task ready --parent t-1a2b3c --json
dots task claim --next --parent t-1a2b3c --owner session-7 --json
dots task update t-4d5e6f --branch netpol/service-a --json
dots task update t-4d5e6f --pr https://github.com/example/project/pull/12 --json
dots task done t-4d5e6f --owner session-7 --note "merged" --json
dots task show t-4d5e6f --json
```

`--parent` always means the whole subtree below that task, so nested groups do
not hide leaves from `list`, `ready`, or `claim --next`. `ready` lists only
leaf tasks without a recorded PR: once a slice has a PR it is in review, stays
out of `claim --next` even after its lease expires, and is picked up by
`claim <id>`. Field edits with `update` need the lease holder or `--force`;
`--note` never does. A task is ready when it is open, has no open or held prerequisites,
has no unclosed children, no PR recorded, and is not under a live lease. JSON output includes
`ready` and, when false, `not_ready_because`, evaluated at the time of the call; a grouping
task whose children have all closed reports `ready: true` (it can be closed) but is still
left out of `dots task ready` and `claim --next`, which hand out leaf work only. Leases default to
two hours; claiming again with the same owner renews, and an expired lease can
be taken over, which the task history records. `hold` parks one task with a
reason; its children stay claimable, and holding a grouping task warns how many
unclosed descendants remain. `drop` closes a task as abandoned, and
`reopen` reverses either; a task that is already open cannot be reopened. Every
status change clears the task's claim, and closing a task unblocks its
dependents. Another owner's live lease blocks `done`, `hold`, `drop`, `reopen`
and `release`, and closing a task that is still blocked or has open children is
refused, unless `--force` is given. `claim --next` only hands out tasks without
children and exits non-zero when nothing is ready, so a script cannot mistake
"nothing to do" for a claim.

Each task reports `prerequisites` with their current status, `last_event`
(the most recent note, reason, or outcome), `branch`, and `pr`, so a session
starting cold can read `list --json` once and know where each slice stands.
Adding a dependency to a task under a live lease succeeds with a warning: the
holder should be told. `list --claimed-by NAME` filters to one owner's tasks;
`events --since SEQ` returns project history after a cursor, oldest first, at
most `--limit` events (default 100, maximum 1000); advance the cursor to the
last returned `seq` and repeat until a page comes back short. With `--json`,
failures also print `{"error": "..."}` on stdout with a non-zero exit.

The project key defaults to the main checkout of the Git repository at
`--directory`; `--project KEY` selects an explicit list. Only the parent
session should edit tasks; delegated workers return results and the parent
records them.
