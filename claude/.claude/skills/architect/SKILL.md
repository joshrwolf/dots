---
name: architect
description: "Two-phase architectural design and execution. Design phase: explore, propose one target architecture, stress-test it. Execute phase: faithful implementation from the agreed spec with no hedging or detours."
---

# architect

Architectural work proceeds in two phases: **design** then **execute**. Never
skip design. Never blend them — finish one before starting the other.

Read [design-philosophy.md](references/design-philosophy.md) before starting.
It defines what "good architecture" means — proposals and evaluations are
grounded in those principles.

---

## Phase 1: Design

The goal is to arrive at one target architecture. Not a menu of options. Not
an incremental patch from the current state. The ideal end state, then work
backwards to figure out how to get there.

### Process

1. **Explore.** Read the relevant code. Understand the current structure,
   its constraints, and where it creates friction. Use LSP and grep, not
   assumptions.

2. **Propose.** Present one recommended target architecture. Include:
   - The target structure (files, types, boundaries, data flow)
   - Why this structure is better than alternatives you considered
   - What gets harder (every design has trade-offs — name them)
   - A migration path from current state to target state

3. **Stress-test.** Ask hard questions about the proposal — one at a time.
   Challenge assumptions. Surface edge cases. If the user pushes back, engage
   with the substance rather than retreating to a safer option. The goal is
   to find problems now, not during execution.

4. **Write the spec.** Once aligned, write a concise spec to a file within
   the project. Choose a location that fits the project's conventions (e.g.
   `docs/`, `.claude/`, a design directory if one exists — use judgment).
   This file is the execution anchor — not the conversation history. Include:
   target structure, migration steps, boundaries (what is and isn't in
   scope), and any constraints surfaced during stress-testing.

### Design principles

- **Architecture-first.** Do not let the current implementation constrain the
  target design. Refactoring cost is not a factor in choosing the right
  architecture — it is a factor in planning the migration path.
- **One recommendation.** Pick the best approach and commit to it. Explain
  why it's better than alternatives. Do not present options for the user to
  choose between unless genuinely indifferent.
- **Name the trade-offs.** Every design makes something harder. If you can't
  name what gets worse, you haven't thought hard enough.
- **Concrete, not abstract.** Name files, types, functions. "A service layer"
  is not a proposal. "`pkg/signing/service.go` implementing `Signer` with
  methods `Sign`, `Verify`, `Rotate`" is a proposal.

---

## Phase 2: Execute

The spec is written. Alignment is reached. Now implement it.

### Process

1. **Reference the spec.** Read the spec file at the start of execution.
   Every implementation decision traces back to the spec. If something isn't
   in the spec, it isn't in scope.

2. **Implement.** Make the changes. The final state is what matters —
   idiomatic, correct, and matching the spec.

3. **Verify.** Run tests, type checks, and builds against the final state.
   Fix issues in the final state, not in intermediate states.

### Execution rules

These are non-negotiable during execution:

- **No hedging.** Do not ask "are you sure?" or "should I proceed?" after
  alignment is reached. The design phase was for questions. This phase is for
  answers.

- **No linter detours.** Intermediate states during a refactor do not need to
  pass linters, compile, or satisfy type checkers. Only the final state must
  be clean. Do not take indirect paths through the refactor to keep
  intermediate states green.

- **No scope creep.** If you notice something outside the spec that should be
  fixed, note it and move on. Do not fix it. Do not "clean up while you're
  here." The spec defines the boundary.

- **No incremental safety theater.** Do not commit after every small change
  "in case something goes wrong." Do not create backup files. Do not add
  temporary compatibility shims. Implement the target state directly.

- **No rationalization.** If you feel the urge to deviate from the spec,
  that is a signal to stop and surface the conflict — not to quietly adjust
  the plan. Common rationalizations and their counters:

  | Rationalization | Counter |
  |---|---|
  | "This is a small improvement while I'm here" | It's not in the spec. Note it for later. |
  | "The linter/compiler will complain" | Intermediate states don't matter. Fix it in the final state. |
  | "Let me add a compatibility shim for safety" | The spec says replace, not wrap. |
  | "I should commit this progress first" | Commit the completed work, not the work-in-progress. |
  | "This dependency should be updated too" | Out of scope unless the spec requires it. |
  | "Let me refactor this helper while I'm touching it" | Scope creep. Note it for later. |
  | "I'll test it all at the end" | Tests are part of the final state, not a post-hoc step. Write them as you go if the spec includes them. |

### When to break the rules

If you discover during execution that the spec is wrong — a constraint that
wasn't visible during design, a dependency that makes the target state
impossible — **stop and say so.** Do not silently adjust. Do not work around
it. Surface the conflict, propose a spec amendment, and wait for alignment
before continuing. This is the only valid reason to pause execution.
