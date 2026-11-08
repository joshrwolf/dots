---
name: dots-diff
description: Show the user a worktree's changes in the dots-diff viewer beside your Rex pane. Use when a worker finishes or you integrate work and want the user to review a worktree or the integrated branch, or when the user asks to see a diff.
---

# dots-diff

`dots-diff` shows every worktree of a repository against its merge base with
the default branch, live as files change. In Rex it runs in a split beside a
pane, one viewer per pane; the user toggles it with Ctrl+S then D.

## Show work for review

From your own Rex terminal (`REX_SESSION` and `REX_BLOCK` set), open the viewer
beside your pane without taking focus, or point the one already there:

```sh
rex do diff_toggle show=true path=/abs/worktree
rex do diff_toggle show=true path=/abs/worktree file=src/changed.rs
```

`path` is the worktree to show; `file`, a path within it or an absolute path,
scrolls to that file. Then tell the user in chat what to look at; the viewer
does not explain itself. Use `show=true` always: without it the action
toggles, and closes a viewer the user has open.

## What the user sees without you

The viewer reads Pi's worker registry and the interactive sessions' inbox
registrations itself. Its worktree switcher lists worktrees waiting for review
first, naming the worker (label and model) or session working in each and the
worker's status; the status line names the shown worktree's. The user steps
through waiting worktrees with `.` and `,`. Opening a worktree counts as
looking at it until its changes change again. A clear,
specific worker label therefore helps the user more than opening the viewer
for each worker.

## Steer a running viewer

```sh
dots-diff ctl list                      # pane and worktree of each viewer
dots-diff ctl worktree /abs/worktree    # show another worktree
dots-diff ctl file /abs/worktree/a.rs   # scroll to a file, switching worktree
dots-diff ctl --block ID refresh        # one viewer, by its pane
```

Without `--block`, `ctl` picks the viewer showing the path's repository, or the
only one running, and lists the candidates when that is ambiguous.

## Explain changes with notes

Pin a short note to the lines you want the user to look at; it shows inline
until the file changes or you clear it:

```sh
dots-diff ctl note --file /abs/worktree/src/a.rs --line 42 --end 48 "Why this loop now saturates"
dots-diff ctl note --file /abs/worktree/src/a.rs --line 7 --side old "Removed: callers moved to b.rs"
dots-diff ctl note --clear [--file /abs/worktree/src/a.rs]
```

Lines are the file's own line numbers, on the new side unless `--side old`.
Without `--line` the note is about the whole file; without `--file`, the review.

## Reviews from the user

The user comments in the viewer and sends the batch to the Pi session in the
pane the viewer was opened from, or one they pick. It arrives as a user
message headed "Review of `branch`": numbered comments with file, line, type,
an excerpt and optional suggested code. Address each, then reply with which
you changed and which you did not, and why. The viewer then shows the user
only what changed since that send.

## Constraints

- The viewer never stages, commits or starts Git daemons; it writes only its
  review state under the repository's Git directory.
- Outside Rex, or with no `dots-diff` on the command path, say so instead of
  opening something else.
