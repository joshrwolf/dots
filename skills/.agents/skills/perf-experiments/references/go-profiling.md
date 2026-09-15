# Diagnose Go performance

Start with project commands and the installed toolchain's help. Select evidence
for the symptom; do not collect every profile on every experiment.

| Symptom | Evidence to inspect |
| --- | --- |
| CPU cost | CPU pprof, flat and cumulative costs, annotated hot source |
| Allocation/GC cost | `alloc_space` and `alloc_objects`, benchmark allocation metrics |
| Retained heap | `inuse_space` and `inuse_objects`, over a representative lifetime |
| Contention | Mutex/block profiles, with sampling enabled for diagnostic runs |
| Latency with little CPU use | Execution trace, scheduling and blocking profiles; external dependency timings |

Go heap profiles do not measure total process RSS. Allocation reductions can
increase retained memory; check the metric the contract actually protects.
CPU profiles alone cannot explain waiting time.

## Collection and interpretation

Go test benchmarks support profile collection without application endpoints.
For running services, use existing diagnostic support or scoped instrumentation
under representative load. Keep CPU, memory, and contention investigations in
separate runs where their collection overhead could interfere.

Retain the exact binary alongside its profiles. Give the agent bounded text
from `go tool pprof` (top, cumulative top, and relevant source listings) and use
the interactive view when it helps resolve a question. Compare absolute costs
for comparable work; profile percentages change when other work gets cheaper.

Execution traces can expose goroutine scheduling, blocking, and GC interaction.
`go tool trace -pprof=TYPE` exports `net`, `sync`, `syscall`, or `sched` profiles
for pprof analysis. Add trace regions or profile labels only when they clarify
workload attribution.

## Benchmark and optimization traps

- Use `testing.B.Loop` for new benchmarks where supported. Check setup timing,
  output use, input diversity, and whether the compiler eliminates the intended work.
- Exercise relevant concurrency and input sizes. A microbenchmark win needs an
  end-to-end check before claiming an application improvement.
- Keep PGO settings explicit and fixed during source comparisons. Evaluate PGO
  separately using representative profiles if it is a plausible improvement.
- Use escape-analysis or compiler diagnostics to investigate a measured hotspot,
  not as a mandate to eliminate every allocation.
- Pooling, caching, additional goroutines, and `unsafe` require evidence and
  checks for ownership, retained memory, invalidation, cancellation, and semantics
  as relevant. Prefer changes whose complexity is justified by measured benefit.

## Primary references

- [Go diagnostics](https://go.dev/doc/diagnostics)
- [runtime/pprof](https://pkg.go.dev/runtime/pprof)
- [Execution trace](https://pkg.go.dev/cmd/trace)
- [Benchmark loops](https://go.dev/blog/testing-b-loop)
- [Go PGO](https://go.dev/doc/pgo)

Consult current documentation when installed behavior is unclear; avoid pinning
release-specific claims or copying tool manuals into this skill.
