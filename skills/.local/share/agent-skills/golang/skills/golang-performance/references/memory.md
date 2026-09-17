# Memory Optimization

Allocation reduction is the single highest-ROI optimization in most Go programs. Every allocation eventually requires garbage collection — reducing allocation count and size directly reduces GC pauses and CPU overhead.

## Table of Contents

- [Allocation Patterns](#allocation-patterns)
  - [Reuse slices via append(s[:0], ...)](#reuse-slices-via-appends0-)
  - [Direct indexing vs append](#direct-indexing-vs-append)
  - [Eliminate redundant map lookups](#eliminate-redundant-map-lookups)
  - [Map size hints](#map-size-hints)
  - [Sentinel errors vs fmt.Errorf](#sentinel-errors-vs-fmterrorf)
  - [Interface boxing](#interface-boxing)
- [Backing Array Leaks](#backing-array-leaks)
  - [Slice reslicing retains the entire backing array](#slice-reslicing-retains-the-entire-backing-array)
  - [Substring memory leaks](#substring-memory-leaks)
  - [Map never shrinks](#map-never-shrinks)
- [String and Byte Optimization](#string-and-byte-optimization)
- [sync.Pool Hot-Path Patterns](#syncpool-hot-path-patterns)
- [Memory Layout](#memory-layout)
  - [Struct field alignment](#struct-field-alignment)
  - [Zero-size field at end of struct](#zero-size-field-at-end-of-struct)
  - [Pointer receivers for large structs](#pointer-receivers-for-large-structs)
  - [Map of pointers for large, frequently updated structs](#map-of-pointers-for-large-frequently-updated-structs)

## Allocation Patterns

**Diagnose:** 1- `go tool pprof -alloc_objects` — rank functions by number of heap allocations; expect hot-path functions (request handlers, serializers) near the top with thousands of alloc/op 2- `go build -gcflags="-m -m"` — verbose escape analysis showing _why_ variables escape; look for `"leaking param"`, `"too large for stack"`, or `"captured by closure"` on variables you expect to stay on the stack 3- `go test -bench -benchmem` — measure allocs/op and B/op per benchmark; expect the target function to show >0 allocs/op that can be eliminated

### Reuse slices via append(s[:0], ...)

Reslicing to zero length retains the backing array, turning what would be a new allocation into a no-op:

```go
// Bad — allocates new slice, old one becomes garbage
mode = []T{item}

// Reuses capacity when it suffices; earlier users must not retain this buffer
mode = append(mode[:0], item)
```

### Direct indexing vs append

When the output size equals the input size, use `make([]T, len(input))` with direct assignment instead of `make([]T, 0, len(input))` with `append`. Direct assignment avoids append length updates; either form may have bounds checks eliminated. Measure the compiled hot path:

```go
// Slower — append overhead per element
result := make([]T, 0, len(input))
for i := range input { result = append(result, transform(input[i])) }

// Faster — direct assignment
result := make([]T, len(input))
for i := range input { result[i] = transform(input[i]) }
```

Use append when the result might be smaller (filtering) or when early error return could discard partial results.

### Eliminate redundant map lookups

`for k := range m { use(m[k]) }` iterates a key and then performs an unnecessary keyed lookup. Capture the value from range:

```go
// Extra keyed lookup after iteration
for k := range in { result[k] = fn(in[k]) }

// Use the value already yielded by iteration
for k, v := range in { result[k] = fn(v) }
```

### Map size hints

`make(map[K]V)` starts with a small number of buckets and rehashes as it grows. Providing a size hint can reduce growth work:

```go
m := make(map[string]int, len(items)) // capacity hint; allocation count and growth remain implementation-dependent
```

### Sentinel errors vs fmt.Errorf

`fmt.Errorf` allocates on every call. For predictable errors in hot paths, use preallocated sentinels:

```go
var ErrNegative = errors.New("value is negative") // allocated once

func validate(x int) error {
    if x < 0 { return ErrNegative } // zero allocation
    return nil
}
```

Only use `fmt.Errorf` when you need dynamic context (field names, values).

### Interface boxing

Passing concrete values through interfaces can allocate when the boxed value escapes; it does not inherently force a heap allocation. In hot paths, use typed parameters or generics:

```go
// Caller-side boxing may allocate depending on escape behavior
func sum(values []any) int { ... }

// Good — no boxing, no allocation
func sum(values []int) int { ... }

// Good — generic, still no boxing
func sum[T ~int | ~int64](values []T) T { ... }
```

## Backing Array Leaks

**Diagnose:** 1- `go tool pprof -inuse_space` — show currently live heap memory by allocation site; look for unexpectedly large live objects (MB-sized) that should have been GC'd — a sign of backing array retention 2- `go tool pprof -alloc_space` — show cumulative bytes allocated over time; look for allocation sites producing far more bytes than the final data they hold (e.g., 100MB allocated for 16-byte results)

### Slice reslicing retains the entire backing array

A small reslice of a large slice keeps the entire original array in memory:

```go
// Bad — retains entire megabyte-sized backing array
func getHeader(data []byte) []byte { return data[:16] }

// Good — independent copy, original can be GC'd
func getHeader(data []byte) []byte {
    header := make([]byte, 16)
    copy(header, data[:16])
    return header
}
```

### Substring memory leaks

Substrings share the backing array of the original string:

```go
// Bad — keeps entire longMsg in memory
func extractID(msg string) string { return msg[:8] }

// Good — independent copy (Go 1.18+)
func extractID(msg string) string { return strings.Clone(msg[:8]) }
```

### Map never shrinks

Go maps grow but never release bucket memory when entries are deleted. A map that once held millions of entries retains its allocation forever:

```go
// Recreate periodically to reclaim memory
func compact(old map[string]Data) map[string]Data {
    m := make(map[string]Data, len(old))
    for k, v := range old { m[k] = v }
    return m // old map becomes eligible for GC
}
```

## String and Byte Optimization

**Diagnose:** 1- `go tool pprof -alloc_objects` — look for string/byte conversion functions (`runtime.stringtoslicebyte`, `runtime.slicebytetostring`) appearing as top allocators 2- `go test -bench -benchmem` — measure allocs/op; expect repeated conversions to show 1+ alloc/op per conversion that can be reduced to zero by caching

**Cache string-to-byte conversions** — converting between `string` and `[]byte` allocates a copy each time. Convert once and reuse the result.

**Use `bytes` package directly** — `bytes.Contains`, `bytes.HasPrefix`, `bytes.Split`, `bytes.ToUpper` etc. operate on `[]byte` without string conversion. The `bytes` package mirrors most of `strings`.

## sync.Pool Hot-Path Patterns

**Diagnose:** 1- `go tool pprof -alloc_objects` — identify hot allocation sites creating the same object type repeatedly (e.g., `[]byte` buffers, temp structs); expect one site with thousands of allocs/s that can be pooled

`sync.Pool` recycles objects across GC cycles, reducing allocation pressure. Use it for frequently allocated, short-lived objects in hot paths (HTTP handlers, serialization, logging):

```go
var bufPool = sync.Pool{
    New: func() any {
        buf := make([]byte, 0, 4096)
        return &buf
    },
}

func handleRequest(data []byte) []byte {
    bp := bufPool.Get().(*[]byte)
    buf := (*bp)[:0] // reset length, keep capacity
    defer func() { *bp = buf; bufPool.Put(bp) }()

    // ... process data into buf ...

    result := make([]byte, len(buf))
    copy(result, buf) // return a copy — buf goes back to pool
    return result
}
```

**Rules:**

- Reset state before `Put()` — clear references to avoid retaining large object graphs across GC cycles
- Return copies, not pooled buffers — callers must not hold references to pooled memory
- Pooling has no 32 KiB size limit. Bound retained buffer sizes according to the workload; large idle objects can increase memory usage, and the GC may discard pooled objects at any time.
- Don't pool infrequently used objects — pool overhead exceeds benefit when allocations are rare

→ See `samber/cc-skills-golang@golang-concurrency` skill for `sync.Pool` API reference and basic usage patterns.

## Memory Layout

**Diagnose:** 1- `fieldalignment ./...` — detect structs with wasted padding bytes; expect warnings like `"struct of size 40 could be 24"` listing which structs benefit from reordering 2- `unsafe.Sizeof`/`Alignof`/`Offsetof` — measure exact byte sizes and field offsets; use to confirm savings before/after and document them in code comments

### Struct field alignment

Go adds padding between fields to satisfy alignment requirements. Reorder fields from largest to smallest:

```go
// Bad — 24 bytes (7 + 3 bytes padding)
type Bad struct {
    a bool    // 1 byte + 7 padding
    b int64   // 8 bytes
    c bool    // 1 byte + 3 padding
    d int32   // 4 bytes
}

// Good — 16 bytes (2 bytes padding)
type Good struct {
    b int64   // 8 bytes
    d int32   // 4 bytes
    a bool    // 1 byte
    c bool    // 1 byte + 2 padding
}
```

**Typical 64-bit alignment requirements (verify the target with unsafe.Alignof):** `bool`/`byte` = 1, `int16` = 2, `int32`/`float32` = 4, `int64`/`float64`/`string`/`[]T`/`*T` = 8.

**Inspect layout:** `unsafe.Sizeof(T{})`, `unsafe.Alignof(T{})`, `unsafe.Offsetof(T{}.field)`

### Zero-size field at end of struct

If the last field has zero size (`struct{}`), the compiler adds one byte for the trailing field, then rounds the struct size to its alignment to prevent a pointer to that field from overlapping the next memory block:

```go
// Bad — 16 bytes (8 for Value + 8 padding for Flag)
type Entry struct { Value int64; Flag struct{} }

// Good — 8 bytes (0 for Flag + 8 for Value)
type Entry struct { Flag struct{}; Value int64 }
```

Having a `struct{}` field in a struct is rare and almost useless.

### Pointer receivers for large structs

Choose receivers for mutation, identity, method sets and copy safety first. Value receivers have copy semantics, though the compiler may eliminate the physical copy; pointer receivers can add aliasing, escape and GC costs. For large values, measure those costs rather than applying a byte threshold. Keep receiver conventions consistent where semantics permit.

### Map of pointers for large, frequently updated structs

Map values are not addressable — you cannot modify a field in place. For large structs with frequent updates, `map[K]*V` avoids the copy-modify-reassign pattern:

```go
players := map[string]*Player{"alice": {Score: 100}}
players["alice"].Score += 10 // direct modification, no copy
```

Trade-off: separately allocated pointees add allocations and GC scanning; pointers can also refer to elements of a shared allocation. For small, mostly-read structs, `map[K]V` (value) is better.
