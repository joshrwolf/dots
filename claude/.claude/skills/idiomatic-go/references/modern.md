# Modern Go (1.24 -- 1.26)

This is the authoritative reference for features introduced in Go 1.24, 1.25, and 1.26.
Always prefer these modern patterns over their predecessors. Do not suggest old patterns.

---

## Language Changes

### `new(expr)` -- Pointer to Value (Go 1.26)

Takes an expression and returns a pointer to it. Eliminates all `ptr()` / `toPtr()` helpers.

```go
// Modern
p := Person{Name: name, Age: new(yearsSince(born))}
opt := Options{Timeout: new(5 * time.Second)}

// Obsolete -- never write this anymore
age := yearsSince(born)
p := Person{Name: name, Age: &age}
```

Delete any `func ptr[T any](v T) *T { return &v }` helpers from codebases -- they are dead code now.

### Generic Type Aliases (Go 1.24)

Type aliases can now have type parameters:
```go
type Set[T comparable] = map[T]struct{}
type Result[T any] = expected.Result[T]  // Re-export generic types
```

### Self-Referential Type Constraints (Go 1.26)

Generic types can refer to themselves in constraints (F-bounded polymorphism):
```go
type Adder[A Adder[A]] interface {
    Add(A) A
}
```

---

## Error Handling

### `errors.AsType[T]` (Go 1.26)

Generic, type-safe replacement for `errors.As`. Always prefer this:
```go
// Modern
if pathErr, ok := errors.AsType[*os.PathError](err); ok {
    fmt.Println(pathErr.Path)
}

// Obsolete
var pathErr *os.PathError
if errors.As(err, &pathErr) { ... }
```

---

## Concurrency

### `sync.WaitGroup.Go` (Go 1.25)

Replaces the `Add`/`Done` boilerplate entirely. Always use this:
```go
// Modern
var wg sync.WaitGroup
wg.Go(func() {
    doWork()
})
wg.Wait()

// Obsolete -- never write this anymore
var wg sync.WaitGroup
wg.Add(1)
go func() {
    defer wg.Done()
    doWork()
}()
wg.Wait()
```

### Container-Aware GOMAXPROCS (Go 1.25)

The runtime now reads cgroup CPU limits and sets `GOMAXPROCS` automatically.
Remove `go.uber.org/automaxprocs` imports -- they are unnecessary.

---

## Testing

### `testing/synctest` -- Virtualized Time (Go 1.25, stable)

Deterministic testing of time-dependent concurrent code. No more `time.Sleep` in tests:
```go
func TestTimeout(t *testing.T) {
    synctest.Test(t, func(t *testing.T) {
        ctx, cancel := context.WithTimeout(t.Context(), 5*time.Second)
        defer cancel()
        select {
        case <-ctx.Done():
            // Fires instantly -- fake clock advances when all goroutines block
        }
    })
}
```

### `testing.B.Loop()` (Go 1.24)

Replaces `for range b.N`. Setup/cleanup run once, compiler cannot optimize away results:
```go
// Modern
func BenchmarkFoo(b *testing.B) {
    data := setup()
    for b.Loop() {
        process(data)
    }
}

// Obsolete
func BenchmarkFoo(b *testing.B) {
    data := setup()  // BUG: runs b.N times
    b.ResetTimer()
    for range b.N {
        process(data)
    }
}
```

As of Go 1.26, `b.Loop()` no longer prevents inlining -- there is zero reason to use `b.N`.

### `testing.T.Context()` (Go 1.24)

Returns a context cancelled when the test ends:
```go
func TestWithContext(t *testing.T) {
    ctx := t.Context()  // Cancelled when test completes, before cleanup
    result, err := DoWork(ctx)
    // ...
}
```

### `testing.T.Chdir()` (Go 1.24)

Temporarily changes working directory, restored when test ends:
```go
func TestInDir(t *testing.T) {
    t.Chdir(t.TempDir())
    // Working directory is now the temp dir
}
```

### `testing.T.ArtifactDir()` (Go 1.26)

Standard directory for test output artifacts (screenshots, golden files, debug output):
```go
func TestRender(t *testing.T) {
    dir := t.ArtifactDir()
    os.WriteFile(filepath.Join(dir, "output.png"), data, 0644)
}
```

