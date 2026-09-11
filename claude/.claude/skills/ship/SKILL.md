---
name: ship
description: "Commit, push to my fork, and open a PR against upstream. Matches my terse, lowercase commit/PR voice. Shows the message then proceeds through commit+push; opening the PR is always a hard gate that needs my explicit OK."
---

# ship

Take the current working changes from "done" to "PR open" in one flow: commit →
push to fork → open PR.

**Autonomy contract:**
- **commit + push**: show me the staged diff summary and the commit message,
  then proceed. Don't ask "ready to commit?" — just show and do it.
- **`gh pr create`**: a hard gate, every time. Show me the title and body, then
  *wait for explicit go-ahead* before running it. Never open a PR unprompted.
- The CLAUDE.md "never post to GitHub on my behalf" rule still binds: no review
  comments, no replies, no issue comments. Opening a PR is the only creation
  action allowed here, and only behind the gate above.

## Tone — this is the whole point

Both commits and PR bodies are **terse, lowercase, first-person, no fluff**.
Get to the point and stop. **Do not write essays.** A body is usually 1–3
sentences. One sentence is fine. Empty is fine for trivial changes.

What my voice looks like (illustrative — generic placeholder names):
- commit: `cache: expire entries on read, not just on write`
- commit: `auth: rejigger token/session interfaces`
- body: `not sure how CI missed the api breakage introduced [here](url), but this
  just severs the build dependency on the opt-in `foo` service.`
- body: `Reverts #123. keeping this around in case something else unknown breaks.`

Rules:
- lowercase to start (proper nouns / acronyms keep their case).
- say *what* and *why-it-matters*, skip the *how* the diff already shows.
- no markdown headers, no bullet checklists, no "## Summary" / "## Changes"
  scaffolding, no "this PR does the following:".
- cross-reference related/follow-up work with `#1234` or a link, the way I do.
- **backtick code-ish tokens** — package/type/function names, file paths,
  commands, flags, branches, env vars (`pkg/auth`, `TokenStore`, `serve --addr`,
  `GITHUB_TOKEN`, `--force-with-lease`). I do this consistently; match it in both
  commit bodies and PR descriptions. Prose stays prose — don't over-backtick
  ordinary words.
- **never** add attribution footers (`Generated with…`, `Co-Authored-By`) — my
  history has none.
- when in doubt, cut it. Re-read your draft and delete every sentence that
  isn't load-bearing.

## Commit

1. **Pre-push checks** (before committing). Run what the repo expects and
   surface failures — don't push broken code. For Go, load `gotools` and use
   its diagnostics; otherwise look for the obvious (`make lint`/`test`,
   formatter). Keep it proportional: a one-line config change doesn't need a
   full test run. If a check fails, stop and report — don't commit over it.

2. **Stage deliberately.** Look at `git status` + `git diff`. Stage the changes
   that belong to *this* unit of work — don't reflexively `git add -A` and sweep
   in unrelated edits or stray untracked files. If the changes are clearly two
   things, say so and offer to split.

3. **Never commit on the default branch.** If `HEAD` is on `main`/the default
   branch, create a branch first (short, lowercase, scope-ish: `cache-expiry`,
   `auth-session-iface`). In a worktree you're usually already on a feature
   branch — check before assuming.

4. **Write the message** in my voice (above). Match the convention already in
   `git log --oneline -20` for this repo: path/scope-style (`pkg/auth/token: init`)
   or conventional (`feat(scope):`, `fix(...)`, `chore(scope):`). Title only,
   unless a body genuinely earns its keep.

5. Show the staged summary + message, then commit.

### gitsign — signed commits are non-negotiable

I sign commits with **gitsign** (keyless OIDC). When you run `git commit`, it
blocks and pops a browser tab I have to ack before the commit succeeds. You
*are* allowed to create the commit and trigger this — just expect the pause.

- The browser ack is mine to do. If I'm away it can **time out** and the commit
  fails. That's fine — report the failure and let me re-run it. Do **not** retry
  on a loop or assume it'll go through.
- **NEVER** work around signing. No `--no-gpg-sign`, no `-c commit.gpgsign=false`,
  no disabling/swapping the `gpg.format`/`gpg.x509.program` config, no "I'll
  commit unsigned and you can fix it later". I **always** want signed commits
  using the **repo's own defaults** — leave the signing config exactly as the
  repo sets it.
