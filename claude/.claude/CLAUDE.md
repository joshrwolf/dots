## Response Style

Answer first, no preamble or postamble. Don't restate my question or narrate
what I'm about to do. Don't re-describe a diff in prose — the diff is the
evidence; add one line only if something isn't obvious from it.

Default to the shortest response that fully answers; expand only when the task
is genuinely complex. Rough ceiling for a normal question: a few sentences, not
a report.

When asked for honest architectural feedback, give it directly. Don't defend
existing implementations by default.

## GitHub

NEVER post comments, replies, or reviews on GitHub PRs/issues on my behalf.
Show me the proposed response and let me post it myself.

## Go code

For ANY Go work — reading, editing, refactoring, navigating, or running tooling
on .go files — you MUST load the `gotools` skill first. It covers LSP
navigation, gopls for rename/format/imports, and the diagnostics hook. Not
optional.

## Comments in code

Default to NO comment. The safest way to not write a bad comment is to not
write one — only add a comment when the code genuinely can't carry the meaning
itself. Earn it with a non-obvious invariant, a constraint, a gotcha, a bug
workaround, or a genuine surprise. Never restate what the code already shows;
never narrate what other components do as a result.

A comment describes *state* — the code as it permanently is — never *change*:

- No history or transient state. The artifact has no "before." Nothing that
  orients to the edit or a past version: "previously", "no longer", "used to",
  "we now", "refactored to", "as part of". ("Currently" for runtime state is
  fine — "the queue is currently draining".)
- No pointers out of the code. No design docs, ADRs, RFCs, plans, phases, or
  PR/ticket numbers. That context belongs in the commit and the tracker; in
  source it rots the moment the doc moves. Put the fact itself in the comment,
  not a reference to where the fact is explained.

Doc comments on exported APIs say what it does and why a caller cares — length
earned by real content, not padding:

    // Get returns the value and true, or zero and false if absent or expired.
    // A nil cache is valid and always misses, so callers need not nil-check.

Bad → good. Transient/history:

    // we now use a buffered channel instead of the old mutex
    // buffered so producers never block on a slow consumer

Reference out of the code:

    // implements the retry strategy from the reliability RFC (docs/adr/0007)
    // retries on 5xx with jittered backoff; gives up after 30s total

## Subagents

Don't spawn a subagent for work you can do directly in one response (read one
file, edit a function you can see, run one grep). Fan out multiple subagents in
the same turn for independent items. Prefer foreground unless I have other work
to do while waiting.
