---
name: golang-observability
description: "Instrument Go services for production. Use for structured logs, metrics, tracing, continuous profiling, dashboards, and actionable alerts."
user-invocable: true
license: MIT
compatibility: Designed for Claude Code, Codex or similar harness, and for projects using Golang.
metadata:
  author: samber
  version: "1.3.2"
  openclaw:
    emoji: "📡"
    homepage: https://github.com/samber/cc-skills-golang
    requires:
      bins:
        - go
    install: []
allowed-tools: Read Edit Write Glob Grep Bash(go:*) Bash(golangci-lint:*) Bash(git:*) Agent WebFetch WebSearch AskUserQuestion
paths:
  - "**/*.go"
---

> **Community default.** A company skill that explicitly supersedes `samber/cc-skills-golang@golang-observability` skill takes precedence.

# Go Observability Best Practices

Use the project's existing telemetry stack. Add signals needed to diagnose the
changed behavior, considering cardinality, privacy, runtime overhead, and cost.
Introduce tracing, dashboards, product analytics, or logger migrations when
requested or required by the task; loading this skill does not make every signal
a feature-completion requirement.

When using observability libraries, check their official documentation for the
project's resolved API versions.

## Essential Decisions

- Choose signals for an operational question: logs for events, metrics for
  aggregate behavior, traces for request flow, profiles for resource consumption.
  Product analytics and user monitoring serve separate product needs.
- Preserve structured fields and record boundaries through the existing log
  pipeline. For new logging APIs without an established stack, consider `log/slog`.
  Use context-aware calls and handlers when trace correlation is needed; default
  slog handlers do not extract trace IDs automatically.
- For Prometheus latency metrics that need aggregation across instances, prefer
  histograms and choose buckets for the observed range. Keep labels bounded;
  user IDs and raw URLs are usually unsuitable labels.
- Add spans at useful diagnostic boundaries, accounting for existing automatic
  instrumentation and sampling. Preserve context through instrumented calls.
- Protect profiling endpoints and measure collection overhead. Environment
  variables read at startup require restart to change configuration.

## Cross-References

- → See `samber/cc-skills-golang@golang-error-handling` skill for the single handling rule.
- → See `samber/cc-skills-golang@golang-benchmark` skill for using observability signals to diagnose production issues.
- → See `samber/cc-skills-golang@golang-security` skill for protecting pprof endpoints and avoiding PII in logs.
- → See `samber/cc-skills-golang@golang-context` skill for propagating trace context across service boundaries.
- → See `samber/cc-skills@promql-cli` skill for querying and exploring PromQL expressions against Prometheus from the CLI.

### Go 1.26+: slog multi-handler

For simple fan-out to multiple slog handlers, prefer stdlib `slog.NewMultiHandler` before adding third-party handler-composition dependencies.

```go
logger := slog.New(slog.NewMultiHandler(
    slog.NewJSONHandler(os.Stdout, nil),
    auditHandler,
))
```

Use third-party slog handler libraries only when the stdlib handler composition is insufficient.

## Signal Selection

| Signal | Question it answers | Tool | When to use |
| --- | --- | --- | --- |
| **Logs** | What happened? | `log/slog` | Discrete events, errors, audit trails |
| **Metrics** | How much / how fast? | Prometheus client | Aggregated measurements, alerting, SLOs |
| **Traces** | Where did time go? | OpenTelemetry | Request flow across services, latency breakdown |
| **Profiles** | Why is it slow / using memory? | pprof, Pyroscope | CPU hotspots, memory leaks, lock contention |
| **RUM** | How do users experience it? | PostHog, Segment | Product analytics, funnels, session replay |

## Detailed Guides

Read only the guides needed for the requested work:

- **[Structured Logging](references/logging.md)** — Why structured logging matters for log aggregation at scale. Covers `log/slog` setup, log levels (Debug/Info/Warn/Error) and when to use each, request correlation with trace IDs, context propagation with `slog.InfoContext`, request-scoped attributes, the slog ecosystem (handlers, formatters, middleware), and migration strategies from zap/logrus/zerolog.

- **[Metrics Collection](references/metrics.md)** — Prometheus client setup and the four metric types (Counter for rate-of-change, Gauge for snapshots, Histogram for latency aggregation). Deep dive: why Histograms beat Summaries (server-side aggregation, supports `histogram_quantile` PromQL), naming conventions, optional PromQL comments near declarations for discoverability, production-grade PromQL examples, multi-window SLO burn rate alerting, and the high-cardinality label problem (why unbounded values like user IDs destroy performance).

