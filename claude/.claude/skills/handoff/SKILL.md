---
name: handoff
description: "Generate a structured handoff prompt for continuing work in a new session. Captures context, decisions, traps, and remaining work without creating files."
---

# handoff

Generate a handoff prompt that a new Claude Code session can use to continue
this work. The prompt is emitted as text in the conversation — no files are
created.

## When to invoke

- The current session is approaching its useful limit.
- Work remains that is tangential to or follows from the current task.
- The user explicitly asks for a handoff.

## What to produce

Emit a single fenced code block (```markdown) containing a ready-to-paste
prompt with exactly four sections:

### 1. Context

State what was accomplished as declarative facts. Reference specific files,
functions, and commit SHAs where relevant. Do not summarize the conversation —
summarize the *outcome*.

### 2. Decisions

List architectural and design decisions made during this session with their
rationale. Each entry: **what** was decided, **why**, and **what was rejected**.
This is the most important section — a new session without it will re-derive
these decisions from scratch and may choose differently.

### 3. Traps

Approaches that were tried and failed, or that look appealing but don't work.
Each entry: **what** was tried, **why it failed**. This prevents the new
session from wasting context re-discovering dead ends.

### 4. Remaining

What is left to do, stated as status ("X is half-implemented", "Y needs
tests") not as instructions ("implement X", "write tests for Y"). The new
session's user will decide what to prioritize — the handoff describes the
landscape, not the plan.

## Rules

- Do NOT repeat anything already in CLAUDE.md or project-level CLAUDE.md
  files. Reference them ("see CLAUDE.md for commit conventions") instead.
- Do NOT include file contents. Use `path:line` references.
- Keep total length under 400 lines. Compress aggressively — the new session
  can always read files itself.
- End the prompt with: "Review this context, then wait for instructions."
  This prevents the new session from executing autonomously on stale intent.
- Do NOT editorialize or include meta-commentary about the handoff itself.
