---
name: golang-testing
description: "Write, review, and debug Go tests using testing and focused fakes. Use for test cases, failure diagnostics, fixtures, integration tests, fuzzing, and flaky tests."
user-invocable: true
license: MIT
compatibility: Designed for Claude Code, Codex or similar harness, and for projects using Golang.
metadata:
  author: samber
  version: "1.4.0"
  openclaw:
    emoji: "🧪"
    homepage: https://github.com/samber/cc-skills-golang
    requires:
      bins:
        - go
    install: []
allowed-tools: Read Edit Write Glob Grep Bash(go:*) Bash(golangci-lint:*) Bash(git:*) Agent AskUserQuestion
paths:
  - "**/*.go"
---

> **Community default.** A company skill that explicitly supersedes `samber/cc-skills-golang@golang-testing` skill takes precedence.

# Go Testing Best Practices

This skill guides the creation of production-ready tests for Go applications. Follow these principles to write maintainable, fast, and reliable tests.

## Best Practices Summary

1. Table-driven tests MUST use named subtests -- every test case needs a `name` field passed to `t.Run`
2. Separate tests requiring external services according to the project's test commands; build tags provide opt-in compilation, while `testing.Short()` provides runtime selection
3. Tests MUST NOT depend on execution order -- each test MUST be independently runnable
4. Independent tests SHOULD use `t.Parallel()` when possible
5. Prefer observable behavior and API contracts; use focused white-box tests when an internal invariant is itself the target or the user requests it. Avoid coupling tests to incidental representation
6. Tests that start goroutines should cancel their work and verify completion before returning
7. Use explicit standard-library checks; use go-cmp for structured failure diffs
8. Implement focused fakes at consumed interface boundaries
9. Keep ordinary test runs independent of unavailable external services
10. Run tests with race detection in CI
11. Include examples as executable documentation
12. Test files MUST be named after the source file under test, not after the function or method being tested
13. Test functions SHOULD appear in the same order as the functions/methods they test in the source file

## Test Structure and Organization

### File Conventions

```go
// package_test.go - tests in same package (white-box, access unexported)
package mypackage

// mypackage_test.go - tests in test package (black-box, public API only)
package mypackage_test
```

Name test files after the source file they cover (`foo.go` -> `foo_test.go`). This is our organizational convention: tests should be easy to locate alongside their implementation. Go itself discovers `_test.go` files at package scope and does not require a source-file match.

```
// ✓ Good — one test file per source file
helloworld.go       -> helloworld_test.go   // contains TestHelloWorld, TestAbcd, TestXyz, ...

// ✗ Bad — test file named after the function/method instead of the source file
helloworld.go       -> abcd_test.go         // wrong: should be helloworld_test.go
```

Exception: very large source files MAY be split into multiple `_test.go` files by concern (e.g. `foo_test.go` + `foo_edgecases_test.go`), but each split file's name MUST still be derived from the source file name, never from an individual function name. Prefer keeping a single `_test.go` file per source file even when it grows large — splitting adds navigation overhead and is rarely worth it; reach for the exception only when a single file becomes genuinely unwieldy to browse or review.

Within a test file, order test functions to match the order their tested functions/methods appear in the source file. A reader (human or agent) scrolling `foo.go` alongside `foo_test.go` can then find the matching test by position instead of searching; drift between the two orderings compounds every time either file grows.

### Naming Conventions

```go
func TestAdd(t *testing.T) { ... }               // function test
func TestMyStruct_MyMethod(t *testing.T) { ... } // method test
func BenchmarkAdd(b *testing.B) { ... }          // benchmark
func ExampleAdd() { ... }                        // example
func FuzzAdd(f *testing.F) { ... }               // fuzz test
```

## Table-Driven Tests

Table-driven tests are the idiomatic Go way to test multiple scenarios. Always name each test case.

```go
func TestCalculatePrice(t *testing.T) {
    tests := []struct {
        name     string
        quantity int
        unitPrice float64
        expected  float64
    }{
        {
            name:      "single item",
            quantity:  1,
            unitPrice: 10.0,
            expected:  10.0,
        },
        {
            name:      "bulk discount - 100 items",
            quantity:  100,
            unitPrice: 10.0,
            expected:  900.0, // 10% discount
        },
        {
            name:      "zero quantity",
            quantity:  0,
            unitPrice: 10.0,
            expected:  0.0,
        },
    }

    for _, tt := range tests {
        t.Run(tt.name, func(t *testing.T) {
            got := CalculatePrice(tt.quantity, tt.unitPrice)
            if got != tt.expected {
                t.Errorf("CalculatePrice(%d, %.2f) = %.2f, want %.2f",
                    tt.quantity, tt.unitPrice, got, tt.expected)
            }
        })
    }
}
```

## Checks and failure diagnostics

Use explicit `if` checks with `t.Errorf` for independent failures. Use `t.Fatalf`
when a failed prerequisite makes further checks invalid, such as a constructor
error before dereferencing its result. Include the operation, actual value, and
expected value. Use the current subtest's `t`, and return worker-goroutine errors
to the test goroutine before calling `Fatalf`.

