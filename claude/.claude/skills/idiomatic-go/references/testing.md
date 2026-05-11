# Testing

## Philosophy

> *"Test tables encourage you to think about what the function does, not how it does it."*
> — Dave Cheney

> *"If you have to mock something, that's a code smell — your API boundary is wrong."*
> — Mitchell Hashimoto

Tests should exercise code **through its public functions**. Each `Test*` function
targets an exported API. Table-driven subtests cover the relevant codepaths. If code
is hard to test through its public boundary, **refactor the code, not the tests.**

### Designing for testability

A function is hard to test when it:
- Depends on global state or singletons
- Makes direct calls to external systems instead of accepting interfaces
- Does too many things (poor separation of concerns)
- Produces results only observable through unexported side effects

The fix is always refactoring production code:
- Accept interfaces for dependencies (enables simple fakes — no mock framework needed)
- Return values instead of mutating state
- Break large functions into units with clear inputs and outputs

### External test packages

Using `package foo_test` gives true black-box testing — it can only access exported
identifiers, which keeps tests coupled to behavior, not implementation. This is ideal
but not required. Use `package foo` when you need internal access.

When an external test package needs one or two internals, use the `export_test.go` pattern:
```go
// export_test.go (in package foo, NOT foo_test)
package foo

var ParseInternal = parseInternal  // Only compiled during testing
```

If you need many of these, your package boundaries are likely wrong.

---

## Table-Driven Tests

The default testing pattern. Each row is a behavior, not an implementation path:

```go
func TestProcess(t *testing.T) {
    tests := []struct {
        name    string
        input   Input
        want    Output
        wantErr bool
    }{
        {name: "valid", input: validInput, want: expectedOutput},
        {name: "empty input", input: Input{}, wantErr: true},
        {name: "boundary case", input: boundaryInput, want: boundaryOutput},
    }

    for _, tt := range tests {
        t.Run(tt.name, func(t *testing.T) {
            got, err := Process(tt.input)
            if (err != nil) != tt.wantErr {
                t.Fatalf("Process() error = %v, wantErr %v", err, tt.wantErr)
            }
            if diff := cmp.Diff(tt.want, got); diff != "" {
                t.Errorf("Process() mismatch (-want +got):\n%s", diff)
            }
        })
    }
}
```

Adding a row is cheap. When implementation changes, the table shouldn't need to change
(unless behavior changes).

---

## Test Helpers

Use `t.Helper()` so failures point to the caller:
```go
func assertNoError(t *testing.T, err error) {
    t.Helper()
    if err != nil {
        t.Fatalf("unexpected error: %v", err)
    }
}
```

---

## Fakes Over Mocks

Prefer simple fakes (real implementations with test behavior) over mock frameworks.
The stdlib models this: `httptest.NewRecorder`, `strings.NewReader`, `bytes.Buffer`.

```go
type Fetcher interface {
    Fetch(url string) (string, error)
}

// Simple fake — not a mock framework
type fakeFetcher struct {
    response string
    err      error
}

func (f *fakeFetcher) Fetch(url string) (string, error) {
    return f.response, f.err
}

func TestProcessData(t *testing.T) {
    tests := []struct {
        name     string
        fetcher  Fetcher
        wantErr  bool
    }{
        {name: "success", fetcher: &fakeFetcher{response: "data"}},
        {name: "fetch error", fetcher: &fakeFetcher{err: errors.New("fail")}, wantErr: true},
    }

    for _, tt := range tests {
        t.Run(tt.name, func(t *testing.T) {
            _, err := ProcessData(tt.fetcher)
            if (err != nil) != tt.wantErr {
                t.Fatalf("unexpected error: %v", err)
            }
        })
    }
}
```

If you need a mock framework to test something, the API boundary is probably wrong.

---

## Testing with Context

Use `t.Context()` (Go 1.24) — cancelled automatically when the test ends:
```go
func TestOperation(t *testing.T) {
    ctx := t.Context()
    result, err := Operation(ctx)
    if err != nil {
        t.Fatalf("Operation failed: %v", err)
    }
}
```

---

## Concurrent / Time-Dependent Code

Use `testing/synctest` (Go 1.25) for deterministic time-based tests:
```go
func TestTimeout(t *testing.T) {
    synctest.Test(t, func(t *testing.T) {
        ctx, cancel := context.WithTimeout(t.Context(), 5*time.Second)
        defer cancel()
        select {
        case <-ctx.Done():
            // Fires instantly -- fake clock advances when goroutines block
        }
    })
}
```

---

## Benchmarks

Always use `b.Loop()` (Go 1.24). Setup runs once, compiler cannot optimize away results:
```go
func BenchmarkProcess(b *testing.B) {
    data := setup()
    for b.Loop() {
        process(data)
    }
}
```

```bash
go test -bench=. ./...
go test -bench=Process -benchmem
```

---

## Test Artifacts

Use `t.ArtifactDir()` (Go 1.26) for test output files:
```go
func TestRender(t *testing.T) {
    dir := t.ArtifactDir()
    os.WriteFile(filepath.Join(dir, "output.png"), data, 0644)
}
// Persist with: go test -artifacts ./testdata/artifacts
```

---

## Assertions

**NEVER use testify.** Use standard library assertions or go-cmp.

For complex comparisons:
```go
import "github.com/google/go-cmp/cmp"

if diff := cmp.Diff(want, got); diff != "" {
    t.Errorf("mismatch (-want +got):\n%s", diff)
}
```

Ignore fields:
```go
import "github.com/google/go-cmp/cmp/cmpopts"

if diff := cmp.Diff(want, got, cmpopts.IgnoreFields(User{}, "ID", "CreatedAt")); diff != "" {
    t.Errorf("mismatch:\n%s", diff)
}
```

---

## Best Practices

- Test through public APIs — each `Test*` targets an exported function
- Table-driven subtests as the default pattern
- If code is hard to test, refactor the code, not the tests
- Fakes over mocks; if you need a mock framework, the boundary is wrong
- `t.Helper()` in all test helpers
- `t.Context()` instead of manually creating contexts
- `synctest.Test` for time-dependent concurrent tests
- `b.Loop()` for benchmarks, never `b.N`
- Run with `-race` flag
- **NEVER use testify** — use standard library or go-cmp
