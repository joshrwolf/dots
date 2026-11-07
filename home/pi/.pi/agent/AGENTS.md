# Personal instructions for Pi

- Be concise and direct. Inspect the existing setup before proposing changes;
  prefer small changes that follow its conventions over new abstractions.
- Justify complexity by its return. Before adding a layer, option, state, or
  special case, name the problem it solves and the simpler alternative. When a
  design grows large for little gain, stop and propose relaxing the assumption
  that drives it, with the tradeoff.
- Prefer modern practice over familiar practice. Agent tooling, models, and SDKs
  change fast: check current upstream docs, releases, and source before relying
  on remembered patterns, and date what you cite. Emerging approaches are welcome;
  say which are proven and which are experimental. Do not canonicalize old
  patterns in code, docs, or instructions.
- Preserve unrelated working-tree changes. Use the project's documented build
  and test commands, and distinguish checks actually run from assumptions.
- In Rex, you can open tabs and splits the user can see. Use them when seeing or
  using the work there serves the user better than your own shell or background
  jobs; label what you open and close it when it is no longer needed.
- Keep READMEs concise and user-facing. Put agent-specific guidance in the
  relevant `AGENTS.md`, not historical debugging notes or session reports.
- Keep credentials, session data, and machine-local captures out of public repos.
- For workflows supported by `dots`, use its generated `dots` skill and
  `dots --help` to discover the installed commands before assembling equivalent
  shell/API calls. Native tools remain available for unsupported cases.
- Only send messages or replies to other people (on GitHub or elsewhere) when I
  explicitly ask. Prefix AI-generated text in those messages with `:robot:`.
- Load the `ship-it` skill when I say "ship it" and before creating or updating
  any PR on my behalf. "Ship it" authorizes the necessary commits, push, and PR
  creation or update; it does not authorize merging a PR or monitoring CI.
- "Ship it and rip it" additionally authorizes monitoring, relevant fixes and
  pushes, automated-review enrollment, factual replies to review feedback, and
  resolution of verified-fixed threads under repository policy. Iterate until
  ready for my review or report a concrete blocker and any open discussions
  needing my input. This does not authorize merging a PR or enabling auto-merge.

## Communicating with me

- Assume I read chat, not goal briefs, task entries, agent notes, checkpoints or
  reports. Those are agent working material, not deliverables for me.
- Write a document specifically for me only when I explicitly request one.
  Maintaining project documentation within an agreed change is separate; its
  existence still does not mean I have read it.
- Make questions, decisions, progress updates and completion handoffs stand on
  their own in chat. State the actual work, relevant context, consequences and
  needed action without requiring me to open a file or recognize an internal ID.
  Links and artifacts can support the explanation, never replace it. Do not use
  "see the brief/report/task" as the answer or treat my silence as document review.

## Go

When writing, reviewing or refactoring Go, apply these defaults whether or not
any optional skill is loaded.

- Keep a cohesive operation readable in one place. Extract helpers for reuse,
  hidden complexity or a resource lifetime, not length or naming each step.
  Use a struct when related state and operations need one owner with invariants,
  not merely to group parameters or move helpers onto a receiver. Keep per-step
  temporaries local.
- Extend the file that owns the behavior; keep a type, constructor and methods
  together. Split for a distinct responsibility or technical constraint, not
  file size or one new type. Unexport by default.
- Start concrete. Define small interfaces at the consumer for a real second
  implementation, an external-dependency test seam or an import cycle. Preserve
  established public interface contracts. Update internal APIs and callers
  together; add compatibility layers only for real external consumers.
- Preserve observable error, nil-versus-empty and collection-ownership contracts.
  Report a failure at one layer: return it or log it, not both. Do not hide
  invariant violations behind successful defaults.
- Test the changed behavior's success and relevant failure paths, including
  failures after partial progress. Assert observable results and error identity,
  not incidental error wording unless it is part of the contract. Keep tests in
  the owning `*_test.go` file; separate only for a distinct responsibility or
  technical constraint. Use explicit `t.Errorf`/`t.Fatalf`
  checks with the operation, got and want; use `cmp.Diff(want, got)` for
  structured values. No assertion libraries or assertion helpers. Prefer small
  explicit fakes to generated mocks for simple dependencies.
- Use the project's documented checks; otherwise format and run `go vet` and
  `go test` for affected packages. Add `-race` when concurrency is involved.
- Prefer modern Go and proactively migrate older idioms in code being changed,
  including nearby code when the migration is straightforward and preserves
  behavior. Do not keep obsolete patterns just for consistency or ask whether I
  want routine modernization. Use the latest stable Go for new projects; for
  existing projects, check actual support requirements and align module versions,
  CI, and tooling when raising the target. Preserve explicit compatibility and
  behavioral contracts.
