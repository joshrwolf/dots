# Shared tasks

`dots task` coordinates project work across sessions and linked worktrees using
a machine-local SQLite store, without a daemon or network service.

## Use

```sh
dots project init "my project" --json
dots task add "update service configuration" --json
dots task ready --json
dots task claim --next --owner session-7 --json
dots task update TASK_ID --branch config/service-a --json
dots task ref set TASK_ID https://github.com/example/project/pull/12 --json
dots task show TASK_ID --json
dots task done TASK_ID --owner session-7 --result "Integrated and verified" --json
```

Use `dots task --help` for grouping, dependencies, claims, and history.
`--parent` selects the whole subtree, not just direct children. Each result
replaces the current one and keeps the earlier ones in history. A done or
dropped task keeps its final result: reopen it before recording another, even
with `--force`. [Retained
evidence](documents.md) explains exact revision selection, retention and offline
backup.

Projects have stable local IDs. `dots project init` explicitly binds the canonical
Git common directory, sharing one project across linked worktrees while keeping
separate clones, bare repositories and submodules distinct. `--project ID` selects
a registered project outside Git. Use `dots project --help` to list bindings or
explicitly rebind a moved checkout; display names do not establish identity.

The catalog lives at `$XDG_DATA_HOME/dots/workspace.sqlite3`, defaulting to
`~/.local/share/dots/workspace.sqlite3`. Task commands never initialize storage.
Memory uses the same project bindings and keeps current Markdown under
`dots/workspaces/<project-id>/knowledge/`; global knowledge uses `global`.

Goal briefs are ordinary Markdown under the project's `briefs/` directory.
Create the file there, then pass `--brief briefs/<name>.md` to `task goal create`
or `attach`. References stay project-relative across worktrees and checkout moves.
`task goal show` returns a bounded current read and its content hash, with explicit
missing, unreadable and truncation diagnostics. `task goal list` shows each
goal's task counts and last activity, most recent first, without reading briefs. Native edits are visible on the next inspection;
association does not preserve historical bytes.

## Constraints

Git and GitHub remain authoritative. Tasks coordinate work; they are not evidence
that code is correct or a PR is merged. Reconcile them when resuming. Only the
parent session edits tasks; workers return results for the parent to record.

Claims are transactional. A claim lasts until its owner releases it, hands the
task off for review, or closes it; Pi releases a session's claims when the
session ends, and `release --all` does the same for other hosts. Another owner's
claim protects edits and status changes unless forced; `claim --take` takes over
a claim whose session has stopped.

`ready` and `claim --next` select unblocked, unclaimed open leaves,
including those with external references. Grouping tasks can report readiness for closure
but are never handed out as leaf work.
`claim --next` fails when nothing is ready.

Holding a group does not hold its descendants. Adding a dependency to claimed
work warns but does not revoke the claim; tell the holder. Closing a task
unblocks dependents, and status changes clear its claim.
