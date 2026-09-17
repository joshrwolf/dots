# Sync Primitives Deep Dive

## Table of Contents

- [sync.Mutex](#syncmutex)
  - [Embedding Convention](#embedding-convention)
- [sync.RWMutex](#syncrwmutex)
- [sync/atomic](#syncatomic)
- [sync.Map](#syncmap)
- [sync.Pool](#syncpool)
- [sync.Once](#synconce)
- [sync.WaitGroup](#syncwaitgroup)
  - [Go 1.25+: `wg.Go`](#go-125-wggo)
  - [Go <1.25 fallback](#go-125-fallback)
- [golang.org/x/sync/singleflight](#golangorgxsyncsingleflight)
- [golang.org/x/sync/errgroup](#golangorgxsyncerrgroup)
  - [Bounded Concurrency with SetLimit](#bounded-concurrency-with-setlimit)

## sync.Mutex

Protects shared state with exclusive access. Keep lock scope small, but preserve the invariant it protects. I/O or channel operations under a lock can create contention or deadlocks; some APIs deliberately serialize I/O and require that lock.

```go
type SafeCache struct {
    mu    sync.Mutex
    items map[string]string
}

func (c *SafeCache) Get(key string) (string, bool) {
    c.mu.Lock()
    defer c.mu.Unlock()
    v, ok := c.items[key]
    return v, ok
}

func (c *SafeCache) Set(key, value string) {
    c.mu.Lock()
    defer c.mu.Unlock()
    c.items[key] = value
}
```

### Embedding Convention

Embed the mutex as an unexported field, placed directly above the fields it protects:

```go
type Registry struct {
    mu      sync.Mutex // protects entries
    entries map[string]Entry
}
```

## sync.RWMutex

SHOULD be used when reads greatly outnumber writes. Multiple goroutines can hold `RLock` simultaneously; `Lock` is exclusive.

```go
type Config struct {
    mu     sync.RWMutex
    values map[string]string
}

func (c *Config) Get(key string) string {
    c.mu.RLock()
    defer c.mu.RUnlock()
    return c.values[key]
}

func (c *Config) Set(key, value string) {
    c.mu.Lock()
    defer c.mu.Unlock()
    c.values[key] = value
}
```

**Pitfall**: Do not upgrade RLock to Lock — this deadlocks. Release RLock first, then acquire Lock and recheck the condition: protected state may change between locks.

## sync/atomic

Lock-free operations for simple values. SHOULD be preferred over Mutex for simple counter operations. Faster than mutex for low-contention counters and flags.

```go
// ✓ Good — atomic for a simple counter
var requestCount atomic.Int64

func handleRequest() {
    requestCount.Add(1)
}

func getCount() int64 {
    return requestCount.Load()
}
```

```go
// ✓ Good — atomic.Bool for a shutdown flag
var shuttingDown atomic.Bool

func shutdown() {
    shuttingDown.Store(true)
}

func isRunning() bool {
    return !shuttingDown.Load()
}
```

Go 1.19+ provides typed atomics (`atomic.Int64`, `atomic.Bool`, `atomic.Pointer[T]`) — prefer these over raw `atomic.AddInt64`/`atomic.LoadInt64`.

## sync.Map

Optimized for two patterns: (1) keys are written once and read many times, (2) goroutines operate on disjoint key sets. Prefer a typed map with a mutex when that makes invariants clearer; benchmark performance rather than promising one implementation always wins.

```go
var cache sync.Map

func Get(key string) (any, bool) {
    return cache.Load(key)
}

func Set(key string, value any) {
    cache.Store(key, value)
}

func GetOrSet(key string, compute func() any) any {
    if v, ok := cache.Load(key); ok {
        return v
    }
    v, _ := cache.LoadOrStore(key, compute())
    return v
}
```

`Range` supports iteration but is not a consistent snapshot; there is no direct Len. Use a mutex-protected map for atomic snapshots or multi-key invariants. `GetOrSet` above may run compute concurrently more than once; LoadOrStore deduplicates publication, not computation.

## sync.Pool

Reuse temporary objects to reduce GC pressure. Pointers to local variables are valid; escape analysis gives them an appropriate lifetime. Objects may disappear from the pool at any time — do not store persistent state.

```go
var bufPool = sync.Pool{
    New: func() any {
        return new(bytes.Buffer)
    },
}

func process(data []byte) string {
    buf := bufPool.Get().(*bytes.Buffer)
    defer func() {
        buf.Reset()
        bufPool.Put(buf)
    }()

    buf.Write(data)
    // ... transform ...
    return buf.String()
}
```

**Rules**:

- Reset mutable contents before reuse, consistently on Get or Put; do not retain aliases after Put
- Do not assume an object from `Get()` is zeroed — Get may use New even when other goroutines have put items into the pool
- Best for short-lived, frequently allocated objects (buffers, encoders, temporary structs)

## sync.Once

MUST be used for one-time initialization. Execute exactly once, regardless of how many goroutines call it concurrently. Thread-safe by design.

```go
type DBClient struct {
    initOnce sync.Once
    conn     *sql.DB
    initErr  error
}

func (c *DBClient) getConn() (*sql.DB, error) {
    c.initOnce.Do(func() {
        c.conn, c.initErr = sql.Open("postgres", dsn)
    })
    return c.conn, c.initErr
}
```

This caches both success and failure; `Once.Do` does not retry. If its function panics, it is still considered done. The owner must stop users before closing the database; sql.Open creates a pool and may require PingContext to verify connectivity.

Go 1.21+ also provides `sync.OnceFunc`, `sync.OnceValue`, and `sync.OnceValues` for simpler use cases:

```go
var loadConfig = sync.OnceValue(func() *Config {
    cfg, err := parseConfig("config.yaml")
    if err != nil {
        panic(err)
    }
    return cfg
})

// Usage: cfg := loadConfig()
```

## sync.WaitGroup

Use `sync.WaitGroup` when you only need to wait for a set of goroutines to finish.

### Go 1.25+: `wg.Go`

`WaitGroup.Go` starts a goroutine, adds it to the group, and removes it from the group when the function returns.

```go
func processAll(items []Item) {
    var wg sync.WaitGroup

    for _, item := range items {
        // Go 1.22+ loop variables are per-iteration when the module has `go 1.22+`.
        // Do not add `item := item` solely for closure capture in modern modules.
        wg.Go(func() {
            process(item)
        })
    }

    wg.Wait()
}
```

Rules:

- `WaitGroup.Go` is Go 1.25+, not Go 1.24.
- The function passed to `wg.Go` must not panic.
- `WaitGroup` does not propagate errors and does not cancel siblings.
- For first-error-wins, cancellation, concurrency limits, or returned values, use `golang.org/x/sync/errgroup`.

**Benefits of `wg.Go()`**:

- No manual `Add`/`Done` bookkeeping
- Lower risk of `Add`/`Wait` ordering bugs
- Cleaner API for simple fire-and-wait work

**When to use**: Go 1.25+ projects for simple goroutines that must all finish, do not return errors, do not need cancellation, and must not panic. Use `errgroup` when work returns errors, needs cancellation, limits, or first-error behavior.

### Go <1.25 fallback

```go
func processAll(ctx context.Context, items []Item) {
    var wg sync.WaitGroup
    for _, item := range items {
        wg.Add(1) // Add BEFORE go
        go func(item Item) {
            defer wg.Done()
            process(ctx, item)
        }(item)
    }
    wg.Wait() // blocks until all goroutines finish
}
```

```go
// ✗ Bad — Add inside the goroutine (race: Wait may return before Add runs)
go func() {
    wg.Add(1)
    defer wg.Done()
    process(item)
}()
```

## golang.org/x/sync/singleflight

Deduplicates concurrent calls for the same key. When multiple goroutines request the same resource simultaneously, only one executes; the rest wait and share the result.

```go
var group singleflight.Group

func GetUser(ctx context.Context, id string) (*User, error) {
    v, err, _ := group.Do(id, func() (any, error) {
        // Only one goroutine executes this for a given id
        return db.QueryUser(ctx, id)
    })
    if err != nil {
        return nil, err
    }
    return v.(*User), nil
}
```

`Do` waits without an independent cancellation channel. In this example the first caller's context governs the shared query and its cancellation affects all waiters. For independent caller cancellation, use DoChan with a select and choose a separately owned bounded work context.

**Use cases**: cache stampede prevention, deduplicating expensive lookups (DB, API), rate-limited external service calls.

## golang.org/x/sync/errgroup

Goroutine group with error propagation. Returns the first error from any goroutine. With `WithContext`, signals cancellation on the first error or when Wait returns. It cannot forcibly stop goroutines; each operation must observe the context.

```go
func fetchAll(ctx context.Context, urls []string) ([]Response, error) {
    g, ctx := errgroup.WithContext(ctx) // cancel siblings on first error
    results := make([]Response, len(urls))

    for i, url := range urls {
        g.Go(func() error {
            resp, err := fetch(ctx, url)
            if err != nil {
                return fmt.Errorf("fetching %s: %w", url, err)
            }
            results[i] = resp // safe: each goroutine writes to its own index
            return nil
        })
    }

    if err := g.Wait(); err != nil {
        return nil, err
    }
    return results, nil
}
```

### Bounded Concurrency with SetLimit

SHOULD use `SetLimit` to bound concurrency and avoid unbounded goroutine spawning.

```go
g, ctx := errgroup.WithContext(ctx)
g.SetLimit(10) // at most 10 goroutines run concurrently

for _, task := range tasks {
    g.Go(func() error {
        return process(ctx, task)
    })
}
return g.Wait()
```

This replaces hand-rolled worker pools for most use cases.

→ See `samber/cc-skills-golang@golang-concurrency` skill for high-level patterns and decision trees.
