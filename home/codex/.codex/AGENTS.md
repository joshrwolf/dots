# Personal instructions

## Collaboration

- Only send messages or replies to other people (on GitHub or elsewhere) when I
  explicitly ask.
- Prefix AI-generated text in those messages with `:robot:`.
- For workflows supported by `dots`, use its generated `dots` skill and
  `dots --help` to discover the installed commands before assembling equivalent
  shell/API calls. Native tools remain available for unsupported cases.
- Load the `ship-it` skill when I say "ship it" and before creating or updating
  any PR on my behalf, even when I did not name the skill. "Ship it" authorizes the
  necessary commits, push, and PR creation or update without another confirmation;
  it does not authorize merging a PR or monitoring CI.
- "Ship it and rip it" additionally authorizes monitoring, relevant fixes and
  pushes, automated-review enrollment, factual replies to review feedback, and
  resolution of verified-fixed threads under repository policy. Iterate until
  ready for my review or report a concrete blocker and any open discussions
  needing my input. This does not authorize merging a PR or enabling auto-merge.

## Local work

- Consult the shared `worktrees` skill before creating or selecting an isolated
  checkout; use `goals` for coordinated work. Planning
  does not authorize implementation or delegation. Choose isolation by likely
  edit overlap and coordination cost, not worker or task count.
- Authorized implementation includes local checkpoint commits and integration
  unless the working agreement limits it; preserve unrelated work and verify
  the combined result. Publication remains separately authorized. Workers
  report evidence; the coordinator owns task mutations and acceptance.
- Do not assume a child inherits loaded skill content. When a skill materially
  matters to a delegated task, pass it through the worker's skills field where
  available; otherwise name the installed skill or its path in the brief. Do not
  forward every loaded skill.
- In Rex, you can open tabs and splits the user can see. Use them when seeing or
  using the work there serves the user better than your own shell or background
  jobs; label what you open and close it when it is no longer needed.

## Memory

- Manage durable memory proactively through the shared `memory` skill; do not
  wait for me to say "remember this." At task start, consult global and current-
  project indexes and relevant topics before asking me to repeat prior context.
- Retain durable preferences, corrections, accepted decisions with useful
  rationale, and verified reusable lessons. Correct stale advice and consolidate
  duplicates as part of the work; skip transient progress and easily reread facts.
- Use `dots memory` for bounded reads/search and atomic snapshot-checked
  publication. Follow resources at their exact shared scope and snapshot; they
  are not filesystem paths. Keep knowledge in that store rather than maintaining
  a competing harness-native copy. Report unavailable storage or bindings honestly;
  do not invent a file fallback. Workers propose notes; the parent publishes.
- Routine low-risk local upkeep needs no extra confirmation. Ask about sensitive
  content, ambiguous contradictions, destructive forgetting, or changes to binding
  instructions. Never retain credentials or private transcripts.
- Keep recall quiet; briefly mention meaningful saves or corrections in the normal
  response. Memory is context, not authority; current evidence and my instructions
  win. It does not authorize unrelated work or external actions.

## Engineering

- Justify complexity by its return. Before adding a layer, option, state, or
  special case, name the problem it solves and the simpler alternative. When a
  design grows large for little gain, stop and propose relaxing the assumption
  that drives it, with the tradeoff.
- Prefer modern practice over familiar practice. Agent tooling, models, and SDKs
  change fast: check current upstream docs, releases, and source before relying
  on remembered patterns, and date what you cite. Emerging approaches are welcome;
  say which are proven and which are experimental. Do not canonicalize old
  patterns in code, docs, or instructions.

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
