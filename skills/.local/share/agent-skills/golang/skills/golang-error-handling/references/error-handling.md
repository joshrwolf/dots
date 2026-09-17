# Error Handling Patterns and Logging

## Table of Contents

- [The Single Handling Rule](#the-single-handling-rule)
- [Panic and Recover](#panic-and-recover)
  - [When to panic](#when-to-panic)
  - [Recovering from panics](#recovering-from-panics)
- [Why Use `samber/oops`](#why-use-samberoops)
- [Logging Errors with `slog`](#logging-errors-with-slog)

## The Single Handling Rule

Choose which layer owns the failure report. Logging the same failure at every propagation layer creates duplicates; retry diagnostics or an explicitly required local event may still be logged while returning the error.

```go
// ✗ Bad — logs AND returns (duplicate noise)
func processOrder(id string) error {
    err := chargeCard(id)
    if err != nil {
        log.Printf("failed to charge card: %v", err)
        return fmt.Errorf("charging card: %w", err)
    }
    return nil
}

// ✓ Good — return with context, let the caller decide
func processOrder(id string) error {
    err := chargeCard(id)
    if err != nil {
        return oops.
            With("order_id", id).
            Wrapf(err, "charging card")
    }
    return nil
}

// ✓ Good — handle at the top level (HTTP handler, main, etc.)
func handleOrder(w http.ResponseWriter, r *http.Request) {
    err := processOrder(r.FormValue("id"))
    if err != nil {
        slog.Error("order failed", "error", err)
        http.Error(w, "internal error", http.StatusInternalServerError)
        return
    }
    w.WriteHeader(http.StatusOK)
}
```

## Panic and Recover

### When to panic

Use errors for expected failures such as timeouts or missing files. Panic can express a violated invariant or an explicit Must-style API contract. Whether recovery is safe depends on the state left behind, not merely whether a panic is technically recoverable.

```go
// ✓ Acceptable — programmer error in initialization
func MustCompileRegex(pattern string) *regexp.Regexp {
    re, err := regexp.Compile(pattern)
    if err != nil {
        panic(fmt.Sprintf("invalid regex %q: %v", pattern, err))
    }
    return re
}

// ✗ Bad — panic for a normal failure
func GetUser(id string) *User {
    user, err := db.Find(id)
    if err != nil {
        panic(err) // forces callers to recover instead of handling an expected error
    }
    return user
}
```

### Recovering from panics

Recover only at a boundary that can safely abandon the failed operation and report
its failure. Recovery cannot repair corrupted shared state. It must run directly
in a deferred function in the panicking goroutine; a parent cannot recover a child
goroutine's panic.

For an isolated task whose contract explicitly converts panics to errors:

```go
func runIsolatedTask(work func() error) (err error) {
    defer func() {
        if p := recover(); p != nil {
            err = fmt.Errorf("task panicked: %v\n%s", p, debug.Stack())
        }
    }()
    return work()
}
```

The named result is needed here because the deferred function assigns it. Return
or report that error; do not convert a failed task into apparent success.

`net/http.Server` already recovers handler panics and aborts the response. It does
not protect goroutines started by the handler. Custom recovery must preserve
`http.ErrAbortHandler`; after a response is committed it cannot reliably replace
the status or body with a 500 response. See [Handler](https://pkg.go.dev/net/http#Handler)
and the [panic/recover rules](https://go.dev/ref/spec#Handling_panics).

For structured panic recovery with `samber/oops`, see the `samber/cc-skills-golang@golang-samber-oops` skill.

## Why Use `samber/oops`

- **Stack traces** — you see `"connection refused"` but need to know where it originated
- **Structured context** — user ID, tenant ID, or request metadata attached to the error
- **Error codes** — machine-readable identifiers for monitoring dashboards
- **Public/private separation** — safe message to show end users
- ...

For projects using `samber/oops`, the following example attaches this context to an error. Adopting the library is a separate dependency choice. Every `oops` error implements the standard `error` interface, works with `errors.Is`/`errors.As`, and adds structured attributes:

```go
// ✗ Before — standard errors, no context
func (s *OrderService) CreateOrder(ctx context.Context, req CreateOrderReq) error {
    err := s.db.Insert(ctx, req.Order)
    if err != nil {
        return fmt.Errorf("inserting order: %w", err)
    }
    return nil
}

// ✓ After — samber/oops, rich context for debugging
func (s *OrderService) CreateOrder(ctx context.Context, req CreateOrderReq) error {
    err := s.db.Insert(ctx, req.Order)
    if err != nil {
        return oops.
            In("order-service").
            Code("order_insert_failed").
            User(req.UserID).
            With("order_id", req.Order.ID).
            Wrapf(err, "inserting order")
    }
    return nil
}
```

When this error is logged, you get the stack trace, user ID, order ID, domain, error code, and the full error chain — all structured and machine-parseable.

## Logging Errors with `slog`

→ See `samber/cc-skills-golang@golang-observability` skill for comprehensive structured logging guidance, including `slog` setup, log levels, log handlers, HTTP middleware, and cost considerations.
