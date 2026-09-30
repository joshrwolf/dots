---
name: golang-spf13-viper
description: "Use spf13/viper for Go configuration. Apply to flag/environment/file precedence, decoding, config reloads, and test isolation."
user-invocable: true
license: MIT
compatibility: Designed for Claude Code, Codex or similar harness, and for projects using Golang.
metadata:
  author: samber
  version: "1.1.2"
  openclaw:
    emoji: "🔧"
    homepage: https://github.com/samber/cc-skills-golang
    requires:
      bins:
        - go
    install: []
    skill-library-version: "1.21.0"
allowed-tools: Read Edit Write Glob Grep Bash(go:*) Bash(golangci-lint:*) Bash(git:*) Agent WebFetch Bash(gopls:*) LSP mcp__gopls__*
paths:
  - "**/*.go"
---

# Using spf13/viper for layered configuration in Go

Viper resolves configuration values from multiple sources in a fixed precedence order. It has no user-facing surface — it doesn't define commands or flags. Its job is to answer "what is the value of key X right now?" by walking its source layers from highest to lowest priority.

**Official Resources:**

- [pkg.go.dev/github.com/spf13/viper](https://pkg.go.dev/github.com/spf13/viper)
- [github.com/spf13/viper](https://github.com/spf13/viper)

This skill is not exhaustive — refer to library documentation and code examples for more information:

- Consult `go doc` for the resolved package API; see `golang-go-doc` for lookup guidance.
- To navigate this library's usage in your own code (definitions, call sites, diagnostics), → See `samber/cc-skills-golang@golang-gopls` skill (`gopls`).

```bash
go get github.com/spf13/viper@latest
```

## Viper vs. cobra

Cobra owns the command tree — subcommands, flags, arg validation, completions. Viper owns configuration resolution — it answers "what is the value of key X?" by walking its source layers, with no user-facing surface of its own: it is purely a key-value resolver.

- **Cobra alone** — flag-only CLIs.
- **Viper alone** — config-file daemons.
- **Both** — bind flags at `PersistentPreRunE` via `BindPFlag`.

→ See `samber/cc-skills-golang@golang-spf13-cobra` for the cobra side of this integration.

## The precedence pipeline

Viper resolves a key by walking sources in this order (first set value wins):

```
1. explicit Set()      — viper.Set("key", val)    highest priority
2. flag                — bound pflag.Flag
3. env var             — BindEnv / AutomaticEnv
4. config file         — ReadInConfig / MergeInConfig
5. KV remote           — etcd / Consul
6. default             — viper.SetDefault("key", val)   lowest priority
```

This pipeline is fixed and cannot be reordered. Understanding it prevents most viper bugs: a key that "should" come from a config file may be shadowed by an env var or an explicitly changed flag. An unchanged flag default does not override env/file/default values.

## Sources and config files

```go
viper.SetConfigName("config")
viper.AddConfigPath("$HOME/.myapp")
if err := viper.ReadInConfig(); err != nil {
    var notFound viper.ConfigFileNotFoundError
    if !errors.As(err, &notFound) {
        return fmt.Errorf("reading config: %w", err) // propagate real errors only
    }
}
```

Ignore `ConfigFileNotFoundError` only for an optional searched config. An explicit `SetConfigFile` path that is missing returns a filesystem error; usually report it to catch a mistyped path.

For supported formats (JSON, TOML, YAML, HCL, INI, properties), `MergeInConfig`, and remote KV, see [sources-and-formats.md](references/sources-and-formats.md).

## Env binding and key replacers

This is the highest-bug-density area in viper. Choose the prefix and replacer to match the application naming scheme. AutomaticEnv enables lookup on Get; BindEnv can bind individual keys instead:

```go
// ✓ Good — all three wired together at startup
viper.SetEnvPrefix("MYAPP")                             // prevent collisions: PORT → MYAPP_PORT
viper.SetEnvKeyReplacer(strings.NewReplacer(".", "_"))  // database.host → MYAPP_DATABASE_HOST
viper.AutomaticEnv()

// ✗ Bad — without SetEnvKeyReplacer, viper looks for MYAPP_DATABASE.HOST (dot preserved)
```

For `BindEnv`, `AllowEmptyEnv`, and env-vs-default interaction, see [binding-and-env.md](references/binding-and-env.md).

## Flag binding (the cobra seam)

Bind flags before reading their values or decoding config. Binding after flag parsing is valid: Viper retains a reference to the flag and reads its current Changed state and value:

```go
func init() {
    rootCmd.PersistentFlags().Int("port", 8080, "listen port")
    viper.BindPFlag("port", rootCmd.PersistentFlags().Lookup("port"))
    // viper.BindPFlags(cmd.Flags()) — bind an entire FlagSet at once
}
```

For `AllowEmptyEnv` and flag/env interaction details, see [binding-and-env.md](references/binding-and-env.md).

## Unmarshaling into structs

`viper.Unmarshal` maps the resolved configuration into a struct using `mapstructure`:

```go
type Config struct {
    Port     int `mapstructure:"port"`
    Database struct {
        MaxConn int `mapstructure:"max_conn"` // explicit tag: mapstructure won't convert underscore→camelCase
    } `mapstructure:"database"`
}
var cfg Config
if err := viper.Unmarshal(&cfg); err != nil {
    return fmt.Errorf("decoding config: %w", err)
}
```

**Always use `mapstructure` tags** — implicit mapping is fragile for nested structs and underscore-named fields. Prefer `UnmarshalKey("database", &dbCfg)` over `Sub("database").Unmarshal` — it avoids the nil-check `Sub` requires when the key is missing.

For `time.Duration` / `net.IP` / slice decoders and custom `DecodeHook` registration, see [unmarshal.md](references/unmarshal.md).

## Sub-trees

`viper.Sub("database")` returns a new `*viper.Viper` scoped to the prefix, or **nil** if the key does not exist — always nil-check before calling methods on the result. Prefer `UnmarshalKey("database", &dbCfg)` which avoids the nil risk entirely.

## Hot reload

```go
viper.OnConfigChange(func(e fsnotify.Event) { /* validate and publish a snapshot */ })
viper.WatchConfig()
```

`WatchConfig` watches the containing directory to handle atomic saves and symlink replacements. Register the callback before starting the watcher and test the actual deployment save pattern. For race-safe reload patterns, see [watch-and-reload.md](references/watch-and-reload.md).

## Test isolation

**Never use the global viper in tests** — state leaks across test cases. Use `viper.New()` per test so each instance is isolated:

```go
v := viper.New()
v.SetConfigFile("testdata/config.yaml")
if err := v.ReadInConfig(); err != nil {
	t.Fatalf("v.ReadInConfig: %v", err)
}
```

For `t.Setenv` interactions and `Reset()` limitations, see [testing-and-isolation.md](references/testing-and-isolation.md).

## Best Practices

1. **Match environment naming explicitly** — prefix and replacer are optional, and AutomaticEnv alone does not enumerate env-only keys for Unmarshal; use BindEnv or defaults to register those keys.
2. **Distinguish optional and required files** — ignore missing-file errors only when the configuration contract allows omission.
3. **Always use `mapstructure` tags on config structs** — implicit mapping silently misses nested and underscore-named fields.
4. **Use `viper.New()` in tests, never the global** — the global accumulates state across test runs; per-test instances are isolated.
5. **Bind before resolution** — parsing can happen first, but bind before Get or Unmarshal.

## Common Mistakes

| Mistake | Why it fails | Fix |
| --- | --- | --- |
| No `mapstructure` tags on struct fields | Silently misses nested and underscore-named fields | Add `mapstructure:"key_name"` to every field |
| Using global viper in tests | State from one test contaminates the next, causing flaky ordering | Create `viper.New()` per test |

## Further Reading

- [sources-and-formats.md](references/sources-and-formats.md) — supported file formats, multi-path search, MergeInConfig, remote KV (etcd/Consul)
- [binding-and-env.md](references/binding-and-env.md) — BindEnv, AutomaticEnv, SetEnvPrefix, SetEnvKeyReplacer, AllowEmptyEnv, timing rules
- [unmarshal.md](references/unmarshal.md) — Unmarshal, UnmarshalKey, mapstructure tags, custom DecodeHooks (Duration, IP, slice)
- [watch-and-reload.md](references/watch-and-reload.md) — WatchConfig, OnConfigChange, fsnotify caveats, atomic-rename trap, race-safe patterns
- [testing-and-isolation.md](references/testing-and-isolation.md) — viper.New() per test, t.Setenv interactions, Reset() limitations, snapshot/restore

## Cross-References

- → See `samber/cc-skills-golang@golang-cli` skill for general CLI architecture — project layout, exit codes, signal handling, cobra+viper integration
- → See `samber/cc-skills-golang@golang-spf13-cobra` skill for the cobra side of this integration (flag definition and binding)
- → See `samber/cc-skills-golang@golang-testing` skill for general Go testing patterns
