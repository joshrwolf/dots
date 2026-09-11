---
name: deslopify
description: Strip AI slop from code — narration comments, history comments, defensive over-engineering, needless abstraction, and marketing prose. Tuned for the comment slop Opus 4.x leaks despite CLAUDE.md. Invoke with a scope (defaults to the current diff).
argument-hint: "[scope: defaults to current diff; or a file, package, or description]"
---

# deslopify

Find and remove AI slop — the cruft a model adds that a human writing the same
change would not. The headline offender is **comments**: Opus 4.x narrates,
journals, and restates the code even when CLAUDE.md forbids it. This skill is the
cleanup pass that catches what leaked through.

## Scope

**Default to the diff**, not the repo. With no argument, review
`git diff $(git merge-base HEAD main)...HEAD` plus unstaged/staged changes.

**An argument overrides the default and sets the scope explicitly** — review
exactly what's named, diff or not:

- `/deslopify foo/` — the whole `foo/` package/dir.
- `/deslopify cmd/server/main.go` — one file.
- `/deslopify the new auth code` — resolve with Glob/Grep, confirm if ambiguous.
- `/deslopify diff and also bar/` — diff *plus* extra scope; widen, don't replace.

Slop is defined relative to scope: a comment or guard that's *outside the
requested change and inconsistent with the surrounding human-written code* is
slop. Within an explicit file/package scope, judge against the rest of that file —
the same line in untouched pre-existing code may be fine. Read all the code under
review before editing.

## What to apply directly vs. confirm

- **Comment removals, narration, dead defensive checks, `as any`/`# type: ignore`
  escapes, emoji** — apply directly. These are unambiguous and low-risk.
- **Structural changes** (collapsing an abstraction, removing a back-compat alias,
  inlining a helper) — these change behavior surface; show the change and apply,
  but flag anything you're unsure preserves logic.
- **Never** alter business logic to "clean up." Slop removal preserves behavior.

## The hit list

Comments are first because they're the worst. Read `references/patterns.md` for
the full catalog, before/after examples, and grep detectors — load it once at the
start of a run.

### Comment slop (primary target)

Reuse the global CLAUDE.md doctrine: a comment describes permanent *state*, never
*change*; no history, no pointers out of the code; default to none. Delete:

- **Restatement** — comment paraphrases the line below it (`// increment counter`
  over `counter++`, `// loop over users` over the loop).
- **Narration** — `// Now we…`, `// First,…`, `// Here we…`, `// Let's…`. The
  model talking through what it's about to write.
- **History / journal** — `// we now use…`, `// previously…`, `// no longer…`,
  `// refactored to…`, `// removed the old…`, `// as part of…`. Orients to a past
  state the artifact doesn't have.
- **Pointers out** — `// see PR #123`, `// per the RFC`, `// TODO(plan)`. Put the
  fact in the comment or delete it.
- **Padded doc comments** — exported-API docs that restate the signature without
  saying what a caller actually needs to know. Trim to real content or cut.

A surviving comment must earn it: a non-obvious invariant, a constraint, a gotcha,
a bug workaround, a genuine surprise.

### Structural slop

- **Defensive over-engineering** — try/catch, nil/guard checks, validation on
  inputs that arrive already-validated from trusted callers; abnormal for the file.
- **Type escape hatches** — `as any`, `@ts-ignore`, `# type: ignore`, `# noqa`
  added to silence an error rather than fix it.
- **Needless abstraction** — single-use helper, wrapper that only forwards args, a
  new util duplicating an existing one. Inline it.
- **Back-compat cruft** — export aliases and shims no caller needs. Remove and fix
  call sites.
- **Inline imports** — mid-function `await import()` / local imports that aren't
  lazy by necessity. Hoist.
- **Magic numbers / over-verbose names** where a named const or the type already
  carries the meaning.

### Prose slop (READMEs, PR bodies, docs, comments-as-prose)

Marketing voice and AI tells: "It's not X, it's Y", rule-of-three padding, em-dash
addiction, "it's worth noting / notably / moreover", false ranges ("from X to Y"),
`→` arrows, bold-first bullets, emoji. State the thing plainly. See
`references/patterns.md` for the full list.

## Process

Separate *review* from *fixes*. Reviewing is bulk pattern-matching that Sonnet
handles fine; applying fixes needs judgment about what preserves behavior and is
the slow, careful part — always keep fixes in the main loop on the default model.

1. **Resolve scope** and enumerate the files under review (the diff's changed
   files, or the files in the named path).

2. **Review.** Whether to fan out is your call based on scope size — fan-out buys
   parallelism but costs agent-spawn overhead, so it only pays once there's real
   volume. Judge by changed files / lines, not a hard rule:
   - **Trivial scope (a small diff, one or two files):** just review inline. Don't
     spawn agents to skim a handful of lines.
   - **Worth parallelizing:** fan out **report-only** agents on `model: sonnet`
     (they never edit). Size and split the fan-out to the work — by dimension
     (comment / structural / prose) for a focused mid-size change, or by file/chunk
     for a package, each agent covering all dimensions over its slice. Cap
     concurrency reasonably. Comment slop is the priority dimension either way.

   Pass reviewers the slop doctrine and tell them to read `references/patterns.md`.
   Findings come back as a flat list: `file:line — kind — the slop — the fix`.
   Grep finds suspects; only an in-context read convicts. No speculative findings.

3. **Triage and apply on the default model.** Collect findings, discard false
   positives and not-worth-the-churn churn. Apply unambiguous removals directly;
   show structural changes as you make them. Never alter business logic.

4. **Report terse.** One block: what was stripped (grouped by kind, with counts),
   and anything deliberately *kept* with a one-line why. No essay. If nothing was
   slop, say so in one line.
