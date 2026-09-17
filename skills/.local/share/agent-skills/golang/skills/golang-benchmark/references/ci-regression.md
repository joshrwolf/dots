# CI Benchmark Regression Detection

Use controlled local or CI environments. Shared CI can be noisier than a quiet developer machine; compare under matched conditions, quantify uncertainty, and reserve hard gates for measurements stable enough to support them. Tools that change Git state, especially cob, belong in disposable isolated checkouts.

## Table of Contents

- [benchdiff](#benchdiff)
- [cob](#cob)
- [gobenchdata](#gobenchdata)
  - [CLI commands](#cli-commands)
  - [GitHub Action setup](#github-action-setup)
  - [Regression gating on PRs](#regression-gating-on-prs)
  - [Dashboard configuration](#dashboard-configuration)
- [Tool Selection Guide](#tool-selection-guide)
- [Noisy Neighbor Mitigation](#noisy-neighbor-mitigation)
  - [Why CI benchmarks are noisy](#why-ci-benchmarks-are-noisy)
  - [Strategies](#strategies)
- [System Tuning for Self-Hosted Runners](#system-tuning-for-self-hosted-runners)
  - [Disable CPU frequency scaling](#disable-cpu-frequency-scaling)
  - [Disable Turbo Boost](#disable-turbo-boost)
  - [Pin benchmarks to specific CPU cores](#pin-benchmarks-to-specific-cpu-cores)
  - [Disable SMT (Hyper-Threading)](#disable-smt-hyper-threading)
  - [Combined CI setup script](#combined-ci-setup-script)

## benchdiff

Runs Go benchmarks on two git refs and uses `benchstat` to display deltas. Caches results for non-worktree refs so re-runs are fast. Prevents macOS sleep during benchmarks.

```bash
go install filippo.io/mostly-harmless/benchdiff@latest
```

```bash
# Compare current worktree against HEAD (default)
benchdiff -- -benchmem

# Compare two specific refs
benchdiff -base-ref main -head-ref feature-branch

# Compare against a specific commit or tag
benchdiff -base-ref v1.2.0

# Pass extra flags to go test — everything after -- goes to go test
benchdiff -- -benchmem -count=10 -benchtime=3s

# Filter to specific benchmarks
benchdiff -- -benchmem -count=10 -bench=BenchmarkParse

# Target a specific package
benchdiff -- -benchmem -count=10 ./pkg/parser/...

# Clear cached results (useful after rebasing or when cache is stale)
benchdiff -clear-cache

# Combine: compare main with 10 iterations, filtered to critical benchmarks
benchdiff -base-ref main -- -benchmem -count=10 -bench='BenchmarkParse|BenchmarkEncode'
```

Best for: quick PR-to-base comparisons in git-based workflows. Leverages `benchstat` for statistical rigor and caches non-worktree refs so re-runs only re-measure the worktree.

## cob

Compares benchmarks between HEAD and HEAD~1, failing the CI job if performance degrades beyond a configurable threshold (default 20%).

```bash
go install github.com/knqyf263/cob@latest
```

```bash
# Run with default 20% threshold — compares HEAD vs HEAD~1
cob

# Stricter threshold for critical paths (10% regression = failure)
cob -threshold 0.10

# Compare against a specific base commit
cob -base main

# Only report regressions (ignore improvements)
cob -only-degression

# Choose which metrics to compare (default: ns/op,B/op)
cob -compare "ns/op,B/op,allocs/op"

# Custom go test arguments
cob -bench-args "test -run '^$' -bench BenchmarkParse -benchmem ./pkg/parser/..."

# Increase benchmark duration for more stable results
cob -bench-args "test -run '^$' -bench . -benchmem -benchtime=3s ./..."

# Skip cob for a specific commit: include [skip cob] in commit message
```

**Caution:** cob resets Git state internally. Run it in a disposable isolated checkout with no user work; committing alone is not adequate isolation for untracked files or branch state.

- A disposable local checkout or CI checkout is suitable; never run it in the active working checkout.
- `cob` requires all benchmarks to pass; it skips CI gating if any benchmark fails.
- `cob` compares single runs without `benchstat`-style statistics, making it more susceptible to noise than `benchdiff`.

Best for: simple post-commit regression gating in CI where statistical rigor is less critical than fast feedback.

## gobenchdata

GitHub Action + CLI that collects benchmark results, publishes to gh-pages as JSON, and visualizes with an interactive web dashboard. Shows performance trends over time.

```bash
go install go.bobheadxi.dev/gobenchdata@latest
```

### CLI commands

```bash
# Parse go test -bench output to JSON
go test -bench=. -benchmem -count=5 ./... | gobenchdata --json bench.json

# Parse from a file
cat bench.txt | gobenchdata --json bench.json

# Add a tag to the benchmark run (e.g., git commit)
cat bench.txt | gobenchdata --json bench.json --tag "$(git rev-parse --short HEAD)"

# Evaluate regression checks against a checks config
gobenchdata checks eval base.json current.json --checks.config .gobenchdata-checks.yml

# Generate the web dashboard app (static Vue.js site)
gobenchdata web generate ./dashboard-app

# Serve the dashboard locally for preview
(cd dashboard-app && gobenchdata web serve)

# Merge multiple benchmark JSON files
gobenchdata merge old-bench.json new-bench.json > combined.json

# Append another run and retain the latest 30 history entries
cat new-bench.txt | gobenchdata --json bench.json --append --prune 30
```

### GitHub Action setup

```yaml
# .github/workflows/benchmark.yml
name: Benchmark
on: [push]
permissions:
  contents: write # required only for the configured result publication
jobs:
  benchmark:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: actions/setup-go@v5
        with:
          go-version-file: go.mod
      - name: Run and publish benchmarks
        run: go run go.bobheadxi.dev/gobenchdata@v1.3.1 action
        env:
          GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}
          INPUT_PRUNE_COUNT: 30
          INPUT_GO_TEST_PKGS: ./...
          INPUT_GO_TEST_FLAGS: -run=^$ -benchmem -count=5
          INPUT_BENCHMARKS_OUT: benchmarks.json
          INPUT_PUBLISH: true
          INPUT_PUBLISH_BRANCH: gh-pages
```

The CLI action uses the job's Go toolchain. The Docker action ships its own Go
version, so a preceding setup-go step does not select its benchmark compiler.
BENCHMARKS_OUT names JSON output, not previously captured go test text. Pin action
revisions according to repository policy; restrict publication to trusted events.

### Regression gating on PRs

```yaml
- name: Check for regressions
  run: gobenchdata checks eval base.json current.json --checks.config .gobenchdata-checks.yml
```

```yaml
# .gobenchdata-checks.yml
checks:
  - name: "Runtime regression"
    package: '^example\.com/myapp/'
    benchmarks: ['BenchmarkParse.*']
    diff: (current.NsPerOp - base.NsPerOp) / base.NsPerOp * 100
    thresholds:
      max: 20 # percentage change; choose from measured variance and tolerance
```

Parse both benchmark outputs with gobenchdata --json before evaluating them.
The diff expression assumes a nonzero baseline. These are threshold checks,
not a substitute for benchstat's uncertainty analysis. Verify that expected checks actually match benchmarks: an empty match is not evidence that the workload passed a regression gate.

### Dashboard configuration

```yaml
# gobenchdata-web.yml — configure the Vue.js dashboard
title: "My Project Benchmarks"
description: "Performance tracking dashboard"
chartGroups:
  - name: Parser
    charts:
      - name: Parse Performance
        package: myapp/pkg/parser
        benchmarks: ["BenchmarkParse.*"]
        metrics: [NsPerOp, AllocedBytesPerOp, AllocsPerOp]
  - name: Encoding
    charts:
      - name: Encode/Decode
        package: myapp/pkg/encoding
        benchmarks: ["Benchmark(Encode|Decode).*"]
        metrics: [NsPerOp, MBPerS]
```

Best for: long-term trend tracking and visualization; complements benchdiff/cob for immediate gating.

## Tool Selection Guide

| Tool | Statistical rigor | Dashboard | Best for |
| --- | --- | --- | --- |
| **benchdiff** | High (uses benchstat) | No | Local dev + CI PR comparisons |
| **cob** | Low (single comparison) | No | Quick CI gate, simple setup |
| **gobenchdata** | Medium (configurable checks) | Yes (Vue.js on gh-pages) | Long-term trend tracking |
| **benchstat** (raw) | High | No (CSV export) | Maximum control, custom workflows |

## Noisy Neighbor Mitigation

Cloud CI may share hardware and vary across allocations. Measure your actual variance; there is no universal percentage.

### Why CI benchmarks are noisy

- **Shared CPU/memory** — other CI jobs compete for resources
- **Thermal throttling** — sustained load reduces clock speed
- **Different hardware across runs** — CI runners may have different specs
- **Kernel scheduling** — context switches add unpredictable latency
- **Disk I/O contention** — shared storage affects I/O-bound benchmarks

### Strategies

**Statistical rigor** — run with `-count=10` or more and compare with `benchstat`. A single run is meaningless. benchstat reports statistical evidence under its assumptions; false positives and systematic bias remain possible.

**Relative comparison in same job** — run both base and head benchmarks in the same CI job on the same machine, rather than comparing against historical absolute values. This reduces machine-to-machine differences but does not remove drift or workload interference. Tools like `benchdiff` do this automatically by checking out both git refs.

**Dedicated benchmark runners** — for critical path benchmarks, use self-hosted CI runners with no other workloads. This removes competing scheduled workloads; OS activity, thermal behavior, and hardware variability still require control.

Choose regression thresholds from acceptable impact, observed noise, sample count, and the number of comparisons. Neither a fixed 7%, 10%, nor 20% threshold guarantees a false-positive rate. Validate the proposed gate against repeated unchanged-code runs.

**Never "retry until pass"** — rerunning benchmarks until they pass introduces selection bias. If a benchmark is flaky, fix the noise source (more iterations, dedicated runner, wider threshold) rather than retrying.

## System Tuning for Self-Hosted Runners

> **WARNING: These commands modify kernel and CPU settings. Apply them ONLY on dedicated CI runners, NEVER on developer machines or shared servers.**

When you control dedicated CI hardware, evaluate these settings and record them with results. They change the workload environment and may not represent production; architecture and kernel support vary.

### Disable CPU frequency scaling

Variable CPU frequency can add drift. Control or record it when comparing small differences:

```bash
# Request the performance governor (does not guarantee a fixed frequency or prevent thermal throttling)
echo performance | sudo tee /sys/devices/system/cpu/cpu*/cpufreq/scaling_governor
```

### Disable Turbo Boost

Turbo Boost temporarily increases clock speed but throttles under sustained load, creating variance between the start and end of a benchmark run:

```bash
# Intel
echo 1 | sudo tee /sys/devices/system/cpu/intel_pstate/no_turbo

# AMD
echo 0 | sudo tee /sys/devices/system/cpu/cpufreq/boost
```

### Pin benchmarks to specific CPU cores

Restricts execution to selected logical CPUs; migration is still possible within the set. Cache topology and SMT siblings vary, so select cores using the actual machine topology:

```bash
# Example only: choose CPUs appropriate for this host and reserve OS capacity separately
taskset -c 2,3 go test -bench=. -count=10 ./...
```

### Disable SMT (Hyper-Threading)

SMT shares execution units between logical cores on the same physical core, causing unpredictable contention:

```bash
# Disable SMT system-wide
echo off | sudo tee /sys/devices/system/cpu/smt/control

# Or disable individual sibling cores (check /sys/devices/system/cpu/cpu*/topology/thread_siblings_list)
echo 0 | sudo tee /sys/devices/system/cpu/cpu1/online  # if cpu0 and cpu1 are siblings
```

### Combined CI setup script

```bash
#!/bin/bash
# benchmark-setup.sh — run on self-hosted CI runner before benchmarks
set -euo pipefail

echo "=== Configuring CPU for stable benchmarks ==="
echo performance | sudo tee /sys/devices/system/cpu/cpu*/cpufreq/scaling_governor
echo 1 | sudo tee /sys/devices/system/cpu/intel_pstate/no_turbo 2>/dev/null || true
echo off | sudo tee /sys/devices/system/cpu/smt/control 2>/dev/null || true

echo "=== Running benchmarks on isolated cores ==="
taskset -c 2,3 go test -bench=. -benchmem -count=10 ./... | tee bench.txt
```
