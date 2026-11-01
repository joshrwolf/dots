---
name: golang-performance
description: "Measure and optimize Go performance. Use for writing and running benchmarks, pprof CPU/heap/block profiles, execution traces, benchstat comparisons, escape analysis, allocation and GC pressure, caching, runtime tuning, and CI regression gates."
license: MIT
---

# Go performance

Measure before optimizing and report what was measured. Explain any optimization
that would otherwise look needlessly complicated.

## Measure

- First rule out external bottlenecks. If off-CPU time dominates (fgprof,
  goroutines blocked in `net` or `database/sql`, slow spans in traces), tune that
  dependency, not the Go code.
- Use `b.Loop()` (Go 1.24+) for new and changed benchmarks. It keeps setup out of
  the timing and the work alive, so neither `ResetTimer` nor sink variables are
  needed. Keep `b.N` only for older targets. Use one `b.Run` sub-benchmark per
  variant or size; that name is what benchstat compares.
- Follow the repository's benchmark file convention. Otherwise put benchmarks in
  the source file's `_test.go`, or in a `foo_bench_test.go` beside it when the
  measurement fixtures are substantial.
- Run `go test -run='^$' -bench=X -benchmem -count=10 ./pkg | tee new.txt`, then
  `benchstat old.txt new.txt`. A `~` result is inconclusive; say so rather than
  claiming a win. Report the workload, hardware and toolchain with the numbers.
- Run benchmarks serially. Concurrent runs on shared CPUs contaminate each other.
  Implementation variants can be built in parallel; measurements cannot.
- Compare on one toolchain. Go 1.27's size-specialized allocator shifts
  allocation-heavy baselines, so a comparison across 1.26→1.27 measures the
  toolchain.
- Profile from the benchmark: `-cpuprofile`, `-memprofile`, then `go tool pprof`;
  or `-trace` with `go tool trace`.

## Optimize by signal

| Signal | Read |
| --- | --- |
| High `alloc_objects`, GC churn | [memory.md](references/memory.md) |
| A function dominates CPU | [cpu.md](references/cpu.md) |
| GC pauses, OOM, container limits | [runtime.md](references/runtime.md) (GOGC, GOMEMLIMIT, PGO) |
| Goroutines blocked on I/O | [io-networking.md](references/io-networking.md) |
| Repeated work, wrong complexity | [caching.md](references/caching.md) |
| Hot mutex or block profile | `golang-concurrency` |
| DB time dominates | `golang-database` |

## Gotchas

- `http.Transport` keeps only 2 idle connections per host by default
  (`MaxIdleConnsPerHost`); raise it to match real concurrency.
- Logging arguments are evaluated even for disabled levels. Guard expensive
  attribute construction in hot paths.
- `GOMEMLIMIT` is a soft limit. Leave headroom for non-Go memory.
- `unsafe` and hand-rolled pooling need a measured benefit that justifies their
  maintenance and safety cost.

## References

- [pprof.md](references/pprof.md), [trace.md](references/trace.md),
  [benchstat.md](references/benchstat.md): reading profiles, traces and comparisons.
- [compiler-analysis.md](references/compiler-analysis.md): unexpected
  allocations, escape analysis, inlining, assembly.
- [tools.md](references/tools.md): fieldalignment, GODEBUG, fgprof, other
  focused diagnostics.
- [pprof-server.md](references/pprof-server.md): enabling and protecting
  `net/http/pprof` on a running service.
- [investigation-session.md](references/investigation-session.md),
  [prometheus-go-metrics.md](references/prometheus-go-metrics.md),
  [observability.md](references/observability.md): production runtime metrics
  and PromQL during an investigation.
- [ci-regression.md](references/ci-regression.md): benchmark regression gates in CI.
- [slice-internals.md](references/slice-internals.md),
  [map-internals.md](references/map-internals.md),
  [pointers.md](references/pointers.md): growth, retention, escape behavior,
  `unsafe.Pointer` rules, `weak.Pointer`.
- [samber-hot.md](references/samber-hot.md): only when the project uses samber/hot.
