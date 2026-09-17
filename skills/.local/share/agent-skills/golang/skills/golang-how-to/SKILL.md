---
name: golang-how-to
description: "Select relevant Go skills when their scopes overlap, or configure project skill guidance when requested."
user-invocable: true
license: MIT
compatibility: Designed for Claude Code, Codex or similar harness.
metadata:
  author: samber
  version: "1.4.2"
  openclaw:
    emoji: "🧭"
    homepage: https://github.com/samber/cc-skills-golang
allowed-tools: Read Edit Write Glob Grep Bash(go:*) Bash(git:*) Agent AskUserQuestion LSP Bash(gopls:*) mcp__gopls__*
---

**Modes:**

- **Route** — choose the skill whose scope matches the task; consult another only for a distinct need.
- **Disambiguate** — use [disambiguation.md](references/disambiguation.md) when two skills overlap.
- **Configure** — when requested, update project guidance using [project-config.md](references/project-config.md).

## Skill loading

Start with the user's task and existing project instructions. If the relevant skill
is already clear, use it directly; this router is not a prerequisite for Go work.
The table offers related references, not a bundle to load automatically. Read
supporting skills only when their guidance helps with a concrete part of the task.
Skill consultation does not expand the requested implementation or audit scope.
Routing alone requires no tool installation or persistent configuration changes.

| Intent | Primary | Consult when relevant |
| --- | --- | --- |
| Design an API, choose a pattern | `golang-design-patterns` | `golang-structs-interfaces`, `golang-naming` |
| Name a type, function, or package | `golang-naming` | `golang-code-style` |
| Handle errors idiomatically | `golang-error-handling` | `golang-safety` (nil-heavy code) |
| Write goroutines, channels, sync | `golang-concurrency` | `golang-context` (if cancellation) |
| Pass deadlines / cancel operations | `golang-context` | `golang-concurrency` (if goroutines) |
| Design structs, embed, use interfaces | `golang-structs-interfaces` | `golang-design-patterns` |
| Database queries and transactions | `golang-database` | `golang-error-handling`, `golang-security` |
| Build a gRPC service | `golang-grpc` | `golang-testing`, `golang-error-handling` |
| Build a GraphQL API | `golang-graphql` | `golang-testing`, `golang-error-handling` |
| Build a CLI command tree | `golang-spf13-cobra` | `golang-cli`, `golang-spf13-viper` (if config) |
| Layer config from flags/env/file | `golang-spf13-viper` | `golang-spf13-cobra` |
| Write tests | `golang-testing` | `golang-go-cmp` (for structural comparisons) |
| Apply optimization patterns | `golang-performance` | `golang-benchmark` (measure first) |
| Measure with pprof / benchstat | `golang-benchmark` | `golang-performance` (fix), `golang-troubleshooting` (root cause) |
| Debug a panic or unexpected behavior | `golang-troubleshooting` | `golang-safety`, `golang-benchmark` (if perf-related) |
| Monitor in production | `golang-observability` | `golang-performance` (if SLO breach) |
| Audit security vulnerabilities | `golang-security` | `golang-safety`, `golang-lint` |
| Review formatting and style | `golang-code-style` | `golang-naming`, `golang-lint` |
| Refactor or restructure existing code | `golang-refactoring` | `golang-naming`, `golang-code-style`, `golang-project-layout` |
| Configure golangci-lint | `golang-lint` | `golang-code-style` |
| Write godoc / README / CHANGELOG | `golang-documentation` | `golang-naming` |
| Set up a new project structure | `golang-project-layout` | `golang-design-patterns`, `golang-dependency-injection`, `golang-lint` |
| Set up CI/CD pipeline | `golang-continuous-integration` | `golang-lint`, `golang-security` |
| Choose a library | `golang-popular-libraries` | relevant library-specific skill |
| Read a package's documentation or API | `golang-go-doc` | `golang-dependency-management` |
| Navigate, diagnose, or refactor local code (definitions, references, rename) | `golang-gopls` | — |
| Adopt new Go language features | `golang-modernize` | `golang-lint` |
| Use samber/oops (structured errors) | `golang-samber-oops` | `golang-error-handling` |
| Use log/slog | `golang-samber-slog` | `golang-observability`, `golang-error-handling` |
| Use dependency injection | `golang-dependency-injection` | `golang-samber-do` (when already used) |

Skill names above refer to installed skills. `golang-go-cmp` is the local
comparison skill; the other skills originate from samber/cc-skills-golang.