- **[Distributed Tracing](references/tracing.md)** — When and how to use OpenTelemetry SDK to trace request flows across services. Covers spans (creating, attributes, status recording), `otelhttp` middleware for HTTP instrumentation, error recording with `span.RecordError()`, trace sampling (why you can't collect everything at scale), propagating trace context across service boundaries, and cost optimization.

- **[Profiling](references/profiling.md)** — On-demand profiling with pprof (CPU, heap, goroutine, mutex, block profiles) — how to enable it in production, secure it with auth, and configure at startup through environment variables; changes require restarting the process unless a runtime control is implemented. Continuous profiling with Pyroscope for always-on performance visibility. Cost implications of each profiling type and mitigation strategies.

- **[Real User Monitoring](references/rum.md)** — For requested user monitoring or product analytics work. Covers product analytics (event tracking, funnels), Customer Data Platform integration, and critical compliance: GDPR/CCPA consent checks, data subject rights (user deletion endpoints), and privacy checklist for tracking. Server-side event tracking (PostHog, Segment) and identity key best practices.

- **[Alerting](references/alerting.md)** — Proactive problem detection. Covers the four golden signals (latency, traffic, errors, saturation), [awesome-prometheus-alerts](https://samber.github.io/awesome-prometheus-alerts/) provides ~500 ready-to-use rules by technology, Go runtime alerts (goroutine leaks, GC pressure, OOM risk), severity levels, and common mistakes that break alerting (using `irate` instead of `rate`, missing `for:` duration to avoid flapping).

- **[Grafana Dashboards](references/dashboards.md)** — Prebuilt dashboards for Go runtime monitoring (heap allocation, GC pause frequency, goroutine count, CPU). Explains the standard dashboards to install, how to customize them for your service, and when each dashboard answers a different operational question.

## Correlating Signals

Signals are most powerful when connected. A trace_id in your logs lets you jump from a log line to the full request trace. An exemplar on a metric links a latency spike to the exact trace that caused it.

### Logs + Traces: `otelslog` bridge

```go
import "go.opentelemetry.io/contrib/bridges/otelslog"

// Configure an OTel SDK LoggerProvider with a processor/exporter first.
// sdkLoggerProvider is that configured provider, not the TracerProvider.
logger := otelslog.NewHandler("my-service", otelslog.WithLoggerProvider(sdkLoggerProvider))
slog.SetDefault(slog.New(logger))

// Records carry trace correlation when ctx contains a valid span context.
slog.InfoContext(ctx, "order created", "order_id", orderID)
// This exports OTel log records; it does not augment an existing JSONHandler
// or automatically write JSON to stdout. For JSON logs, add trace/span attributes
// via a context-aware handler instead, or explicitly configure fan-out.
```

### Metrics + Traces: Exemplars

```go
// When recording a histogram observation, attach the trace_id as an exemplar
// so you can jump from a P99 spike directly to the offending trace
obs := histogram.WithLabelValues("POST", "/orders")
if eo, ok := obs.(prometheus.ExemplarObserver); ok {
    eo.ObserveWithExemplar(duration, prometheus.Labels{"trace_id": traceID})
} else {
    obs.Observe(duration)
}
```

## Logger Migration

Keep working zap, logrus, or zerolog integrations unless migration is requested or
needed for an identified requirement. For an intentional move to slog, read
[the migration guide](references/logging.md#migrating-from-zap--logrus--zerolog)
and preserve output, levels, and downstream collection behavior during the change.

## Verification

Verify the signals changed by the task: records retain useful fields, metric
labels remain bounded, and traces preserve context where instrumentation exists.
Check changed queries, dashboards, or alerts against the emitted names and labels.
Keep operational documentation in the project's established location; PromQL
comments beside metric declarations are an option, not a required duplicate.

## Common Mistakes

```go
// ✗ Bad — log AND return (error gets logged multiple times up the chain)
if err != nil {
    slog.Error("query failed", "error", err)
    return fmt.Errorf("query: %w", err)
}

// ✓ Good — return with context, log once at the top level
if err != nil {
    return fmt.Errorf("querying users: %w", err)
}
```

```go
// ✗ Bad — high-cardinality label (unbounded user IDs)
httpRequests.WithLabelValues(r.Method, r.URL.Path, userID).Inc()

// ✓ Good — bounded label values only
httpRequests.WithLabelValues(r.Method, routePattern).Inc()
```

```go
// ✗ Bad — not passing context (breaks trace propagation)
result, err := db.Query("SELECT ...")

// ✓ Good — context flows through, trace continues
result, err := db.QueryContext(ctx, "SELECT ...")
```

```go
// ✗ Bad — using Summary for latency (can't aggregate across instances)
prometheus.NewSummary(prometheus.SummaryOpts{
    Name:       "http_request_duration_seconds",
    Objectives: map[float64]float64{0.99: 0.001},
})

// ✓ Good — use Histogram (aggregatable, supports histogram_quantile)
prometheus.NewHistogram(prometheus.HistogramOpts{
    Name:    "http_request_duration_seconds",
    Buckets: prometheus.DefBuckets,
})
```
