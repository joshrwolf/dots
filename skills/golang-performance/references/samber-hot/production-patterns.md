# Production Patterns

## Table of Contents

- [Stale-While-Revalidate](#stale-while-revalidate)
- [Sharding](#sharding)
- [Missing Key Caching (Negative Caching)](#missing-key-caching-negative-caching)
  - [Dedicated missing cache (recommended)](#dedicated-missing-cache-recommended)
  - [Shared missing cache](#shared-missing-cache)
  - [Manual missing key marking](#manual-missing-key-marking)
- [Loader Chains](#loader-chains)
- [Copy-on-Read / Copy-on-Write](#copy-on-read--copy-on-write)
- [Prometheus Monitoring](#prometheus-monitoring)
  - [Setup](#setup)
  - [Key PromQL Queries](#key-promql-queries)
- [Warm-Up on Startup](#warm-up-on-startup)
- [Graceful Shutdown](#graceful-shutdown)

## Stale-While-Revalidate

Return stale data immediately while refreshing in the background. Two time thresholds:

1. **TTL** — after this, entries become "stale" and trigger async background refresh via loaders
2. **Stale duration** — after TTL + stale, entries are hard-expired and removed

```go
refreshLoader := func(keys []string) (map[string]*Config, error) {
    return fetchConfigsFromDB(keys)
}

cache := hot.NewHotCache[string, *Config](hot.WTinyLFU, 1_000).
    WithTTL(5 * time.Minute).                              // stale after 5min
    WithLoaders(refreshLoader).                           // load cold misses too
    WithRevalidation(1 * time.Minute, refreshLoader).       // hard-expire after 6min total
    WithRevalidationErrorPolicy(hot.KeepOnError).           // keep stale value if refresh fails
    WithJanitor().
    Build()
defer cache.StopJanitor()
```

**Timeline without a successful refresh for an entry set at T=0:**

- T=0 to T=5min: fresh — returned directly
- T=5min to T=6min: stale — returned immediately, background refresh triggered
- T>6min: expired — removed, next `Get()` blocks on loader

**Error policies:**

- `hot.KeepOnError` — if background refresh fails, reinsert the stale value with a fresh TTL (v0.13.1), so repeated refresh failures can retain it beyond the original expiry; this is not a bounded maximum-age policy
- `hot.DropOnError` — if refresh fails, drop the entry immediately

Use `KeepOnError` when stale data is better than no data (config caches, product catalogs). Use `DropOnError` when correctness matters more than availability.

## Sharding

Split the cache into N independent segments to reduce lock contention under high concurrency:

```go
cache := hot.NewHotCache[string, *User](hot.WTinyLFU, 100_000).
    WithTTL(5 * time.Minute).
    WithSharding(16, func(key string) uint64 {
        h := fnv.New64a()
        h.Write([]byte(key))
        return h.Sum64()
    }).
    WithJanitor().
    Build()
defer cache.StopJanitor()
```

**Sizing guidance:**

- Choose shard count using contention measurements and a well-distributed hash. Powers of two are not required.
- Capacity is split across shards (rounded per shard); a skewed hash can fill one shard while others remain underused.
- More shards add metadata and can reduce eviction effectiveness.

## Missing Key Caching (Negative Caching)

Prevents repeated loader calls for keys that don't exist in the source:

### Dedicated missing cache (recommended)

Independent eviction algorithm and capacity — gives fine-grained control:

```go
cache := hot.NewHotCache[string, *User](hot.WTinyLFU, 100_000).
    WithTTL(1 * time.Hour).
    WithMissingCache(hot.LFU, 10_000).  // separate LFU cache for missing keys
    WithLoaders(userLoader).
    WithJanitor().
    Build()
defer cache.StopJanitor()
```

### Shared missing cache

Missing entries stored in the main cache — simpler but uses main cache capacity:

```go
cache := hot.NewHotCache[string, *User](hot.WTinyLFU, 100_000).
    WithTTL(1 * time.Hour).
    WithMissingSharedCache().
    WithLoaders(userLoader).
    WithJanitor().
    Build()
defer cache.StopJanitor()
```

### Manual missing key marking

```go
// Mark individual key as missing
cache.SetMissing("nonexistent-user")
cache.SetMissingWithTTL("temp-missing", 5*time.Minute)

// Batch mark missing
cache.SetMissingMany([]string{"user:404", "user:405"})
```

**Important:** `Keys()`, `Values()`, `All()` exclude missing entries — they only return real values.

## Loader Chains

Multiple loaders execute sequentially for L1/L2 cache patterns:

```go
redisLoader := func(keys []string) (map[string]*User, error) {
    return redis.MGet(ctx, keys...)
}

dbLoader := func(keys []string) (map[string]*User, error) {
    return db.GetUsersByIDs(ctx, keys)
}

cache := hot.NewHotCache[string, *User](hot.WTinyLFU, 10_000).
    WithTTL(5 * time.Minute).
    WithLoaders(redisLoader, dbLoader).  // Redis first, then DB for remaining
    WithJanitor().
    Build()
defer cache.StopJanitor()
```

**Chain behavior:**

- `redisLoader` called first with all missing keys
- `dbLoader` called only with keys NOT found by `redisLoader`
- If both return the same key, `dbLoader`'s value wins (later overwrites earlier)
- Any error stops the chain — partial results from earlier loaders are discarded

## Copy-on-Read / Copy-on-Write

Copying is one way to isolate mutable values; immutable values or coordinated ownership are alternatives. This example assumes User contains only value fields:

```go
cache := hot.NewHotCache[string, *User](hot.WTinyLFU, 10_000).
    WithTTL(5 * time.Minute).
    WithCopyOnRead(func(u *User) *User {
        copy := *u
        return &copy
    }).
    WithCopyOnWrite(func(u *User) *User {
        copy := *u
        return &copy
    }).
    WithJanitor().
    Build()
defer cache.StopJanitor()
```

- **CopyOnRead** — clones at retrieval: callers get independent copies, mutations don't affect cache
- **CopyOnWrite** — clones at storage: cache holds a snapshot, external mutations to the original don't corrupt cached value
- Deep-copy mutable maps, slices, and pointed-to objects too; a shallow struct copy still aliases them. The source must not be mutated concurrently while copying.

## Prometheus Monitoring

### Setup

```go
cache := hot.NewHotCache[string, *User](hot.WTinyLFU, 10_000).
    WithTTL(5 * time.Minute).
    WithPrometheusMetrics("user_cache").
    WithJanitor().
    Build()
defer cache.StopJanitor()

prometheus.MustRegister(cache)
```

### Key PromQL Queries

```promql
# Hit ratio (interpret against workload requirements)
sum(rate(hot_hit_total{name="user_cache",mode="main"}[5m])) /
(sum(rate(hot_hit_total{name="user_cache",mode="main"}[5m])) +
 sum(rate(hot_miss_total{name="user_cache",mode="main"}[5m])))

# Eviction rate (high = cache too small or TTL too short)
rate(hot_eviction_total{name="user_cache",mode="main"}[5m])

# Cache size vs capacity
sum(hot_length{name="user_cache",mode="main"}) /
sum(hot_settings_capacity{name="user_cache",mode="main"})
```

**Alerts to consider:**

- Hit rate drops materially below its workload baseline — investigate capacity, traffic mix, TTLs, and loader errors
- Eviction rate spikes — working set exceeds capacity
- Cache size near capacity — consider increasing capacity or reviewing TTLs

## Warm-Up on Startup

Pre-populate the cache before serving traffic:

```go
cache := hot.NewHotCache[string, *User](hot.WTinyLFU, 10_000).
    WithTTL(1 * time.Hour).
    WithMissingSharedCache(). // required for the missing keys returned below
    WithWarmUp(func() (map[string]*User, []string, error) {
        users, err := db.GetFrequentUsers(ctx)
        if err != nil {
            return nil, nil, err
        }
        missingKeys := []string{"deleted-user-1", "deleted-user-2"}
        return users, missingKeys, nil  // values + known missing keys + error
    }).
    WithJanitor().
    Build()
defer cache.StopJanitor()
```

In v0.13.1, `Build` discards warm-up errors. If successful warm-up is required before accepting traffic, call `cache.WarmUp(fn)` explicitly and handle its error. `WithWarmUpWithTimeout` bounds waiting but does not cancel the loader; the loader needs its own deadline.

## Graceful Shutdown

Always stop the janitor goroutine before exit:

```go
cache := hot.NewHotCache[string, *User](hot.WTinyLFU, 10_000).
    WithTTL(5 * time.Minute).
    WithJanitor().
    Build()
defer cache.StopJanitor()  // clean up background goroutine
```

In applications with graceful shutdown orchestration, call `cache.StopJanitor()` during the shutdown phase alongside other resource cleanup.
