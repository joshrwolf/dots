---
name: golang-concurrency
description: "Design and debug Go concurrency and cancellation. Use for goroutine lifetimes, channels and select, mutexes and atomics, errgroup and worker pools, context propagation, deadlines and timeouts, graceful shutdown, races, deadlocks, and goroutine leaks."
license: MIT
---

# Go concurrency and context

Every goroutine has an owner, a predictable exit and a way to await it. Do not add
concurrency without a measured or structural need.

## Goroutines and channels

- Before starting a goroutine, settle how it exits, who signals it to stop, who
  waits for it, and who owns its channels. `WaitGroup.Wait` only waits; it does
  not signal shutdown.
- Only the sender closes a channel. Specify channel direction in signatures.
- Start with unbuffered channels. A buffer needs a reason, such as a known burst
  size, because it hides backpressure.
- Every blocking send, receive or wait selects on `ctx.Done()` when its owner can
  stop waiting.
- A sent struct that contains slices, maps or pointers still shares that data.
  Transfer ownership explicitly, copy, or synchronize.
- Since Go 1.23, `time.After` in a loop does not leak. Reuse a timer with `Reset`
  only when profiling shows its allocation matters; see
  [channels-and-select.md](references/channels-and-select.md#timeafter-and-timer-reuse-in-loops).

## Choosing a primitive

| Need | Use |
| --- | --- |
| Pass data or ownership between goroutines | Channel |
| Protect shared fields | `sync.Mutex` / `RWMutex`; never upgrade RLock to Lock |
| Counters and flags | Typed atomics (`atomic.Int64`, `atomic.Bool`) |
| Write-once/read-many or disjoint-key map | `sync.Map`; otherwise a mutex and map |
| One-time initialization | `sync.OnceFunc` / `OnceValue` / `OnceValues` (cached errors are not retried) |
| Deduplicate concurrent calls | `x/sync/singleflight` |
| Wait for tasks, no errors | `wg.Go(f)` (Go 1.25+) instead of `Add`/`go`/`Done` |
| First error, cancel siblings, bounded parallelism | `errgroup.WithContext` plus `SetLimit(n)` |

Call `wg.Add` before `go` in the manual form; calling it inside the goroutine lets
`Wait` return early. `wg.Go` requires a function that does not panic.

## Context

- `ctx context.Context` comes first and the caller's context flows through the
  whole operation. Create `context.Background()` only at roots or for an explicitly
  independent lifetime. Use `t.Context()` in tests and `r.Context()` in handlers.
- Pass context per call. Do not store it in long-lived structs; a value whose
  lifetime is one operation may hold one if its API says so.
- Pass `context.TODO()`, never `nil`, when no context exists yet.
- Call `cancel` on every path unless ownership of it is returned to the caller.
- Use `context.WithoutCancel` (Go 1.21+) for work that must outlive the request,
  such as audit writes, and bound it with its own timeout.
- Context values carry request-scoped metadata under unexported key types, never
  optional parameters.
- Cancellation is cooperative. Long loops check `ctx.Err()`, and retries back off
  with `select` on `ctx.Done()`.

## Leaks, races and deadlocks

- Run `go test -race` on concurrent code. It finds data races that were exercised,
  not leaks or deadlocks.
- In tests, cancel owned goroutines and wait for them. Use `testing/synctest` for
  timer-dependent code (see `golang-testing`).
- In Go 1.27+, `runtime/pprof` exposes a `goroutineleak` profile
  (`/debug/pprof/goroutineleak`) with no build flag. A full stack dump is at
  `/debug/pprof/goroutine?debug=2`.

## References

- [channels-and-select.md](references/channels-and-select.md): goroutine lifecycle,
  panics at goroutine boundaries, closing, buffer sizing, select.
- [sync-primitives.md](references/sync-primitives.md): mutex, atomics, Once,
  Pool, singleflight and errgroup details.
- [pipelines.md](references/pipelines.md): fan-out/fan-in, bounded workers,
  iterator pipelines.
- [context-cancellation.md](references/context-cancellation.md): `WithCancelCause`,
  `AfterFunc`, deadlines, `WithoutCancel`.
- [context-http-services.md](references/context-http-services.md): handler and
  client context, `NewRequestWithContext`, `*Context` database calls.
- [context-values-tracing.md](references/context-values-tracing.md): value keys,
  correlation IDs, trace propagation.
- [tracebacks.md](references/tracebacks.md): `GOTRACEBACK` and live goroutine dumps.
- [samber-ro.md](references/samber-ro.md): only when the project uses samber/ro.

Graceful service shutdown is covered by the `golang` skill's resources
reference. Lock contention profiling belongs to `golang-performance`.
