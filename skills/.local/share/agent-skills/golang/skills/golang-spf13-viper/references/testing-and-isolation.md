# Viper Test Isolation

## Table of Contents

- [The global state problem](#the-global-state-problem)
- [viper.New() per test (correct approach)](#vipernew-per-test-correct-approach)
- [Injecting viper into your app](#injecting-viper-into-your-app)
- [Reading config files in tests](#reading-config-files-in-tests)
- [t.Setenv interactions](#tsetenv-interactions)
- [viper.Reset() — use with caution](#viperreset--use-with-caution)
- [Snapshot and restore pattern](#snapshot-and-restore-pattern)

## The global state problem

The top-level `viper.*` functions operate on a global `*viper.Viper` instance shared across all tests in the same process. Tests that call `viper.SetConfigFile`, `viper.Set`, or `viper.ReadInConfig` pollute this global state, causing flaky test ordering.

```go
// ✗ Bad — sets global state that affects later tests
func TestPortConfig(t *testing.T) {
	viper.SetDefault("port", 8080)
	viper.Set("port", 9090)
	if got, want := viper.GetInt("port"), 9090; got != want {
		t.Errorf("got %v, want %v", got, want)
	}
	// global viper now has port=9090 for all subsequent tests
}
```

## viper.New() per test (correct approach)

```go
func TestPortConfig(t *testing.T) {
	v := viper.New()
	v.SetDefault("port", 8080)
	v.Set("port", 9090)
	if got, want := v.GetInt("port"), 9090; got != want {
		t.Errorf("got %v, want %v", got, want)
	}
}

func TestDefaultPort(t *testing.T) {
	v := viper.New()
	v.SetDefault("port", 8080)
	if got, want := v.GetInt("port"), 8080; got != want {
		t.Errorf("got %v, want %v", got, want)
	} // clean — not affected by TestPortConfig
}
```

## Injecting viper into your app

For test isolation to work, your application code must accept a `*viper.Viper` instead of calling the global functions directly:

```go
// ✓ Good — accepts a viper instance
type Server struct {
	cfg *viper.Viper
}

func NewServer(v *viper.Viper) *Server {
	return &Server{cfg: v}
}

func (s *Server) Port() int {
	return s.cfg.GetInt("port")
}

// In tests:
func TestServer(t *testing.T) {
	v := viper.New()
	v.Set("port", 9090)
	s := NewServer(v)
	if got, want := s.Port(), 9090; got != want {
		t.Errorf("got %v, want %v", got, want)
	}
}

// In main:
func main() {
	// viper setup...
	s := NewServer(viper.GetViper()) // pass the global instance in production
}
```

## Reading config files in tests

```go
func TestReadConfig(t *testing.T) {
	v := viper.New()
	v.SetConfigFile("testdata/config.yaml")
	if err := v.ReadInConfig(); err != nil {
		t.Fatalf("v.ReadInConfig: %v", err)
	}
	if got, want := v.GetString("host"), "localhost"; got != want {
		t.Errorf("got %v, want %v", got, want)
	}
}
```

Use `testdata/` for config files. Go test tooling sets the working directory to the package directory, so relative paths work reliably.

## t.Setenv interactions

`t.Setenv` sets an env var for the duration of a test and restores it on cleanup. Combined with `viper.New()` + `AutomaticEnv`, this lets you test env var binding without global pollution:

```go
func TestEnvBinding(t *testing.T) {
	t.Setenv("MYAPP_PORT", "9090")

	v := viper.New()
	v.SetEnvPrefix("MYAPP")
	v.AutomaticEnv()

	if got, want := v.GetInt("port"), 9090; got != want {
		t.Errorf("got %v, want %v", got, want)
	}
	// t.Setenv restores original MYAPP_PORT (or unsets it) after this test
}
```

## viper.Reset() — use with caution

`viper.Reset()` resets the global viper instance to its zero state. It is rarely the right solution:

- It affects all code running concurrently that also uses the global viper.
- It does not stop any active `WatchConfig` goroutines.
- Using it in `TestMain` or `t.Cleanup` makes tests order-dependent.

Prefer `viper.New()` per test. Reserve `Reset()` for tools that call into viper-based libraries and must restore state between runs.

## Snapshot and restore pattern

The following is a lossy value copy, not restoration of a Viper instance. Do not use it to isolate tests that depend on bindings or precedence:

```go
func snapshotViper() map[string]interface{} {
    return viper.AllSettings()
}

func restoreViper(snapshot map[string]interface{}) {
    viper.Reset()
    for k, v := range snapshot {
        viper.Set(k, v)
    }
}

func TestWithGlobalViper(t *testing.T) {
    snapshot := snapshotViper()
    t.Cleanup(func() { restoreViper(snapshot) })

    viper.Set("port", 9090)
    // test code...
}
```

AllSettings captures resolved values, not bindings, defaults, source precedence, aliases, or watchers. Replaying them with Set makes every copied value a highest-priority override. Inject a fresh instance or recreate the full known setup in isolated tests; Reset does not stop existing watchers.
