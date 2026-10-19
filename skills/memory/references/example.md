# Example: a corrected project decision

A small scope might have these files. The names and evidence are illustrative,
not claims about a real project. Omit sections that add no useful information.

## MEMORY.md

```markdown
# Project memory

- 2026-10-19: Streaming exports replace buffered exports for large datasets; see [export strategy](wiki/export-strategy.md).
```

## wiki/export-strategy.md

```markdown
# Export strategy

## Current understanding

Use streaming for large exports. Keep buffering for the small preview endpoint.
This decision applies to the export API, not every response handler.

## Why

Buffering the entire export exceeded the agreed memory budget in the large-data
check. Streaming stayed within that budget. Preview responses remain bounded
and do not need the additional lifecycle handling.

## Evidence

- [Export checkpoint](../sources/2026-10-19-export-check.md): the observed check
  and the user's accepted decision.

## Changed understanding

2026-10-19 supersedes the earlier recommendation to buffer all exports. That
recommendation was based on small inputs and did not establish large-input safety.

## Uncertainty

Client disconnect cleanup has not been verified; do not describe it as solved.
```

## sources/2026-10-19-export-check.md

```markdown
# Export checkpoint — 2026-10-19

User decision: stream large exports; retain buffered previews.

Observation: the large-data check exceeded its memory budget with buffering and
passed with streaming. In a real checkpoint, link the exact commit and check
artifact and state the tested input and budget. Do not fabricate missing details.

Not tested: client disconnect during streaming.
```

## log.md

```markdown
# Knowledge changes

- 2026-10-19: Revised [export strategy](wiki/export-strategy.md) after the large-data check; kept preview exception and disconnect uncertainty explicit.
```

## Cold-recall checks

A new session should answer:

- “Why do exports stream?” with the rationale and evidence link.
- “Should previews stream too?” with the explicit exception.
- “Buffering was the recommended approach, right?” with the superseding decision,
  not agreement with a stale premise.
- “Is disconnect cleanup verified?” with “not yet,” not a claim inferred from
  the successful memory-budget check.

These questions exercise the workflow. They are not proof of model behavior
until actually tried in a fresh context.
