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

## Steer a running viewer

```sh
dots-diff ctl list                      # pane and worktree of each viewer
dots-diff ctl worktree /abs/worktree    # show another worktree
dots-diff ctl file /abs/worktree/a.rs   # scroll to a file, switching worktree
dots-diff ctl --block ID refresh        # one viewer, by its pane
```

Without `--block`, `ctl` picks the viewer showing the path's repository, or the
only one running, and lists the candidates when that is ambiguous.

## Constraints

- The viewer is read-only. It never stages, commits or starts Git daemons.
- Outside Rex, or with no `dots-diff` on the command path, say so instead of
  opening something else.
