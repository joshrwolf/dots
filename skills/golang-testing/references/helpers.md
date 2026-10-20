# Test helpers and cleanup

Use helpers for domain setup: constructing a valid fixture, opening a test
resource, or starting an in-process server. Call `t.Helper()` so failures point
to the caller, and register cleanup as soon as a resource is acquired.

```go
func writeConfig(t *testing.T, contents string) string {
	t.Helper()
	path := filepath.Join(t.TempDir(), "config.json")
	if err := os.WriteFile(path, []byte(contents), 0o600); err != nil {
		t.Fatalf("write test config: %v", err)
	}
	return path
}
```

`t.Cleanup` runs after a test and its subtests complete, in reverse registration
order. Use `t.Errorf` for cleanup errors so later cleanup actions still run.
Use `t.Setenv` for environment changes; these tests cannot run in parallel with
other tests because the environment is process-wide.

For blocking operations, pass a cancellable context with an appropriate timeout
and wait for owned goroutines to exit. Set `go test -timeout` as a final bound;
a detached watchdog that panics can terminate unrelated tests. Do not call
`t.Fatal` from a worker goroutine: send its result back to the test goroutine.