```go
got, err := Parse(input)
if err != nil {
	t.Fatalf("Parse(%q): %v", input, err)
}
if got.Name != wantName {
	t.Errorf("Parse(%q).Name = %q, want %q", input, got.Name, wantName)
}
```

Use `errors.Is` for sentinel errors and `errors.As` for typed errors; check error
text only when that text is the contract. Use direct equality for scalars,
`bytes.Equal` for bytes, and `slices.Equal` or `maps.Equal` when their equality
semantics fit. For structured values needing readable diagnostics:

```go
if diff := cmp.Diff(want, got); diff != "" {
	t.Errorf("Parse() mismatch (-want +got):\n%s", diff)
}
```

Import `github.com/google/go-cmp/cmp`. Read `golang-go-cmp` when comparison
options or unexported fields require special handling. Keep checks visible at
the call site; helpers should arrange domain fixtures, not implement an assertion
framework.

## Unit Tests

Unit tests should be fast, isolated from unavailable external dependencies, and repeatable; appropriate duration depends on the behavior under test.

## Testing HTTP Handlers

Use `httptest` for handler tests with table-driven patterns. See [HTTP Testing](./references/http-testing.md) for examples with request/response bodies, query parameters, headers, and status code assertions.

## Goroutine lifetime tests

Give owned goroutines a cancellation path and an observable completion signal.
Exercise success, error, and cancellation paths, then wait for completion before
the test ends. Use channels or a WaitGroup rather than sleeps. A global goroutine
count is noisy and cannot prove that a particular worker stopped. The race
detector checks data races; it is not a leak detector.

## testing/synctest for Deterministic Goroutine Testing

`testing/synctest` (Go 1.25+) provides fake time and synchronization for tests of goroutines, timers, deadlines, and context cancellation. Time advances when every goroutine in the bubble is durably blocked. Goroutine scheduling remains nondeterministic; use synchronization to establish the ordering a test requires.

When to use `synctest` instead of real time:

- Testing concurrent code with time-based operations (time.Sleep, time.After, time.Ticker)
- When explicit synchronization can replace timing guesses; keep `-race` enabled to detect exercised data races
- When tests are flaky due to timing issues

```go
import (
    "context"
    "testing"
    "testing/synctest"
    "time"
)

func TestContextTimeout(t *testing.T) {
    synctest.Test(t, func(t *testing.T) {
        const timeout = 5 * time.Second

        ctx, cancel := context.WithTimeout(t.Context(), timeout)
        defer cancel()

        time.Sleep(timeout - time.Nanosecond)
        synctest.Wait()
        if err := ctx.Err(); err != nil {
            t.Fatalf("before timeout: %v", err)
        }

        time.Sleep(time.Nanosecond)
        synctest.Wait()
        if err := ctx.Err(); err != context.DeadlineExceeded {
            t.Fatalf("after timeout: got %v, want DeadlineExceeded", err)
        }
    })
}
```

Use `synctest.Test` in Go 1.25+ and later. Do not use the old Go 1.24 experimental `synctest.Run` API in Go 1.25+ code. If a module explicitly targets Go 1.24 and opts into `GOEXPERIMENT=synctest`, use the old API only as a compatibility fallback.

Key differences in `synctest`:

- `time.Sleep` blocks its goroutine until synthetic time reaches the deadline
- `time.After` fires when synthetic time reaches the duration
- Time advances only when all bubble goroutines are durably blocked; external I/O does not qualify
- `synctest.Wait()` waits for other bubble goroutines to become durably blocked; it does not impose a deterministic schedule
- Go 1.27+ adds `synctest.Sleep(d)` as a direct helper to advance the bubble's fake clock, equivalent to `time.Sleep(d)` followed by `synctest.Wait()`

Go 1.27+ also adds `httptest.NewTestServer(t, handler)`, an in-memory fake-network variant of `httptest.NewServer` that composes with `synctest` — use its `Client()` for requests. It uses no real socket, so server tests can run inside a `synctest.Test` bubble instead of needing `httptest.NewServer` plus real timers.

## Test Timeouts

Bound potentially blocking operations with cancellation and the test runner timeout. See [Helpers](./references/helpers.md).

## Benchmarks

Write benchmarks as sub-benchmarks (`b.Run` per variant) so each variant gets its own name in the output — that name is what comparison tooling diffs. For Go 1.24+, use `b.Loop()` rather than a `b.N` loop.

→ See [Benchmarks in a Test Suite](./references/benchmarks.md) for the code shape and size-parameterized examples.

→ See `samber/cc-skills-golang@golang-benchmark` skill for measurement methodology: `benchstat`, profiling from benchmarks, and CI regression detection.

## Go 1.26+: test artifacts

When a test, benchmark, or fuzz target needs to persist files for inspection, use `ArtifactDir()` instead of ad-hoc paths or repo-local output.

