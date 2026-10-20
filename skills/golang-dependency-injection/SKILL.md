---
name: golang-dependency-injection
description: "Design dependency wiring in Go. Use when choosing constructor injection or a DI library, arranging service lifecycles, or improving test seams."
user-invocable: true
license: MIT
compatibility: Designed for Claude Code, Codex or similar harness, and for projects using Golang.
metadata:
  author: samber
  version: "1.3.1"
  openclaw:
    emoji: "🔌"
    homepage: https://github.com/samber/cc-skills-golang
    requires:
      bins:
        - go
    install: []
allowed-tools: Read Edit Write Glob Grep Bash(go:*) Bash(golangci-lint:*) Bash(git:*) Agent WebFetch AskUserQuestion
paths:
  - "**/*.go"
---

Assess the existing dependency graph and lifecycle needs before changing the wiring.
For coupled code, map hidden dependencies and service-locator usage before planning
the migration.

> **Community default.** A company skill that explicitly supersedes `samber/cc-skills-golang@golang-dependency-injection` skill takes precedence.

# Dependency Injection in Go

Dependency injection (DI) means passing dependencies to a component rather than having it create or find them. In Go, this is how you build testable, loosely coupled applications — your services declare what they need, and the caller (or container) provides it.


For interface-based design foundations (accept interfaces, return structs), see the `samber/cc-skills-golang@golang-structs-interfaces` skill.

## Best Practices Summary

1. Make component dependencies explicit. Constructors suit long-lived services; function parameters or a small configuration value can suit simpler code. Avoid hidden mutable dependencies and fallible service startup in `init()`
2. Prefer manual constructor injection while wiring remains straightforward; a service count alone does not justify a library
3. Use narrow consumer-owned interfaces for dependency seams, or package-owned public interfaces when they deliberately hide implementations. Choose return types for the intended API and project conventions
4. Package-level service lookup hides dependencies and complicates test isolation. Keep application wiring explicit; framework registration is a separate concern and does not require rewriting an established integration
5. Keep container resolution in application wiring or explicit scope factories. Pass resolved dependencies to ordinary business code so its requirements remain visible
6. **Choose initialization timing deliberately** — initialize required dependencies before readiness; lazy initialization suits optional expensive work
7. **Choose lifetime from ownership and concurrency** — share concurrency-safe pools; isolate request state. Stateless services can be shared too
8. **Mock at the interface boundary** — DI makes this trivial
9. **Keep ownership and dependency direction clear** — investigate cycles and unnecessary indirection; a valid layered graph can be deep
10. Add a DI library only when its wiring or lifecycle support earns the added dependency and runtime failure modes

## Why Dependency Injection?

| Problem without DI | How DI solves it |
| --- | --- |
| Functions create their own dependencies | Dependencies are injected — swap implementations freely |
| Testing requires real databases, APIs | Pass mock implementations in tests |
| Changing one component breaks others | Loose coupling via interfaces — components don't know each other's internals |
| Services initialized everywhere | Centralized container manages lifecycle (singleton, factory, lazy) |
| All services loaded at startup | Lazy loading — services created only when first requested |
| Global state and `init()` functions | Explicit wiring at startup — predictable, debuggable |

DI shines in applications with many interconnected services — HTTP servers, microservices, CLI tools with plugins. For a small script with 2-3 functions, manual wiring is fine. Don't over-engineer.

## Manual Constructor Injection (No Library)

For small projects, pass dependencies through constructors. See [Manual DI examples](./references/manual-di.md) for a complete application example.

```go
// ✓ Good — explicit dependencies, testable
type UserService struct {
    db     UserStore
    mailer Mailer
    logger *slog.Logger
}

func NewUserService(db UserStore, mailer Mailer, logger *slog.Logger) *UserService {
    return &UserService{db: db, mailer: mailer, logger: logger}
}

// main.go — manual wiring
func main() {
    logger := slog.Default()
    db := postgres.NewUserStore(connStr)
    mailer := smtp.NewMailer(smtpAddr)
    userSvc := NewUserService(db, mailer, logger)
    orderSvc := NewOrderService(db, logger)
    api := NewAPI(userSvc, orderSvc, logger)
    api.ListenAndServe(":8080")
}
```

