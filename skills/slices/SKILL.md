---
name: slices
description: Run a multi-PR effort as small, independently shippable slices across sessions and worktrees. Use when the user describes a repo-wide change, migration, or sweep that needs several PRs; says "plan this", "do the next one/two", "where does X stand", "pause new work"; or resumes such an effort in a new session.
---

# Slices

An effort is a parent task with one child per PR-sized slice. Goal mode is
optional: attach a durable brief when focus, progress visibility, and cold
resume are useful. Three stores, three jobs, no overlap:

- `dots task`: what remains and who holds what. Slices, claims, branch, PR,
  per-slice notes.
- Project memory: why and how. The pattern, accepted decisions, exceptions.
  Never task IDs, slice lists, or progress; the task list is the index of the
  work, memory is the index of the knowledge.
- Git and GitHub: the truth about code. Reconcile against them before acting.

The coordinating session plans, claims, records, and verifies. It can implement
slices itself. Delegate only when the user asks or the agreed scope explicitly
includes delegation; the number of requested slices does not imply workers.
When used, workers change code and report; they do not edit tasks or memory.

## Plan, state it, then write it down once

Explore the repository and settle the slice boundaries and the reference
pattern. If the request already authorizes work ("plan this and do the next
two"), state the plan and any pattern decisions you added in your first reply
and proceed; stop for agreement only when the pattern itself is in doubt.

```sh
dots task add "<effort>" --body "pattern: <path>; done when: <criterion>" --json
dots task add "<slice>" --parent <effort> --priority <n> [--blocked-by <id>] --json
```

For goal mode, write the agreed Markdown brief and use
`dots task goal create "<goal>" --brief <path> --json` instead of adding an ordinary
parent, or `dots task goal attach <parent> --brief <path> --json` to adopt one.
In Pi, `goal_focus { id: "<parent>" }` adds the goal to this session's followed
set without starting work or removing other goals.
Keep the working agreement in the brief or conversation; no separate execution
protocol is needed. Goal overviews show active slices and complete totals;
`dots task goal show <parent> --all --json` includes closed details.

Keep the working pattern in the goal brief or a design document until it belongs
in the repository. Use the memory skill for reusable, source-backed lessons;
keep work-specific plans and progress in the brief and task store.

Correct: five slices, priorities reflecting what proves the pattern first, with the agreed default-deny and explicit-egress design in `netpol.md`.

Wrong: a 40-item plan before the first slice has shown whether the pattern holds; or `2026-10-16: effort t-560e07, slices t-aa64..t-4690` in memory.

## Claim before implementation or delegation

```sh
dots task claim --next --parent <effort> --owner <session> --json
```

`--next` is atomic, hands out only fresh leaf work, and exits non-zero when
nothing is ready. A slice with a PR recorded is in review, not fresh: it stays
out of `--next` even after its lease expires, and is picked up by `claim <id>`.
Use one owner name for the life of a session. A session resuming an effort it
did not run uses a new owner. `claim` never overrides another owner's live
lease; when the user directs a change on such a slice, use `hold --force` (or
`done`/`drop --force`) on it and say so, then `reopen` and `claim` if the work
should continue under the new owner.

Correct: claim, then implement the slice or launch an authorized worker with the claimed ID in its brief.

Wrong: launching two workers and claiming afterwards; reusing an old session's owner name to dodge a lease.

## When delegating, give the worker a bounded brief

In Pi, spawn with `worker_spawn`, `cwd` set to the slice's worktree, and a brief that
contains: the slice title and ID; the pattern reference (a path, not prose); the
allowed change surface; the checks to run; the exact branch and PR title when
the repository has no convention; and the stopping point (implement and verify, or open a PR only when authorized;
do not merge; report results and anything that did not fit the pattern). The child
contract already forbids editing tasks or memory. One writer per worktree;
parallel slices get parallel worktrees. Name the check to run; when the
repository has none, say so and name a substitute rather than "run the tests".
If the pattern file sits outside the worktree, say that it is read-only.

Other harnesses use their native background-agent, worktree, and resume controls;
Pi's tool names are examples, not tools to assume exist everywhere. Keep the same
brief, ownership, and verification requirements.

In Pi, after spawning, end your turn: a worker's completion or question arrives as an
event that starts your next turn, and you answer questions with
`message { to: <id>, text }`.
If you are yourself a worker or a script with nothing else to do, block with
`worker_wait` instead of sleeping and polling `worker_status`. "Do the next
two" requests two slices, not two workers: execute sequentially in the parent
when appropriate, or delegate within the authorized working agreement.

Correct, when delegation and PR creation are authorized: `worker_spawn { label: "netpol-api", cwd: "../wt-api", task: "Slice t-4d5e6f: add a NetworkPolicy for api/ following /…/memory/…/netpol.md. Touch only api/. Run make test once at the end. Branch netpol/api, PR title 'api: add default-deny NetworkPolicy'. Open the PR and stop; do not merge. Report the URL and any service that needs an exception." }`

Wrong: "Do the network policies."

## Record results

```sh
dots task update <id> --branch <branch> --pr <url> --owner <session> --json
dots task update <id> --note "<exception or discovery>" --json
```

Field edits need the lease holder (or `--force`); notes never do. A worker that
reports a blocker or an open PR with red CI keeps its session: `message` in Pi
(the native resume control elsewhere) continues it with the fix instead of
spawning a new one. A result that
changes the pattern is a decision: bring it to the user, then update the
authoritative brief or design document. Later slices read the new pattern
because the brief points at it. Use the memory skill separately when the
result yields a reusable lesson; do not assume a memory entry already exists.

Correct: worker reports "`worker/` uses host networking"; you ask the user, record the accepted exemption in the design document, and note it on the task.

Wrong: the worker decides host-network services are exempt and moves on.

## Close on verified state, not on worker success

`done` means the PR merged or the user called the slice finished. Verify in
GitHub first:

```sh
dots task done <id> --owner <session> --note "merged #<n>" --json
```

`done` refuses while blocked or with unclosed children unless `--force`; treat
the refusal as information. `hold --reason` for a slice waiting on something
outside the effort; `drop --reason` for one no longer wanted.

Correct: PR open with red CI → the slice keeps its PR and a note; nothing is done.

Wrong: `done` because the worker's turn completed.

## Resume cold from the list, then reconcile

```sh
dots task list --roots --json                 # find the effort
dots task list --parent <effort> --json       # active slices; add --all for closed history
dots task events --since <seq> --json         # history after a cursor; 0 when cold
```

One `list` answers "where does it stand"; `ready` is a subset of it. Before
acting on a slice from a previous session, check its branch and PR in
Git/GitHub: an open PR means "pick up the PR", not "redo the slice".

Correct: "3 merged, 2 open PRs (one red CI), 1 held pending the host-network decision, 4 ready. Next: fix CI on #41, or start the next ready slice?"

Wrong: reciting the task list without checking that the open PRs still exist.

## Change direction without losing work

"Pause the rest" means `hold --reason` every open slice the user did not
exempt, including leased ones with PRs (branch and PR stay recorded); a slice
leased to a previous session needs `--force`, and you say so.
"Reprioritize": `update --priority`. "Now blocked by that": `dep add` (it warns
when the holder must be told). "Drop it": `drop --reason`. `message` or
`worker_stop` running workers before launching replacements (use the harness's
equivalent steering/stopping controls outside Pi).

## When PRs are open and need to land

That is the shepherd loop, not this skill: use `local-ship-it` ("ship it and
rip it") to move open PRs to mergeable. The task record stays the map from
slice to PR.
