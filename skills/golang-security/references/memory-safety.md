# Memory Safety Security Rules

Memory safety vulnerabilities can lead to crashes, data corruption, and security compromises.

**Rules:**

1. Integer overflow MUST be checked at boundaries — NEVER trust unchecked arithmetic on external input.
2. `unsafe` MUST NOT be used in application code — restrict to low-level libraries with thorough review.
3. Data races MUST be detected with `-race` flag in CI.

---

## Table of Contents

- [Integer Overflow — High](#integer-overflow--high)
- [math/big.Rat Issues — Low](#mathbigrat-issues--low)
- [Memory Aliasing Vulnerability — Medium](#memory-aliasing-vulnerability--medium)
- [Use of unsafe Package — High](#use-of-unsafe-package--high)
- [Data Races — High](#data-races--high)
- [Always Run Race Detector](#always-run-race-detector)
- [CWE References](#cwe-references)

## Integer Overflow — High

Integer overflows can cause unexpected behavior and crashes.

**Bad:**

```go
func allocateBuffer(rows, cols int) []byte {
    size := rows * cols  // DON'T: Can overflow
    return make([]byte, size)
}
```

**Good:**

```go
import "math"

func safeMultiply(a, b int) (int, error) {
    if a < 0 || b < 0 {
        return 0, errors.New("negative dimension")
    }
    if a == 0 || b == 0 {
        return 0, nil
    }
    if a > math.MaxInt/b {
        return 0, errors.New("integer overflow")
    }
    result := a * b
    if result/b != a {
        return 0, errors.New("overflow detected")
    }
    return result, nil
}

func allocateBuffer(rows, cols int) ([]byte, error) {
    size, err := safeMultiply(rows, cols)
    if err != nil {
        return nil, err
    }
    const maxBufferSize = 100 * 1024 * 1024 // 100MB limit
    if size > maxBufferSize {
        return nil, errors.New("buffer size exceeds limit")
    }
    return make([]byte, size), nil
}
```

---

## math/big.Rat Issues — Low

Rat can consume large amounts of memory if denominators grow without bounds.

**Bad:**

```go
import "math/big"

func unsafeFraction(operations int) *big.Rat {
    r := big.NewRat(1, 1)
    for i := 0; i < operations; i++ {
        r.Add(r, big.NewRat(1, int64(i+2)))  // DON'T
    }
    return r  // Could be memory intensive
}
```

**Good:**

```go
const maxRatNumBits = 1000

func safeFraction(operations int) (*big.Rat, error) {
    // Bound work before allocating; a post-operation bit check alone is too late.
    if operations < 0 || operations > 1000 {
        return nil, errors.New("too many operations")
    }
    r := big.NewRat(1, 1)
    for i := 0; i < operations; i++ {
        r.Add(r, big.NewRat(1, int64(i+2)))
        if r.Num().BitLen() > maxRatNumBits || r.Denom().BitLen() > maxRatNumBits {
            return nil, errors.New("fraction precision too large")
        }
    }
    return r, nil
}
```

---

## Memory Aliasing Vulnerability — Medium

Mutation through a slice affects other slices sharing its backing array. That is expected behavior, not itself a vulnerability. Clone when the API promises an independent result, and synchronize concurrent access.

```go
func reversed(data []byte) []byte {
    out := slices.Clone(data)
    slices.Reverse(out)
    return out
}

// Builtin copy explicitly supports overlapping source and destination.
copy(data[1:], data[:len(data)-1]) // data must be non-empty
```

No unsafe pointer-range test or temporary copy is needed for builtin `copy`.

---

## Use of unsafe Package — High

The unsafe package bypasses Go's type safety and memory safety.

**Bad:**

```go
import "unsafe"

func UnsafeStringToBytes(s string) []byte {
    return (*[0x7fffffff]byte)(unsafe.Pointer(
        (*reflect.StringHeader)(unsafe.Pointer(&s)).Data,
    ))[:len(s):len(s)]  // DON'T: memory corruption risk
}

func TypePun(value uint64) float64 {
    return *(*float64)(unsafe.Pointer(&value))  // DON'T
}
```

**Good:**

```go
// Safe string encoding
func StringToBytes(s string) []byte {
    return []byte(s)
}
func BytesToString(b []byte) string {
    return string(b)
}

// Reinterpret IEEE-754 bits using the standard library.
func Uint64ToFloat64(value uint64) float64 {
    return math.Float64frombits(value)
}
```

---

## Data Races — High

Go's race detector is your primary defense.

**Bad:**

```go
type Counter struct {
    value int
}

func (c *Counter) Increment() {
    c.value++  // DON'T: Data race without sync
}
```

**Good:**

```go
import "sync"

type Counter struct {
    value int
    mu    sync.Mutex
}

func (c *Counter) Increment() {
    c.mu.Lock()
    defer c.mu.Unlock()
    c.value++
}

// Or atomic for simple cases
import "sync/atomic"
type AtomicCounter struct {
    value int64
}
func (c *AtomicCounter) Increment() {
    atomic.AddInt64(&c.value, 1)
}
```

## Always Run Race Detector

```bash
go test -race ./...
go build -race
```

---

## CWE References

- **CWE-190**: Integer Overflow or Wraparound
- **CWE-119**: Improper Restriction of Operations within Bounds
- **CWE-125**: Out-of-bounds Read
- **CWE-787**: Out-of-bounds Write
- **CWE-362**: Race Condition
- **CWE-367**: Time-of-check Time-of-use (TOCTOU)