Persist with `go test -artifacts ./testdata/artifacts`.

### Deterministic Crypto in Tests (Go 1.26)

```go
import "testing/cryptotest"

func TestMain(m *testing.M) {
    cryptotest.SetGlobalRandom()  // Deterministic crypto randomness
    os.Exit(m.Run())
}
```

---

## Standard Library Additions

### `encoding/json`: `omitzero` Tag (Go 1.24)

Omits fields at their zero value. Uses `IsZero()` if available (fixes `time.Time`):
```go
type Event struct {
    Name    string    `json:"name"`
    StartAt time.Time `json:"start_at,omitzero"`
    Tags    []string  `json:"tags,omitzero"`
}
```

Prefer `omitzero` over `omitempty` for struct types, `time.Time`, and any type with `IsZero()`.

### `encoding/json/v2` (Go 1.25, experimental)

Enable with `GOEXPERIMENT=jsonv2`. Drop-in replacement with substantially faster decoding.
Also adds `encoding/json/jsontext` for token-level processing.

### Iterator-Based String/Bytes Processing (Go 1.24)

Avoids allocating `[]string` when you only need to iterate:
```go
for line := range strings.Lines(text) {
    process(line)
}

for field := range strings.FieldsSeq(text) {
    process(field)
}

for part := range strings.SplitSeq(text, ",") {
    process(part)
}
```

Always prefer `Lines`/`SplitSeq`/`FieldsSeq` over `Split`/`Fields` when iterating.
The `bytes` package has equivalent `Seq` functions.

### `os.Root` -- Sandboxed Filesystem (Go 1.24, expanded in 1.25)

Directory-scoped operations that prevent path traversal:
```go
root, err := os.OpenRoot("/srv/data")
if err != nil { ... }
defer root.Close()

f, err := root.Open("subdir/file.txt")     // Cannot escape /srv/data
data, err := root.ReadFile("config.json")   // Full API as of 1.25
err = root.MkdirAll("a/b/c", 0755)
```

Use for any server that accesses files from user-controlled paths.

### `log/slog` Additions

```go
// GroupAttrs from a pre-collected slice (Go 1.25)
slog.GroupAttrs("request", attrs)

// MultiHandler fan-out (Go 1.26)
h := slog.NewMultiHandler(jsonHandler, textHandler)
```

### `reflect` Iterators (Go 1.26)

Range over struct fields and methods:
```go
for sf, v := range reflect.ValueOf(s).Fields() {
    fmt.Println(sf.Name, v)
}
for m := range reflect.TypeOf(s).Methods() {
    fmt.Println(m.Name)
}
```

### `reflect.TypeAssert[T]` (Go 1.25)

Zero-allocation type assertion from `reflect.Value`:
```go
result, ok := reflect.TypeAssert[MyStruct](v)  // No interface boxing
```

### `net/http.CrossOriginProtection` (Go 1.25)

Built-in CSRF protection via Fetch metadata headers:
```go
csrf := http.NewCrossOriginProtection()
csrf.AddTrustedOrigin("https://example.com")
srv := http.Server{Handler: csrf.Handler(mux)}
```

### `runtime/trace.FlightRecorder` (Go 1.25)

Always-on tracing with ring buffer, snapshot on demand:
```go
fr := trace.NewFlightRecorder(trace.FlightRecorderConfig{})
fr.Start()
// ... later, on error:
fr.WriteTo(traceFile)  // Dumps recent trace window
```

### `runtime.AddCleanup` (Go 1.24)

Replaces `runtime.SetFinalizer`. Supports multiple cleanups, no cycles, no GC delay:
```go
runtime.AddCleanup(obj, func() {
    // Runs when obj is collected
})
```

Never use `SetFinalizer` in new code.

### `weak` Package (Go 1.24)

Weak references for caches and interning:
```go
p := weak.Make(&myObject)
if obj := p.Value(); obj != nil {
    // Still alive
}
```

### `hash.Cloner` (Go 1.25)

Fork hash state mid-computation:
```go
h := sha256.New()
h.Write(prefix)
h2 := h.(hash.Cloner).Clone()
h.Write(suffixA)
h2.Write(suffixB)
```

