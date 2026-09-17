---
name: golang-refactoring
description: "Plan and execute Go refactors. Use for renaming, extracting functions, moving code across packages, breaking import cycles, or sequencing structural changes."
user-invocable: true
license: MIT
compatibility: Designed for Claude Code, Codex or similar harness, and for projects using Golang. Requires gopls and git.
metadata:
  author: samber
  version: "1.1.1"
  openclaw:
    emoji: "♻️"
    homepage: https://github.com/samber/cc-skills-golang
    requires:
      bins:
        - go
        - gopls
    install:
      - kind: go
        package: golang.org/x/tools/gopls@latest
        bins: [gopls]
      - kind: go
        package: golang.org/x/perf/cmd/benchstat@latest
        bins: [benchstat]
    skill-library-version: "0.20.0"
allowed-tools: Read Edit Write Glob Grep Bash(go:*) Bash(golangci-lint:*) Bash(git:*) Bash(gh:*) Bash(gopls:*) Bash(benchstat:*) LSP mcp__gopls__* Agent AskUserQuestion EnterWorktree ExitWorktree WebFetch WebSearch
paths:
  - "**/*.go"
---

> Apply repository instructions and required project skills first. These guidelines are defaults where the project leaves a choice.

**Modes:**

- **Plan** — map affected symbols and dependencies; scale the plan to the change.
- **Execute** — implement the authorized refactor in coherent, verifiable steps.
- **Review** — check behavior preservation and the affected callers.

Use the existing request and session decisions as authorization. Clarify material
unresolved behavior or compatibility requirements; routine file moves, deletions,
and package edits within an agreed refactor do not require another approval gate.
See [workflow.md](references/workflow.md).

**Dependencies:** `gopls` (primary actuator) — `go install golang.org/x/tools/gopls@latest`. Optional: `golangci-lint`, `benchstat`, `deadcode`, `eg`. Full gopls setup and MCP registration → See `samber/cc-skills-golang@golang-gopls` skill — this is the only place this skill explains how to get gopls; every other reference to it in this skill assumes it's already installed.

# Go Refactoring — Safe Change at Scale

- Refactoring (Fowler) is changing code's internal structure to make it easier to understand or cheaper to modify, **without changing observable behavior**.
- Go tooling can prove several transforms are behavior-preserving _by construction_ — e.g. gopls refuses a Rename rather than risk a broken build.
- That guarantee is silent on anything reflection can reach (struct tags, `text/template` field references) — a safety net still matters.

## The Core Loop

**Understand → Safety net → Small tool-driven step → Verify → Review the diff.** Repeat.

1. **Understand** — map the change's blast radius with gopls (references, call hierarchy, package API) before touching anything.
2. **Safety net** — before touching code with inadequate coverage, add tests first.
   - Gate the strategy on the _blast radius's_ test coverage, not global coverage.
   - Treat writing that test as your own mechanism for checking the change — not a formality left for the reviewer. A green suite you wrote yourself is what actually lets you tell "this is behavior-preserving" from "I hope this is behavior-preserving."
   - See [safety-net.md](references/safety-net.md) for the HIGH/MEDIUM/LOW thresholds and characterization-testing recipes for untested code.
3. **Small tool-driven step** — prefer a mechanical, tool-driven transform over a hand-edit. See [go-tooling.md](references/go-tooling.md) and [catalog.md](references/catalog.md).
4. **Verify** — `go build ./... && go vet ./... && go test ./...`; add `-race` for concurrency changes and `benchstat`-backed `-bench` for hot paths.
5. **Review the diff** — confirm the intended scope and behavior. Commit when requested or covered by the agreed workflow.

## Hard Rules

- **Never mix structural and behavioral changes in one commit or PR.**
  - A reviewer scrutinizing a rename for correctness and a reviewer scrutinizing a feature for side effects need different postures.
  - Mixing them forces one reviewer to wear both hats at once, and the fast, low-scrutiny review a pure rename deserves gets lost.
- **Separate code movement from optimization into reviewable steps.** Verify the
  move with builds and tests; measure the optimization separately. Choose commit
  and PR boundaries for the task rather than imposing a fixed count or line limit.
- **Prefer gopls Rename/Inline over LLM hand-edits.**
  - Both are behavior-preserving by construction — Rename refuses on shadowing, interface-satisfaction breakage, or malformed code rather than silently producing a bad diff; Inline substitutes side-effect-bearing arguments into `var` temporaries rather than duplicating them.
  - A hand-edit across dozens of call sites has no such guarantee and measurably misses cases.
- **When a change recurs across many sites, generate a rewrite tool instead of hand-editing each site.**
  - Escalate `gofmt -r` → `eg` → a `go/analysis` fixer, in order of increasing power (see [go-tooling.md](references/go-tooling.md)).
  - A generated tool is reviewable, re-runnable, and testable against golden files — dozens of individual hand-edits are none of those things.
- **Move declarations and update affected callers directly.** Remove obsolete
  declarations and imports, then verify affected packages and consumers. Do not
  introduce compatibility aliases, forwarding wrappers, or deprecation stages by
  default. If actual external consumers impose a compatibility contract, resolve
  that requirement before changing the public API. See [structural.md](references/structural.md).

- **Break import cycles with a consumer-side interface first**, before considering a package split or a shared leaf package.
  - Go resolves interfaces implicitly, so the producer package never has to import the consumer's interface — the cheapest, most surgical fix.
  - See [structural.md](references/structural.md).
