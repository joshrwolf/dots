# Performance Investigation Reference

Runtime metrics, queries, and instrumentation tradeoffs for investigating performance under real traffic. Use existing observations where they answer the question; additional instrumentation is conditional on the missing evidence.

## Table of Contents

- [Instrumentation Tradeoffs](#instrumentation-tradeoffs)
- [Prometheus Go Runtime Collector](#prometheus-go-runtime-collector)
  - [Key Series](#key-series)
- [PromQL Deep-Dive Queries](#promql-deep-dive-queries)
  - [GC pressure](#gc-pressure)
  - [Memory leak detection](#memory-leak-detection)
  - [Goroutine leak detection](#goroutine-leak-detection)
  - [CPU saturation](#cpu-saturation)
  - [Post-deploy regression detection](#post-deploy-regression-detection)
  - [Example alerting rules](#example-alerting-rules)
- [Host-Level Correlation](#host-level-correlation)
- [Cost Warnings](#cost-warnings)

## Instrumentation Tradeoffs

- **Scrape resolution:** a shorter interval can reveal short-lived patterns that existing samples miss, but adds collection and storage overhead. Choose resolution for the timescale being investigated; no fixed interval is required.
- **Runtime controls versus rollouts:** an environment variable only works if the application implements it. Changing a Deployment environment variable rolls all replicas; it is not an instant per-pod toggle. Existing runtime controls or an isolated diagnostic deployment can target one instance. Changing the pod template already triggers a rollout.
- **Profiling scope:** select profiles and targets for the suspected bottleneck and available capacity. Continuous profiling can be fleet-wide when designed and budgeted for it.
- **Temporary logging and tracing:** bound overhead and data volume, and restore temporary settings when no longer needed. Startup configuration may require a restart; a supported runtime control can avoid it.

## Prometheus Go Runtime Collector

The `prometheus/client_golang` library automatically registers collectors that expose Go runtime metrics. These are invaluable during investigation sessions — they provide a time-series view of memory, GC, goroutines, and CPU that complements point-in-time profiles.

When using `prometheus/client_golang`, refer to the library's official documentation to verify collector setup and available options.

### Key Series

→ See [prometheus-go-metrics.md](./prometheus-go-metrics.md) for a version-dependent reference of common exported metrics. **Note:** runtime/metrics list varies by Go version — use `metrics.All()` at runtime for your specific Go version.

Modern client_golang Go collectors use runtime/metrics, including when constructing compatibility go_memstats metrics. Do not assume every scrape calls ReadMemStats or that MetricsAll switches the collection backend: it opts into more series. Check the installed library, enabled collectors, and capture evidence before attributing a pause to scraping. A custom registry makes enabled collectors explicit:

```go
import "github.com/prometheus/client_golang/prometheus/collectors"

// Use runtime/metrics-based collector (lower overhead)
reg := prometheus.NewRegistry()
reg.MustRegister(collectors.NewGoCollector(
    collectors.WithGoCollectorRuntimeMetrics(collectors.MetricsAll),
))
reg.MustRegister(collectors.NewProcessCollector(collectors.ProcessCollectorOpts{}))
```

## PromQL Deep-Dive Queries

Use these during investigation sessions with the reduced scrape interval. Each query includes what to look for and what the result means.

### GC pressure

| PromQL | What to look for |
| --- | --- |
| `rate(go_gc_duration_seconds_count[5m])` | GC cycles/s. Compare with workload, allocation rate, live heap, and GC CPU; there is no universal excessive frequency. |
| `rate(go_gc_duration_seconds_sum[5m]) / rate(go_gc_duration_seconds_count[5m])` | Average GC pause. Increasing trend = heap growing or too many pointers to scan. |
| `go_gc_duration_seconds{quantile="1"}` | Maximum pause among recent runtime pause samples, not an all-time maximum. Correlate with latency before attributing a P99 change. |

### Memory leak detection

| PromQL | What to look for |
| --- | --- |
| `go_memstats_alloc_bytes` | Should be roughly stable under constant load. Persistent growth across comparable post-GC observations can signal retention or a leak. |
| `rate(go_memstats_alloc_bytes_total[5m])` | Allocation rate (bytes/s). Compare before/after deploy — significant increase = new allocation pattern. |
| `process_resident_memory_bytes` alongside `go_memstats_sys_bytes - go_memstats_heap_released_bytes` | RSS and Go runtime-managed memory have different accounting; their difference is not a non-Go-memory measurement. Inspect OS mappings, stacks, cgo, and released pages when they diverge. |

### Goroutine leak detection

| PromQL | What to look for |
| --- | --- |
| `go_goroutines` | Should correlate with load. Sustained unexplained growth warrants goroutine-stack comparison; it does not alone prove a leak. |
| `delta(go_goroutines[1h])` | Net goroutine change over 1h. Persistent unexplained growth suggests checking goroutine ownership and stacks. |

### CPU saturation

| PromQL | What to look for |
| --- | --- |
| `rate(process_cpu_seconds_total[5m])` | CPU cores consumed. Compare with CPU quota, host capacity, GOMAXPROCS, and cgo/OS-thread activity. |
| `rate(process_cpu_seconds_total[5m]) / <GOMAXPROCS>` | Approximate ratio to Go execution capacity, not a guaranteed 0–1 process CPU utilization; cgo and runtime work can use other threads. |

### Post-deploy regression detection

| PromQL | What to look for |
| --- | --- |
| `rate(go_memstats_alloc_bytes_total[5m])` | Compare before/after deploy window. Significant increase = new allocation pattern introduced. |
| `histogram_quantile(0.99, rate(http_request_duration_seconds_bucket[5m]))` | P99 latency change after deploy; correlate workload and configuration before assigning cause. Requires an app histogram and deliberate aggregation preserving le. |

### Example alerting rules

```yaml
# GC taking too much time
- alert: HighGCPauseTime
  expr: rate(go_gc_duration_seconds_sum[5m]) / rate(go_gc_duration_seconds_count[5m]) > 0.01
  for: 10m
  annotations:
    summary: "Average GC pause >10ms — reduce allocations or tune GOGC"

# Goroutine leak
- alert: GoroutineLeak
  expr: go_goroutines > 10000
  for: 5m
  annotations:
    summary: "Goroutine count >10K — check for leaked goroutines"

# Memory approaching container limit
- alert: MemoryNearLimit
  expr: predict_linear(process_resident_memory_bytes[1h], 3600) > <container_limit_bytes>
  for: 15m
  annotations:
    summary: "RSS projected to exceed container limit within 1h"
```

Adjust thresholds to your application — a data pipeline has different baselines than an API server.

## Host-Level Correlation

Go runtime metrics alone don't show the full picture. Host-level metrics reveal whether the problem is in your application or the infrastructure.

- **`node_exporter`** — host CPU, memory, disk I/O, network. Correlate with Go app metrics: compare non-idle rates of node_cpu_seconds_total with process CPU rates and CPU quotas; high host usage with low process CPU suggests contention but does not prove a noisy neighbor.
- **`process-exporter`** — per-process metrics on Linux. Useful when multiple Go services share a host.

## Cost Warnings

Collection overhead depends on workload, profile type, and sampling settings:

- **CPU profiles and continuous profiling** — measure CPU overhead and backend ingestion/storage; continuous collection is a valid production design with an appropriate budget.
- **Execution traces** — start with a short capture or a flight recorder around the event of interest. Size and parsing cost vary; 5–10 seconds is a starting point, not a hard limit.
- **Debug logging** — watch allocation, I/O, volume, and sensitive data; keep temporary verbosity bounded.

- Use the existing deployment or runtime control mechanism. Changing environment variables on a running process is not an automatic live reconfiguration API.