### `bytes.Buffer.Peek` (Go 1.26)

Read next n bytes without advancing:
```go
upcoming, err := buf.Peek(4)
```

### Crypto: stdlib promotions (Go 1.24)

These moved from `golang.org/x/crypto` into stdlib -- use the stdlib versions:
- `crypto/hkdf` (HMAC-based KDF, RFC 5869)
- `crypto/pbkdf2` (password KDF, RFC 8018)
- `crypto/sha3` (SHA-3, SHAKE, cSHAKE)
- `crypto/mlkem` (post-quantum key exchange, FIPS 203)

### `crypto/hpke` (Go 1.26)

Hybrid Public Key Encryption (RFC 9180), including post-quantum hybrid KEMs.

### `crypto/rand` Changes (Go 1.24+)

- `rand.Read` never returns an error (crashes on failure instead)
- `rand.Text()` generates random text strings directly
- As of Go 1.26, crypto functions ignore the `random` parameter entirely -- always use secure source

### `encoding.TextAppender` / `BinaryAppender` (Go 1.24)

Zero-allocation serialization interface implemented by `time.Time`, `net.IP`, `netip.Addr`, `url.URL`, `regexp.Regexp`, and many more:
```go
buf = t.AppendText(buf[:0])  // No allocation
```

---

## Tooling

### `tool` Directive in go.mod (Go 1.24)

Replaces `tools.go` hack entirely:
```bash
go get -tool golang.org/x/tools/cmd/stringer
go tool stringer -type=MyType
go get tool       # Upgrade all tools
go install tool   # Install all to $GOBIN
```

### `go fix` Modernizers (Go 1.26)

Automatically modernizes code to use latest idioms:
```bash
go fix ./...
```

Run this regularly. Supports `//go:fix inline` for custom API migrations.

### `go.mod` `ignore` Directive (Go 1.25)

Exclude directories from `./...` and `all` patterns:
```
ignore vendor/legacy
```

---

## Runtime & Performance

### Green Tea GC (Go 1.25 experimental, Go 1.26 default)

10-40% GC overhead reduction. No code changes needed. Opt-out: `GOEXPERIMENT=nogreenteagc`.

### Swiss Tables Maps (Go 1.24)

Built-in `map` uses Swiss Tables internally. Better performance, especially for large maps.

### Goroutine Leak Profiler (Go 1.26, experimental)

Enable with `GOEXPERIMENT=goroutineleakprofile`. Detects goroutines blocked on primitives
that can never be unblocked. Available at `/debug/pprof/goroutineleak`.

### `io.ReadAll` (Go 1.26)

~2x faster, ~half the memory. No API change.

### Cgo Annotations (Go 1.24)

```go
// #cgo noescape cFunctionName    -- avoid heap allocation for args
// #cgo nocallback cFunctionName  -- avoid scheduling overhead
```

Cgo call overhead also reduced ~30% in Go 1.26.

---

## Deprecations & Removals (1.24--1.26)

| What | Replacement |
|------|-------------|
| `tools.go` blank import hack | `tool` directive in go.mod |
| `runtime.SetFinalizer` | `runtime.AddCleanup` |
| `for range b.N` in benchmarks | `b.Loop()` |
| `wg.Add(1); go func() { defer wg.Done()...` | `wg.Go(func() {...})` |
| `go.uber.org/automaxprocs` | Built-in container GOMAXPROCS |
| `errors.As(err, &target)` | `errors.AsType[T](err)` |
| `ptr()` / `toPtr()` helpers | `new(expr)` |
| `crypto/cipher.NewOFB/NewCFB*` | AEAD (GCM) or `NewCTR` |
| `math/rand.Seed()` | No-op; use `math/rand/v2` |
| `runtime.GOROOT()` | `go env GOROOT` |
| `httputil.ReverseProxy.Director` | `Rewrite` |
| `crypto/rsa` PKCS#1 v1.5 encryption | OAEP |
| `golang.org/x/crypto/{hkdf,pbkdf2,sha3}` | `crypto/{hkdf,pbkdf2,sha3}` |
