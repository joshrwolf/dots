---
name: golang-concurrency
description: "Design and debug Go concurrency. Use for goroutine lifetimes, channel ownership, synchronization, worker pools, and bounded parallelism."
user-invocable: true
license: MIT
compatibility: Designed for Claude Code, Codex or similar harness, and for projects using Golang.
metadata:
  author: samber
  version: "1.2.1"
  openclaw:
    emoji: "⚡"
    homepage: https://github.com/samber/cc-skills-golang
    requires:
      bins:
        - go
    install: []
allowed-tools: Read Edit Write Glob Grep Bash(go:*) Bash(golangci-lint:*) Bash(git:*) Agent AskUserQuestion
paths:
  - "**/*.go"
---

> **Community default.** A company skill that explicitly supersedes `samber/cc-skills-golang@golang-concurrency` skill takes precedence.

# Go Concurrency Best Practices

Go's concurrency model is built on goroutines and channels. Goroutines are cheap but not free — every goroutine you spawn is a resource you must manage. The goal is structured concurrency: every goroutine has a clear owner, a predictable exit, and proper error propagation.

## Core Principles

1. **Every goroutine must have a clear exit** — use cancellation or bounded work plus a way to await completion; WaitGroup waits but does not signal shutdown
2. **Share memory by communicating** — channels transfer ownership explicitly; mutexes protect shared state but make ownership implicit
3. **Define ownership of sent values** — pointers can transfer ownership safely; copied structs containing slices, maps, or pointers still share referenced data
4. **Only the sender closes a channel** — closing from the receiver side panics if the sender writes after close
5. **Specify channel direction** (`chan<-`, `<-chan`) — the compiler prevents misuse at build time
6. **Default to unbuffered channels** — larger buffers mask backpressure; use them only with measured justification
7. **Make blocking operations cancellable when their owner can stop waiting** — use `ctx.Done()` or another explicit completion contract
8. **Avoid repeated `time.After` in hot loops** — each call allocates a timer and creates unnecessary churn; use `time.NewTimer` + `Reset` for long-running loops
9. **Verify goroutine shutdown in tests** using cancellation and explicit completion signals

For detailed channel/select code examples, see [Channels and Select Patterns](references/channels-and-select.md).

## Channel vs Mutex vs Atomic

| Scenario | Use | Why |
| --- | --- | --- |
| Passing data between goroutines | Channel | Communicates ownership transfer |
| Coordinating goroutine lifecycle | Channel + context | Clean shutdown with select |
| Protecting shared struct fields | `sync.Mutex` / `sync.RWMutex` | Simple critical sections |
| Simple counters, flags | `sync/atomic` | Lock-free, lower overhead |
| Many readers, few writers on a map | `sync.Map` | Specialized for write-once/read-many or disjoint-key access; unsynchronized ordinary map reads/writes race and may cause a fatal runtime error |
| Caching expensive computations | `sync.Once` / `singleflight` | Execute once or deduplicate |

## WaitGroup vs errgroup

| Need | Use | Why |
| --- | --- | --- |
| Wait for goroutines, errors not needed | `sync.WaitGroup` | Await completion |
| Wait + collect first error | `errgroup.Group` | Error propagation |
| Wait + cancel siblings on first error | `errgroup.WithContext` | Context cancellation on error |
| Wait + limit concurrency | `errgroup.SetLimit(n)` | Built-in worker pool |

For new or changed Go 1.25+ code, prefer `wg.Go` over manual `Add`/`go`/`Done`
when the task only needs completion tracking and the function must not panic.
Cancellation can still be handled inside that function. Use `errgroup` when the
group should propagate errors, cancel siblings, or limit concurrency. Keep manual
bookkeeping for older targets or lifetimes that are not one function invocation.

## Sync Primitives Quick Reference