## Choosing how to execute

Decide whether to work directly or delegate based on the task. Use subagents when
independent investigation, implementation, or review would improve speed or
confidence enough to justify coordination and context sharing. Work directly when
the task is small, tightly coupled, or faster to complete than to hand off.
Choose the number and scope of agents to fit useful independent work, following
the environment's collaboration rules. Give delegated tasks clear boundaries and
expected results, avoid overlapping edits, and review and integrate their output.
Reassess as the task develops; neither approach requires a fixed staffing pattern.

## Tool selection

Use local semantic tools for the resolved build and package documentation for
package APIs. When choosing between gopls, go doc, and govulncheck,
read [Tool selection](references/tool-selection.md). Setup details belong to
`golang-gopls` and `golang-go-doc`.

## Categories at a glance

Full catalog with "use when" hooks: [by-category.md](references/by-category.md)

| Category | Skills |
| --- | --- |
| Code Quality | `golang-code-style` `golang-documentation` `golang-error-handling` `golang-lint` `golang-naming` `golang-safety` `golang-security` `golang-structs-interfaces` |
| Architecture & Design | `golang-concurrency` `golang-context` `golang-data-structures` `golang-database` `golang-dependency-injection` `golang-design-patterns` `golang-modernize` `golang-refactoring` |
| QA & Performance | `golang-benchmark` `golang-observability` `golang-performance` `golang-testing` `golang-troubleshooting` |
| Project Setup | `golang-cli` `golang-continuous-integration` `golang-dependency-management` `golang-gopls` `golang-go-doc` `golang-popular-libraries` `golang-project-layout` `golang-stay-updated` |
| APIs | `golang-graphql` `golang-grpc` |
| Dependency Injection | `golang-dependency-injection` `golang-samber-do` |
| Frameworks | `golang-spf13-cobra` `golang-spf13-viper` |
| samber/\* | `golang-samber-do` `golang-samber-hot` `golang-samber-mo` `golang-samber-oops` `golang-samber-ro` `golang-samber-slog` |
| Testing | `golang-go-cmp` `golang-testing` |

## Competing clusters — boundary lines

Full boundary tables with routing examples: [disambiguation.md](references/disambiguation.md)

Key clusters and their owners:

- **Performance**: `golang-performance` (optimization patterns) · `golang-benchmark` (measurement) · `golang-troubleshooting` (root cause) · `golang-observability` (always-on production)
- **DI**: `golang-dependency-injection` (constructor wiring) · `golang-samber-do` (existing container usage)
- **samber/**: `golang-samber-ro` (streams) · `golang-samber-mo` (monadic types)
- **Errors**: `golang-error-handling` (idioms) · `golang-samber-oops` (structured errors) · `golang-safety` (prevent panics)
- **Style**: `golang-code-style` · `golang-naming` · `golang-lint` · `golang-documentation`
- **CLI**: `golang-cli` (architecture) · `golang-spf13-cobra` (command tree) · `golang-spf13-viper` (config layering)
- **Package lookup**: `golang-go-doc` (read the resolved package API with go doc) · `golang-gopls` (navigate/refactor your locally resolved build) · `golang-popular-libraries` (which library to adopt) · `golang-dependency-management` (manage go.mod) · `golang-security` (whole-tree CVE scan)
- **Gap — type vs arch**: `golang-structs-interfaces` (type design) vs `golang-design-patterns` (architectural patterns)
- **Gap — goroutine vs cancel**: `golang-concurrency` + `golang-context` — load both when cancelling goroutines via context
- **Gap — correctness vs threat**: `golang-safety` (internal bugs) vs `golang-security` (external threats)
- **Gap — features vs rules**: `golang-modernize` (language adoption) vs `golang-lint` (static analysis config)
- **Gap — process vs target rules**: `golang-refactoring` (the safe, staged, at-scale _process_ of changing existing code — planning, ordering, gopls-driven mechanics, sequenced changes) vs `golang-naming`/`golang-code-style`/`golang-project-layout`/`golang-design-patterns`/`golang-modernize` (what the resulting code should look like) — load `golang-refactoring` alongside whichever of these owns the target shape

## Configure mode

Only change project skill configuration when the user requests it. Follow
[project-config.md](references/project-config.md) for scoped guidance that preserves
existing instructions and avoids compulsory skill bundles.

---

This skill is not exhaustive. Refer to individual skill files and the official Go documentation for detailed guidance.