```go
func TestRenderGoldenArtifact(t *testing.T) {
    dir := t.ArtifactDir()

    out := filepath.Join(dir, "rendered.json")
    if err := os.WriteFile(out, renderedBytes, 0o644); err != nil {
        t.Fatal(err)
    }

    t.Logf("artifact written: %s", out)
}
```

Available on `*testing.T`, `*testing.B`, and `*testing.F` in Go 1.26+. Run with `go test -artifacts ./artifacts ./...` to retain files; without `-artifacts`, artifact directories are temporary and removed when testing completes.

### Go 1.27+: `stdversion` runs automatically

`go test` now invokes the `stdversion` vet check by default, flagging any use of an API newer than the module's `go` directive. A CI failure from this check means either the `go` directive needs bumping or the code needs to stop using the newer API — it is not a check to silence.

## Parallel Tests

Use `t.Parallel()` to run tests concurrently:

```go
func TestParallelOperations(t *testing.T) {
	tests := []struct {
		name string
		data []byte
	}{
		{"small data", make([]byte, 1024)},
		{"medium data", make([]byte, 1024*1024)},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			t.Parallel()

			result := Process(tt.data)
			if result == nil {
				t.Fatal("Process returned nil")
			}
		})
	}
}
```

## Fuzzing

Use fuzzing to find edge cases and bugs:

```go
func FuzzReverse(f *testing.F) {
    f.Add("hello")
    f.Add("")
    f.Add("a")

    f.Fuzz(func(t *testing.T, input string) {
        reversed := Reverse(input)
        doubleReversed := Reverse(reversed)
        if input != doubleReversed {
            t.Errorf("Reverse(Reverse(%q)) = %q, want %q", input, doubleReversed, input)
        }
    })
}
```

## Examples as Documentation

`ExampleXxx` functions are executable documentation: `go test` compares their stdout to the `// Output:` comment, so a drifting example fails the build instead of misleading readers.

→ See [Examples as Documentation](./references/examples.md) for naming rules, `Unordered output`, and placement.

## Code Coverage

Generate a profile with `go test -coverprofile=coverage.out ./...`, then read the uncovered lines with `go tool cover -html=coverage.out`. Coverage locates untested paths; it does not measure assertion quality, so treat a percentage as a gap finder rather than a target.

→ See [Code Coverage](./references/coverage.md) for coverage modes, `-coverpkg`, and reporting pitfalls.

## Integration Tests

For projects using opt-in integration build tags:

```go
//go:build integration

package mypackage

func TestDatabaseIntegration(t *testing.T) {
    db, err := sql.Open("postgres", os.Getenv("DATABASE_URL"))
    if err != nil {
        t.Fatal(err)
    }
    defer db.Close()

    // Test real database operations
}
```

Run the tagged tests with:

```bash
go test -tags=integration ./...
```

For Docker Compose fixtures, SQL schemas, and integration tests with cleanup helpers, see [Integration Testing](./references/integration-testing.md).

## Mocking

Define narrow interfaces where consumed, then implement focused fakes with explicit behavior and recorded inputs. Avoid generating a framework of expectations for simple dependencies.

For mock patterns, test fixtures, and time mocking, see [Mocking](./references/mocking.md).

## Enforce with Linters

Many test best practices are enforced automatically by linters: `thelper`, `paralleltest`. See the `samber/cc-skills-golang@golang-lint` skill for configuration and usage.

## Cross-References

- See `golang-go-cmp` for structural diffs and comparison semantics
- → See `samber/cc-skills-golang@golang-database` skill (testing.md) for database integration test patterns
- → See `samber/cc-skills-golang@golang-concurrency` skill for goroutine ownership and shutdown
- → See `samber/cc-skills-golang@golang-continuous-integration` skill for CI test configuration and GitHub Actions workflows
- → See `samber/cc-skills-golang@golang-lint` skill for test helper and paralleltest configuration
- → See `samber/cc-skills-golang@golang-continuous-integration` skill for automated AI-driven code review in CI using these guidelines

## Quick Reference

```bash
go test ./...                          # all tests
go test -run '^TestName$' ./...        # specific top-level test by exact name
go test -run TestName/subtest ./...    # subtests within a test
go test -run 'Test(Add|Sub)' ./...     # multiple tests (regexp OR)
go test -run 'Test[A-Z]' ./...         # tests starting with capital letter
go test -run 'TestUser.*' ./...        # tests matching prefix
go test -run '.*Validation.*' ./...    # tests containing substring
go test -run TestName/. ./...          # all subtests of TestName
go test -run '/(unit|integration)' ./... # filter by subtest name
go test -race ./...                    # race detection
go test -cover ./...                   # coverage summary
go test -bench=. -benchmem ./...       # benchmarks
go test -fuzz='^FuzzName$' ./pkgname   # fuzz one target in one package
go test -tags=integration ./...        # integration tests
```

For flaky failures, read [Flaky tests and test isolation](references/flaky-tests.md)
for cache, ordering, repetition, shared fixture, and port-conflict diagnostics.
