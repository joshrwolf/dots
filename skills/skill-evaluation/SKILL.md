---
name: skill-evaluation
description: Evaluate skill discovery, delegated handoff, and output quality. Use when establishing a baseline, comparing skill revisions, or checking whether guidance improves real agent behavior without unnecessary work.
license: Apache-2.0
---

# Skill evaluation

Measure useful behavior, not compliance theater. Use the real agent host and its
native sessions; a few cases and independent checks usually suffice. Do not build
a runner, dashboard, or model client when existing tools cover the experiment.

This workflow adapts the [Agent Skills evaluation guidance](https://agentskills.io/skill-creation/evaluating-skills)
and [Anthropic's skill-creator](https://github.com/anthropics/skills/tree/dbd4588f9e1033efb41dad4bef2f7947c8993d44/skills/skill-creator).
Its grading and blind-comparison roles inform [judging](references/judging.md).

## Define the question

Separate these questions; one experiment need not test all of them:

- **Discovery:** does the agent consult useful guidance naturally? Do not name the
  skill in the task or force-load its body.
- **Effectiveness:** does access to the guidance improve the resulting work?
  Explicit consultation can isolate this from discovery.
- **Handoff:** does a parent select and pass useful guidance, and does its child
  receive and apply it? Inspect both sides.
- **Restraint:** does the agent avoid unnecessary reading, churn, and scope
  expansion, including on tasks that need no skill?

The primary baseline for a refactor is the current setup. A no-skill ablation is
optional and requires controlling the actual available catalog: omitting a path
from a brief does not hide an installed skill.

## Freeze before execution

Start with a few realistic prompts and neutral fixture inputs. Include a boundary
or near-miss case. Define observable outcomes and the judgment criteria before
looking at outputs; keep discovery criteria distinct from output correctness.
Reserve some unseen cases for the final check rather than tuning on everything.

Specify the task's functionality without spelling out the engineering preferences
being tested. Include choices the guidance can actually influence, and allow
materially different correct implementations. Goldens describe acceptable
behavior and justified tradeoffs, not one exact source output. An independent
author can derive them from user criteria without seeing candidate guidance or
outputs, with freedom to replace weak cases and challenge the rubric.

Keep case definitions, expected outcomes, and checks alongside the skill when they
are reusable. Keep machine-local captures, configuration, results, and session
references outside public repositories. Never snapshot credentials.

Record the skill/instruction revisions, catalog, host, model, effort, tool access,
and relevant environment. Retain exact rendered prompts and launch inputs;
freeze resource contents or record how pinned source revisions reconstruct them.
Hashes of mutable paths detect drift but do not reconstruct missing resources.
Freeze the current baseline before changing live
guidance. Later corrections get an explicit protocol revision; preserve attempts
already run instead of silently replacing inconvenient results.

## Execute in the host

Use fresh contexts and independent fixture copies. Fresh context does not imply
an empty skill catalog or absence of global instructions; verify what each run
actually receives. Keep model, effort, fixtures, and execution path matched
between configurations. Record unavoidable differences.

Use a session mode that stays alive until delegated work settles. Do not assume a
one-shot invocation awaits background children. A host lifecycle failure is not
evidence that the skill produced bad code.

Repeat enough cases to distinguish recurring behavior from a lucky run. Run order
and concurrency can affect cache state, timing, and cost; record them. Keep native
transcripts in their host storage and retain references rather than duplicate
transcripts. Instructed filesystem isolation is not a security sandbox.

## Check and judge

Use native deterministic checks for compilation, tests, file boundaries, formats,
and other mechanical properties. Validate new checks against a known-good result
and relevant deliberately broken results before trusting them. Run hidden checks
on separate acceptance copies after execution; do not coach the executor.
Keep `evals/`, assertions and hidden checks out of the executor's skill copy where
possible. Otherwise record reads of those materials as potential contamination;
normal skill discovery does not make evaluation expectations safe to expose.

Use a fresh read-only judge for output quality, then audit traces separately.
Follow [judging](references/judging.md). Read the actual artifacts and check
results, not just the executor's account of success.

Classify unavailable hosts/providers and corrupt setup separately from behavior.
Submitted code that does not compile, deadlocks, or times out is normally an
outcome failure, not an infrastructure excuse. Preserve invalid attempts and
explain exclusions. Unknown evidence stays unknown.

## Compare and improve

Compare the current and candidate outputs blindly where practical, using the same
frozen rubric and randomized A/B labels. Allow ties and no meaningful difference.
Only then reveal identities and inspect traces to investigate the cause.

Report quality, consultation, restraint, and cost separately. When instructions,
content, discovery or runtime features change together, attribute observations to
the whole bundle; isolate a component only when that causal question matters.
A working handoff verifies a feature, not improved output quality.

More reads are not success by themselves; passing tests do not prove good design.
If controls already satisfy the functional checks without guidance, investigate
whether the cases leave any guidance-relevant choices to measure. Quality
judgments may still distinguish results, but a functional ceiling limits claims
about effectiveness. Small suites and
single-run differences do not establish causation or statistical significance.
A set used repeatedly to select revisions is validation data, not an untouched
final test.

Generalize useful corrections instead of adding a rule for every failed example.
Keep what improves outcomes, cut guidance that merely adds work, and finish with
the reserved cases. State checks actually run, remaining uncertainty, and the next
decision; publish nothing externally without authorization.
