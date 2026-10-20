# Plugin Ecosystem

samber/ro ships 40+ plugins that extend the core library with domain-specific operators. Plugins are separate Go modules — install only what you need.

```bash
go get github.com/samber/ro/plugins/<category>/<name>
```

## Table of Contents

- [Data Manipulation](#data-manipulation)
- [Encoding and Serialization](#encoding-and-serialization)
- [Scheduling](#scheduling)
- [Network and I/O](#network-and-io)
- [Observability and Logging](#observability-and-logging)
- [Rate Limiting](#rate-limiting)
- [Text Processing](#text-processing)
- [System Integration](#system-integration)
- [Validation](#validation)
- [Utilities](#utilities)
- [Plugin Design Convention](#plugin-design-convention)

## Data Manipulation

| Plugin | Import | Purpose |
| --- | --- | --- |
| Bytes | `plugins/bytes` | Byte slice operations on streams |
| Strings | `plugins/strings` | String transformations (split, trim, join) |
| Sort | `plugins/sort` | Sorting operators for ordered streams |
| Strconv | `plugins/strconv` | Type conversion (string <-> numeric) |
| Iter | `plugins/iter` | Go 1.23+ iterator interop |
| SIMD (experimental) | `plugins/exp/simd` | SIMD-accelerated numeric transforms |

## Encoding and Serialization

| Plugin | Import                    | Purpose                          |
| ------ | ------------------------- | -------------------------------- |
| JSON   | `plugins/encoding/json`   | Marshal/unmarshal JSON in stream |
| CSV    | `plugins/encoding/csv`    | Parse/generate CSV rows          |
| Base64 | `plugins/encoding/base64` | Encode/decode Base64             |
| Gob    | `plugins/encoding/gob`    | Go binary encoding               |

```go
import rojson "github.com/samber/ro/plugins/encoding/json"

// Parse JSON stream: []byte -> MyStruct
parsed := ro.Pipe1(rawBytes, rojson.Unmarshal[MyStruct]())
```

## Scheduling

| Plugin | Import         | Purpose                               |
| ------ | -------------- | ------------------------------------- |
| Cron   | `plugins/cron` | Emit on cron expressions or intervals |
| ICS    | `plugins/ics`  | Parse iCal files into event streams   |

```go
import rocron "github.com/samber/ro/plugins/cron"

// Emit every day at midnight
daily := rocron.NewScheduler(gocron.CronJob("0 0 * * *", false)) // github.com/go-co-op/gocron/v2
```

## Network and I/O

| Plugin   | Import             | Purpose                                  |
| -------- | ------------------ | ---------------------------------------- |
| I/O      | `plugins/io`       | File and stream reading/writing          |
| FSNotify | `plugins/fsnotify` | File system change events                |

```go
import rofsnotify "github.com/samber/ro/plugins/fsnotify"

// Watch directory for changes
events := rofsnotify.NewFSListener("/var/log/app/")
ro.Pipe1(events, ro.Filter(func(e fsnotify.Event) bool {
    return e.Op == fsnotify.Write
})).Subscribe(ro.OnNext(func(e fsnotify.Event) {
    log.Println("Modified:", e.Name)
}))
```

The upstream README lists `plugins/http`, but that module was not available at the audited v0.4.1 release. Wrap net/http in your own observable instead of relying on an unverified import.

## Observability and Logging

| Plugin  | Import                          | Purpose                         |
| ------- | ------------------------------- | ------------------------------- |
| Log     | `plugins/observability/log`     | stdlib `log` integration        |
| Zap     | `plugins/observability/zap`     | Uber Zap structured logging     |
| Logrus  | `plugins/observability/logrus`  | Logrus logging                  |
| Slog    | `plugins/observability/slog`    | Go 1.21+ `log/slog` integration |
| Zerolog | `plugins/observability/zerolog` | Zerolog logging                 |
| Sentry  | `plugins/observability/sentry`  | Sentry error tracking           |
| Oops    | `plugins/samber/oops`           | samber/oops structured errors   |

```go
import roslog "github.com/samber/ro/plugins/observability/slog"

// Log all stream events via slog
ro.Pipe1(
    dataStream,
    roslog.LogWithNotification[Data](*logger, slog.LevelInfo),
)
```

## Rate Limiting

| Plugin | Import                     | Purpose                            |
| ------ | -------------------------- | ---------------------------------- |
| Native | `plugins/ratelimit/native` | Built-in token bucket rate limiter |
| Ulule  | `plugins/ratelimit/ulule`  | ulule/limiter integration          |

## Text Processing

| Plugin   | Import             | Purpose                              |
| -------- | ------------------ | ------------------------------------ |
| Regexp   | `plugins/regexp`   | Regex matching/extraction on streams |
| Template | `plugins/template` | Go text/html template rendering      |

## System Integration

| Plugin  | Import           | Purpose                                    |
| ------- | ---------------- | ------------------------------------------ |
| Process | `plugins/proc`   | Process execution operators                |
| Signal  | `plugins/signal` | OS signal handling (SIGTERM, SIGINT, etc.) |

```go
import rosignal "github.com/samber/ro/plugins/signal"

// Observable that emits on SIGTERM/SIGINT
shutdown := rosignal.NewSignalCatcher(syscall.SIGTERM, syscall.SIGINT)

// Use as TakeUntil signal for graceful shutdown
ro.Pipe1(workStream, ro.TakeUntil[Work, os.Signal](shutdown))
```

## Validation

| Plugin | Import | Purpose |
| --- | --- | --- |
| Ozzo Validation | `plugins/ozzo/ozzo-validation` | Stream-level input validation |

## Utilities

| Plugin      | Import                | Purpose                                |
| ----------- | --------------------- | -------------------------------------- |
| HyperLogLog | `plugins/hyperloglog` | Cardinality estimation on streams      |
| Hot         | `plugins/samber/hot`  | In-memory caching integration          |
| PSI         | `plugins/samber/psi`  | Starvation notifier / pressure metrics |

## Plugin Design Convention

Plugins expose sources or operators; inspect each package signature:

1. Import the plugin package
2. Use plugin-provided operators in `Pipe` chains
3. Operators return `func(Observable[T]) Observable[R]`; sources such as NewFSListener return an Observable directly
4. Check lifecycle and sharing semantics for the individual plugin

Plugins are documented individually in their package directories. API details for each plugin are available at `pkg.go.dev/github.com/samber/ro/plugins/...`.
