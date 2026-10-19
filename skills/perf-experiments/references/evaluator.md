# Independently evaluate a candidate

Own the experiment's credibility. Use the agreed contract and task-specific
commands; do not change the objective to accommodate a candidate. During setup,
read [experiment-design.md](experiment-design.md). Once validated, preserve the
measurement assets independently of optimizer edits.

## Receive and check

Receive the candidate patch or revision, parent identity, hypothesis, semantic
assumptions, and affected workloads. Treat the optimizer's speedup claim as
unverified. Obtain fresh measurements rather than reusing its selected runs.

Apply the candidate in a clean, isolated evaluation workspace. Confirm the
source being built, effective build flags, and contract identity. Inspect the
diff for changed tests, fixtures, benchmark detection, removed work, narrower
input handling, and changes to validation, caching, or resource limits. Such
changes need semantic justification even when existing tests pass.

Run the established correctness checks and add targeted verification for the
candidate's risks. Differential tests against the original implementation,
fuzzing, and race detection are useful when applicable. Passing tests establish
only the behavior they exercise; report remaining gaps.

Supplemental checks may exercise the existing correctness contract without
changing the frozen measurement procedure. Changes to accepted semantics or the
measurement procedure require a contract revision and rebaseline.

## Measure

- Reserve the measurement host through the coordinator. Avoid competing builds,
  tests, or benchmark processes; record material interference outside your control.
- Run baseline and candidate with the same environment and state/reset policy.
  Interleave repetitions in balanced or randomized order to reduce drift.
- Separate diagnostic profiling, tracing, race detection, and coverage from
  acceptance timing. Build comparable optimized binaries.
- Retain individual samples, failures, exact commands, and relevant metadata.
  Use existing statistics tools; do not write replacement statistical machinery.
- Require a practically useful effect supported by enough precision. A lack of
  statistical significance does not establish equivalence or acceptable regression.
- Inspect protected metrics and individual workloads, not just an aggregate.
  Fewer completed operations or skipped error cases cannot count as a speedup.

For Go benchmark output, use `benchstat`; its documentation recommends at least
ten measurements per benchmark as a starting point. More repetitions cannot fix
systematic bias or an unrepresentative workload. Preselect a measurement budget;
do not keep sampling only until a favorable significance result appears.

Repeated candidate selection creates lucky winners. Confirm finalists using
fresh measurements and held-out representative cases where possible. Compare
both against the parent when useful and against the original baseline for total
gain. Recheck combinations of individually accepted changes.

## Return a verdict

- **Supported improvement:** correctness gates passed, the objective improved
  meaningfully, and protected metrics remain within their limits.
- **Rejected:** correctness failed, a protected limit was exceeded, or the
  evidence supports no worthwhile improvement within the agreed precision.
- **Inconclusive:** uncertainty is too large to resolve acceptance; explain
  whether more measurement would be worthwhile within budget.
- **Invalid experiment:** setup, environment, contract drift, or execution failure
  prevents comparison. Identify the cause without mislabeling it a slowdown.

Return the verdict with patch/parent and contract identities, correctness
results, before/after values with uncertainty and sample counts where available,
artifact paths, and limitations. A profile explaining the effect is useful
supporting evidence; a lower percentage in a profile is not itself a speedup.

Reference: [benchstat documentation](https://pkg.go.dev/golang.org/x/perf/cmd/benchstat).
