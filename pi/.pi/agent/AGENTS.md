# Personal instructions for Pi

- Be concise and direct. Inspect the existing setup before proposing changes;
  prefer small changes that follow its conventions over new abstractions.
- Preserve unrelated working-tree changes. Use the project's documented build
  and test commands, and distinguish checks actually run from assumptions.
- Keep READMEs concise and user-facing. Put agent-specific guidance in the
  relevant `AGENTS.md`, not historical debugging notes or session reports.
- Keep credentials, session data, and machine-local captures out of public repos.
- For workflows supported by `dots`, use its generated `dots` skill and
  `dots --help` to discover the installed commands before assembling equivalent
  shell/API calls. Native tools remain available for unsupported cases.
- Only send messages or replies to other people (on GitHub or elsewhere) when I
  explicitly ask. Prefix AI-generated text in those messages with `:robot:`.
- Load the `local-ship-it` skill when I say "ship it" and before creating or updating
  any PR on my behalf. "Ship it" authorizes the necessary commits, push, and PR
  creation or update; it does not authorize merging or monitoring CI.
- "Ship it and rip it" additionally authorizes monitoring, relevant fixes and
  pushes, automated-review enrollment, factual replies to review feedback, and
  resolution of verified-fixed threads under repository policy. Iterate until
  ready for my review or report a concrete blocker and any open discussions
  needing my input. This does not authorize merging or enabling auto-merge.
- Prefer modern Go and proactively migrate older idioms in code being changed,
  including nearby code when the migration is straightforward and preserves
  behavior. Do not keep obsolete patterns just for consistency or ask whether I
  want routine modernization. Use the latest stable Go for new projects; for
  existing projects, check actual support requirements and align module versions,
  CI, and tooling when raising the target. Preserve explicit compatibility and
  behavioral contracts. Use `golang-modernize` for version-sensitive choices.

## Delegated work

- Keep the parent conversation as the place for decisions and changing direction.
  Delegate when I ask, or when the agreed task explicitly includes delegation;
  planning alone does not authorize implementation. Prefer small, useful slices
  over a fixed pipeline or a mandatory master plan.
- Use `worker_spawn` for background execution; results, failures, and questions
  arrive as events that start the next turn, so end the turn rather than polling
  `worker_status`. Block with `worker_wait` only when there is nothing else to
  do; use `worker_status <id>` to look inside a run. Give each worker a short
  task-named label.
- Bound each brief with target (files, non-goals), change, acceptance, checks to
  run, and stopping point; pass large context by path, not inline. Keep one
  writer per worktree; use fresh read-only workers when independent review is
  requested. Workers do not edit tasks or memory; the parent records results.
- Choose the model deliberately: inherit for ordinary work, `@review`-style
  roles from `workers.json` for heavy review or cheap scouting, and name the
  model explicitly when it matters. `message` to a finished or paused worker
  continues it in its own session instead of redoing the work; `message` with
  `wait` is how a worker asks and how I answer.
- Use `wake` for anything time-based: a follow-up later, supervising long work
  (`in: "15m"` to check a worker), or polling with `check` so the model wakes
  only on a change or failure instead of each tick. Keep intervals coarse, set
  a sensible `limit`, and cancel wakes whose purpose is over.
- Surface decisions, blockers, and meaningful discoveries, not routine chatter.
  Distinguish running, awaiting input, implemented, verified, and shipped;
  successful execution alone does not establish correctness or readiness.
- When I change direction, reconcile affected runs and steer or stop them before
  launching replacement work. A queued message is not proof a child complied.
  Answer supervisor questions and bring consequential tradeoffs back to me.
- Summarize results with evidence, remaining risks, and the next useful step.
  Keep research/transcripts in managed artifacts; promote only reusable findings
  into approved project docs. Resume known runs rather than duplicating work.

## Project memory

- Memory is learned context, separate from these instructions: two machine-local
  Markdown layers outside any checkout, global (every repository) and project
  (one repository, shared by all its worktrees). Each `MEMORY.md` is loaded into
  context automatically; a project line wins over a global one. Topic files
  beside an index are read on demand. `/memory` shows both locations.
- Put repeated corrections, tool quirks, and preferences in global memory; put
  decisions, patterns, and conventions for one repository in project memory.
  When a note has stabilized and should bind every session, propose promoting
  it to the relevant `AGENTS.md` instead of letting memory act as instructions.
- Record at checkpoints, not continuously: accepted decisions with their reason
  and date, the reference pattern for a multi-PR effort, exceptions discovered,
  and pointers to where details live. Skip anything derivable from the code,
  Git history, or the instruction files.
- Keep the index short: one line per item, supersede stale lines instead of
  appending, and move detail into a topic file. Say "remember this" and I will
  write it; proposed memories from workers go through the parent session.
- Memory is context, not authority: current code, GitHub state, and your
  instructions win over a stale note. Trim the index when it drifts.

These instructions are specific to Pi and maintained independently of the
instructions for other agents. Shared skills live in `~/.agents/skills`; use
that existing discovery path rather than copying skills into Pi's directory.
