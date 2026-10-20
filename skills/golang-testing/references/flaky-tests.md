# Flaky tests and test isolation

## Useful Test Flags

```bash
go test -v ./...                          # verbose output
go test -run TestName -v ./pkg/...        # single test
go test -count=1 ./...                    # disable cache
go test -timeout 10s ./...                # short timeout (find hangs)
go test -parallel 1 ./...                 # limit parallel tests within each test binary
go test -race ./...                       # race detector
go test -cover ./...                      # coverage summary
go test -coverprofile=c.out ./... && go tool cover -html=c.out  # coverage report
go test -failfast ./...                   # stop on first failure
go test -shuffle=on ./...                # randomize test order (Go 1.17+)
```

## Debugging Flaky Tests

Flaky tests (pass sometimes, fail sometimes) are usually caused by one of:

1. **Shared mutable state between tests** — global variables, package-level maps, singletons
   - Fix: give each test isolated state, or restore shared state with `t.Cleanup` when the tests cannot overlap
2. **Test order dependence** — one test sets up state another test relies on
   - Diagnose: `go test -run TestSuspect -count=1` (run in isolation)
   - Diagnose: `go test -shuffle=on` (randomize order)
   - Fix: each test must set up its own preconditions
3. **Timing sensitivity** — `time.Sleep` in tests, race between goroutines
   - Fix: use channels/waitgroups to synchronize, not sleeps
4. **Port conflicts** — tests binding to fixed ports
   - Fix: use port `0` and read the assigned port
5. **File system pollution** — tests writing to shared temp directories
   - Fix: use `t.TempDir()` for per-test directories

```bash
# Confirm flakiness by running many times
go test -count=100 -run TestSuspect ./pkg/... -failfast

# Check for within-package parallel-test interaction
go test -parallel 1 -count=10 ./pkg/...

# Check for order dependence
go test -shuffle=on ./pkg/...
```

`-parallel` does not serialize separate package test binaries or goroutines started
by the code under test. Use `-p=1` when checking cross-package resource contention.
Anchor `-run` expressions when an exact test name is intended.
