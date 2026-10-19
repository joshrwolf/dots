# Establish a useful experiment

The reusable contract is methodological. Commands and fixtures depend on the
task; use existing Make targets, Go benchmarks, integration runners, or load
tools before writing a script. Add only the missing setup, execution, and
cleanup. Do not introduce a configuration language or universal metric schema.

## Workload contract

Record these in a short document in the target workspace:

- **Purpose:** user operation and why its cost matters; evidence that chosen
  inputs and frequencies represent relevant use.
- **State:** input sizes/distribution, concurrency or arrival rate, cache policy,
  external dependencies, and reset between runs.
- **Timing boundary:** startup, initialization, useful work, shutdown; explicitly
  identify what is included. Identify whether child-process costs count.
- **Correctness:** outputs, errors, exit status, ordering, cancellation, and side
  effects relevant to this operation. Identify gaps in existing tests.
- **Objective:** metric, units, aggregation, direction, and minimum worthwhile
  gain. Protect important memory, latency, throughput, or resource constraints.
- **Execution:** build, verify, measure, and diagnose commands; environment and
  toolchain; per-command deadlines and artifact locations.
- **Scope:** original baseline, editable code, fixed measurement assets, and
  time or candidate budget.

Prefer an objective with explicit constraints over a blended score that could
hide an unacceptable regression. Choose limits from user requirements and noise
measurements; do not invent universal percentage thresholds.

If production workload information is missing, use a clearly labeled proxy and
limit conclusions to it. A plausible benchmark is not evidence of production
impact. Ask for domain input when it would change what gets optimized.

## Fit the setup to the program

| Target | Typical execution | Checks that change the experiment |
| --- | --- | --- |
| CLI | Prepare files, invoke binary, inspect outputs | Startup inclusion, exit codes, child processes, file-cache state |
| Service/application | Start, wait for readiness, drive operations, stop | Offered load, errors, steady state, backend reset, resource accounting |
| Library | Run a focused benchmark matrix | Input diversity, setup outside timing, concurrency, optimizer-eliminated work |

Keep preparation and verification outside the timed interval unless they are
part of the intended user operation. For long-lived applications, measure over
enough time to expose GC, caches, and accumulated state. A short warmed run can
miss memory growth or startup costs that matter elsewhere.

For services, maintain comparable offered load and account for failures and
dropped work. A load generator that waits for completion can reduce demand
when the service slows. Document whether a closed or arrival-rate workload fits
the use case. Ensure the driver itself is not the bottleneck.

Use bounded readiness checks and reliable cleanup of owned processes even on
failure. Keep diagnostic endpoints local or within the existing access controls.
Keep fixture preparation deterministic enough to reproduce relevant conditions.

## Validate before searching

Run the baseline against itself using the intended comparison procedure. Inspect
spread and drift; lengthen runs, improve isolation, or narrow the objective when
the noise is larger than useful gains. Confirm the runner detects failed
commands and incorrect output. When practical, use a disposable deliberate
slowdown to check that the measured interval covers the operation being changed.

Pin build settings and inputs for comparisons. Record Go version, architecture,
CPU allocation, relevant runtime settings, and PGO configuration when applicable.
Use deployment-like hardware for final claims; local results have local scope.

Freeze measurement assets in an evaluator-controlled location outside the
optimizer's checkout. Record their revision or checksums. File separation and
hash checks detect accidental drift; they are not a security boundary against
an agent with access to both locations. Do not claim tamper-proof evaluation.