| Primitive | Use case | Key notes |
| --- | --- | --- |
| `sync.Mutex` | Protect shared state | Keep critical sections short; hold across I/O only when the protected operation requires serialization |
| `sync.RWMutex` | Many readers, few writers | Never upgrade RLock to Lock (deadlock) |
| `sync/atomic` | Simple counters, flags | Prefer typed atomics (Go 1.19+): `atomic.Int64`, `atomic.Bool` |
| `sync.Map` | Concurrent map, read-heavy | No explicit locking; use `RWMutex`+map when writes dominate |
| `sync.Pool` | Reuse temporary objects | Reset before reuse; items may disappear at any time |
| `sync.Once` | One-time initialization | Go 1.21+: `OnceFunc`, `OnceValue`, `OnceValues` |
| `sync.WaitGroup` | Await task completion | Prefer `Go` on Go 1.25+ as described above; `Wait` does not signal cancellation |
| `x/sync/singleflight` | Deduplicate concurrent calls | Cache stampede prevention |
| `x/sync/errgroup` | Goroutine group + errors | `SetLimit(n)` replaces hand-rolled worker pools |

For detailed examples and anti-patterns, see [Sync Primitives Deep Dive](references/sync-primitives.md).

## Concurrency Checklist

Before spawning a goroutine, answer:

- [ ] **How will it exit?** — context cancellation, channel close, or explicit signal
- [ ] **Can I signal it to stop?** — pass `context.Context` or done channel
- [ ] **Can I wait for it?** — `sync.WaitGroup` or `errgroup`
- [ ] **Who owns the channels?** — creator/sender owns and closes
- [ ] **Should this be synchronous instead?** — don't add concurrency without measured need

## Pipelines and Worker Pools

For pipeline patterns (fan-out/fan-in, bounded workers, generator chains, Go 1.23+ iterators, `samber/ro`), see [Pipelines and Worker Pools](references/pipelines.md).

## Concurrency Audits

For a requested concurrency audit, check:

1. Find all goroutine spawns (`go func`, `go method`) and verify shutdown mechanisms
2. Search for mutable globals and shared state without synchronization
3. Audit channel usage — ownership, direction, closure, buffer sizes
4. Find `time.After` in loops, missing `ctx.Done()` in select, unbounded spawning
5. Check mutex usage, `sync.Map`, atomics, and thread-safety documentation

## Common Mistakes

| Mistake | Fix |
| --- | --- |
| Fire-and-forget goroutine | Provide stop mechanism (context, done channel) |
| Closing channel from receiver | Only the sender closes |
| `time.After` in hot loop | Reuse `time.NewTimer` + `Reset` |
| Missing `ctx.Done()` in select | Always select on context to allow cancellation |
| Unbounded goroutine spawning | Use `errgroup.SetLimit(n)` or semaphore |
| Mutating shared data after a send | Transfer exclusive ownership, copy referenced mutable data, or synchronize access |
| `wg.Add` inside goroutine | Call `Add` before `go` — `Wait` may return early otherwise |
| Forgetting `-race` in CI | Always run `go test -race ./...` |

## Cross-References

- → See `samber/cc-skills-golang@golang-performance` skill for false sharing, cache-line padding, `sync.Pool` hot-path patterns
- → See `samber/cc-skills-golang@golang-context` skill for cancellation propagation and timeout patterns
- → See `samber/cc-skills-golang@golang-safety` skill for concurrent map access and race condition prevention
- Read [Panic tracebacks](references/tracebacks.md) for GOTRACEBACK settings and live goroutine stack dumps.
- → See `samber/cc-skills-golang@golang-design-patterns` skill for graceful shutdown patterns
- → See `samber/cc-skills-golang@golang-continuous-integration` skill for automated AI-driven code review in CI using these guidelines

### Goroutine leak profile

The goroutine leak profile (experimental behind `GOEXPERIMENT=goroutineleakprofile` in Go 1.26) is generally available in `runtime/pprof` since Go 1.27 — no build flag required. It is a useful production-oriented leak signal alongside the existing tools below.

```bash
curl http://localhost:6060/debug/pprof/goroutineleak?debug=2
go tool pprof http://localhost:6060/debug/pprof/goroutineleak
```

Keep existing tools:

- tests: cancel owned goroutines and wait for their completion
- runtime count: `runtime.NumGoroutine()`
- stack dump: `/debug/pprof/goroutine?debug=2`
- race checks: `go test -race ./...`

## References

- [Go Concurrency Patterns: Pipelines](https://go.dev/blog/pipelines)
- [Effective Go: Concurrency](https://go.dev/doc/effective_go#concurrency)