- If signing fails for any reason, stop and tell me what happened. A failed
  signed commit is the correct outcome; an unsigned commit is never acceptable.

## Push

Push to my fork. `origin` = my fork, `push.autoSetupRemote = true`, so a plain
`git push` sets up tracking to origin on first push. That's the default — no
`-u` needed. Just push and report the branch.

**NEVER push to `upstream`.** I work off forks; `origin` is my fork and the only
thing you ever push to. `upstream` is the org repo I don't have (or don't use)
write access to — pushing there is wrong, and a **force-push there would be a
disaster**. Always push to `origin` explicitly when there's any ambiguity, and
**never** pass a remote of `upstream` to `git push` under any circumstance,
force or not. If `git push` would target anything other than my fork, stop and
flag it instead of pushing.

## Open PR — GATED

Fork → upstream, so the invocation is explicit (no default repo is set; a bare
`gh pr create` would prompt). Derive the pieces from the remotes:

- upstream `OWNER/REPO` from `git remote get-url upstream`
- fork owner from `git remote get-url origin`
- current branch from `git branch --show-current`

```bash
gh pr create \
  --repo <upstream-owner>/<repo> \
  --base <upstream-default-branch> \
  --head <fork-owner>:<branch> \
  --title "<title>" \
  --body "<body>" \
  [--draft]
```

- **Title** mirrors the commit convention/voice.
- **Body** is terse per the tone rules. No template scaffolding.
- **`--draft`** when the work is incomplete, large, or not meant to land yet
  (I draft big slices on purpose).
- **Duplicate guard**: before creating, check
  `gh pr list --repo <upstream> --head <fork-owner>:<branch>`. If a PR already
  exists, *don't* create another — the branch is already pushed, so just report
  the existing PR URL and stop. (Updating an existing PR's body/commits is out
  of scope for this skill.)

**Then stop and show me the exact title + body + flags, and wait.** Only run
`gh pr create` after I explicitly say go. Report the PR URL when done.

## Watch CI → ping me — opt-in only

**Off by default.** Only watch if I opt in — either when I invoke `/ship`, or at
the PR review gate. Don't offer it; don't watch unprompted.

When I opt in, that's the whole instruction: **ping me when the PR goes green.**
Don't ask follow-ups about it.

- Drive the watch with a `/loop` that re-polls `gh pr checks` on an interval
  until checks reach a terminal state. Don't narrate each poll.
- CI takes time to register and spin up, so an early poll can show *no checks
  reported* or everything *pending* — that is "not started yet," not "green."
  Only conclude an outcome once checks have appeared *and* every one has left
  pending. Keep looping until then.
- **green** → send a `PushNotification`, e.g.
  `PR #<n> "<title>" is green — ready for a reviewer`. This is the one I asked
  for.
- **failed** → don't reflexively ping. Triage first:
  - **Small / mechanical** (lint, formatting, import order, an obviously flaky
    test) → use your judgment and just fix it, or re-run a flake. Don't pull me
    in for these. Ping me only with the *resolved* result if it matters, or stay
    silent and let the green ping land when CI re-passes.
  - **Needs my attention** (a real test failure, a design/logic problem, ambiguous
    cause, anything where you'd be guessing at intent) → `PushNotification`,
    e.g. `PR #<n> "<title>" CI failed: <failing checks>`. Bias toward pinging
    when unsure whether it's "small."
- The watch only lives as long as the session does; if it dies with the session,
  that's fine, don't try to daemonize it.

### How to land a CI fix

When you fix a CI failure (or any cleanup), match the commit to the *nature* of
the change:

- **Cleanup / fixups** (addressed lints, formatting, a flaky re-run, review
  nits) → **amend into the relevant commit and force-push** with
  `git push --force-with-lease` **to `origin` (my fork) — never `upstream`**.
  Never a `fix: addressed lints` commit — that noise doesn't belong in my
  history.
- **Genuinely new functionality** → a new commit is fine, in my usual voice.

If the fix targets a commit deeper in a stack, use `--fixup` + autosquash rather
than amending the tip.
