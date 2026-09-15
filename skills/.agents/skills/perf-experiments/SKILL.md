---
name: perf-experiments
description: Run a bounded performance optimization campaign with repeated profile-driven experiments and separate optimizer/evaluator roles. Use when the user requests an iterative or semi-unattended optimization hunt, or names perf-experiments. Do not select for ordinary performance fixes, one-off profiling, benchmark authoring, or performance questions.
---

# Bounded performance experiments

Use the existing coding-agent session to coordinate the work. Reuse project
tooling and create task-specific measurements where needed. Do not build a
generic harness, model API integration, or agent controller for this workflow.

## Establish the experiment

Read [references/experiment-design.md](references/experiment-design.md) when
establishing or revising a workload. Discover existing tests, benchmarks,
profiling support, and performance requirements before adding instrumentation.

Produce a short experiment contract: representative operation and inputs,
correctness checks, objective and regression limits, exact commands, initial
state, measured interval, and stopping budget. Resolve these from available
context first. Ask the user only for consequential missing domain information,
such as whether startup or steady-state latency matters. Continue independent
setup while waiting; do not optimize against an unresolved objective.

Make the contract concrete and reviewable before unattended optimization.
Honor an already-authorized workload without introducing another approval gate.
Validate measurement on unchanged code, then freeze the contract and evaluator
assets for the run. Corrections require a recorded contract revision and new
baseline; they cannot silently redefine an existing win.

## Coordinate the roles

Use native subagents when available and permitted. The main session is the
coordinator; delegate these bounded roles with explicit checkout ownership:

- **Evaluator:** establish and validate task-specific measurements; independently
  check candidate patches. Give it the contract and
  [references/evaluator.md](references/evaluator.md).
- **Optimizer:** inspect profiles, propose a hypothesis, and implement one
  coherent candidate. Give it the frozen contract, baseline artifacts, editable
  scope, and previous experiment outcomes. For Go, give it
  [references/go-profiling.md](references/go-profiling.md).

During evaluator setup, the optimizer can inspect code and existing profiles.
Begin candidate experiments once the contract is established. Start with one
optimizer; add more only for independent investigations that justify the cost.
Workers must not delegate the coordinator role or spawn recursive teams.

The coordinator tracks the original baseline, current best candidate, budget,
and open uncertainties. The optimizer owns candidate source edits; the evaluator
owns measurement assets and its clean evaluation workspace. Do not let workers
edit the same checkout. Patches may be handed off without committing when
commits are outside the authorized scope.

If native delegation is unavailable or not permitted, perform the roles sequentially and disclose
that evaluation was not by an independent agent. Do not install an agent runtime
or call model APIs to manufacture delegation.

## Iterate with evidence

1. Profile the agreed workload and identify a bottleneck relevant to the objective.
2. State what the candidate should improve, why, and which semantics are at risk.
3. Make the change in the candidate workspace and run focused checks.
4. Hand the evaluator a patch or revision, parent identity, hypothesis, relevant
   assumptions, and affected workloads. Optimizer timings are exploratory.
5. Accept only improvements supported by fresh execution of the frozen checks
   in the evaluation workspace, by the evaluator agent when available. Record
   rejected, inconclusive, and invalid experiments too.

Serialize performance measurements on a host. Pause competing builds, tests,
and workloads under the team's control during measurement; text investigation
can continue. Re-evaluate combined candidates because their effects may overlap.
Confirm finalists with fresh runs against the original baseline and additional
representative inputs where available.

## Persist and finish

Keep a compact local progress note and experiment ledger: contract identity,
parent and candidate identity, hypothesis, verdict, artifact paths, and next
step. Use an ignored results directory or an external scratch directory for
raw measurements, profiles, binaries, and logs. Never commit live captures;
use neutral, hand-authored fixtures for reusable examples. Preserve existing
user changes and retain enough evidence to reproduce reported results.

Follow the user's time, experiment, and resource budget. Define counting in the
contract: by default each distinct candidate, including a combination, consumes
one slot; retries and confirmation measurements use the remaining time/resource
budget without adding candidate slots. Reserve capacity for final confirmation.
If unspecified, start
with at most three candidate evaluations and report before extending the run.
Stop earlier when the objective is met, measurements are too noisy to resolve
useful gains, or progress needs domain information. This instruction is a
best-effort session limit; use task-specific command timeouts for hung processes.

Finish with the reviewable patch, workload scope, correctness evidence,
before/after measurements, tradeoffs, and unresolved limitations. Report no
verified improvement when that is the result. Publishing, messaging, and merging
follow the user's existing authorization; this skill grants none of them.
