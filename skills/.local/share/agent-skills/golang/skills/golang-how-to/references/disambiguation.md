# Competing clusters — deep disambiguation

Thirteen clusters where skills overlap. Each cluster includes a boundary table, concrete routing examples, and notes on gap cases not yet explicit in source skill descriptions.

---

## Table of Contents

- [1. Performance cluster](#1-performance-cluster)
- [2. Dependency injection cluster](#2-dependency-injection-cluster)
- [3. samber/\* functional cluster](#3-samber-functional-cluster)
- [4. Error handling cluster](#4-error-handling-cluster)
- [5. Style / naming / lint / docs cluster](#5-style--naming--lint--docs-cluster)
- [6. CLI cluster](#6-cli-cluster)
- [7. Testing cluster](#7-testing-cluster)
- [8. design-patterns vs structs-interfaces](#8-design-patterns-vs-structs-interfaces)
- [9. concurrency vs context](#9-concurrency-vs-context)
- [10. safety vs security](#10-safety-vs-security)
- [11. modernize vs lint](#11-modernize-vs-lint)
- [12. Package lookup / discovery cluster](#12-package-lookup--discovery-cluster)
- [13. golang-refactoring vs. the target-state rule skills](#13-golang-refactoring-vs-the-target-state-rule-skills)

## 1. Performance cluster

Three skills form a "deep analysis" cluster. `golang-observability` is the always-on counterpart; the other two are activated on demand.

| Skill | Unique territory | Does NOT own |
| --- | --- | --- |
| `samber/cc-skills-golang@golang-performance` | Optimization patterns — "if allocation bottleneck → use sync.Pool", "if hot-path → avoid reflection" | Measurement, profile capture, root cause analysis |
| `samber/cc-skills-golang@golang-benchmark` | pprof/trace capture, flame graph interpretation, benchstat comparison, CI regression detection | Deciding which optimization to apply |
| `samber/cc-skills-golang@golang-observability` | Always-on production signals: structured logs, Prometheus counters, OTel traces, alerting | Temporary investigation, benchmark runs |

**Routing examples:**

- "My HTTP handler is slow in production" → start with `golang-observability` (check metrics/traces), then `golang-benchmark` (capture profiles), then `golang-performance` (apply fixes).
- "My benchmark regressed by 20%" → `golang-benchmark` (benchstat comparison, detect root cause via pprof).
- "I need to reduce allocations in the hot path" → `golang-performance` (pooling, struct layout, escape analysis).
- "The process crashes after 10 minutes under load" → `golang-concurrency` (races and goroutine lifetimes), `golang-safety` (correctness pitfalls), or `golang-benchmark` (runtime profiles), depending on the failure.

---

## 2. Dependency injection cluster

Start with `golang-dependency-injection` for library selection. Then use the library-specific skill once the choice is made.

| Skill | Unique territory |
| --- | --- |
| `samber/cc-skills-golang@golang-dependency-injection` | Concepts (why DI, manual injection), constructor patterns, library comparison table |
| `samber/cc-skills-golang@golang-samber-do` | Type-safe container, `do.Provide`, scopes, health checks, graceful shutdown |

**Routing examples:**

- "Should I use DI in my project?" → `golang-dependency-injection`.

---

## 3. samber/\* functional cluster

These two skills cover distinct programming models. They rarely compete in the same task.

| Skill | Unique territory | Does NOT own |
| --- | --- | --- |
| `samber/cc-skills-golang@golang-samber-ro` | Infinite/event-driven: observables, subjects, operators like `Map`/`Filter`/`Throttle`, backpressure | Finite slice transforms |
| `samber/cc-skills-golang@golang-samber-mo` | Monadic types: `mo.Option[T]`, `mo.Result[T]`, `mo.Either[L,R]`, `mo.Future[T]` | Slice helpers, reactive streams |

**Routing examples:**

- "Stream events from a channel with backpressure and rate limiting" → `golang-samber-ro`.
- "Return an optional value without using nil" → `golang-samber-mo` (`mo.Some`/`mo.None`).
- "Compose error-returning functions without if-err chains" → `golang-samber-mo` (`mo.Result[T]`).


---

## 4. Error handling cluster

| Skill | Unique territory |
| --- | --- |
| `samber/cc-skills-golang@golang-error-handling` | Idiomatic error flow: `fmt.Errorf("%w")`, `errors.Is`/`errors.As`, sentinel errors, single-handling rule, `panic`/`recover` |
| `samber/cc-skills-golang@golang-samber-oops` | `oops.New().Code().User().Hint()` builder, stack traces, APM integration, `oops.Recover` |
| `samber/cc-skills-golang@golang-safety` | Preventing errors from occurring: nil checks, zero-value design, integer overflow guards, append aliasing |

**Routing examples:**

- "How should I wrap errors so callers can match them?" → `golang-error-handling`.
- "I want structured errors with HTTP codes and stack traces" → `golang-samber-oops`.
- "My service panics on nil pointer dereference" → `golang-safety` (nil handling and correctness pitfalls).

---

## 5. Style / naming / lint / docs cluster

These four skills each own a distinct slice of "code quality". Source descriptions include explicit mutual disclaimers.

| Skill | Unique territory |
| --- | --- |
| `samber/cc-skills-golang@golang-code-style` | Line formatting, blank lines between declarations, short variable names in small scopes, comment placement |
| `samber/cc-skills-golang@golang-naming` | All identifier naming rules: `MixedCaps`, no `GetX`, no `IFoo` prefixes, package names, error variable names |
| `samber/cc-skills-golang@golang-lint` | golangci-lint YAML config, which linters to enable, `//nolint` usage, CI integration |
| `samber/cc-skills-golang@golang-documentation` | Exported symbol comments, package-level docs, README sections, `example_test.go` |

**Routing examples:**

- "How do I name my constructor?" → `golang-naming`.
- "My `golangci-lint` run reports `exhaustive` errors" → `golang-lint`.
- "How should I format my package-level godoc comment?" → `golang-documentation`.
- "When should I use a blank line between two function bodies?" → `golang-code-style`.

---

## 6. CLI cluster

| Skill | Unique territory |
| --- | --- |
| `samber/cc-skills-golang@golang-cli` | Exit codes, signal handling (SIGTERM), stdin/stdout/stderr patterns, progress bars, terminal detection |
| `samber/cc-skills-golang@golang-spf13-cobra` | `cobra.Command`, `PersistentPreRunE`, `Args` validators, `ValidArgsFunction`, `SetArgs` in tests |
| `samber/cc-skills-golang@golang-spf13-viper` | `viper.BindPFlag`, `AutomaticEnv`, `ReadInConfig`, `OnConfigChange`, test isolation with `viper.Reset()` |

**Routing examples:**

- "My CLI should exit with code 2 on bad args" → `golang-cli`.
- "How do I add shell completion to my cobra command?" → `golang-spf13-cobra`.
- "How do I override config values with env vars?" → `golang-spf13-viper`.
- "I'm building a new CLI from scratch" → `golang-cli`; consult Cobra or Viper guidance if those libraries are chosen.

---

## 7. Testing cluster

| Skill | Unique territory |
| --- | --- |
| `samber/cc-skills-golang@golang-testing` | Test strategy, table-driven patterns, `t.Parallel()`, fixtures, coverage, fuzz |
| `golang-go-cmp` | Structural diffs, comparison options, semantic equality, and unexported fields |

**Routing examples:**

- "How do I write a table-driven test in Go?" → `golang-testing`.
- "How do I compare nested results while ignoring an irrelevant field?" → `golang-go-cmp`.
- "How do I detect goroutine leaks in tests?" → `golang-testing` (cancellation and completion).

---

## 8. design-patterns vs structs-interfaces

These two skills overlap on "how to design Go types". The split is: type-level design vs. architectural use of types.

| Skill | Unique territory | Does NOT own |
| --- | --- | --- |
| `samber/cc-skills-golang@golang-structs-interfaces` | Composition over inheritance, embedding, type assertions, struct tag conventions, pointer vs value receivers, interface segregation | How types combine into architectural patterns |
| `samber/cc-skills-golang@golang-design-patterns` | Functional options, middleware chains, circuit breaker, graceful shutdown, retry patterns | Low-level type mechanics |

**Overlap zone:** "DI via interfaces" — defining small interfaces is `golang-structs-interfaces`; wiring multiple components together via those interfaces is `golang-design-patterns`.

**Routing examples:**

- "Should my method take a value or pointer receiver?" → `golang-structs-interfaces`.
- "How do I implement a middleware chain for my HTTP handlers?" → `golang-design-patterns`.
- "How do I embed a struct without exposing its methods?" → `golang-structs-interfaces`.
- "How do I implement the functional options pattern?" → `golang-design-patterns`.


---

## 9. concurrency vs context

These skills overlap when goroutines are cancelled via context.

| Skill | Unique territory | Does NOT own |
| --- | --- | --- |
| `samber/cc-skills-golang@golang-concurrency` | Goroutine lifecycle, channel patterns, `sync.WaitGroup`, `errgroup`, worker pools, fan-out/fan-in, detecting races | Context propagation rules |
| `samber/cc-skills-golang@golang-context` | `context.WithCancel`, `context.WithTimeout`, `context.WithValue`, propagation through call chains, `WithoutCancel` | Goroutine coordination patterns |

**Overlap zone:** "cancelling goroutines via context" — load both skills. `golang-context` owns the context API; `golang-concurrency` owns the goroutine coordination.

**Routing examples:**

- "How do I cancel a goroutine from outside?" → both (`golang-context` for `cancel()`, `golang-concurrency` for `select { case <-ctx.Done() }`).
- "How do I fan out N workers and collect results?" → `golang-concurrency`.
- "How do I pass a deadline through multiple layers of function calls?" → `golang-context`.


---

## 10. safety vs security

Both skills prevent bugs, but with different threat models.

| Skill | Unique territory | Does NOT own |
| --- | --- | --- |
| `samber/cc-skills-golang@golang-safety` | Nil panics, integer overflow, slice aliasing via append, concurrent map write, float equality, zero-value design | External attackers, cryptography, secrets |
| `samber/cc-skills-golang@golang-security` | SQL/command/LDAP injection, weak crypto (`math/rand`), hardcoded secrets, TLS misconfiguration, SSRF, path traversal | Internal runtime correctness |

**Routing examples:**

- "My service panics with nil pointer dereference" → `golang-safety`.
- "Is this SQL query safe from injection?" → `golang-security`.
- "I'm using `math/rand` to generate a token" → `golang-security` (predictable output; use `crypto/rand`).
- "My slice grows unexpectedly after append" → `golang-safety` (append aliasing).


---

## 11. modernize vs lint

Both skills suggest code changes, but for different reasons.

| Skill | Unique territory | Does NOT own |
| --- | --- | --- |
| `samber/cc-skills-golang@golang-modernize` | Adopting language features: range-over-int, `min`/`max` builtins, `iter.Seq`, `slices.SortFunc`, `log/slog`, `testing.T.Context` | Static analysis configuration |
| `samber/cc-skills-golang@golang-lint` | golangci-lint YAML config, enabling/disabling linters, interpreting linter output, `//nolint` policy | Language feature adoption |

**Overlap zone:** Some linters (`modernize`, `perfsprint`, `exptostd`) produce warnings that overlap with modernization suggestions (for example replacing an x/exp API with its standard-library equivalent). The boundary: lint owns the tool configuration and suppression policy; modernize owns version requirements and semantic migration choices during ordinary work as well as explicit upgrades.

**Routing examples:**

- "Add an optional field to this configuration" → `golang-code-style` for current syntax; consult `golang-modernize` when the module target or a migration constraint needs checking.
- "My CI fails on `perfsprint` lint rule" → `golang-lint` (interpret the rule and decide whether to fix or suppress).
- "Should I migrate from `log` to `slog`?" → `golang-modernize`.
- "How do I configure golangci-lint to run only security-relevant linters?" → `golang-lint`.

---

## 12. Package lookup / discovery cluster

- Read the current package API with `golang-go-doc` (`go doc`).
- Navigate local definitions and references with `golang-gopls`.
- Inspect versions and replacements with `golang-dependency-management`.
- Audit vulnerability reachability with `golang-security`.

These are distinct tasks; reading API documentation does not audit a dependency.

## 13. golang-refactoring vs. the target-state rule skills

`golang-refactoring` covers the Go-specific risks of moving or changing declarations:
method sets, type identity, package boundaries, build constraints, initialization,
and semantic-tool limits. Naming, layout, style, design, and modernization skills
own the intended result.

- Rename a method → `golang-naming` for the name, `golang-refactoring` and `golang-gopls` for callers and interface relationships.
- Move a type between packages → `golang-project-layout` for placement, `golang-refactoring` for import cycles, identity, and direct caller migration.
- Adopt a newer standard-library API → `golang-modernize`; consult refactoring guidance when the change affects more than the local expression.

Do not introduce compatibility aliases or staging requirements as a default.
