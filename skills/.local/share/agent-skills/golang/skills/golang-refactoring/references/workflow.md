# Refactoring workflow

## Establish scope

Read the request, project instructions, and working-tree diff. Preserve unrelated
changes. Use existing session decisions; an authorized refactor does not need a
second approval just because it crosses package boundaries or removes obsolete code.
Ask when an unresolved behavior, compatibility requirement, or scope choice would
materially change the implementation. Continue independent work while clarifying.

Map relevant definitions, references, callers, and package dependencies with gopls
and repository searches. Inspect reflection, tags, generated code, and external
consumers where applicable. For a large change, record the affected components,
dependencies, risks, and verification steps. For a small rename, a short plan is enough.

## Sequence coherent changes

Keep structural changes distinguishable from behavioral changes. Move or rename
before optimizing, and migrate dependencies before their consumers. Choose stages
that leave understandable diffs and can be verified. A single coherent change may
fit one commit; do not impose one PR, worktree, or agent per operation.

Use the session's collaboration and Git workflow. Decide whether independent
caller analysis, separate package migrations, or a focused review would benefit
from subagents. Delegate when that benefit outweighs coordination overhead; handle
small or tightly coupled changes directly. Give each agent a bounded task, avoid
concurrent writes to overlapping files or symbols, and integrate and verify results. Create branches, commits, and PRs only
within the authorized workflow. Local implementation does not imply permission to
publish, merge, or message other people.

If a migration needs temporary compatibility code, document its removal condition.
Remove it when the migration finishes; do not introduce shims or TODO markers
solely to satisfy a staged workflow.

## Verify the affected behavior

Use [safety-net.md](safety-net.md) to identify behavior needing coverage and
[go-tooling.md](go-tooling.md) to select an appropriate transform. Test affected
packages and consumers, widening checks when package boundaries or shared APIs
change. Run project-required checks. Add race checks for changed concurrency and
measure hot-path optimizations against a baseline.

When coverage is missing, add focused characterization tests where they protect
important behavior. Missing coverage calls for investigation and verification,
not an automatic user approval prompt. Report any remaining verification limits.

## Recover and finish

Inspect failures before retrying. Correct your own edits or undo the specific
failed transformation without overwriting unrelated work. A dirty tree does not
justify a reset or an unsolicited checkpoint commit.

Review the final diff for scope, obsolete references, and temporary scaffolding.
Report behavior changes, checks performed, and unresolved limitations. Commit or
publish only as requested or already authorized by the session and repository rules.
