---
name: golang-samber-do
description: "Use samber/do for Go dependency injection. Apply to service registration, scopes, lifecycle management, health checks, and shutdown."
user-invocable: true
license: MIT
compatibility: Designed for Claude Code, Codex or similar harness, and for projects using Golang.
metadata:
  author: samber
  version: "1.3.2"
  openclaw:
    emoji: "💉"
    homepage: https://github.com/samber/cc-skills-golang
    requires:
      bins:
        - go
    install: []
    skill-library-version: "2.0.0"
allowed-tools: Read Edit Write Glob Grep Bash(go:*) Bash(golangci-lint:*) Bash(git:*) Agent WebFetch Bash(gopls:*) LSP mcp__gopls__*
paths:
  - "**/*.go"
---

# Using samber/do for Dependency Injection in Go

Type-safe dependency injection toolkit for Go based on Go 1.18+ generics.

**Official Resources:**

- [pkg.go.dev/github.com/samber/do/v2](https://pkg.go.dev/github.com/samber/do/v2)
- [do.samber.dev](https://do.samber.dev)
- [github.com/samber/do/v2](https://github.com/samber/do)

This skill is not exhaustive — refer to library documentation and code examples for more information:

- Consult `go doc` for the resolved package API; see `golang-go-doc` for lookup guidance.
- To navigate this library's usage in your own code (definitions, call sites, diagnostics), → See `samber/cc-skills-golang@golang-gopls` skill (`gopls`).

These examples target v2. The v1 library also uses generics, but its injector and scope APIs differ; check the version already used by the project:

```bash
go get github.com/samber/do/v2
```

## Core Concepts

### The Injector (Container)

```go
import "github.com/samber/do/v2"

injector := do.New()
```

### Service Types

- **Lazy** (default): Created when first requested
- **Eager/value**: An already-created value registered with `ProvideValue` or `Eager`
- **Transient**: New instance created on every request

### Provider Functions

Lazy and transient services use provider functions; eager services accept values directly:

```go
type Provider[T any] func(i Injector) (T, error)
```

## Basic Usage

### 1. Define and Register Services

Follow "Accept Interfaces, Return Structs":

```go
// Register a service (lazy by default)
do.Provide(injector, func(i do.Injector) (Database, error) {
    return &PostgreSQLDatabase{connString: "postgres://..."}, nil
})

// Register a pre-created value
do.ProvideValue(injector, &Config{Port: 8080})

// Register a transient service (new instance each time)
do.ProvideTransient(injector, func(i do.Injector) (*Logger, error) {
    return &Logger{}, nil
})

// For a lazy provider that must initialize before serving, invoke it at startup.
// do.ProvideValue above already registered Config eagerly.
```

### 2. Invoke Services

The container MUST only be accessed at the composition root:

```go
// Invoke at the composition root with error handling
db, err := do.Invoke[Database](injector)

// MustInvoke panics on error — preferred in providers, recovered by do.Invoke on the parent call
db := do.MustInvoke[Database](injector)
```

Inside a provider function, you may use `do.MustInvoke` (or `MustInvokeAs`/`MustInvokeNamed`/`MustInvokeStruct`) rather than the error-returning variant:

- A provider already returns `(T, error)`, so propagating a dependency failure with `do.Invoke` costs an extra `if err != nil { return nil, err }` on every call.
- `do.MustInvoke` panics instead, but samber/do correctly catches and recovers that panic at the enclosing `Invoke` call and converts it back into a regular error — this recover happens inside the library itself, not in caller code, so `MustInvoke` is safe to use inside providers.
- The failure still surfaces as an error at the composition root, just without the manual boilerplate in every provider.

### 3. Service Dependencies

```go
func NewUserService(i do.Injector) (UserService, error) {
    db := do.MustInvoke[Database](i)
    cache := do.MustInvoke[Cache](i)
    return &userService{db: db, cache: cache}, nil
}

do.Provide(injector, NewUserService)
```

### 4. Implicit Aliasing

Register a concrete type and invoke as an interface without explicit aliasing:

```go
// Register concrete type
do.Provide(injector, func(i do.Injector) (*PostgreSQLDatabase, error) {
    return &PostgreSQLDatabase{}, nil
})

// Invoke directly as interface (implicit aliasing)
db := do.MustInvokeAs[Database](injector)
```

### 5. Named Services

Register multiple services of the same type:

```go
do.ProvideNamed(injector, "primary-db", func(i do.Injector) (*Database, error) {
    return &Database{URL: "postgres://primary..."}, nil
})

mainDB := do.MustInvokeNamed[*Database](injector, "primary-db")
```

## Package Organization

Use `do.Package()` to organize service registration by module:

```go
// infrastructure/package.go
var Package = do.Package(
    do.Lazy(func(i do.Injector) (*postgres.DB, error) {
        cfg := do.MustInvoke[*Config](i)
        return postgres.Connect(cfg.DatabaseURL)
    }),
    do.Lazy(func(i do.Injector) (*redis.Client, error) {
        cfg := do.MustInvoke[*Config](i)
        opts, err := redis.ParseURL(cfg.RedisURL) // github.com/redis/go-redis/v9
        if err != nil {
            return nil, err
        }
        return redis.NewClient(opts), nil
    }),
)

// main.go
injector := do.New(infrastructure.Package, service.Package)
```

## Full Application Setup

```go
func main() {
    injector := do.New(
        infrastructure.Package,
        repository.Package,
        service.Package,
        transport.Package,
    )

    server := do.MustInvoke[*http.Server](injector)
    ctx, stop := signal.NotifyContext(context.Background(), os.Interrupt, syscall.SIGTERM)
    defer stop()
    serveErr := make(chan error, 1)
    go func() { serveErr <- server.ListenAndServe() }()
    select {
    case err := <-serveErr:
        if err != nil && !errors.Is(err, http.ErrServerClosed) {
            log.Printf("serve: %v", err)
        }
    case <-ctx.Done():
    }
    shutdownCtx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
    defer cancel()
    report := injector.ShutdownWithContext(shutdownCtx)
    if !report.Succeed {
        log.Printf("shutdown: %v", report.Error())
    }
}
```

## Best Practices

1. Depend on interfaces, not concrete types — lets you swap implementations in tests without touching production code
2. Each service should have one job — services with multiple responsibilities are harder to test and harder to replace
3. Keep dependencies acyclic — cycles prevent successful initialization; nesting depth alone does not make initialization order incorrect
4. Handle errors in provider functions — a silently failing provider creates a broken service that crashes later in unexpected places
5. Use scopes to organize services by lifecycle — request-scoped services prevent leaks, global services prevent redundant initialization
6. Use `do.MustInvoke*` inside provider functions instead of `do.Invoke*` — samber/do correctly catches and recovers the panic at the outer `Invoke` call, turning it back into a returned error, so it's safe to use inside providers and you get the same error propagation without the boilerplate

For scopes, lifecycle management, struct injection, and debugging, see [Advanced Usage](./references/advanced.md).

For testing patterns (cloning, overrides, mocks), see [Testing](./references/testing.md).

## Quick Reference

### Registration

| Function                        | Purpose                          |
| ------------------------------- | -------------------------------- |
| `do.Provide[T]()`               | Register lazy service (default)  |
| `do.ProvideNamed[T]()`          | Register named lazy service      |
| `do.ProvideValue[T]()`          | Register pre-created value       |
| `do.ProvideNamedValue[T]()`     | Register named value             |
| `do.ProvideTransient[T]()`      | Register new instance each time  |
| `do.ProvideNamedTransient[T]()` | Register named transient service |
| `do.Package()`                  | Group service registrations      |

### Invocation

| Function                   | Purpose                                   |
| -------------------------- | ----------------------------------------- |
| `do.Invoke[T]()`           | Get service (with error)                  |
| `do.InvokeNamed[T]()`      | Get named service                         |
| `do.InvokeAs[T]()`         | Get first service matching interface      |
| `do.InvokeStruct[T]()`     | Inject into struct fields using tags      |
| `do.MustInvoke[T]()`       | Get service (panic on error)              |
| `do.MustInvokeNamed[T]()`  | Get named service (panic on error)        |
| `do.MustInvokeAs[T]()`     | Get service by interface (panic on error) |
| `do.MustInvokeStruct[T]()` | Inject into struct (panic on error)       |

## Cross-References

- → See `samber/cc-skills-golang@golang-dependency-injection` skill for DI concepts, comparison, and when to adopt a DI library
- → See `samber/cc-skills-golang@golang-structs-interfaces` skill for interface design patterns
- → See `samber/cc-skills-golang@golang-testing` skill for general testing patterns
