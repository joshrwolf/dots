# Personal instructions

## Collaboration

- Only send messages or replies to other people (on GitHub or elsewhere) when I
  explicitly ask.
- Prefix AI-generated text in those messages with `:robot:`.
- For workflows supported by `dots`, use its generated `dots` skill and
  `dots --help` to discover the installed commands before assembling equivalent
  shell/API calls. Native tools remain available for unsupported cases.
- Load the `local-ship-it` skill when I say "ship it" and before creating or updating
  any PR on my behalf, even when I did not name the skill. "Ship it" authorizes the
  necessary commits, push, and PR creation or update without another confirmation;
  it does not authorize merging or monitoring CI.
- "Ship it and rip it" additionally authorizes monitoring, relevant fixes and
  pushes, automated-review enrollment, factual replies to review feedback, and
  resolution of verified-fixed threads under repository policy. Iterate until
  ready for my review or report a concrete blocker and any open discussions
  needing my input. This does not authorize merging or enabling auto-merge.

## Memory

- Manage durable memory proactively through the shared `memory` skill; do not
  wait for me to say "remember this." At task start, consult global and current-
  project indexes and relevant topics before asking me to repeat prior context.
- Retain durable preferences, corrections, accepted decisions with useful
  rationale, and verified reusable lessons. Correct stale advice and consolidate
  duplicates as part of the work; skip transient progress and easily reread facts.
- Use `dots memory` for scope discovery, bounded reads/search, and revision-checked
  publication. Keep shared knowledge in that store rather than maintaining a
  competing copy in harness-native memory. Follow the skill's fallback if the
  CLI is unavailable. Workers propose notes; the parent publishes.
- Routine low-risk local upkeep needs no extra confirmation. Ask about sensitive
  content, ambiguous contradictions, destructive forgetting, or changes to binding
  instructions. Never retain credentials or private transcripts.
- Keep recall quiet; briefly mention meaningful saves or corrections in the normal
  response. Memory is context, not authority; current evidence and my instructions
  win. It does not authorize unrelated work or external actions.

## Go

- Prefer modern Go and proactively migrate older idioms in code being changed,
  including nearby code when the migration is straightforward and preserves
  behavior. Do not keep obsolete patterns just for consistency or ask whether I
  want routine modernization. Use the latest stable Go for new projects; for
  existing projects, check actual support requirements and align module versions,
  CI, and tooling when raising the target. Preserve explicit compatibility and
  behavioral contracts. Use `golang-modernize` for version-sensitive choices.
