---
name: golang-samber-hot
description: "Use samber/hot for in-memory Go caches. Apply to eviction, expiration, loaders, sharding, stale-while-revalidate, and cache metrics."
user-invocable: true
license: MIT
compatibility: Designed for Claude Code, Codex or similar harness, and for projects using Golang.
metadata:
  author: samber
  version: "1.1.2"
  openclaw:
    emoji: "🔥"
    homepage: https://github.com/samber/cc-skills-golang
    requires:
      bins:
        - go
    install: []
    skill-library-version: "0.13.0"
allowed-tools: Read Edit Write Glob Grep Bash(go:*) Bash(golangci-lint:*) Bash(git:*) Agent WebFetch AskUserQuestion Bash(gopls:*) LSP mcp__gopls__*
paths:
  - "**/*.go"
---

# Using samber/hot for In-Memory Caching in Go

Generic, type-safe in-memory caching library for Go 1.22+ with 8 builder-selectable eviction algorithms, TTL, loader chains with singleflight deduplication, sharding, stale-while-revalidate, and Prometheus metrics.

**Official Resources:**

- [pkg.go.dev/github.com/samber/hot](https://pkg.go.dev/github.com/samber/hot)
- [github.com/samber/hot](https://github.com/samber/hot)

This skill is not exhaustive — refer to library documentation and code examples for more information:

- Consult `go doc` for the resolved package API; see `golang-go-doc` for lookup guidance.
- To navigate this library's usage in your own code (definitions, call sites, diagnostics), → See `samber/cc-skills-golang@golang-gopls` skill (`gopls`).

```bash
go get -u github.com/samber/hot
```

## Algorithm Selection

Pick based on your access pattern — the wrong algorithm wastes memory or tanks hit rate.

| Algorithm | Constant | Best for | Avoid when |
| --- | --- | --- | --- |
| **W-TinyLFU** | `hot.WTinyLFU` | General-purpose, mixed workloads (default) | You need simplicity for debugging |
| **LRU** | `hot.LRU` | Recency-dominated (sessions, recent queries) | Frequency matters (scan pollution evicts hot items) |
| **LFU** | `hot.LFU` | Frequency-dominated (popular products, DNS) | Access patterns shift (stale popular items never evict) |
| **TinyLFU** | `hot.TinyLFU` | Read-heavy with frequency bias | Write-heavy (admission filter overhead) |
| **S3FIFO** | not exposed by the v0.13.1 HotCache builder | High throughput, scan-resistant | Measure on the actual cache size |
| **ARC** | `hot.ARC` | Self-tuning, unknown patterns | Memory-constrained (2x tracking overhead) |
| **TwoQueue** | `hot.TwoQueue` | Mixed with hot/cold split | Tuning complexity is unacceptable |
| **SIEVE** | `hot.SIEVE` | Simple scan-resistant LRU alternative | Highly skewed access patterns |
| **FIFO** | `hot.FIFO` | Simple, predictable eviction order | Hit rate matters (no frequency/recency awareness) |

**Decision shortcut:** Start with `hot.WTinyLFU`. Switch only when profiling shows the miss rate is too high for your SLO.

For detailed algorithm comparison, benchmarks, and a decision tree, see [Algorithm Guide](./references/algorithm-guide.md).

## Core Usage

### Basic Cache with TTL

```go
import "github.com/samber/hot"

cache := hot.NewHotCache[string, *User](hot.WTinyLFU, 10_000).
    WithTTL(5 * time.Minute).
    WithJanitor().
    Build()
defer cache.StopJanitor()

cache.Set("user:123", user)
cache.SetWithTTL("user:456", anotherUser, 30*time.Minute)

value, found, err := cache.Get("user:123")
```

### Loader Pattern (Read-Through)

Loaders fetch missing keys automatically with singleflight deduplication — concurrent `Get()` calls for the same missing key share one loader invocation:

```go
cache := hot.NewHotCache[int, *User](hot.WTinyLFU, 10_000).
    WithTTL(5 * time.Minute).
    WithLoaders(func(ids []int) (map[int]*User, error) {
        return db.GetUsersByIDs(ctx, ids) // batch query
    }).
    WithJanitor().
    Build()
defer cache.StopJanitor()

user, found, err := cache.Get(123) // triggers loader on miss
```

## Capacity Sizing

Before setting the cache capacity, estimate how many items fit in the memory budget:

1. **Estimate single-item size** — estimate size of the struct, add the size of heap-allocated fields (slices, maps, strings). Include the key size. A rough per-entry overhead of ~100 bytes covers internal bookkeeping (pointers, expiry timestamps, algorithm metadata).
2. **Ask the developer** how much memory is dedicated to this cache in production (e.g., 256 MB, 1 GB). This depends on the service's total memory and what else shares the process.
3. **Compute capacity** — `capacity = memoryBudget / estimatedItemSize`. Round down to leave headroom.

```
Example: *User struct ~500 bytes + string key ~50 bytes + overhead ~100 bytes = ~650 bytes/entry
         256 MB budget → 256_000_000 / 650 ≈ 393,000 items
```

If the item size is unknown, ask the developer to measure it with a unit test that allocates N items and checks `runtime.ReadMemStats`. Guessing capacity without measuring leads to OOM or wasted memory.

## Common Mistakes

1. **Confusing expiration with cleanup** — `Get` checks expiration even without a janitor. `WithJanitor()` proactively reclaims expired entries; it is optional. Stop it when the cache is no longer needed.
2. **Calling `SetMissing()` without missing cache config** — panics at runtime. Enable `WithMissingCache(algorithm, capacity)` or `WithMissingSharedCache()` in the builder first.
3. **`WithoutLocking()` + `WithJanitor()`** — mutually exclusive, panics. `WithoutLocking()` is only safe for single-goroutine access without background cleanup.
4. **Oversized cache** — a cache holding everything is a map with overhead. Size to measured workload and memory budget. Monitor hit rate to validate.
5. **Ignoring loader errors** — `Get()` returns `(zero, false, err)` on loader failure. Always check `err`, not just `found`.

## Best Practices

1. Choose TTL or explicit invalidation according to freshness requirements; capacity-bounded caches do not inherently need TTL
2. Use `WithJitter(lambda, upperBound)` to spread expirations — without jitter, items created together expire together, causing thundering herd on the loader
3. Monitor with `WithPrometheusMetrics(cacheName)` — interpret hit rate alongside miss cost, freshness requirements, and access patterns
4. Use `WithCopyOnRead(fn)` / `WithCopyOnWrite(fn)` for mutable values — without copies, callers mutate cached objects and corrupt shared state

For advanced patterns (revalidation, sharding, missing cache, monitoring setup), see [Production Patterns](./references/production-patterns.md).

For the complete API surface, see [API Reference](./references/api-reference.md).


## Cross-References

- → See `samber/cc-skills-golang@golang-performance` skill for general caching strategy and when to use in-memory cache vs Redis vs CDN
- → See `samber/cc-skills-golang@golang-observability` skill for Prometheus metrics integration and monitoring
- → See `samber/cc-skills-golang@golang-database` skill for database query patterns that pair with cache loaders