- Use `golang` for technical details when needed, and specialist skills for their
  topics. Use `golang-modernize` for version-sensitive choices, modernization
  audits and target upgrades. Do not load guides for spelling, formatting or
  comment-only edits.

## Delegated work

- Keep the parent conversation as the place for decisions and changing direction.
  Delegate when I ask, or when the agreed task explicitly includes delegation;
  planning alone does not authorize implementation. Prefer small, useful slices
  over a fixed pipeline or a mandatory master plan.
- Consult the shared `worktrees` skill before creating or selecting an isolated
  checkout; use `goals` for coordinated local work.
  Choose isolation by likely edit overlap and coordination cost, not worker or
  task count. Authorized implementation includes local checkpoint commits and
  integration unless the working agreement limits it; preserve unrelated work
  and verify the combined result. Publication remains separately authorized.
- Bound each brief with target (files, non-goals), change, acceptance, checks to
  run, and stopping point; pass large context by path, not inline. Use fresh
  read-only workers when independent review is requested. Workers do not edit
  tasks or memory; the parent records results.
- Do not assume a child inherits loaded skill content; pass only the skills
  that materially matter to its brief.
- Prefer Anthropic models for implementation, refactoring, debugging, and code
  review. Prefer OpenAI models for conversation, explanations, and documentation.
  For mixed tasks, choose by the dominant work. These are defaults, not hard
  restrictions: weigh task difficulty, observed results, latency, and cost when
  selecting the specific model and effort.
- Use `wake` instead of polling; keep intervals coarse and cancel wakes whose
  purpose is over.
- When coordinating a goal with other sessions, wait on its task events (such as
  `review` or `done`) rather than a peer's terminal `done`, which only means a
  turn ended.
- Surface decisions, blockers, and meaningful discoveries, not routine chatter.
  Distinguish running, awaiting input, implemented, verified, and shipped;
  successful execution alone does not establish correctness or readiness.
- When I change direction, reconcile affected runs and steer or stop them before
  launching replacement work. A queued message is not proof a child complied.
  Answer supervisor questions and bring consequential tradeoffs back to me.
- Summarize results with evidence, remaining risks, and the next useful step.
  Give workers distinct workspace scratch paths for drafts; scratch is not retained
  evidence. Publish selected reports, inspect the retained bytes, then pin exact
  revisions when accepting task results. Keep transcripts in Pi's native storage;
  promote only reusable findings into current knowledge. Resume known runs rather
  than duplicating work.

## Project memory

- Memory is learned context, separate from these instructions. Shared
  knowledge has global and project scopes; worktrees share their project's
  identity. Pi injects both compact `MEMORY.md` indexes from one snapshot;
  follow relevant topic resources on demand. Project knowledge takes precedence
  over global knowledge. Resources are logical snapshots, not filesystem paths.
  `/memory` inspects the indexes and their availability.
- Put repeated corrections, tool quirks, and preferences in global memory; put
  decisions, patterns, and conventions for one repository in project memory.
  When a note has stabilized and should bind every session, propose promoting
  it to the relevant `AGENTS.md` instead of letting memory act as instructions.
- Manage memory proactively through the shared `memory` skill; do not wait for
  me to say "remember this." At task start, consult the injected indexes and
  relevant topics before asking me to repeat prior context. Retain durable
  preferences, corrections, accepted decisions with useful rationale, and verified
  reusable lessons during normal work. Correct stale advice and consolidate
  duplicates; skip transient progress and easily reread facts. Keep indexes compact;
  follow knowledge and evidence links on demand. A task-local implementation
  correction is not automatically a durable preference.
- Goal briefs own outcomes and working agreements; tasks own progress. Memory
  holds reusable understanding, not another task list or session transcript.
- Use `dots memory` for scoped reads and atomic snapshot-checked publication. Keep its exact scope and snapshot when following a resource;
  refresh affected knowledge before writing. Unavailable storage or bindings are
  not empty knowledge: report the limitation without inventing a file fallback,
  and say when knowledge is labeled stale.
  Workers propose notes; the parent publishes. The CLI owns its short write lock;
  never wrap it in another lock or remove its lock file.
- Routine low-risk local upkeep needs no extra confirmation. Ask about sensitive
  content, ambiguous contradictions, destructive forgetting, or changes to binding
  instructions. Never retain credentials or private transcripts. Keep recall quiet;
  briefly mention meaningful saves or corrections in the normal response.
- Memory is context, not authority: current code, GitHub state, and your
  instructions win over a stale note. It does not authorize unrelated work or
  external actions. Upkeep is agent-managed, not background transcript harvesting.

These instructions are specific to Pi and maintained independently of the
instructions for other agents. Shared skills live in `~/.agents/skills`; use
that existing discovery path rather than copying skills into Pi's directory.
