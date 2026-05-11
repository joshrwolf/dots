---
name: idiomatic-go
description: "Idiomatic Go design: type-oriented structure, no single-use helpers, correct error handling, concurrency patterns, and modern stdlib usage (Go 1.26). Use when writing, reviewing, or refactoring Go code."
---

# Idiomatic Go

Always use latest Go (currently 1.26). Always prefer modern patterns —
see [modern.md](references/modern.md) for Go 1.24--1.26 features and replacements
for obsolete patterns.

---

## Design Philosophy

### Do not extract single-use helper functions

> *"There is definitely such a thing as too repetitive tiny functions, and the solution
> is to change where the function boundaries are, not to start counting lines."*
> — Go Code Review Comments

A function must earn its existence. Three lines of code called once is not a function —
it's an indirection that forces the reader to jump somewhere else for no reason.
**Inline code is the clear code.** Extract a function when you have a genuine second
caller, not before. "Clean code" is not measured by function count.

> *"Duplication is far cheaper than the wrong abstraction."* — Sandy Metz (widely cited in Go community by Dave Cheney)

```go
// WRONG: extracting a single-use helper
func (s *Server) Start() error {
    addr := buildAddr(s.Host, s.Port)  // Why is this a function?
    return http.ListenAndServe(addr, s.handler)
}

func buildAddr(host string, port int) string {
    return fmt.Sprintf("%s:%d", host, port)
}

// RIGHT: inline what's only used once
func (s *Server) Start() error {
    return http.ListenAndServe(fmt.Sprintf("%s:%d", s.Host, s.Port), s.handler)
}
```

This applies to "validation helpers", "formatting helpers", "conversion helpers" —
if it's called once, it belongs at the call site.

### Structure code around typed data, not function chains

> *"If you don't understand the data, you don't understand the problem."*
> — Bill Kennedy

> *"C++ and Java are about type hierarchies and the taxonomy of types. Go is about composition."*
> — Rob Pike

Good Go code models the problem as **types with methods**. Data flows through
well-typed structures. The stdlib demonstrates this everywhere: `http.Server`,
`bufio.Scanner`, `json.Encoder`, `exec.Cmd` — a constructor accepts dependencies,
returns a concrete struct, and methods on that struct do the work.

#### Method vs function

Use Kennedy's test: *"Methods are valid when it is practical or reasonable for a
piece of data to have a capability."* Use Cheney's naming test: if you'd name it
after **an action** (`.Scan()`, `.Encode()`, `.Process()`) → method on a type. If
you'd name it after **what it returns** (`ParseInt()`, `ReadAll()`) → function.

Functions are for pure transformations with clear inputs and outputs. Methods are
for when data carries state and has capabilities.

#### The `New*` constructor pattern

The stdlib's pervasive pattern: constructor accepts interfaces, returns a concrete
struct. The struct is the configured unit of work.

```go
// Constructor accepts dependencies as interfaces, returns concrete type
func NewProcessor(repo Repository, logger *slog.Logger) *Processor {
    return &Processor{repo: repo, logger: logger}
}

// Methods are the capabilities of the configured type
func (p *Processor) Process(ctx context.Context, order Order) error {
    if err := order.Validate(); err != nil {
        return fmt.Errorf("invalid order: %w", err)
    }
    return p.repo.Save(ctx, order)
}
```

This is `json.NewEncoder(w)`, `bufio.NewScanner(r)`, `csv.NewReader(r)` —
the same shape everywhere in the stdlib.

#### When functions should become a type

- **Same cluster of arguments passed to multiple functions** → those arguments are
  the struct you haven't defined yet
- **A function keeps gaining parameters** → the parameters are fields begging for a type
- **State configured once, used repeatedly** → constructor + methods
- **Lifecycle exists** (create → configure → use → close) → a type with methods

