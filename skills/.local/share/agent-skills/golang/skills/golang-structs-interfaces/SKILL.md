---
name: golang-structs-interfaces
description: "Design Go structs and interfaces. Use for composition, embedding, method sets, receiver choices, interface boundaries, and type assertions."
user-invocable: true
license: MIT
compatibility: Designed for Claude Code, Codex or similar harness, and for projects using Golang.
metadata:
  author: samber
  version: "1.2.1"
  openclaw:
    emoji: "🧩"
    homepage: https://github.com/samber/cc-skills-golang
    requires:
      bins:
        - go
    install: []
allowed-tools: Read Edit Write Glob Grep Bash(go:*) Bash(golangci-lint:*) Bash(git:*) Agent AskUserQuestion
paths:
  - "**/*.go"
---

> Apply repository instructions and required project skills first. These guidelines are defaults where the project leaves a choice.

# Go Structs & Interfaces

## Interface Design Principles

### Keep Interfaces Small

> "The bigger the interface, the weaker the abstraction." — Go Proverbs

Interfaces SHOULD have 1-3 methods. Small interfaces are easier to implement, mock, and compose. If you need a larger contract, compose it from small interfaces:

→ See `samber/cc-skills-golang@golang-naming` skill for interface naming conventions (method + "-er" suffix, canonical names)

```go
type Reader interface {
    Read(p []byte) (n int, err error)
}

type Writer interface {
    Write(p []byte) (n int, err error)
}

// Composed from small interfaces
type ReadWriter interface {
    Reader
    Writer
}
```

Compose larger interfaces from smaller ones:

```go
type ReadWriteCloser interface {
    io.Reader
    io.Writer
    io.Closer
}
```

### Define Interfaces Where They're Consumed

Choose interface ownership around the contract.

A consumer-owned interface is useful when a caller needs a narrow capability. A package may instead own a public interface when that interface is its intended API over private implementations. Follow project conventions and actual consumer needs; do not relocate an established contract merely to enforce a slogan.

```go
// package notification — defines only what it needs
type Sender interface {
    Send(to, body string) error
}

type Service struct {
    sender Sender
}
```

The `email` package exports a concrete `Client` struct — it doesn't need to know about `Sender`.

### Choose the Public Contract

Accept the capabilities a function needs. Return a concrete type when callers
need its API and exposing it is appropriate. Return an interface when the intended
public contract hides a private implementation or supports interchangeable behavior.
Do not add interfaces speculatively, or expose an implementation solely to satisfy
a return-type rule. Follow explicit project API-design requirements.

```go
// Concrete return when the concrete type is the public API.
func NewClient(store Store) *Client

// Interface return when implementations are deliberately private.
func NewService(store Store) Service
```

### Don't Create Interfaces Prematurely

> "Don't design with interfaces, discover them."

An interface written before a second implementation exists is a guess about which methods will vary — and the guess is usually wrong, so the abstraction has to be reshaped anyway. Meanwhile it costs a layer of indirection that hides the concrete type from readers and tooling. Start with concrete types; extract an interface once a second consumer, a second implementation, or a test mock demands it.

```go
// Bad — premature interface with a single implementation
type UserRepository interface {
    FindByID(ctx context.Context, id string) (*User, error)
}
type userRepository struct { db *sql.DB }

// Good — start concrete, extract an interface later when needed
type UserRepository struct { db *sql.DB }
```

## Make the Zero Value Useful

Design structs so they work without explicit initialization. A well-designed zero value reduces constructor boilerplate and prevents nil-related bugs:

```go
// Good — zero value is ready to use
var buf bytes.Buffer
buf.WriteString("hello")

var mu sync.Mutex
mu.Lock()

// Bad — zero value is broken, requires constructor
type Registry struct {
    items map[string]Item // nil map, panics on write
}

// Good — lazy initialization guards the zero value
func (r *Registry) Register(name string, item Item) {
    if r.items == nil {
        r.items = make(map[string]Item)
    }
    r.items[name] = item
}
```

## Avoid `any` / `interface{}` When a Specific Type Will Do

Since Go 1.18+, MUST prefer generics over `any` for type-safe operations. Use `any` only at true boundaries where the type is genuinely unknown (e.g., JSON decoding, reflection):

```go
// Bad — loses type safety
func Contains(slice []any, target any) bool { ... }

// Good — generic, type-safe
func Contains[T comparable](slice []T, target T) bool { ... }
```

## Key Standard Library Interfaces

| Interface     | Package         | Method                                |
| ------------- | --------------- | ------------------------------------- |
| `Reader`      | `io`            | `Read(p []byte) (n int, err error)`   |
| `Writer`      | `io`            | `Write(p []byte) (n int, err error)`  |
| `Closer`      | `io`            | `Close() error`                       |
| `Stringer`    | `fmt`           | `String() string`                     |
| `error`       | builtin         | `Error() string`                      |
| `Handler`     | `net/http`      | `ServeHTTP(ResponseWriter, *Request)` |
| `Marshaler`   | `encoding/json` | `MarshalJSON() ([]byte, error)`       |
| `Unmarshaler` | `encoding/json` | `UnmarshalJSON([]byte) error`         |

Canonical method signatures MUST be honored — if your type has a `String()` method, it must match `fmt.Stringer`. Don't invent `ToString()` or `ReadData()`.

## Compile-Time Interface Check

Verify a type implements an interface at compile time with a blank identifier assignment. Place it near the type definition:

