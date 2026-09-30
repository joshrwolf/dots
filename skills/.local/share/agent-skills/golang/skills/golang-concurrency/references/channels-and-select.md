# Channels and Select Patterns

## Table of Contents

- [Goroutine Lifecycle](#goroutine-lifecycle)
  - [Panic Recovery at Goroutine Boundaries](#panic-recovery-at-goroutine-boundaries)
- [Channel Direction](#channel-direction)
- [Channel Closing](#channel-closing)
- [Buffer Size](#buffer-size)
- [Select for Non-Blocking Communication](#select-for-non-blocking-communication)
- [Avoid Repeated `time.After` in Hot Loops](#avoid-repeated-timeafter-in-hot-loops)

## Goroutine Lifecycle

NEVER start a goroutine without knowing how it stops. Every goroutine MUST answer: **how will it stop?**

```go
// ✗ Bad — fire-and-forget, no way to stop or wait
func startWorker() {
    go func() {
        for {
            doWork() // runs forever, leaks on shutdown
        }
    }()
}

// ✓ Good (Go 1.25+) — task respects context cancellation, caller can wait
func startWorker(ctx context.Context) *sync.WaitGroup {
    var wg sync.WaitGroup
    wg.Go(func() {
        for {
            select {
            case <-ctx.Done():
                return
            default:
                doWork(ctx)
            }
        }
    })
    return &wg
}
```

### Panic Recovery at Goroutine Boundaries

An unrecovered panic in any goroutine terminates the process. Recover only when the operation can be safely abandoned without leaving shared state invalid. Recovery must run in the panicking goroutine, and the failure must reach its owner. For an explicitly isolated worker:

```go
go func() {
    defer func() {
        if r := recover(); r != nil {
            reportFailure(fmt.Errorf("worker panicked: %v\n%s", r, debug.Stack()))
        }
    }()
    doWork(ctx)
}()
```

## Channel Direction

Specify direction in function signatures to prevent misuse at compile time:

```go
// ✗ Bad — caller could accidentally close or send on a receive-only channel
func consume(ch chan int) { ... }

// ✓ Good — compiler enforces correct usage
func produce(ch chan<- int) { ... } // send-only
func consume(ch <-chan int) { ... } // receive-only
```

## Channel Closing

Close a data channel only after all sends have finished, usually from its producer or a coordinator that waits for all producers. Receivers should signal cancellation separately. Closing is unnecessary solely for garbage collection.

```go
// ✓ Good — producer closes when done
func generate(ctx context.Context) <-chan int {
    ch := make(chan int)
    go func() {
        defer close(ch) // sender closes
        for i := 0; ; i++ {
            select {
            case ch <- i:
            case <-ctx.Done():
                return
            }
        }
    }()
    return ch
}
```

## Buffer Size

| Size | When to use |
| --- | --- |
| 0 (unbuffered) | Default. Synchronizes sender and receiver — use when you need handoff guarantees |
| 1 | Signal channels (`done := make(chan struct{}, 1)`), or when sender must not block on a single pending item |
| N > 1 | Only with measured justification — document why N was chosen and what happens when the buffer fills |

```go
// ✓ Good — unbuffered for synchronous handoff
ch := make(chan Result)

// ✓ Good — buffered 1 for signal
done := make(chan struct{}, 1)

// ✗ Suspicious — arbitrary large buffer hides backpressure problems
// Give explanation in comments.
ch := make(chan Task, 1000) // why 1000? what if it fills?
```

## Select for Non-Blocking Communication

For a cancellable stage, include `ctx.Done()` around potentially blocked sends and receives:

```go
func process(ctx context.Context, in <-chan Task, out chan<- Result) {
    for {
        select {
        case <-ctx.Done():
            return
        case task, ok := <-in:
            if !ok {
                return // channel closed
            }
            result := handle(ctx, task)
            select {
            case out <- result:
            case <-ctx.Done():
                return
            }
        }
    }
}
```

## Avoid Repeated `time.After` in Hot Loops

```go
// ✗ Bad — creates a new timer on every iteration
for {
    select {
    case msg := <-ch:
        handle(msg)
    case <-time.After(5 * time.Second): // repeated allocation/churn
        handleTimeout()
    }
}

// ✓ Good (Go 1.23+) — reuse the timer
timer := time.NewTimer(5 * time.Second)
defer timer.Stop()
for {
    select {
    case msg, ok := <-ch:
        if !ok {
            return
        }
        timer.Stop()
        timer.Reset(5 * time.Second)
        handle(msg)
    case <-timer.C:
        handleTimeout()
        timer.Reset(5 * time.Second)
    }
}
```

The synchronous timer-channel guarantees apply when the main module declares `go 1.23` or newer and `GODEBUG=asynctimerchan=1` has not restored old behavior. With old semantics, Stop returning false does not prove an unread value exists: it may already have been received. Coordinate ownership and drain only a pending value before Reset; do not blindly block on a drained timer channel.
