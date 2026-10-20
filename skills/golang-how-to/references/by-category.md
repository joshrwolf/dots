# Golang skills — full catalog by category

39 skills. Select by task; the markers do not require loading a bundle. Skills marked ⚙️ can be superseded by a company-specific skill.

---

## Table of Contents

- [Code Quality](#code-quality)
  - [`samber/cc-skills-golang@golang-code-style` ⭐️ ⚙️](#sambercc-skills-golanggolang-code-style-%EF%B8%8F-%EF%B8%8F)
  - [`samber/cc-skills-golang@golang-documentation` ⭐️ ⚙️](#sambercc-skills-golanggolang-documentation-%EF%B8%8F-%EF%B8%8F)
  - [`samber/cc-skills-golang@golang-error-handling` ⭐️ ⚙️](#sambercc-skills-golanggolang-error-handling-%EF%B8%8F-%EF%B8%8F)
  - [`samber/cc-skills-golang@golang-lint`](#sambercc-skills-golanggolang-lint)
  - [`samber/cc-skills-golang@golang-naming` ⭐️ ⚙️](#sambercc-skills-golanggolang-naming-%EF%B8%8F-%EF%B8%8F)
  - [`samber/cc-skills-golang@golang-safety` ⭐️](#sambercc-skills-golanggolang-safety-%EF%B8%8F)
  - [`samber/cc-skills-golang@golang-security` ⭐️ 🧠](#sambercc-skills-golanggolang-security-%EF%B8%8F-)
  - [`samber/cc-skills-golang@golang-structs-interfaces` ⚙️](#sambercc-skills-golanggolang-structs-interfaces-%EF%B8%8F)
- [Architecture & Design](#architecture--design)
  - [`samber/cc-skills-golang@golang-concurrency` ⚙️](#sambercc-skills-golanggolang-concurrency-%EF%B8%8F)
  - [`samber/cc-skills-golang@golang-context` ⚙️](#sambercc-skills-golanggolang-context-%EF%B8%8F)
  - [`samber/cc-skills-golang@golang-data-structures` ⭐️](#sambercc-skills-golanggolang-data-structures-%EF%B8%8F)
  - [`samber/cc-skills-golang@golang-database` ⭐️ ⚙️](#sambercc-skills-golanggolang-database-%EF%B8%8F-%EF%B8%8F)
  - [`samber/cc-skills-golang@golang-dependency-injection` ⚙️](#sambercc-skills-golanggolang-dependency-injection-%EF%B8%8F)
  - [`samber/cc-skills-golang@golang-design-patterns` ⭐️ ⚙️](#sambercc-skills-golanggolang-design-patterns-%EF%B8%8F-%EF%B8%8F)
  - [`samber/cc-skills-golang@golang-modernize` ⭐️](#sambercc-skills-golanggolang-modernize-%EF%B8%8F)
- [QA & Performance](#qa--performance)
  - [`samber/cc-skills-golang@golang-benchmark` 🧠](#sambercc-skills-golanggolang-benchmark-)
  - [`samber/cc-skills-golang@golang-observability` ⚙️](#sambercc-skills-golanggolang-observability-%EF%B8%8F)
  - [`samber/cc-skills-golang@golang-performance` 🧠](#sambercc-skills-golanggolang-performance-)
  - [`samber/cc-skills-golang@golang-testing` ⭐️ 🧠 ⚙️](#sambercc-skills-golanggolang-testing-%EF%B8%8F--%EF%B8%8F)
- [Project Setup](#project-setup)
  - [`samber/cc-skills-golang@golang-cli`](#sambercc-skills-golanggolang-cli)
  - [`samber/cc-skills-golang@golang-continuous-integration`](#sambercc-skills-golanggolang-continuous-integration)
  - [`samber/cc-skills-golang@golang-dependency-management`](#sambercc-skills-golanggolang-dependency-management)
  - [`golang-go-doc`](#golang-go-doc)
  - [`golang-gopls`](#golang-gopls)
  - [`golang-refactoring`](#golang-refactoring)
  - [`samber/cc-skills-golang@golang-popular-libraries`](#sambercc-skills-golanggolang-popular-libraries)
  - [`samber/cc-skills-golang@golang-project-layout`](#sambercc-skills-golanggolang-project-layout)
- [APIs](#apis)
  - [`samber/cc-skills-golang@golang-graphql`](#sambercc-skills-golanggolang-graphql)
  - [`samber/cc-skills-golang@golang-grpc`](#sambercc-skills-golanggolang-grpc)
- [Dependency Injection](#dependency-injection)
  - [`samber/cc-skills-golang@golang-dependency-injection` ⚙️](#sambercc-skills-golanggolang-dependency-injection-%EF%B8%8F-1)
  - [`samber/cc-skills-golang@golang-samber-do`](#sambercc-skills-golanggolang-samber-do)
- [Frameworks](#frameworks)
  - [`samber/cc-skills-golang@golang-spf13-cobra`](#sambercc-skills-golanggolang-spf13-cobra)
  - [`samber/cc-skills-golang@golang-spf13-viper`](#sambercc-skills-golanggolang-spf13-viper)
- [samber/\*](#samber)
  - [`samber/cc-skills-golang@golang-samber-do`](#sambercc-skills-golanggolang-samber-do-1)
  - [`samber/cc-skills-golang@golang-samber-hot`](#sambercc-skills-golanggolang-samber-hot)
  - [`samber/cc-skills-golang@golang-samber-mo` 🧠](#sambercc-skills-golanggolang-samber-mo-)
  - [`samber/cc-skills-golang@golang-samber-oops`](#sambercc-skills-golanggolang-samber-oops)
  - [`samber/cc-skills-golang@golang-samber-ro` 🧠](#sambercc-skills-golanggolang-samber-ro-)
  - [`samber/cc-skills-golang@golang-samber-slog`](#sambercc-skills-golanggolang-samber-slog)
- [Testing](#testing)
  - [`golang-go-cmp`](#golang-go-cmp)
  - [`samber/cc-skills-golang@golang-testing` ⭐️ 🧠 ⚙️](#sambercc-skills-golanggolang-testing-%EF%B8%8F--%EF%B8%8F-1)

## Code Quality

### `samber/cc-skills-golang@golang-code-style` ⭐️ ⚙️

Golang code formatting, conventions, and project-level style consistency — gofmt, goimports, line length, var declarations, blank lines, comment heuristics.

Use when: the user asks about formatting rules, style review, or project coding standards. Not for naming conventions (→ `golang-naming`), linter configuration (→ `golang-lint`), or doc comments (→ `golang-documentation`).

---

### `samber/cc-skills-golang@golang-documentation` ⭐️ ⚙️

Golang documentation standards — package docs, godoc conventions, example functions, README structure, CHANGELOG, API reference generation.

Use when: writing or reviewing Go doc comments, README files, or API reference. Not for code comments that explain logic (→ `golang-code-style`).

---

### `samber/cc-skills-golang@golang-error-handling` ⭐️ ⚙️

Golang error handling best practices — error creation, wrapping with fmt.Errorf and errors.Is/As, sentinel errors, custom error types, panic recovery.

Use when: writing or reviewing error propagation, wrapping, logging, or recovery patterns. For samber/oops specifics → `golang-samber-oops`. For preventing panics → `golang-safety`.

---

### `samber/cc-skills-golang@golang-lint`

Golang linting — golangci-lint configuration, presets, custom rules, CI integration, nolint suppressions, linter selection and output interpretation.

Use when: setting up or tuning golangci-lint, interpreting lint failures, or deciding which linters to enable. For style conventions not enforced by linters → `golang-code-style`.

---

### `samber/cc-skills-golang@golang-naming` ⭐️ ⚙️

Golang naming conventions across all identifier types — packages, constructors, structs, interfaces, constants, errors, receivers, acronyms, test functions. Covers MixedCaps rules, Get-prefix, and utils/helpers anti-patterns.

Use when: naming a new type, function, package, or constant. Not for broader formatting (→ `golang-code-style`).

---

### `samber/cc-skills-golang@golang-safety` ⭐️

Defensive Golang coding — prevents panics, silent data corruption, and runtime bugs. Nil safety, append aliasing, map concurrent access, float comparison, zero-value design, numeric overflow.

Use when: writing or reviewing code that could silently produce wrong results or panic. Not for external threats (→ `golang-security`) or error handling idioms (→ `golang-error-handling`).

---

### `samber/cc-skills-golang@golang-security` ⭐️ 🧠

Golang security best practices — injection prevention (SQL, command, XSS), cryptography, filesystem/network safety, secrets management, cookie security, tool configuration. For security audits and reviews.

Use when: auditing a codebase for vulnerabilities, writing security-sensitive code, or reviewing auth/crypto/secrets handling. Not for runtime correctness bugs (→ `golang-safety`).

---

### `samber/cc-skills-golang@golang-structs-interfaces` ⚙️

Golang struct and interface design — composition, embedding, type assertions, interface segregation, struct tags (JSON/YAML/DB), pointer vs value receivers.

Use when: designing types, choosing between value vs pointer receivers, writing struct tags, or working with interface hierarchies. For architectural patterns that use interfaces → `golang-design-patterns`.

---

## Architecture & Design

### `samber/cc-skills-golang@golang-concurrency` ⚙️

Golang concurrency patterns — goroutines, channels, sync primitives, context cancellation, worker pools, fan-out/fan-in, pipelines, errgroup.

Use when: writing concurrent code, coordinating goroutines, or reviewing for race conditions. For context propagation specifically → `golang-context`. When cancelling goroutines via context — load both.

---

### `samber/cc-skills-golang@golang-context` ⚙️

Idiomatic context.Context usage — creation, cancellation, timeouts, values, propagation patterns, WithoutCancel, common anti-patterns.

Use when: propagating deadlines and cancellation signals, or passing request-scoped values. Not for code that merely accepts ctx as first parameter.

---

### `samber/cc-skills-golang@golang-data-structures` ⭐️

Golang data structures internals and usage — slices (capacity growth, append aliasing), maps, channels, sync primitives, container/\*, generic collections, and when to use each.

Use when: choosing a data structure, understanding slice/map performance characteristics, or using container/list, container/heap, or ring.

---

### `samber/cc-skills-golang@golang-database` ⭐️ ⚙️

Golang database access patterns — parameter binding, connection pooling, transactions, migrations, sqlboiler/sqlc code generation, query builders.

Use when: writing SQL queries, designing repository patterns, or configuring database connections. For security aspects of queries (injection) → also consult `golang-security`.

---

### `samber/cc-skills-golang@golang-dependency-injection` ⚙️

Dependency injection patterns in Golang — constructor injection, interface-based DI, explicit dependency wiring, and when DI is worth the complexity.

Use when: deciding whether to use DI, designing constructor signatures, or comparing DI libraries.

---

### `samber/cc-skills-golang@golang-design-patterns` ⭐️ ⚙️

Idiomatic Golang design patterns — functional options, constructors, builder pattern, middleware chains, circuit breaker, and architecture guides.

Use when: choosing architectural patterns, designing APIs, or implementing resilience patterns. For type-level design (embedding, receivers) → `golang-structs-interfaces`.

---

### `samber/cc-skills-golang@golang-modernize` ⭐️

Modernize Golang code using recent language features — range-over-int, min/max builtins, iterators, slices/maps/cmp/slog stdlib packages, testing patterns (t.Context, b.Loop, synctest), and tooling upgrades.

Use when: choosing version-sensitive idioms during ordinary implementation or review, migrating older patterns, or upgrading Go. Everyday defaults stay with their topic owners; lint configuration belongs to `golang-lint`.

---

## QA & Performance

### `samber/cc-skills-golang@golang-benchmark` 🧠

Golang benchmarking, profiling, and performance measurement — pprof, trace, CPU/memory/block profiles, flame graphs, benchstat, CI regression detection, continuous profiling.

Use when: measuring performance, capturing profiles, comparing benchmark runs, or setting up CI regression detection. For applying optimization patterns → `golang-performance`. For correctness pitfalls → `golang-safety`; for goroutine failures → `golang-concurrency`.

---

### `samber/cc-skills-golang@golang-observability` ⚙️

Golang production observability — structured logging (slog), Prometheus metrics, OpenTelemetry tracing, pprof profiling endpoints, alerting, Grafana dashboards.

Use when: instrumenting a service for production monitoring. Not for temporary deep-dive investigation (→ `golang-benchmark`, `golang-performance`).

---

### `samber/cc-skills-golang@golang-performance` 🧠

Golang performance optimization — allocation reduction, CPU efficiency, memory layout, GC tuning, pooling, caching, hot-path optimization.

Use when: applying optimization patterns after profiling. Not for measurement methodology (→ `golang-benchmark`).

---

### `samber/cc-skills-golang@golang-testing` ⭐️ 🧠 ⚙️

Production-ready Golang tests — table-driven tests, fuzzing, fixtures, goroutine leak detection (cancellation and completion), snapshot testing, code coverage, integration tests, parallel tests.

Use when: writing or reviewing tests. For structural comparison options → `golang-go-cmp`. For measurement methodology → `golang-benchmark`.

---

## Project Setup

### `samber/cc-skills-golang@golang-cli`

Golang CLI application development — project layout, exit codes, signal handling, I/O patterns, argument parsing, terminal UX.

Use when: building a CLI tool from scratch. For cobra-specific APIs → `golang-spf13-cobra`. For viper configuration → `golang-spf13-viper`.

---

### `samber/cc-skills-golang@golang-continuous-integration`

CI/CD pipeline configuration for Golang projects using GitHub Actions — build, test, lint, and release workflows.

Use when: setting up or improving a CI pipeline for a Go project.

---

### `samber/cc-skills-golang@golang-dependency-management`

Golang module dependency strategies — go.mod conventions, versioning, replace directives, tool dependencies, and multi-module workspaces.

Use when: managing go.mod, dealing with replace directives, or structuring a multi-module repo.

---

### `golang-go-doc`

Read the locally resolved package and symbol API with `go doc`. Use module tools
for dependency versions and vulnerability tooling for security checks.

### `golang-gopls`

Semantic navigation, diagnostics, and refactoring for the locally resolved build.

### `golang-refactoring`

Go-specific risks when changing declarations, APIs, and package boundaries.

---

### `samber/cc-skills-golang@golang-popular-libraries`

Curated recommendations for production-ready Golang libraries — when the stdlib is enough vs when to reach for a package.

Use when: choosing a library for a new concern (HTTP, logging, testing, etc.). For deep guidance on a specific library → use the library-specific skill.

---

### `samber/cc-skills-golang@golang-project-layout`

Golang project structure and workspace setup — cmd/internal/pkg conventions, monorepo layout, CLI project structure, and when to keep things flat.

Use when: starting a new project or restructuring an existing one. For architectural patterns within the project → `golang-design-patterns`.

---

## APIs

### `samber/cc-skills-golang@golang-graphql`

GraphQL API development in Golang using gqlgen/graphql-go — schema definition, resolvers, subscriptions, dataloader, federation.

Use when: building a GraphQL API in Go.

---

### `samber/cc-skills-golang@golang-grpc`

gRPC in Golang — protobuf organization, service definitions, streaming, interceptors, error codes, code generation workflow.

Use when: building or consuming a gRPC service.

---

## Dependency Injection

### `samber/cc-skills-golang@golang-dependency-injection` ⚙️

See "Architecture & Design" section above.

---

### `samber/cc-skills-golang@golang-samber-do`

Dependency injection with samber/do — type-safe service containers, lifecycle management, scopes, health checks, graceful shutdown.

Use when: the codebase imports `github.com/samber/do`.

---

## Frameworks

### `samber/cc-skills-golang@golang-spf13-cobra`

CLI command trees with spf13/cobra — command hierarchy, RunE hooks, flag management, shell completion, usage templates, testing with SetArgs.

Use when: the codebase imports `github.com/spf13/cobra`. For configuration layering → `golang-spf13-viper`. For general CLI architecture → `golang-cli`.

---

### `samber/cc-skills-golang@golang-spf13-viper`

Layered configuration with spf13/viper — explicit Set > changed flags > env > file > KV > default precedence, BindPFlag, hot reload, test isolation, remote KV integration.

Use when: the codebase imports `github.com/spf13/viper`. For CLI command structure → `golang-spf13-cobra`. For general CLI architecture → `golang-cli`.

---

## samber/\*

### `samber/cc-skills-golang@golang-samber-do`

See "Dependency Injection" section above.

---

### `samber/cc-skills-golang@golang-samber-hot`

In-memory caching with samber/hot — eviction policy selection, TTL, loaders, sharding, stale-while-revalidate, and Prometheus metrics. Check which algorithms the resolved release exposes through its cache builder.

Use when: the codebase imports `github.com/samber/hot`.

---

### `samber/cc-skills-golang@golang-samber-mo` 🧠

Monadic types with samber/mo — Option, Result, Either, Future, IO, Task, State for type-safe nullable values, error handling, and functional composition.

Use when: the codebase imports `github.com/samber/mo`.

---

### `samber/cc-skills-golang@golang-samber-oops`

Structured error handling with samber/oops — error builders, stack traces, error codes, context attributes, public vs developer messages, panic recovery, APM integration.

Use when: the codebase imports `github.com/samber/oops`.

---

### `samber/cc-skills-golang@golang-samber-ro` 🧠

Reactive streams with samber/ro — 150+ type-safe operators, cold/hot observables, 5 subject types, 40+ plugins, automatic backpressure, Go context integration.

Use when: the codebase imports `github.com/samber/ro`.

---

### `samber/cc-skills-golang@golang-samber-slog`

Structured logging pipeline with samber/slog-\* packages — multi-handler routing (slog-multi), sampling, formatting, HTTP middleware, 20+ backend sinks.

Use when: the codebase imports any `github.com/samber/slog-*` package.

---

## Testing

### `golang-go-cmp`

Structural comparisons with google/go-cmp — readable diffs, comparison options, and semantic equality.

Use when: a Go test needs a structural diff or customized comparison semantics. For test architecture and strategy → `golang-testing`.

---

### `samber/cc-skills-golang@golang-testing` ⭐️ 🧠 ⚙️

See "QA & Performance" section above.
