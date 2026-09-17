# samber/do — Generics-Based DI

> **For the full samber/do API, patterns, and advanced features, see the `samber/cc-skills-golang@golang-samber-do` skill.**

Dependency injection using generics, without code generation. Type parameters check provider/result types; registration lookup and construction still happen at runtime.

- Docs: [do.samber.dev](https://do.samber.dev) | [github.com/samber/do/v2](https://github.com/samber/do)

## Core Pattern

```go
// Register services with providers
injector := do.New()
do.Provide(injector, func(i do.Injector) (*UserService, error) {
    db := do.MustInvoke[*Database](i)
    return NewUserService(db), nil
})

// Invoke services (lazy — created on demand)
svc := do.MustInvoke[*UserService](injector)

// Graceful shutdown — all services implementing Shutdowner are closed
injector.ShutdownOnSignalsWithContext(ctx, os.Interrupt)
```

## Why samber/do

- **No code generation** — no build step, no generated files to maintain
- **Runtime resolution** — missing services, cycles, and provider failures surface when resolved; generics do not statically validate the dependency graph
- **Strongly typed** — Go generics provide full type safety without `interface{}` casts
- **Built-in lifecycle** — health checks and graceful shutdown detected automatically
- **Container cloning** — copy registrations for testing; eagerly provided mutable values may remain shared
- **Simple API** — `Provide`, `Invoke`, `Shutdown` — that's most of what you need
- **Package system** — organize services by domain without manual wiring order

→ See `samber/cc-skills-golang@golang-samber-do` for full application setup, package organization, lifecycle management, debugging, testing with clone + override, and complete API reference.