- **Grep for tag and reflection references after any rename.**
  - gopls Rename only guards against _compilation_ breakage — it cannot see a struct tag, a `text/template` field reference, or a `reflect`-driven dispatch that still points at the old name.
  - Renaming a field silently desyncs it from its `json`/`db` tag.
- **Consult focused guidance when a change introduces a specific risk.** Use
  `golang-security` for trust boundaries or security-sensitive behavior and
  `golang-safety` for correctness hazards. A logic edit alone does not require
  loading both skills or auditing unrelated code.
- **Inspect the working tree before editing and preserve unrelated changes.**
  Verify each coherent step. Investigate failures and fix or undo your own edits;
  do not require a clean tree, automatically commit, or reset someone else's work.

## When Not to Refactor

Refactoring is an investment that only pays off if a future change is coming to spend it on. Question it — or skip it — when:

- **The code works and nothing planned will touch it again.**
  - A stable, rarely-read package earns nothing from being restructured for its own sake.
  - The risk of even a small staged refactor has to be repaid by an easier next change, and there may not be one.
- **It's critical production code with no tests.** Don't refactor it directly.
  - Establish a characterization-test baseline for the affected behavior. If the required behavior is unclear, clarify it before changing that behavior.
- **The deadline is tight.**
  - A broad refactor needs enough time for verification and review.
  - Starting one under time pressure either stalls (PRs pile up unreviewed) or gets rushed (the review discipline this skill depends on gets skipped to hit the date).
  - Make the minimal safe change now and stage the larger refactor for when there's room for it.
- **There's no clear purpose.**
  - "Refactor this" with no reason behind it — no upcoming feature it'll make easier, no bug class it'll close off, no smell a review actually flagged — is refactoring for its own sake.
  - Infer the purpose from the request and context; clarify only if the missing purpose changes the work.

## Risk Stratification

| Risk | Transforms | Safety requirement |
| --- | --- | --- |
| **Low** | gopls Rename, Extract Variable/Constant, Inline Variable, `gofmt -s`, organize imports, local `refactor.rewrite.*` actions | Build/vet/test after the step is enough |
| **Medium** | Extract Function/Method (Extract is best-effort — verify comments/behavior survived), Inline Call across packages, single-parameter add/remove, introducing generics | Add or confirm targeted tests over the blast radius first |
| **High** | Change signature across many callers, moving types/functions across packages, splitting/merging packages, breaking import cycles, exported-API or major-version changes | Tests over affected callers and resolved compatibility requirements |

**Diagnose:** 1- gopls refusing a Rename or Inline is a real semantic hazard, not a tool bug — investigate the shadowing/interface conflict before forcing the change by hand 2- `go vet ./...` / `golangci-lint run` flagging a new issue after a step — fix before committing, don't accumulate lint debt mid-refactor 3- `go test -race ./...` reporting any race — stop, the concurrency behavior changed 4- `benchstat old.txt new.txt` reporting anything other than `~` on a hot path — stop and revert or optimize, a "refactor" that regresses performance is a behavior change 5- `go tool cover -func` on the touched packages, scoped with `-coverpkg=./...` — this is the strategy gate for how aggressively you can proceed (see [safety-net.md](references/safety-net.md))

## Workflow

Use [workflow.md](references/workflow.md) for scope, ordering, verification, and
recovery. A small refactor can be one coherent change; larger migrations may need
stages. Branches, worktrees, delegation, commits, and publication follow the
session's agreed workflow rather than a skill-mandated sequence.

## Detailed References

- **[workflow.md](references/workflow.md)** — scope, change ordering, verification, and recovery.
- **[catalog.md](references/catalog.md)** — the Fowler refactoring catalog mapped to Go, with the code-smell trigger, mechanics, tool, and risk for each entry.
- **[go-tooling.md](references/go-tooling.md)** — gopls code actions, CLI invocation, `gofmt -r`, `eg`, `go/analysis`/`//go:fix inline`, `dave/dst`, and the deprecated-tool notes.
- **[safety-net.md](references/safety-net.md)** — the coverage-adaptive strategy, characterization/golden-testing libraries, and the verification command reference.
- **[structural.md](references/structural.md)** — breaking import cycles, package-boundary design, direct cross-package moves, and exported-API/versioning moves.

## Cross-References

- → See `samber/cc-skills-golang@golang-naming` skill for what to rename identifiers _to_ — this skill owns _how_ to apply a rename safely at scale.
- → See `samber/cc-skills-golang@golang-project-layout` skill for target directory/package layout — this skill owns the mechanics of moving code there without breaking callers.
- → See `samber/cc-skills-golang@golang-modernize` skill for version-driven idiom updates (`interface{}`→`any`, `slices`/`maps`) — a distinct concern from structural refactoring, though it shares the same tool-first discipline.
- → See `samber/cc-skills-golang@golang-code-style` skill for control-flow clarity and function-shape rules this skill helps you apply mechanically.
- → See `samber/cc-skills-golang@golang-design-patterns` skill for target patterns (options struct, DI, consumer-side interfaces) this skill helps you migrate toward.
- → See `samber/cc-skills-golang@golang-testing` skill for the test-writing practices that make the safety net in this skill trustworthy.
- → See `samber/cc-skills-golang@golang-lint` skill for configuring `golangci-lint`, run here only as a post-step verification gate.
- → See `samber/cc-skills-golang@golang-security` skill (and `golang-safety`) when the change raises a specific security or correctness concern.
