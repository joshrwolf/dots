---
name: ci-fix
description: Inspect failing CI on the current branch's PR and fix it, or hand it to a dedicated agent in its own herdr tab. Triggers on "CI is red", "checks are failing", "fix the build", "why did CI fail", or any request to look at PR check status. Uses the `herdr-ci` helper; do not hand-roll `gh` calls for this.
argument-hint: "[optional: branch, worktree path, or which check to focus on]"
---

# ci-fix

Read failing CI for a PR and either fix it inline or delegate it. `herdr-ci` does
all the GitHub work — never reimplement it with raw `gh` calls.

## Commands

```sh
herdr-ci status [dir]   # one line: #48510◦ ● 1/117   (◦ = draft)
herdr-ci brief  [dir]   # markdown: failing checks, required ones first, + log excerpts
herdr-ci fix    [dir]   # new herdr tab + claude, prompted with the brief
```

`dir` defaults to `$PWD`. Everything is read-only except `fix`.

## Process

1. **Get the brief.** `herdr-ci brief` is the source of truth for what's failing.
   It sorts required checks first and marks them `**(required)**` — only those
   block the merge, so lead with them. A check with no log excerpt is a
   non-Actions external service: report its URL and do not guess at the cause.

2. **Decide inline vs delegated.** Fix it yourself in this pane when it's small
   and related to what you're already doing. Delegate when it's unrelated to the
   current conversation, or when the failing branch is a different worktree than
   the one you're in — a fixer running in the wrong checkout is the main way this
   goes wrong.

3. **When delegating, add what the brief cannot know.** This is the whole point of
   going through you rather than a keybinding. The brief has check names and logs;
   you have the conversation. Write a supplementary file and reference both:

   ```sh
   herdr-ci brief > /tmp/ci-<pr>.md      # then append your own context
   ```

   Include the decisions and constraints from this conversation that bear on the
   fix, what has already been tried, and anything explicitly out of scope. Put it
   in `/tmp` or the state dir — never inside the worktree, where it gets committed.

4. **Respect the attempt cap.** `herdr-ci fix` bails at
   `HERDR_CI_MAX_ATTEMPTS` (default 3) per commit, so a bad fix can't push-loop
   CI. If you hit the cap, stop and report — do not raise the cap to get past it.

## Rules

- Codegen and lint failures usually have a stated remedy in the log excerpt (for
  example "run ./hack/update-codegen-ci.sh"). Run the remedy; don't hand-edit
  generated files to match.
- Never post comments, replies, or reviews on the PR. Show the user any text
  meant for GitHub and let them post it.
- Report which checks you fixed and which you couldn't, by name.