```go
// WRONG: stateless function chain with growing arguments
func processOrder(
    ctx context.Context, userID int, items []Item,
    discount float64, taxRate float64, shipping Address,
    notify bool, logger *slog.Logger,
) error {
    total := calculateTotal(items, discount, taxRate)
    addr := formatAddress(shipping)
    if err := chargeUser(ctx, userID, total, logger); err != nil {
        return err
    }
    return shipOrder(ctx, addr, items, notify, logger)
}

// RIGHT: data and behavior live together
type Order struct {
    UserID   int
    Items    []Item
    Discount float64
    TaxRate  float64
    Shipping Address
    Notify   bool

    payments PaymentService
    logger   *slog.Logger
}

func (o *Order) Total() float64 {
    subtotal := 0.0
    for _, item := range o.Items {
        subtotal += item.Price * float64(item.Qty)
    }
    return subtotal * (1 - o.Discount) * (1 + o.TaxRate)
}

func (o *Order) Process(ctx context.Context) error {
    if err := o.payments.Charge(ctx, o.UserID, o.Total()); err != nil {
        return fmt.Errorf("charge user %d: %w", o.UserID, err)
    }
    return o.ship(ctx)
}
```

#### Anti-patterns

- **Anemic types**: structs with exported fields but no methods, operated on entirely
  by external functions. If functions always take the same struct as their first
  argument, those functions should be methods.
- **Growing parameter lists**: 4+ parameters, especially when several travel together
  through multiple call sites. That's a struct.
- **"Bag of functions" packages**: `util`, `helpers`, `common` — name packages after
  what they provide, not what they contain. Move functions to the types they operate
  on. (Cheney)
- **Premature interfaces**: define interfaces where they're consumed, not where types
  are implemented. Write concrete types first; small interfaces emerge naturally from
  shared method sets. "The bigger the interface, the weaker the abstraction." (Pike)

### Design for testability

> *"If you have to mock something, that's a code smell — your API boundary is wrong."*
> — Mitchell Hashimoto

Code should be testable through its **public functions** using table-driven subtests.
If it's hard to test through the public API, the design is wrong — refactor the code,
not the tests.

- Accept interfaces for dependencies → fakes are trivial, no mock framework needed
- Return values instead of mutating state → assertions are straightforward
- Avoid global state and singletons → tests are independent and parallelizable

The stdlib models this: `httptest.NewRecorder`, `strings.NewReader`, `bytes.Buffer` —
real implementations used as test fakes, not mock frameworks.

**See [testing.md](references/testing.md)** for table-driven patterns, fakes, and test design.

---

## Quick Reference

### Error Handling

Always check and wrap errors with context:
```go
result, err := doSomething()
if err != nil {
    return fmt.Errorf("context: %w", err)
}
```

Use `errors.AsType` for type-safe matching (Go 1.26):
```go
if pathErr, ok := errors.AsType[*os.PathError](err); ok {
    fmt.Println(pathErr.Path)
}
```

**See [errors.md](references/errors.md)** for wrapping, sentinel errors, custom types.

### Concurrency

Pass context as first parameter. Use `WaitGroup.Go` (Go 1.25):
```go
var wg sync.WaitGroup
wg.Go(func() {
    processItem(ctx, item)
})
wg.Wait()
```

**See [concurrency.md](references/concurrency.md)** for channels, errgroup, sync primitives.

### Allocation

- `new(expr)` returns `*T` initialized to `expr` (Go 1.26); `new(T)` returns zero-valued `*T`
- `make(T)` initializes slices, maps, channels; returns `T`

```go
p := new(5 * time.Second)  // *time.Duration, no temp variable needed
```

---

## Debugging Checklist

- **nil interface gotcha**: interface with nil concrete value is not nil → [interfaces.md](references/interfaces.md)
- **Map concurrency**: maps aren't safe for concurrent use → [concurrency.md](references/concurrency.md)
- **Slice sharing**: slices share underlying arrays → [conventions.md](references/conventions.md)
- **Defer order**: LIFO (last in, first out) → [conventions.md](references/conventions.md)
- **Context cancellation**: check `ctx.Done()` in long operations → [concurrency.md](references/concurrency.md)
- **Goroutine leaks**: enable `GOEXPERIMENT=goroutineleakprofile` for detection
- **Race conditions**: always test with `go test -race ./...`

---

## References

- **[modern.md](references/modern.md)** — **Read first.** Go 1.24--1.26 features, modern replacements for obsolete patterns
- **[errors.md](references/errors.md)** — Error handling patterns
- **[concurrency.md](references/concurrency.md)** — Goroutines, channels, context, errgroup
- **[interfaces.md](references/interfaces.md)** — Interface design, composition, nil interface gotcha
- **[testing.md](references/testing.md)** — Testing patterns, go-cmp (never testify)
- **[conventions.md](references/conventions.md)** — Naming, project structure, slices, defer
