---
name: worktrees
description: "Personal policy for choosing, reusing, and cleaning up agent worktrees with Worktrunk (wt). Use before selecting an isolated checkout. Upstream's worktrunk skill and wt --help own the CLI mechanics."
---

# Worktrees

Policy only; use upstream's `worktrunk` skill or `wt COMMAND --help` for
mechanics. Goals and tasks own intent and progress; the harness owns workers.

## Choose the checkout

Stay in the current checkout for sequential work in one session, and for
read-only inspection. Create a worktree for concurrent writers, delegated
slices, a different branch or base, or checks that would disturb ongoing work.
Inspect `git status` and `wt list --format json` first, and reuse a suitable
existing worktree after checking its state and who is using it.

When a parent delegates, it works in its own home worktree and gives each
writer worker its own worktree based on the home branch. It merges finished
slices into the home branch, so `git diff <default>...` there shows the whole
effort.

## Create

Use `wt switch --create BRANCH --base REF --no-cd --format json`, or
`wt switch BRANCH --no-cd --format json` to resume. Pass the returned path as
the worker's or command's working directory. Create worktrees in the
coordinating session, never with plain `git worktree add`.

- Name branches for the work, not for models or worker numbers.
- Choose the base explicitly; wt defaults to the default branch, not HEAD. A new
  worktree does not see uncommitted changes from another checkout.
- Never pass `--yes` to approve project hooks or `--clobber` to force a path;
  stop and ask instead.
- Placement under `~/.wt/` comes from the user config; do not override it.

## Integrate and clean up

Review the diff and checks, then merge within the working agreement; local
integration is not publication. Read `wt merge --help` before using it, since
its defaults can squash, rebase, and remove the worktree.

After a slice is merged, stop its worker and `wt remove` its worktree in the
same step. For a sweep, `wt step prune --dry-run` lists merged, clean worktrees
older than a day; check that no running session uses them, then run it. Remove
an unmerged worktree only deliberately, with `--no-delete-branch`, and never
force-remove one with changes.

wt's activity markers and recorded task branches are hints, not locks; check
the actual session before treating a checkout as unused.