```go
// ✗ Bad — hardcoded dependencies, untestable
type UserService struct {
    db *sql.DB
}

func NewUserService() *UserService {
    db, _ := sql.Open("postgres", os.Getenv("DATABASE_URL")) // hidden dependency
    return &UserService{db: db}
}
```

A container may help when repeated wiring or lifecycle coordination becomes hard to maintain. Manual DI can also support health checks, graceful shutdown, lazy initialization, and scoped values; needing these features does not itself require a library. Evaluate the additional dependency and runtime failure modes against the concrete wiring it replaces.

## Existing container-based projects

Keep constructor wiring explicit. Use manual injection for code that does not
already depend on a container. For a project already using samber/do, consult
`golang-samber-do` for registration and lifecycle details.

## Testing with DI

DI makes testing straightforward — inject mocks instead of real implementations:

```go
// Define a mock
type MockUserStore struct {
    users map[string]*User
}

func (m *MockUserStore) FindByID(ctx context.Context, id string) (*User, error) {
    u, ok := m.users[id]
    if !ok {
        return nil, ErrNotFound
    }
    return u, nil
}

// Test with manual injection
func TestUserService_GetUser(t *testing.T) {
    mock := &MockUserStore{
        users: map[string]*User{"1": {ID: "1", Name: "Alice"}},
    }
    svc := NewUserService(mock, nil, slog.Default())

    user, err := svc.GetUser(context.Background(), "1")
    if err != nil {
        t.Fatalf("unexpected error: %v", err)
    }
    if user.Name != "Alice" {
        t.Errorf("got %q, want %q", user.Name, "Alice")
    }
}
```

### Testing in an existing samber/do v2 project

Register the interface type used by the provider and provide the service itself before invoking it:

```go
func TestUserService_WithDo(t *testing.T) {
    testInjector := do.New()
    do.ProvideValue[UserStore](testInjector, &MockUserStore{
        users: map[string]*User{"1": {ID: "1", Name: "Alice"}},
    })
    do.Provide(testInjector, func(i do.Injector) (*UserService, error) {
        return NewUserService(do.MustInvoke[UserStore](i), nil, slog.Default()), nil
    })
    svc := do.MustInvoke[*UserService](testInjector)
    user, err := svc.GetUser(context.Background(), "1")
    if err != nil { t.Fatal(err) }
    if user.Name != "Alice" { t.Errorf("name = %q, want Alice", user.Name) }
}
```

A fresh test container avoids sharing mutable production values. RootScope.Clone
copies registrations, but values registered eagerly can still be shared; it is not
a deep copy of application state.

## When to Adopt a DI Library

| Signal | Action |
| --- | --- |
| Straightforward dependencies and startup | Stay with manual constructor injection |
| Repeated wiring or lifecycle coordination is becoming costly | Evaluate whether a library reduces that concrete cost |
| Need health checks, scopes, or ordered shutdown | Compare library support with explicit application wiring |
| Team unfamiliar with DI concepts | Start manual and introduce additional machinery when it solves an observed problem |

## Common Mistakes

| Mistake | Fix |
| --- | --- |
| Global variables as dependencies | Pass through constructors or DI container |
| `init()` for service setup | Explicit initialization in `main()` or container |
| Depending on concrete types | Accept interfaces at consumption boundaries |
| Passing the container everywhere (service locator) | Inject specific dependencies, not the container |
| Dependency chains that obscure ownership or create cycles | Revisit responsibilities; chain length alone is not a reason to bypass domain boundaries |
| Creating a new container per request | One container per application; use scopes for request-level isolation |

## Cross-References

- → See `samber/cc-skills-golang@golang-samber-do` skill for detailed samber/do usage patterns
- → See `samber/cc-skills-golang@golang-structs-interfaces` skill for interface design and composition
- → See `samber/cc-skills-golang@golang-testing` skill for testing with dependency injection
- → See `samber/cc-skills-golang@golang-project-layout` skill for DI initialization placement

## References

- [samber/do/v2 documentation](https://do.samber.dev) | [github.com/samber/do/v2](https://github.com/samber/do)
