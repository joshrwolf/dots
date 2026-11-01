---
name: goals
description: Coordinate a multi-part outcome across sessions, local work, and optional external issues or PRs. Use when planning an effort, resuming tracked work, checking progress, accepting results, or changing direction. Goals and tasks do not imply delegation or publication.
---

# Goals

A goal is a root task with a Markdown brief. Use `dots task --help` for the
commands and `worktrees` for where work runs.

- The brief owns the outcome, constraints, working agreement, and decisions.
  Edit it in place when they change.
- Tasks own the slices, dependencies, claims, and current results.
- Git and GitHub own the code, branches, issues, and PRs. Recorded results and
  references are claims to check, not current state.
- Memory keeps reusable lessons, not task lists or progress.

Planning or following a goal does not authorize implementation, delegation, or
publication; the user's request and the brief's working agreement do.

## Plan

Write the brief under the project workspace's `briefs/`, create the goal with
`dots task goal create`, and follow it with `goal_focus`. Split work into
tasks that can each be verified on their own, add only real dependencies, and
skip a speculative backlog. Small work needs no goal.

## Work

Claim a task before working on it or delegating it. A claim lasts until you
release, hand off, or close the task; Pi releases a session's claims when it
ends. Use `--take` only when the owning session has stopped working on it.

Keep the task's result current with `update --result` as work progresses,
replacing the previous text. Keep the goal's own result current the same way: where
things stand overall and the next step. Pi shows it to every session following
the goal, so it carries the thread across compactions and handoffs. Write it for the user in plain sentences: what
changed, where it is (branch, commit, path), which checks ran, and what remains.
No shorthand, and no lists of things that did not happen.

Workers report to the coordinating session, which records their results. Give
each worker its own scratch directory under the workspace for drafts. To cite a
report, publish it with `dots output publish` and pass `--evidence SLUG DIGEST`.

## Hand off and accept

Use `review` when work needs someone else to check or integrate it, and `done`
only after you have verified the acceptance criteria yourself. A worker's
success or a recorded result is not acceptance. Closing every task does not
accept the goal: check the combined result, then close the root.

Use `hold --reason` for paused work and `drop --reason` for abandoned work.
Link a task to its PR or ticket, such as a Linear issue, with `ref set`. Links
record context only; never comment on, close, or change the linked item because
a task links it.

## Resume and change direction

Read the brief and `dots task goal show`, then check branches, worktrees,
workers, and PRs before continuing. Resume existing work rather than
duplicating it. When direction changes, update the brief, stop or steer
affected workers, and hold, drop, or reopen tasks to match.
