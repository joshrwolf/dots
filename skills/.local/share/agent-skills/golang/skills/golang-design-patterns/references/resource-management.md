# Resource Management Patterns

## Table of Contents

- [Defer Close Immediately](#defer-close-immediately)
- [`runtime.AddCleanup` over `runtime.SetFinalizer`](#runtimeaddcleanup-over-runtimesetfinalizer)
- [Resource Pools](#resource-pools)
- [Graceful Shutdown](#graceful-shutdown)

## Defer Close Immediately

`defer Close()` MUST be called immediately after opening — NEVER delay. This prevents leaks when code is modified later and new return paths are added:

```go
// Good — defer is right next to open
f, err := os.Open(path)
if err != nil {
    return err
}
defer f.Close()

// Bad — Close() is far from Open(), easy to forget when adding early returns
f, err := os.Open(path)
if err != nil {
    return err
}
// ... 50 lines of code ...
f.Close() // might never run if a new return is added above
```

This applies to all closeable resources: files, SQL rows, HTTP response bodies, gzip readers, underlying readers owned by code using bufio.Scanner (Scanner itself has no Close method), etc.

```go
resp, err := http.Get(url)
if err != nil {
    return err
}
defer resp.Body.Close()

rows, err := db.QueryContext(ctx, query)
if err != nil {
    return err
}
defer rows.Close()
```

## `runtime.AddCleanup` over `runtime.SetFinalizer`

`runtime.AddCleanup` SHOULD be preferred over `runtime.SetFinalizer` (Go 1.24+):

```go
type Resource struct {
    handle uintptr
}

func NewResource() *Resource {
    r := &Resource{handle: acquireHandle()}
    runtime.AddCleanup(r, func(handle uintptr) {
        releaseHandle(handle)
    }, r.handle)
    return r
}
```

`AddCleanup` is preferred because:

- Multiple cleanups can be attached to the same object
- The cleanup function receives a copy of the value, not the object itself — no resurrection risk
- Cleanups run even if the object is part of a cycle

Cleanups are a fallback, not prompt release: they may never run before exit. The callback and argument must not retain the wrapper. Methods using the raw handle may need runtime.KeepAlive(r) after the last operation. An explicit Close must coordinate with the cleanup to avoid double release.

## Resource Pools

For database/sql use its existing DB connection pool and configure its limits. For other resource types, a bounded channel can represent ownership; sync.Pool is not a resource pool and cannot enforce limits or close evicted items. This example is a fixed-size pool: callers return each borrowed connection exactly once and stop borrowing before shutdown.

```go
type ConnPool struct {
    conns chan *Conn
}

func NewConnPool(maxSize int, factory func() (*Conn, error)) (*ConnPool, error) {
    if maxSize <= 0 { return nil, errors.New("pool size must be positive") }
    pool := &ConnPool{
        conns: make(chan *Conn, maxSize),
    }
    // Pre-fill with initial connections
    for range maxSize {
        conn, err := factory()
        if err != nil {
            close(pool.conns)
            for opened := range pool.conns { opened.Close() }
            return nil, fmt.Errorf("creating connection: %w", err)
        }
        pool.conns <- conn
    }
    return pool, nil
}

func (p *ConnPool) Get(ctx context.Context) (*Conn, error) {
    select {
    case conn := <-p.conns:
        return conn, nil
    case <-ctx.Done():
        return nil, ctx.Err()
    }
}

func (p *ConnPool) Put(conn *Conn) {
    select {
    case p.conns <- conn:
    default:
        conn.Close() // pool is full, discard
    }
}
```

## Graceful Shutdown

Use `signal.NotifyContext` to initiate shutdown, stop accepting work, and wait for
in-flight operations before closing their dependencies. Choose what happens when
the grace period expires. This schematic executable uses a hard process exit on
failure; it assumes ordinary HTTP handlers own all work using `db`, with no
hijacked connections or detached background workers:

```go
func main() {
    ctx, stop := signal.NotifyContext(context.Background(),
        syscall.SIGINT, syscall.SIGTERM,
    )
    defer stop()

    srv := &http.Server{Addr: ":8080", Handler: router}

    // Report startup failures instead of waiting forever for a signal.
    serveErr := make(chan error, 1)
    go func() { serveErr <- srv.ListenAndServe() }()

    slog.Info("server started", "addr", ":8080")

    // Wait for interrupt or serving failure, retaining a failure exit status.
    exitCode := 0
    select {
    case <-ctx.Done():
    case err := <-serveErr:
        if !errors.Is(err, http.ErrServerClosed) {
            slog.Error("server error", "error", err)
            exitCode = 1
        }
    }
    stop() // restore normal signal handling for a second interrupt
    slog.Info("shutting down...")

    // Give outstanding requests time to complete
    shutdownCtx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
    defer cancel()

    if err := srv.Shutdown(shutdownCtx); err != nil {
        slog.Error("shutdown error", "error", err)
        // Policy: terminate without closing dependencies under active handlers.
        // os.Exit skips defers; the OS reclaims connections, but work may be lost.
        os.Exit(1)
    }

    // Successful Shutdown has waited for ordinary HTTP handlers to return.
    if err := db.Close(); err != nil {
        slog.Error("database close error", "error", err)
        os.Exit(1)
    }
    slog.Info("shutdown complete")
    if exitCode != 0 {
        os.Exit(exitCode)
    }
}
```

`Server.Close` closes connections but does not join handler goroutines; calling
it after a timed-out `Shutdown` does not make it safe to close their dependencies.
If a hard exit is unacceptable, explicitly cancel and await all owned work before
closing those dependencies, accepting that non-cooperative work can delay exit.

`Shutdown` does not wait for hijacked connections such as WebSockets or detached
background workers. Track their completion separately. In applications with a
`run() error` boundary, preserve the same distinction: graceful completion permits
cleanup; a timeout requires the chosen wait-or-exit policy, not unconditional
deferred dependency closure.

This pattern applies to any long-running service — gRPC servers, message consumers, background workers. The key elements are:

1. Capture OS signals with `signal.NotifyContext`
2. Start the server in a goroutine
3. Block on context cancellation
4. Shut down with a timeout to drain in-flight requests
5. Close dependencies after their users finish, or apply the explicit hard-exit policy

For goroutine shutdown patterns, see the `samber/cc-skills-golang@golang-concurrency` skill.