```go
var _ io.ReadWriter = (*MyBuffer)(nil)
```

This costs nothing at runtime. If `MyBuffer` ever stops satisfying `io.ReadWriter`, the build fails immediately.

## Type Assertions & Type Switches

Type assertions MUST use the comma-ok form (`s, ok := val.(string)`) — the single-value form panics on a type mismatch instead of branching. Use a type switch to dispatch on the dynamic type, and an assertion to a small optional interface (`if f, ok := w.(Flusher); ok`) to exploit richer implementations without widening the declared parameter type.

→ See [Type Assertions & Type Switches](references/type-assertions.md) for type switch ordering, nil cases, and the optional-behavior pattern.

## Struct & Interface Embedding

### Struct Embedding

Embedding promotes the inner type's methods and fields to the outer type — composition, not inheritance:

```go
type Logger struct {
    *slog.Logger
}

type Server struct {
    Logger
    addr string
}

// s.Info(...) works — promoted from slog.Logger through Logger
s := Server{Logger: Logger{slog.Default()}, addr: ":8080"}
s.Info("starting", "addr", s.addr)
```

The receiver of promoted methods is the _inner_ type, not the outer. The outer type can override by defining its own method with the same name.

### When to Embed vs Named Field

| Use | When |
| --- | --- |
| **Embed** | You want to promote the full API of the inner type — the outer type "is a" enhanced version |
| **Named field** | You only need the inner type internally — the outer type "has a" dependency |

```go
// Embed — Server exposes all http.Handler methods
type Server struct {
    http.Handler
}

// Named field — Server uses the store but doesn't expose its methods
type Server struct {
    store *DataStore
}
```

## Dependency Injection via Interfaces

Accept dependencies as interfaces in constructors. This decouples components and makes testing straightforward:

```go
type UserStore interface {
    FindByID(ctx context.Context, id string) (*User, error)
}

type UserService struct {
    store UserStore
}

func NewUserService(store UserStore) *UserService {
    return &UserService{store: store}
}
```

In tests, pass a mock or stub that satisfies `UserStore` — no real database needed.

## Struct Field Tags

Exported fields in serialized structs MUST have field tags — without one, the encoder falls back to the Go field name, so renaming a field silently changes the wire format:

```go
type Order struct {
    ID        string    `json:"id"         db:"id"`
    Total     float64   `json:"total"      db:"total"`
    CreatedAt time.Time `json:"created_at" db:"created_at"`
    Internal  string    `json:"-"          db:"-"`
}
```

→ See [Struct Fields: Tags and Copy Safety](references/struct-fields.md) for the full tag directive table, the `omitempty` vs `omitzero` trap, and `go vet` diagnostics.

## Pointer vs Value Receivers

| Use pointer `(s *Server)` | Use value `(s Server)` |
| --- | --- |
| Method modifies the receiver | Receiver is small and immutable |
| Receiver contains `sync.Mutex` or similar | Receiver is a basic type (int, string) |
| Receiver is a large struct | Method is a read-only accessor |
| Consistency: if any method uses a pointer, all should | Map and function values (already reference types) |

Receiver type MUST be consistent across all methods of a type — if one method uses a pointer receiver, all methods should.

## Preventing Struct Copies with `noCopy`

A struct holding a mutex, a channel, or internal pointers breaks when copied: the copy duplicates the lock state, so two goroutines guard two different mutexes and the invariant disappears silently. Embed a `noCopy` sentinel so `go vet` reports every value copy, and pass such structs by pointer.

**Diagnose:** 1- `go vet ./...` — `copylocks` reports value copies of lock-bearing structs

→ See [Struct Fields: Tags and Copy Safety](references/struct-fields.md) for the `noCopy` implementation and how `vet` detects it.

## Cross-References

- → See `samber/cc-skills-golang@golang-naming` skill for interface naming conventions (Reader, Closer, Stringer)
- → See `samber/cc-skills-golang@golang-design-patterns` skill for functional options, constructors, and builder patterns
- → See `samber/cc-skills-golang@golang-dependency-injection` skill for DI patterns using interfaces
- → See `samber/cc-skills-golang@golang-code-style` skill for value vs pointer function parameters (distinct from receivers)
- → See `samber/cc-skills-golang@golang-gopls` skill for safe rename and the `implementInterface` code action — renaming a method or receiver that participates in interface satisfaction updates every call site and refuses a rename that would silently break the interface, which grep/sed cannot detect

## Common Mistakes

| Mistake | Fix |
| --- | --- |
| Large interfaces (5+ methods) | Split into focused 1-3 method interfaces, compose if needed |
| Interface ownership chosen by rote | Use consumer needs and the intended public contract |
| Return type chosen by slogan | Expose the intended API; keep implementation details private |
| Bare type assertions without comma-ok | Always use `v, ok := x.(T)` |
| Embedding when you only need a few methods | Use a named field and delegate explicitly |
| Missing field tags on serialized structs | Tag all exported fields in marshaled types |
| Mixing pointer and value receivers on a type | Pick one and be consistent |
| Forgetting compile-time interface check | Add `var _ Interface = (*Type)(nil)` |
| Using `ToString()` instead of `String()` | Honor canonical method names |
| Premature interface with a single implementation | Start concrete, extract interface when needed |
| Nil map/slice in zero value struct | Use lazy initialization in methods |
| Using `any` for type-safe operations | Use generics (`[T comparable]`) instead |
