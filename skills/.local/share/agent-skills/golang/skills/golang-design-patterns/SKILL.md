---
name: golang-design-patterns
description: "Design Go APIs and application structure. Use for constructors, functional options, resource lifecycles, retries, or architectural tradeoffs."
user-invocable: true
license: MIT
compatibility: Designed for Claude Code, Codex or similar harness, and for projects using Golang.
metadata:
  author: samber
  version: "1.2.1"
  openclaw:
    emoji: "🏗"
    homepage: https://github.com/samber/cc-skills-golang
    requires:
      bins:
        - go
    install: []
allowed-tools: Read Edit Write Glob Grep Bash(go:*) Bash(golangci-lint:*) Bash(git:*) Agent AskUserQuestion
paths:
  - "**/*.go"
---

Follow the project's architecture preferences and favor the smallest pattern that
satisfies the requirement.

> **Community default.** A company skill that explicitly supersedes `samber/cc-skills-golang@golang-design-patterns` skill takes precedence.

# Go Design Patterns & Idioms

Idiomatic Go patterns for production-ready code. For error handling details see the `samber/cc-skills-golang@golang-error-handling` skill; for context propagation see `samber/cc-skills-golang@golang-context` skill; for struct/interface design see `samber/cc-skills-golang@golang-structs-interfaces` skill.

## Best Practices Summary

1. Constructors SHOULD use **functional options** — they scale better as APIs evolve (one function per option, no breaking changes)
2. Functional options MUST **return an error** if validation can fail — catch bad config at construction, not at runtime
3. **Avoid `init()`** — runs implicitly, cannot return errors, makes testing unpredictable. Use explicit constructors
4. **Choose enum zero values deliberately** — reserve an Unknown sentinel if omission matters; otherwise zero can be a useful default. Validate values at boundaries
5. Error cases MUST be **handled first** with early return — keep happy path flat
6. **Panic is for bugs, not expected errors** — callers can handle returned errors; panics crash the process
7. **`defer Close()` immediately after opening** — later code changes can accidentally skip cleanup
8. **`runtime.AddCleanup`** over `runtime.SetFinalizer` — finalizers are unpredictable and can resurrect objects
9. Every external call SHOULD **have a timeout** — a slow upstream hangs your goroutine indefinitely
10. **Limit everything** (pool sizes, queue depths, buffers) — unbounded resources grow until they crash
11. Retry logic MUST **check context cancellation** between attempts
12. **Use `strings.Builder`** for concatenation in loops → see `samber/cc-skills-golang@golang-code-style`
13. string vs []byte: **use `[]byte` for mutation and I/O**, `string` for immutable bytes and keys — conversions may copy/allocate; compilers can eliminate some
14. Iterators (Go 1.23+): **use for lazy evaluation** — avoid loading everything into memory
15. **Stream large transfers** — loading millions of rows causes OOM; stream keeps memory constant
16. `//go:embed` for **static assets** — embeds at compile time, eliminates runtime file I/O errors
17. **Use `crypto/rand`** for keys/tokens — `math/rand` is predictable → see `samber/cc-skills-golang@golang-security`
18. Compile **reused regular expressions once** — package-level MustCompile fits fixed developer-controlled patterns; dynamic patterns need Compile and error handling
19. Compile-time interface checks: **`var _ Interface = (*Type)(nil)`**
20. **A little recode > a big dependency** — each dep adds attack surface and maintenance burden
21. **Design for testability** — accept interfaces, inject dependencies

## Represent Uniform Behavior as Data

When cases perform the same operations and differ mainly in values, prefer a
table of concrete records and straightforward processing over repeated branches.
Keep explicit control flow when the cases have different behavior. A growing set
of callbacks, mode flags, or exceptions in the table is a reason to reconsider
whether the cases belong in one abstraction.

Choose the representation to make the operation easier to understand. See
`golang-structs-interfaces` for types that own coherent state and invariants, and
`golang-code-style` for keeping cohesive operations together.

## Constructor Patterns: Functional Options vs Builder

### Functional Options (Preferred)

```go
type Server struct {
    addr         string
    readTimeout  time.Duration
    writeTimeout time.Duration
    maxConns     int
}

type Option func(*Server)

func WithReadTimeout(d time.Duration) Option {
    return func(s *Server) { s.readTimeout = d }
}

func WithWriteTimeout(d time.Duration) Option {
    return func(s *Server) { s.writeTimeout = d }
}

func WithMaxConns(n int) Option {
    return func(s *Server) { s.maxConns = n }
}

func NewServer(addr string, opts ...Option) *Server {
    // Default options
    s := &Server{
        addr:         addr,
        readTimeout:  5 * time.Second,
        writeTimeout: 10 * time.Second,
        maxConns:     100,
    }
    for _, opt := range opts {
        opt(s)
    }
    return s
}

// Usage
srv := NewServer(":8080",
    WithReadTimeout(30*time.Second),
    WithMaxConns(500),
)
```

Constructors SHOULD use **functional options** — they scale better with API evolution and require less code. Use builder pattern only if you need complex validation between configuration steps.

## Constructors & Initialization

### Avoid `init()` and Mutable Globals

`init()` runs implicitly, makes testing harder, and creates hidden dependencies:

- Multiple init functions follow source presentation order; Go build tools conventionally present files in lexical filename order, but the language does not mandate a filename-based contract
- Cannot return errors — failures must panic or `log.Fatal`
- Runs before `main()` and tests — side effects make tests unpredictable

```go
// Bad — hidden global state
var db *sql.DB

func init() {
    var err error
    db, err = sql.Open("postgres", os.Getenv("DATABASE_URL"))
    if err != nil {
        log.Fatal(err)
    }
}

// Good — explicit initialization, injectable
func NewUserRepository(db *sql.DB) *UserRepository {
    return &UserRepository{db: db}
}
```

