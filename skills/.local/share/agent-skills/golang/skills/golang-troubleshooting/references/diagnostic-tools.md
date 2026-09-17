# Diagnostic Tools

## Table of Contents

- [Runtime Diagnostics (GODEBUG)](#runtime-diagnostics-godebug)
  - [Go documentation command](#go-documentation-command)
  - [GC Tracing](#gc-tracing)
  - [Scheduler Tracing](#scheduler-tracing)
  - [GOTRACEBACK](#gotraceback)
- [Advanced Analysis](#advanced-analysis)

## Runtime Diagnostics (GODEBUG)

### Go documentation command

Use `go doc`, not `go tool doc`. Go 1.26 removed the old `cmd/doc` / `go tool doc` path. Go 1.27 added `package@version` lookups (`go doc golang.org/x/tools/cmd/stringer@v0.30.0`) and an `-ex` flag that lists executable examples.

### GC Tracing

```bash
GODEBUG=gctrace=1 ./app
```

**Output:**

```
gc 123 @45.67s 4%: 0.8+10+0.3 ms clock, 6+5/10/0 ms cpu, 512->300->150 MB
```

| Field            | Meaning                                         |
| ---------------- | ----------------------------------------------- |
| 4%               | GC CPU overhead (if >10%, over-allocating)      |
| 512->300->150 MB | Heap at GC start -> heap at GC end -> live heap |
| Large pause      | Allocation storm                                |

### Scheduler Tracing

```bash
GODEBUG=schedtrace=1000,scheddetail=1 ./app
```

| Signal               | Meaning                            |
| -------------------- | ---------------------------------- |
| runqueue high        | CPU saturation, goroutines waiting |
| idleprocs=0          | Fully busy, at capacity            |
| spinningthreads      | Lock contention                    |
| threads > gomaxprocs | Blocking syscalls                  |

### GOTRACEBACK

Get full stack traces on panic:

```bash
GOTRACEBACK=all ./app
```

| Level    | Shows                                 |
| -------- | ------------------------------------- |
| `none`   | No stack traces                       |
| `single` | Current goroutine only (default)      |
| `all`    | All goroutines (useful for deadlocks) |
| `system` | All goroutines + runtime frames       |

---

## Advanced Analysis

→ See `samber/cc-skills-golang@golang-benchmark` skill (compiler-analysis.md) for detailed guides on escape analysis interpretation, assembly inspection, and compiler diagnostics (SSA dump, inlining decisions). See also trace.md for execution tracer analysis.
