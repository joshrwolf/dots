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

**Dependencies:** `gopls` (primary actuator) — `go install golang.org/x/tools/gopls@latest`. Optional: `golangci-lint`, `benchstat`, `deadcode`, `eg`. Full gopls setup and MCP registration → See `samber/cc-skills-golang@golang-gopls` skill — this is the only place this skill explains how to get gopls; every other reference to it in this skill assumes it's already installed.

# Go Refactoring — Safe Change at Scale

- Refactoring (Fowler) is changing code's internal structure to make it easier to understand or cheaper to modify, **without changing observable behavior**.
- Semantic tools reduce manual-edit mistakes and check many static conflicts; they do not prove runtime behavior preservation.
- Review runtime interface assertions, reflection, serialized names, generated code, and external consumers where affected; verify behavior with focused tests.

## Go-specific refactoring concerns

- Keep code movement distinguishable from behavioral or performance changes so
  their effects can be checked independently.
- **Prefer gopls Rename/Inline over LLM hand-edits.**
  - Rename checks static name conflicts and visible interface conversions; Inline accounts for evaluation order and side effects. Inspect the resulting diff and verify affected behavior, including dynamic uses the tool cannot resolve.
  - Use semantic edits when available to reduce missed references; still check workspace boundaries and affected consumers.
- For repeated mechanical transforms, [go-tooling.md](references/go-tooling.md)
  compares `gofmt -r`, `eg`, and `go/analysis` fixers and their limitations.
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
  - Explicit json/db tags can intentionally preserve the wire name across a Go-field rename; untagged fields and reflection-based names may change behavior.
- **Consult focused guidance when a change introduces a specific risk.** Use
  `golang-security` for trust boundaries or security-sensitive behavior and
  `golang-safety` for correctness hazards. A logic edit alone does not require
  loading both skills or auditing unrelated code.
## Risk Stratification

| Risk | Transforms | Safety requirement |
| --- | --- | --- |
| **Low** | gopls Rename, Extract Variable/Constant, Inline Variable, `gofmt -s`, organize imports, local `refactor.rewrite.*` actions | Build/vet/test after the step is enough |
| **Medium** | Extract Function/Method (Extract is best-effort — verify comments/behavior survived), Inline Call across packages, single-parameter add/remove, introducing generics | Add or confirm targeted tests over the blast radius first |
| **High** | Change signature across many callers, moving types/functions across packages, splitting/merging packages, breaking import cycles, exported-API or major-version changes | Tests over affected callers and resolved compatibility requirements |

## Detailed References

- **[catalog.md](references/catalog.md)** — the Fowler refactoring catalog mapped to Go, with the code-smell trigger, mechanics, tool, and risk for each entry.
- **[go-tooling.md](references/go-tooling.md)** — gopls code actions, CLI invocation, `gofmt -r`, `eg`, `go/analysis`/`//go:fix inline`, `dave/dst`, and the deprecated-tool notes.
- **[safety-net.md](references/safety-net.md)** — coverage limitations, characterization tests, Go test seams, and verification commands.
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
