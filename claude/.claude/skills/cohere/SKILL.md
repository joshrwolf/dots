---
name: cohere
description: "Restore conceptual integrity to code written by many hands (or many agents): unify vocabulary, collapse parallel abstractions, converge on one pattern per concern. Inventory → convergence plan → hard approval gate → execute. Invoke with a scope and optional churn budget (surface | structural | foundational)."
argument-hint: "[scope] [budget: surface | structural | foundational]"
---

# cohere

> *"Conceptual integrity is the most important consideration in system design.
> It is better to have a system reflect one set of design ideas than to have one
> that contains many good but independent and uncoordinated ideas."*
> — Fred Brooks, The Mythical Man-Month

Code assembled by multiple independent agents works, but reads like a committee
wrote it: the same concept has three names, every module boundary reshapes data,
each "author" brought their own error style. That's Conway's Law — the code
mirrors the communication structure of whoever built it, and subagents don't
talk to each other.

This skill hunts the seams and converges the code onto one design center — the
code one mind would have written. It is a **whole-scope comparative analysis**,
not a local review: gofix judges a function against an external standard;
cohere judges files against *each other*. Incohesion is invisible one file at
a time.

## Churn budget

The argument sets how much rewriting is funded. Default: **structural**.
Every finding gets tiered under the smallest budget that could fund it, so a
bigger budget strictly adds work, never changes it.

- **surface** — unify naming vocabulary, error/log message style, comment
  register, test structure. No signature changes.
- **structural** — collapse parallel abstractions into one owning type, merge
  duplicated helpers, converge on one pattern per concern (one config idiom,
  one constructor style, one error strategy). Internal API churn allowed;
  public API and package boundaries stay.
- **foundational** — greenfield mode. Redraw package boundaries around the
  actual concepts and rebuild toward the single design center. Refactoring
  cost is not a consideration; compatibility is not a consideration.

## Phase 1: Inventory

Resolve the scope (default: the current repo or package the user is working
in). Then launch three agents in parallel over it. Each **reports findings
only** — no edits. Findings must cite locations and counts, not impressions.

### Agent 1: Vocabulary

Build the concept inventory. Enumerate the domain nouns and where each appears.
Flag:

- **Synonyms** — one concept, multiple names (`job` / `task` / `workItem`).
  For each, list every name, its file locations, and occurrence counts.
- **Homonyms** — one name, multiple meanings across packages (a `Client` that
  is an HTTP wrapper here and a domain aggregate there).
- **Verb drift** — the same operation named differently (`Get`/`Fetch`/`Load`/
  `Retrieve` for identical semantics).

### Agent 2: Concerns

For each cross-cutting concern, list every variant found with counts and
locations: error handling and wrapping style, construction (validating
constructor vs bare struct vs options), configuration, logging, context
propagation, test structure, file/package layout conventions. A concern with
one variant is healthy — skip it. Two or more variants is a finding.

### Agent 3: Seams

Find where the task-decomposition boundaries show through:

- **Parallel abstractions** — half-overlapping types or helpers where one type
  should own the concept.
- **Boundary reshaping** — data converted or adapted at module borders because
  each side invented its own shape.
- **Private utility layers** — each module carrying its own `parseX`/`formatY`
  scaffolding that duplicates a sibling's.
- **Register shifts** — comment density, defensiveness, or abstraction level
  changing sharply at file boundaries, marking a different "author".

## Phase 2: Convergence plan — hard gate

Merge the findings into one plan. For each item:

- **What**: the concept or concern, with variants, locations, and counts.
- **Canonical pick**: which variant wins — usually the best-designed or
  best-represented one already in the codebase — and one line on why.
- **Migration**: what converges onto it, and the churn estimate.
- **Tier**: the smallest budget that funds it.

Present the plan filtered to the requested budget, with one line noting what a
bigger budget would unlock. Then **stop**.

**Do not edit anything until the user approves.** They may approve the whole
plan or cherry-pick items. No approval, no edits — this gate is not optional,
regardless of how confident the plan is.

## Phase 3: Converge

Apply approved items only — nothing that was skipped, nothing "while you're
here". Go work requires loading `gotools` and `idiomatic-go` first, per global
rules. Convergence is mechanical once the canonical pick is made: rename,
merge, delete the losing variants. Verify build and tests against the final
state.

## Phase 4: Constitution

Offer to distill the converged state into a **design constitution** appended to
the project's `CLAUDE.md` (or `.claude/CLAUDE.md`), so every future agent and
subagent inherits it and the drift doesn't recur:

- the vocabulary table (concept → canonical name)
- one canonical pattern per concern, each a single line
- one exemplar file to imitate

Keep it under ~30 lines. It's a contract, not documentation — every line must
be a rule an agent could violate.
