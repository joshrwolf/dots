# Shared tasks

`dots task` coordinates project work across sessions and linked worktrees using
a machine-local SQLite store, without a daemon or network service.

## Use

```sh
dots task add "update service configuration" --json
dots task ready --json
dots task claim --next --owner session-7 --json
dots task update TASK_ID --branch config/service-a --json
dots task update TASK_ID --pr https://github.com/example/project/pull/12 --json
dots task done TASK_ID --owner session-7 --note "merged" --json
```

Use `dots task --help` for grouping, dependencies, notes, history, and lease
operations. `--parent` selects the whole subtree, not just direct children.

The project defaults to the repository's main checkout; `--project KEY` selects
an explicit list. Storage lives under `$XDG_STATE_HOME/dots/tasks/`, defaulting
to `~/.local/state/dots/tasks/`.

## Constraints

Git and GitHub remain authoritative. Tasks coordinate work; they are not evidence
that code is correct or a PR is merged. Reconcile them when resuming. Only the
parent session edits tasks; workers return results for the parent to record.

Claims are transactional. Leases expire, and the same owner can renew one.
Another owner's live lease protects edits and status changes unless explicitly
forced. Notes do not require a lease.

`ready` and `claim --next` hand out leaf work without a recorded PR. A task with
a PR needs an explicit claim even after its lease expires. Grouping tasks can
report readiness for closure but are never handed out as leaf work.
`claim --next` fails when nothing is ready.

Holding a group does not hold its descendants. Adding a dependency to leased
work warns but does not revoke the lease; tell the holder. Closing a task
unblocks dependents, and status changes clear its claim.