### Enums: Start at 1

Reserve zero for invalid/unset only when the domain needs that distinction; a useful default at zero is also valid. A named integer type still accepts other integer values and needs boundary validation:

```go
type Status int

const (
    StatusUnknown Status = iota // 0 = invalid/unset
    StatusActive                // 1
    StatusInactive              // 2
    StatusSuspended             // 3
)
```

### Compile Regexp Once

```go
// Good — compiled once at package level
var emailRegex = regexp.MustCompile(`^[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}$`)

// Illustrative restricted syntax, not full email-address validation.
func ValidateEmail(email string) bool {
    return emailRegex.MatchString(email)
}
```

### Use `//go:embed` for Static Assets

```go
import "embed"

//go:embed templates/*
var templateFS embed.FS

//go:embed version.txt
var version string
```

### Compile-Time Interface Checks

→ See `samber/cc-skills-golang@golang-structs-interfaces` for the `var _ Interface = (*Type)(nil)` pattern.

## Error Flow Patterns

Error cases MUST be handled first with early return — keep the happy path at minimal indentation. → See `samber/cc-skills-golang@golang-code-style` for the full pattern and examples.

### When to Panic vs Return Error

- **Return error**: network failures, file not found, invalid input — anything a caller can handle
- **Panic**: nil pointer in a place that should be impossible, violated invariant, `Must*` constructors used at init time
- **`.Close()` / `Flush()` errors**: read-only cleanup can often use `defer f.Close()`, but write/flush resources must report close or flush errors when durability matters

## Data Handling

### string vs []byte vs []rune

| Type     | Default for | Use when                                            |
| -------- | ----------- | --------------------------------------------------- |
| `string` | Text or byte keys | Immutable bytes; not necessarily valid UTF-8                              |
| `[]byte` | I/O         | Writing to `io.Writer`, building strings, mutations |
| `[]rune` | Unicode ops | Unicode code points; not grapheme clusters/user-perceived characters             |

Avoid unnecessary repeated conversions; they may copy or allocate. A rune is a Unicode code point, so combining sequences and emoji may occupy multiple runes. Decoding invalid UTF-8 to runes substitutes RuneError and is not byte-preserving.

### Iterators & Streaming for Large Data

Use iterators (Go 1.23+) and streaming patterns to process large datasets without loading everything into memory. For large transfers between services (e.g., 1M rows DB to HTTP), stream to prevent OOM.

For code examples, see [Data Handling Patterns](references/data-handling.md).

## Resource Management

`defer Close()` immediately after opening — don't wait, don't forget:

```go
f, err := os.Open(path)
if err != nil {
    return err
}
defer f.Close() // right here, not 50 lines later

rows, err := db.QueryContext(ctx, query)
if err != nil {
    return err
}
defer rows.Close()
```

For graceful shutdown, resource pools, and `runtime.AddCleanup`, see [Resource Management](references/resource-management.md).

## Resilience & Limits

### Timeout Every External Call

```go
ctx, cancel := context.WithTimeout(ctx, 5*time.Second)
defer cancel()

resp, err := httpClient.Do(req.WithContext(ctx))
```

### Retry & Context Checks

Retry logic MUST check `ctx.Err()` between attempts and use exponential/linear backoff via `select` on `ctx.Done()`. Long loops MUST check `ctx.Err()` periodically. → See `samber/cc-skills-golang@golang-context` skill.

## Database Patterns

→ See `samber/cc-skills-golang@golang-database` skill for sqlx/pgx, transactions, nullable columns, connection pools, repository interfaces, testing.

## Architecture

Ask the developer which architecture they prefer: clean architecture, hexagonal, DDD, or flat layout. Don't impose complex architecture on a small project.

Core principles regardless of architecture:

- **Keep domain pure** — no framework dependencies in the domain layer
- **Fail fast** — validate at boundaries, trust internal code
- **Make illegal states unrepresentable** — use types to enforce invariants
- **Respect 12-factor app** principles — → see `samber/cc-skills-golang@golang-project-layout`

## Detailed Guides

| Guide | Scope |
| --- | --- |
| [Architecture Patterns](references/architecture.md) | High-level principles, when each architecture fits |
| [Clean Architecture](references/clean-architecture.md) | Use cases, dependency rule, layered adapters |
| [Hexagonal Architecture](references/hexagonal-architecture.md) | Ports and adapters, domain core isolation |
| [Domain-Driven Design](references/ddd.md) | Aggregates, value objects, bounded contexts |

## Code Philosophy

- **Avoid repetitive code** — but don't abstract prematurely
- **Minimize dependencies** — a little recode > a big dependency
- **Design for testability** — accept interfaces, inject dependencies, keep functions pure

## Cross-References

- → See `samber/cc-skills-golang@golang-data-structures` skill for data structure selection, internals, and container/ packages
- → See `samber/cc-skills-golang@golang-error-handling` skill for error wrapping, sentinel errors, and the single handling rule
- → See `samber/cc-skills-golang@golang-structs-interfaces` skill for interface design and composition
- → See `samber/cc-skills-golang@golang-concurrency` skill for goroutine lifecycle and graceful shutdown
- → See `samber/cc-skills-golang@golang-context` skill for timeout and cancellation patterns
- → See `samber/cc-skills-golang@golang-project-layout` skill for architecture and directory structure
- → See `samber/cc-skills-golang@golang-refactoring` skill for safely staging a migration toward one of these patterns (options struct, DI, consumer-side interfaces) across an existing codebase
