# Viper WatchConfig and Hot Reload

## Table of Contents

- [Basic setup](#basic-setup)
- [Atomic saves and watcher events](#atomic-saves-and-watcher-events)
- [Race-safe reload pattern](#race-safe-reload-pattern)
- [Debouncing rapid changes](#debouncing-rapid-changes)
- [Validating config before applying](#validating-config-before-applying)
- [Stopping the watcher](#stopping-the-watcher)

## Basic setup

```go
viper.OnConfigChange(func(e fsnotify.Event) {
    log.Printf("config changed: %s (op: %s)", e.Name, e.Op)
    // decode, validate, and publish a candidate snapshot
})
viper.WatchConfig()
```

`WatchConfig` starts a background goroutine that watches the config file using fsnotify. Call it after `ReadInConfig`.

## Atomic saves and watcher events

Viper watches the containing directory specifically to support atomic replacement. It reacts to Write/Create for the configured path and changes to a symlink's resolved target. Filesystem and mount behavior still varies: test direct writes, atomic replacement, and the deployment's symlink updates. Do not diagnose missed callbacks solely from an editor's use of rename.

Register OnConfigChange before WatchConfig. The callback runs after Viper attempts to reread the file; it can also run when that read fails.

## Race-safe reload pattern

Config reload happens in a background goroutine. Any shared state updated in `OnConfigChange` must be synchronized:

```go
type Config struct {
    LogLevel string `mapstructure:"log_level"`
    MaxConn  int    `mapstructure:"max_conn"`
}

var cfg Config
var cfgMu sync.RWMutex

viper.OnConfigChange(func(e fsnotify.Event) {
    var newCfg Config
    if err := viper.Unmarshal(&newCfg); err != nil {
        log.Printf("error reloading config: %v", err)
        return  // keep old config on error
    }

    cfgMu.Lock()
    cfg.LogLevel = newCfg.LogLevel
    cfg.MaxConn = newCfg.MaxConn
    cfgMu.Unlock()

    log.Printf("config reloaded: log_level=%s", newCfg.LogLevel)
})
```

Reads use `cfgMu.RLock()`. Once the watcher starts, other goroutines should read the synchronized snapshot, not the Viper instance: Viper does not support concurrent reads and writes. Its callback follows the reread, rather than racing ahead of it.

## Debouncing rapid changes

Multiple events can represent one save. Do not read the watched Viper instance from a timer callback: the watcher may write it concurrently. Decode a candidate synchronously in OnConfigChange, then send the immutable candidate to a single owner that coalesces updates and publishes it. Stopping a time.AfterFunc timer does not wait for an already-running callback.

## Validating config before applying

Always validate reloaded config before applying it — an invalid config mid-reload should keep the previous working config:

```go
viper.OnConfigChange(func(e fsnotify.Event) {
    var candidate Config
    if err := viper.Unmarshal(&candidate); err != nil {
        log.Printf("reload: invalid config, keeping previous: %v", err)
        return
    }
    if err := validate(candidate); err != nil {
        log.Printf("reload: validation failed, keeping previous: %v", err)
        return
    }
    applyConfig(candidate)
})
```

## Stopping the watcher

There is no documented way to stop `WatchConfig` once started. Design your application so that the watcher's lifetime matches the process lifetime. A fresh Viper instance does not stop an old watcher; the goroutine keeps it reachable. Avoid starting process-lifetime watchers in ordinary unit tests; use a subprocess or an explicitly owned fsnotify watcher for lifecycle tests.
