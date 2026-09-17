# CPU Optimization

CPU-bound bottlenecks show up as functions dominating the CPU profile. The patterns below target the most common causes: missed inlining opportunities, poor cache utilization, and unnecessary computation.

## Table of Contents

- [Function Inlining](#function-inlining)
  - [Receiver semantics and inlining](#receiver-semantics-and-inlining)
- [Cache Locality](#cache-locality)
  - [Row-major traversal](#row-major-traversal)
  - [Contiguous 2D allocation](#contiguous-2d-allocation)
  - [Struct of Arrays (SoA) vs Array of Structs (AoS)](#struct-of-arrays-soa-vs-array-of-structs-aos)
  - [Pointer-heavy vs value-heavy data](#pointer-heavy-vs-value-heavy-data)
- [False Sharing](#false-sharing)
- [Instruction-Level Parallelism](#instruction-level-parallelism)
- [SIMD (Single Instruction, Multiple Data)](#simd-single-instruction-multiple-data)
  - [Handling CPU-specific instruction sets](#handling-cpu-specific-instruction-sets)
- [Tight Loops and the Scheduler](#tight-loops-and-the-scheduler)
- [Reflection and Type Assertions](#reflection-and-type-assertions)
- [Monotonic Time](#monotonic-time)

## Function Inlining

**Diagnose:** 1- `go tool pprof` (CPU profile) — look for hot functions with high cumulative CPU time; inlined functions can still appear in profiles, so CPU attribution alone does not prove a call was uninlined 2- `go build -gcflags="-m"` — grep for `"cannot inline"` on your hot-path functions; the reason (e.g., `"function too complex"`, `"unhandled op"`) tells you what to simplify

The Go compiler inlines small functions, eliminating call overhead. Inlining depends on the compiler's cost model, optimizations, and call site. Loops or calls are not universal disqualifiers; use `-gcflags="-m=2"` for decisions and reasons.

```go
// Logging adds work and may push the function over the inline budget
func abs(x int) int {
    if x < 0 {
        log.Printf("negative: %d", x) // inspect compiler diagnostics
        return -x
    }
    return x
}

// Good — simple enough to inline
func abs(x int) int {
    if x < 0 { return -x }
    return x
}
```

**Check inlining decisions:**

```bash
go build -gcflags="-m" ./... 2>&1 | grep "can inline"
go build -gcflags="-m" ./... 2>&1 | grep "inlining call"
```

Move side effects (logging, metrics) outside hot-path functions or guard them with conditional checks.

### Receiver semantics and inlining

Both pointer and value receiver methods can be inlined. Choose receiver semantics
for mutation, identity, and copy safety; changing from pointer to value changes
behavior. Check compiler diagnostics for the supported toolchain:

```go
// Pointer receiver — mutates the original; can be inlined
func (c *config) WithTimeout(d time.Duration) *config { c.timeout = d; return c }

// Value receiver — returns a modified copy; can also be inlined
func (c config) WithTimeout(d time.Duration) config { c.timeout = d; return c }
```

## Cache Locality

**Diagnose:** Use a CPU profile to identify expensive loops or memory copies. Time in `runtime.memmove` or indexing operations does not establish cache misses; investigate with hardware performance counters where available and controlled traversal/layout experiments. Benchmark row-first vs column-first traversal on the actual matrix size and hardware.

Many CPUs use 64-byte cache lines, while some targets use larger lines; verify the target architecture. Sequential memory access is dramatically faster than random access because the prefetcher can load the next cache line before you need it.

### Row-major traversal

Go stores 2D arrays in row-major order. Column-first traversal jumps across memory, causing cache misses:

```go
// Bad — column-first, jumps across memory (~10M cache misses)
for col := 0; col < 1024; col++ {
    for row := 0; row < 1024; row++ {
        sum += matrix[row][col]
    }
}

// Good — row-first, sequential access (~125K cache misses)
for row := 0; row < 1024; row++ {
    for col := 0; col < 1024; col++ {
        sum += matrix[row][col]
    }
}
```

The improvement depends on cache sizes, memory bandwidth and matrix dimensions; measure it.

### Contiguous 2D allocation

Allocating each row separately scatters data across the heap:

```go
// Bad — N separate allocations, poor cache locality
matrix := make([][]float64, rows)
for i := range matrix { matrix[i] = make([]float64, cols) }

// Good — single contiguous allocation, cache-friendly
data := make([]float64, rows*cols)
matrix := make([][]float64, rows)
for i := range matrix { matrix[i] = data[i*cols : (i+1)*cols] }
```

### Struct of Arrays (SoA) vs Array of Structs (AoS)

When iterating over a single field of a struct, AoS wastes cache space loading unused fields:

```go
// AoS — loading each Point (24 bytes) to read only x (8 bytes) = 66% cache waste
type Point struct { x, y, z float64 }
points := make([]Point, n)
for i := range points { sum += points[i].x }

// SoA — all x values contiguous, 100% cache utilization
type Points struct { xs, ys, zs []float64 }
for i := range ps.xs { sum += ps.xs[i] }
```

Use SoA when iterating over a subset of fields (physics, graphics, analytics). AoS is fine when accessing all fields together or for small structs.

### Pointer-heavy vs value-heavy data

Index-based data structures (nodes stored in a contiguous array, referenced by index) beat pointer-based structures for cache locality:

```go
// Pointer-based tree — each node scattered in heap, random cache misses
type Node struct { value int; left, right *Node }

// Index-based tree — nodes in contiguous array, cache-friendly
type Tree struct { nodes []Node }
type Node struct { value int; left, right int } // indices into nodes
```

## False Sharing

**Diagnose:** 1- `go tool pprof` (CPU profile + mutex profile) — look for atomic operations or counter updates consuming unexpectedly high CPU; in the mutex profile, look for contention on variables that shouldn't need locking 2- `go test -bench` — benchmark concurrent counter increments; if adding goroutines makes it _slower_ instead of faster, false sharing is likely

When goroutines update variables that share the same 64-byte CPU cache line, each write invalidates the other core's cache, causing severe degradation:

```go
// Bad — a and b on same cache line, cores fight for it
type Counters struct { a, b int64 }

// Good — separate cache lines, no interference
type Counters struct {
    a int64    // 8 bytes
    _ [56]byte // 64 - 8 = 56 bytes padding
    b int64    // 8 bytes
}
```

Only apply cache-line padding when profiling confirms contention on concurrent counters/flags.

## Instruction-Level Parallelism

**Diagnose:** 1- `go tool pprof` (CPU profile) — look for tight arithmetic loops (sum, dot product) where the loop body itself dominates CPU; these are candidates for multi-accumulator optimization 2- `go test -bench` — benchmark single vs multi-accumulator versions; measure whether breaking the dependency chain helps on the target CPU

Modern CPUs execute multiple independent instructions simultaneously. A single accumulator creates a dependency chain — each addition waits for the previous one:

```go
// Bad — sequential dependency, CPU pipeline stalls
var total int64
for _, v := range data { total += v }

// Good — 4 independent accumulators, CPU pipelines all 4 in parallel
var s0, s1, s2, s3 int64
limit := len(data) - len(data)%4
for i := 0; i < limit; i += 4 {
    s0 += data[i]; s1 += data[i+1]; s2 += data[i+2]; s3 += data[i+3]
}
for i := limit; i < len(data); i++ { s0 += data[i] }
total := s0 + s1 + s2 + s3
```

Measure the gain. For floating-point sums, reassociation changes rounding and may change results; preserve the required numerical semantics.

## SIMD (Single Instruction, Multiple Data)

**Diagnose:** 1- `go tool pprof` (CPU profile) — confirm a numeric inner loop consumes >20% of CPU; SIMD only helps CPU-bound numeric work, not allocation or I/O bottlenecks 2- `go test -bench` — measure the loop's baseline ns/op; provides the reference point to validate SIMD gains 3- `go build -gcflags="-d=ssa/prove/debug=2"` reports proven conditions such as eliminated bounds checks, not proof of SIMD vectorization 4- `GOSSAFUNC=MyFunc go build` — generate SSA dump (`ssa.html`) to inspect whether the compiler produces vector instructions for the hot loop 5- `go tool objdump -s MyFunc ./binary` — verify the final assembly contains SIMD instructions (e.g., `VMOVAPD`, `VADDPD` on amd64) rather than scalar equivalents

Go 1.26+ includes an experimental `simd/archsimd` package (requires `GOEXPERIMENT=simd` flag) providing low-level, architecture-specific SIMD intrinsics — amd64 with 128/256/512-bit vectors, and arm64/wasm at 128-bit as of Go 1.27. Go 1.27 also adds a portable `simd` package (vector types like `Int8s`, `Float32s`) that works across architectures and falls back to scalar code without hardware support. Do not assume an arbitrary loop is auto-vectorized; inspect generated instructions for the exact compiler and target.

**Options for explicit SIMD in Go:**

- **Experimental `simd`/`simd/archsimd` (Go 1.26+, speculative)** — Direct SIMD intrinsics via vector types with CPU feature detection. `simd` (Go 1.27+) is portable across architectures; `simd/archsimd` is architecture-specific (amd64, plus arm64/wasm as of Go 1.27). Use with caution: both are experimental, in-progress APIs (`GOEXPERIMENT=simd`) whose package paths and type names are subject to change before stabilization. Not covered by Go 1 compatibility guarantees, and should never be exposed in public APIs. Verify the actual import path and API against the Go toolchain you are using.

  ```go
  // Requires: GOEXPERIMENT=simd go build
  // WARNING: experimental API — package path and types may change
  import "simd/archsimd"

  v := archsimd.LoadInt32x4([]int32{1, 2, 3, 4})
  ```

- **Let the compiler do it** — write simple, idiomatic loops on `[]float64`/`[]int32` slices. Inspect the final assembly; SSA prove diagnostics are not a vectorization report
- **`math/bits`** — operations like `OnesCount`, `LeadingZeros`, `RotateLeft` map directly to hardware instructions (POPCNT, CLZ, ROL)
- **Hand-written assembly** — `.s` files with AVX2/NEON instructions for critical inner loops. Libraries like `klauspost/compress` and `minio/sha256-simd` use this approach
- **Third-party vectorized libraries** — for common operations (hashing, compression, encoding), use libraries that already have optimized SIMD implementations rather than writing your own

### Handling CPU-specific instruction sets

Hand-written assembly unlocks higher performance but couples code to specific CPU features (AVX2, NEON, etc.). Three strategies exist:

**1. Set the target feature level explicitly**

The compiler does not tune code to the build host's exact CPU. Set the deployment baseline explicitly, for example `GOARCH=amd64 GOAMD64=v3 go build` for machines that support all v3 features. Building on a production host does not itself enable AVX2. Keep a portable baseline or runtime dispatch when machines differ.

**2. Runtime CPU feature detection + multiple implementations**

Implement the function multiple times — one for each CPU capability — and dispatch at runtime:

```go
// dispatch.go
var sumImpl func([]int64) int64

func init() {
    if cpu.X86.HasAVX2 {
        sumImpl = sumAVX2
    } else {
        sumImpl = sumGeneric
    }
}

func Sum(data []int64) int64 {
    return sumImpl(data)
}

// sum_generic.go
func sumGeneric(data []int64) int64 {
    var total int64
    for _, v := range data { total += v }
    return total
}

// sum_amd64.s
TEXT ·sumAVX2(SB), NOSPLIT, $0-32
    // AVX2 implementation
    VMOVAPD (SI), Y0
    // ...
```

**Tradeoff:** Single binary works everywhere; trades one function-call dispatch overhead for full CPU feature utilization. Libraries like `encoding/base64` and `sha256` use this pattern.

**3. Compile-time selection with `//go:build` tags**

Use conditional compilation to generate different code at build time for each target:

```go
// sum_fast.go
//go:build amd64.v3 && !nosimd

package mylib

// AVX2 assembly via cgo or inline
func Sum(data []int64) int64 {
    return sumAVX2(data) // or calls to .s file
}

// sum_generic.go
//go:build !amd64.v3 || nosimd

package mylib

func Sum(data []int64) int64 {
    var total int64
    for _, v := range data { total += v }
    return total
}
```

Build different binaries per target:

```bash
GOOS=linux GOARCH=amd64 GOAMD64=v3 go build -o app-avx2 .     # Uses sum_fast.go
GOOS=darwin GOARCH=arm64 go build -o app-neon .    # Uses sum_generic.go
go build -tags=nosimd -o app-safe .                # Fallback everywhere
```

**Tradeoff:** Zero runtime overhead; each binary is fully optimized for its target. Requires shipping multiple binaries and coordinating which binary runs where.

**When SIMD is NOT worth pursuing:**

- Assembly and experimental intrinsic APIs have portability and maintenance costs
- Simple scalar code may already be adequate; verify any compiler vectorization in assembly
- If your bottleneck is allocations or I/O, SIMD won't help

**Recommendation:**

- Start with straightforward code and inspect generated instructions.
- For Go 1.27+, evaluate the portable `simd` package for cross-architecture code, or `simd/archsimd` for architecture-specific tuning (amd64, arm64, wasm) — remembering both are experimental.
- Move to runtime detection (option 2 above) if profiling shows a bottleneck and the code needs to run on heterogeneous hardware.
- Only use compile-time selection (option 3) if you control the deployment environment and can test each per-binary variant.

Invest in hand-written SIMD only when the measured benefit justifies its portability and maintenance costs.

## Tight Loops and the Scheduler

**Diagnose:** 1- `go tool pprof` (goroutine profile) — look for many goroutines stuck in `"runnable"` state (waiting for CPU) while one goroutine monopolizes execution 2- `go tool trace` — visualize goroutine scheduling over time; look for long uninterrupted execution spans on one goroutine while others show scheduling gaps 3- `GODEBUG=schedtrace=1000` — print scheduler state every second; look for unbalanced `runqueue` counts across P's indicating one P is starved 4- `runtime/metrics` (`/sched/latencies:seconds`) — measure how long goroutines wait before getting CPU; high p99 latencies confirm starvation 5- Prometheus `rate(process_cpu_seconds_total[2m])` — monitor if CPU usage hits GOMAXPROCS ceiling; if saturated while other goroutines are starved, a tight loop is monopolizing P's

Go 1.14+ supports asynchronous preemption, including ordinary tight Go loops without calls. `//go:noinline` is not a scheduling primitive and does not guarantee fairness or prevent starvation. A runnable goroutine waiting briefly is normal; correlate scheduler latency with CPU saturation, quotas and traces before diagnosing starvation.

Bound CPU work and check cancellation between chunks when the operation needs a cancellation contract:

```go
for start := 0; start < len(work); start += batchSize {
    if err := ctx.Err(); err != nil { return err }
    processBatch(work[start:min(start+batchSize, len(work))])
}
```

Here `batchSize` must be positive. Choose it by measuring cancellation latency and throughput; asynchronous preemption does not make application work automatically observe context cancellation.

## Reflection and Type Assertions

**Diagnose:** 1- `go tool pprof` (CPU profile) — look for `reflect.Value.*`, `reflect.DeepEqual`, or `fmt.Sprintf` (which uses reflect internally) appearing in hot paths 2- `go test -bench` — compare reflection-based vs typed versions; expect 10-200x difference depending on the reflection operation

- **`reflect` in hot paths** — type inspection and boxing may add overhead. Measure a typed alternative on the actual workload
- **`reflect.DeepEqual`** — typed comparisons may be faster but can differ on nil versus empty collections and other equality semantics. Verify the contract before replacing it
- **Type switch vs repeated assertions** — type switch dispatches in one evaluation:

```go
// Bad — evaluates interface multiple times
if s, ok := v.(string); ok { return s }
if i, ok := v.(int); ok { return strconv.Itoa(i) }

// Good — single dispatch
switch v := v.(type) {
case string: return v
case int:    return strconv.Itoa(v)
}
```

## Monotonic Time

**Diagnose:** 1- `go test -bench` — benchmark `time.Since(start)` vs `time.Now().Sub(start)`; measure any implementation-dependent difference; time.Now need not issue a wall-clock syscall

`time.Since(start)` uses monotonic time when start retains its monotonic reading. `time.Now().Sub(start)` has the same monotonic semantics when both readings contain it; benchmark any speed difference:

```go
var appStart = time.Now() // captures monotonic time + wall-clock on program start

func myFunc() {
    // Compare durations, not wall-clock times
    elapsed := time.Since(appStart)
    if elapsed > threshold { ... }
}
```
