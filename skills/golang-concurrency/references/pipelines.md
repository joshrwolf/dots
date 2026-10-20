# Pipelines and Worker Pools

## Table of Contents

- [Pipeline Pattern](#pipeline-pattern)
- [Fan-Out / Fan-In](#fan-out--fan-in)
- [Worker Pool with errgroup](#worker-pool-with-errgroup)
- [Bounded Concurrency with Semaphore](#bounded-concurrency-with-semaphore)
- [Pipeline Alternatives](#pipeline-alternatives)
  - [Go 1.23+ Iterators (range-over-func)](#go-123-iterators-range-over-func)
  - [samber/ro](#samberro)
- [Goroutine Leak Detection](#goroutine-leak-detection)
- [Common Pipeline Mistakes](#common-pipeline-mistakes)

## Pipeline Pattern

A pipeline is a series of stages connected by channels, where each stage is a goroutine (or group of goroutines) that:

1. Receives values from an upstream channel
2. Processes each value
3. Sends results to a downstream channel

```go
// Stage 1: Generate integers
func generate(ctx context.Context, nums ...int) <-chan int {
    out := make(chan int)
    go func() {
        defer close(out)
        for _, n := range nums {
            select {
            case out <- n:
            case <-ctx.Done():
                return
            }
        }
    }()
    return out
}

// Stage 2: Square each integer
func square(ctx context.Context, in <-chan int) <-chan int {
    out := make(chan int)
    go func() {
        defer close(out)
        for {
            select {
            case <-ctx.Done():
                return
            case n, ok := <-in:
                if !ok { return }
                select {
                case out <- n * n:
                case <-ctx.Done(): return
                }
            }
        }
    }()
    return out
}

// Usage
func main() {
    ctx, cancel := context.WithCancel(context.Background())
    defer cancel()

    ch := generate(ctx, 2, 3, 4)
    results := square(ctx, ch)

    for v := range results {
        fmt.Println(v) // 4, 9, 16
    }
}
```

**Key rules for pipelines**:

- Pipeline stages MUST accept and respect context cancellation — every stage must select on `ctx.Done()` to avoid goroutine leaks on early cancellation
- The producer (first stage) closes its output channel; each subsequent stage closes its own output
- NEVER create unbounded goroutines in pipeline stages
- Use unbuffered channels unless you have measured throughput needs

## Fan-Out / Fan-In

These examples use `WaitGroup.Go` (Go 1.25+); tasks observe the passed context.

**Fan-out**: multiple goroutines read from the same channel to parallelize CPU-bound work. **Fan-in**: multiple channels are merged into a single output channel.

```go
// Fan-out: N workers reading from the same input channel
func fanOut(ctx context.Context, in <-chan Task, workers int) <-chan Result {
    out := make(chan Result)
    var wg sync.WaitGroup

    for range workers {
        wg.Go(func() {
            for {
                select {
                case task, ok := <-in:
                    if !ok {
                        return
                    }
                    select {
                    case out <- process(ctx, task):
                    case <-ctx.Done():
                        return
                    }
                case <-ctx.Done():
                    return
                }
            }
        })
    }

    go func() {
        wg.Wait()
        close(out)
    }()
    return out
}
```

```go
// Fan-in: merge multiple channels into one
func fanIn(ctx context.Context, channels ...<-chan Result) <-chan Result {
    out := make(chan Result)
    var wg sync.WaitGroup

    for _, ch := range channels {
        wg.Go(func() {
            for {
                select {
                case <-ctx.Done(): return
                case v, ok := <-ch:
                    if !ok { return }
                    select {
                    case out <- v:
                    case <-ctx.Done(): return
                    }
                }
            }
        })
    }

    go func() {
        wg.Wait()
        close(out)
    }()
    return out
}
```

## Worker Pool with errgroup

Fan-out workers SHOULD use `errgroup.SetLimit` for bounded concurrency. For most use cases, `errgroup.SetLimit` replaces hand-rolled worker pools:

```go
func processAll(ctx context.Context, tasks []Task) error {
    g, ctx := errgroup.WithContext(ctx)
    g.SetLimit(10) // max 10 concurrent workers

    for _, task := range tasks {
        g.Go(func() error {
            if err := ctx.Err(); err != nil { return err }
            return process(ctx, task)
        })
    }
    return g.Wait()
}
```

`SetLimit` blocks `Go` while full; cancellation does not interrupt that admission wait. Tasks must respect cancellation, and a cancelled group may still admit queued calls. Check the derived context before starting new work. Use a worker pool when you need:

- Per-worker state (connections, buffers)
- Custom backpressure or priority scheduling
- Graceful draining with in-flight task completion

## Bounded Concurrency with Semaphore

When you need fine-grained concurrency control without errgroup:

```go
func processAll(ctx context.Context, items []Item) error {
    sem := make(chan struct{}, 10) // semaphore of 10
    var wg sync.WaitGroup

    for _, item := range items {
        select {
        case sem <- struct{}{}: // acquire before starting a goroutine
        case <-ctx.Done():
            wg.Wait()
            return ctx.Err()
        }
        wg.Go(func() {
            defer func() { <-sem }() // release
            process(ctx, item)
        })
    }
    wg.Wait()
    return nil
}
```

Prefer `errgroup.SetLimit` over this pattern when error propagation is needed.

## Pipeline Alternatives

### Go 1.23+ Iterators (range-over-func)

For in-process data transformations that do not need concurrency, iterators avoid the overhead of goroutines and channels:

```go
func Filter[T any](seq iter.Seq[T], pred func(T) bool) iter.Seq[T] {
    return func(yield func(T) bool) {
        for v := range seq {
            if pred(v) {
                if !yield(v) {
                    return
                }
            }
        }
    }
}

func Map[T, U any](seq iter.Seq[T], f func(T) U) iter.Seq[U] {
    return func(yield func(U) bool) {
        for v := range seq {
            if !yield(f(v)) {
                return
            }
        }
    }
}
```

Use iterators when:

- Processing is CPU-bound and does not benefit from parallelism
- You want lazy evaluation without goroutine overhead
- The data source is already sequential (slice, database cursor)

Use goroutine+channel pipelines when:

- Stages involve I/O (network, disk) that benefits from concurrency
- You need true parallelism across CPU cores
- Stages have different throughput characteristics

### samber/ro

For projects already using `samber/ro`, see `golang-samber-ro` for its observable/observer model and operators. It is an external reactive-stream library, not a standard iterator helper; consult the resolved version for Collect and Pipe signatures.

## Goroutine Leak Detection

Cancel each pipeline stage and wait for its completion in tests, including when
a consumer stops early. Exercise blocked sends and receives explicitly.

## Common Pipeline Mistakes

| Mistake | Fix |
| --- | --- |
| Missing `ctx.Done()` in pipeline stage | Always select on context to allow cancellation |
| Not closing output channel | Producer must `defer close(out)` |
| Unbounded goroutine spawning | Use `errgroup.SetLimit` or a semaphore |
| Sending mutable data through channel | Send copies or immutable values |
| Blocking send without select | Wrap channel sends in select with `ctx.Done()` |

→ See `samber/cc-skills-golang@golang-concurrency` skill for sync primitives and channel patterns.
