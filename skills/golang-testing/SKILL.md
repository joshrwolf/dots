---
name: golang-testing
description: "Design, write, and debug Go tests. Use for test structure and placement, table-driven cases, fakes and fixtures, go-cmp comparisons, goroutine and time-dependent tests with synctest, HTTP handler tests, fuzzing, integration tests, coverage, and flaky tests."
license: MIT
---

# Go testing

Follow the project's test commands and existing suite boundaries first.

## Defaults

- **Placement.** Extend the source file's test file (`foo.go` → `foo_test.go`).
  Put a new regression or edge case for `foo.go` there, not in
  `foo_regression_test.go`. Use a separate file only for a distinct responsibility
  or a technical constraint: build tags, or external (`package foo_test`) versus
  internal tests. Name it for that responsibility. File size is not a reason.
  Where practical, order tests like the functions they cover.
- **Behavior over representation.** Test observable behavior and API contracts.
  Use white-box tests when an internal invariant is itself the target.
- **Tables when cases share a shape.** Each case has a `name` passed to `t.Run`.
  Use `got`/`want` naming. Do not force unrelated scenarios into one table.
- **Explicit checks.** Use `t.Errorf` for independent failures, and `t.Fatalf`
  when later checks would be invalid. Include the operation, `got` and `want`:
  `t.Errorf("Parse(%q).Name = %q, want %q", in, got.Name, want)`. Use `errors.Is`
  or `errors.AsType` for errors; check error text only when it is the contract.
  Use `cmp.Diff(want, got)` for structured values. Helpers arrange fixtures;
  they do not hide assertions.
- **Fakes at consumed boundaries.** Write small hand-written fakes for the
  interface the code under test consumes. Do not add interfaces only to mock
  internals.
- **Independence.** Tests must not depend on execution order or on unavailable
  external services. Use `t.Parallel()` when a test is independent and safe.
  Tests that use `t.Setenv` or `t.Chdir` cannot run in parallel. Shared fixtures
  must be read-only or provide explicit isolation or synchronization.
- **Lifetimes.** Use `t.Context()`, `t.TempDir()`, `t.Setenv()`, `t.Chdir()` and
  `t.Cleanup()`. `Context` and `Chdir` need Go 1.24+.

## Gotchas

- `t.Context()` is canceled *before* `t.Cleanup` callbacks run, but *after*
  deferred calls. A `defer wg.Wait()` that relies on that cancellation deadlocks.
  Register the wait with `t.Cleanup`, or cancel a derived context explicitly.
- Call `t.Fatal` only from the test goroutine. Send worker errors back to it.
- Tests that start goroutines must stop them and wait for them. Use channels or
  a `WaitGroup`, not sleeps or global goroutine counts. `-race` finds data races,
  not leaks.
- For timers, deadlines and timeouts, use `testing/synctest` (Go 1.25+:
  `synctest.Test`, not the experimental `Run`). Time advances only when every
  goroutine in the bubble is durably blocked, and external I/O does not count.
  `synctest.Wait` does not impose an order. On Go 1.27+, `synctest.Sleep(d)`
  advances the clock and waits, and `httptest.NewTestServer(t, h)` runs a server
  on an in-memory network inside a bubble.
- Use `httptest.NewRequestWithContext(t.Context(), ...)` for handler requests.
- Use `t.ArtifactDir()` (Go 1.26+) for files kept for inspection, and run with
  `-artifacts` to retain them.
- Coverage finds unexecuted code. It does not show that results were checked.

## References

- [go-cmp.md](references/go-cmp.md): nil versus empty, unordered slices,
  unexported fields, tolerances, or custom equality in `cmp.Diff`.
- [mocking.md](references/mocking.md): fake design, recorded inputs, time control.
- [helpers.md](references/helpers.md): fixture helpers, cleanup, test timeouts.
- [http-testing.md](references/http-testing.md): handler tests with `httptest`.
- [integration-testing.md](references/integration-testing.md): build-tagged
  integration suites, containers, SQL fixtures.
- [flaky-tests.md](references/flaky-tests.md): diagnosing caching, ordering,
  shared fixtures, port conflicts and repetition failures.
- [examples.md](references/examples.md): `Example` functions as executable docs.
- [coverage.md](references/coverage.md): coverage modes, `-coverpkg`, reporting.

Fuzz targets (`FuzzX(f *testing.F)`) suit parsers and round-trip properties: seed
them with `f.Add`, and run one target with `go test -fuzz='^FuzzX$' ./pkg`.
Benchmarks live with the tests too. Measurement and comparison belong to
`golang-performance`.
