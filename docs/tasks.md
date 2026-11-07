# Shared tasks

`dots task` coordinates project work across sessions and linked worktrees using
a local SQLite store, without a daemon. Machines that opt in replicate goals of
shared projects through the [substrate](../src/substrate/README.md).

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
Memory has a separate [shared knowledge store](../src/memory/README.md). Explicit
associations connect local catalog projects to shared knowledge identities;
those associations also select what task sharing replicates.

Goal briefs are ordinary Markdown under the project's `briefs/` directory.
Create the file there, then pass `--brief briefs/<name>.md` to `task goal create`
or `attach`. References stay project-relative across worktrees and checkout moves.
`task goal show` returns a bounded current read and its content hash, with explicit
missing, unreadable and truncation diagnostics. It also draws the goal's leaf
tasks as a dependency graph, prerequisites above dependents: an edge to a group
waits on the group's leaves, and a group's prerequisites hold for every leaf
below it. Cycles and prerequisites outside the goal appear as notes rather than
edges. `task goal list` shows each
goal's task counts and last activity, most recent first, without reading briefs. Native edits are visible on the next inspection;
association does not preserve historical bytes.

## Goals and attention

Each goal has one coordinator: the owner of its root claim, taken with
`dots task goal coordinate ROOT`. A new goal starts uncoordinated. Only the
coordinator adds, edits, claims, accepts or closes the goal's tasks, edits its
references and brief association, and raises or incorporates its requests;
other sessions read, and workers report to the coordinator. `--take` moves
every other owner's claim in the goal to the coordinator in one recorded
transaction, and `--resume` reopens a held or closed root. Managed workers
cannot coordinate, and a Pi session coordinates as its live runtime owner.
Releasing the root stops coordinating; holding it pauses the goal.
`dots task goal coordinating` lists the exact goals the acting owner coordinates
across every registered project, read from root claims alone.

`dots attention` keeps the goal's durable questions for the user and its
decisions, including the execution mandate, in the same catalog. A request
moves open, answered, closed and never changes content; its logical key makes
raising idempotent. An open or answered request holds the tasks it names and
their descendants until the coordinator incorporates the answer, which records
a decision and closes the request together. Every answer and standalone
decision carries the human's own input with its source and provenance, separate
from any agent interpretation; provenance is cooperative bookkeeping, not
authentication. `attention list` and `decisions` read an explicit scope: every
registered project, or exact `PROJECT:GOAL` identities. `dots attention --help`
has the content-file shapes.

## Across machines

`dots task share enable` turns replication on for one machine; it is off by
default. While on, every goal or top-level task tree of a project bound to a
shared identity (`dots memory project bind`) replicates; unbound projects never
leave the machine. A tree carries its tasks, dependencies, references, brief,
requests, decisions, history and the outputs its tasks name. Claims stay local.

One machine, the tree's home, changes a tree. Its writes commit locally at once;
the sync agent pushes them within its push budget, so a burst of writes leaves
as one push. Other machines import the tree on each sync pass, or on `--fresh`,
and show it read-only: changes refuse and name `dots task share take ROOT`.
Taking over compares against the version this machine last saw; when the tree
moved on, it imports the new version, reports what changed and refuses until
rerun. Coordinate the goal afterwards; claims never move. When the old home
was offline and wrote meanwhile, the remote refuses its update; the substrate
keeps it as a stranded entry, `dots task share status` lists the tasks and
events it held that never published, and the tree turns read-only there.

An answer given on a machine that is not the home travels to the home, whose
coordinator incorporates it; the closure then replicates back. Disabling keeps
imported trees read-only as last seen.

A brief or output revision the home cannot send, because it exceeds the
substrate's 1 MiB blob limit or its bytes are gone, stays behind while the rest
of the tree replicates; both machines list it in `dots task share status`, and
other machines read the revision as missing. Publication caps outputs at the
same 1 MiB, so a published output fits. A tree that cannot be exported at all
reports the error and stays queued without holding back other trees.

Pi marks imported goals read-only in their views and context, and rereads them
when the sync agent imports a change.

Events keep a stable `uid` across machines, while `seq` stays each machine's
own cursor; follow and history read local positions.

## Constraints

Git and GitHub remain authoritative. Tasks coordinate work; they are not evidence
that code is correct or a PR is merged. Reconcile them when resuming. Only the
parent session edits tasks; workers return results for the parent to record.

Claims are transactional. Claims and events default to the invoking agent's
live runtime owner, else its session; pass `--owner` for direct CLI use. A claim
lasts until its owner releases it, hands the task off for review, or closes it.
Pi releases only the ending runtime's claims; reload preserves them.
`release --all` releases the invoking owner's claims across projects. Outside
goals, another owner's claim protects edits and status changes unless forced.
Goal changes always require the current coordinator, including with `--force`.
Takeover requires reconciling the previous work; `--take` does not stop commands
or establish that a process ended.

`ready` lists unblocked, unclaimed, unheld open leaves, including those with
external references. It reports eligibility, not coordinator authority.
`claim --next` additionally skips goal work the invoking owner does not
coordinate, and fails when it cannot claim a ready leaf. Grouping tasks can
report readiness for closure but are never handed out as leaf work.

Holding a group holds every descendant, and accepting or claiming held work
fails even with `--force`. Adding a dependency to claimed
work warns but does not revoke the claim; tell the holder. Closing a task
unblocks dependents, and status changes clear its claim.
