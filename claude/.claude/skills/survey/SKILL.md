---
name: survey
description: "Hunt for architectural problems in a codebase — structural friction, unnecessary complexity, broken abstractions, coupling. Surfaces actionable findings that feed into /architect."
---

# survey

Hunt for architectural problems in a codebase. The output is a list of
findings — things that are wrong, inefficient, or fighting the domain — each
one a potential `/architect` session.

Read [design-philosophy.md](../architect/references/design-philosophy.md)
before starting. It defines the principles that determine what counts as a
problem.

## Process

1. **Scope.** If the user specified a package, directory, or area, start
   there. Otherwise start from module boundaries, package structure,
   dependency graph, and entry points.

2. **Explore.** Read code. Use LSP for navigation. Follow the dependency
   graph. Check git log for churn — files that change together are coupled
   whether the type system says so or not. Read tests — their shape reveals
   the real API boundaries and where they're fighting the implementation.

3. **Hunt.** Look for problems:

   - **Wrong abstractions** — interfaces that don't match how callers use
     them. Wrapper types that add ceremony but no value. Generics or
     indirection introduced for a single concrete case.
   - **Missing boundaries** — two or more concerns tangled in the same
     package or function. Business logic mixed with transport. Configuration
     mixed with runtime state.
   - **Unnecessary coupling** — packages that import each other or share
     types when they shouldn't need to. Types that change together across
     package boundaries (git evidence).
   - **Complexity without justification** — functions or types that are
     complex not because the problem demands it but because the structure
     is wrong. Long functions that should be decomposition points, not
     sequential steps.
   - **Shallow modules** — packages where the interface is nearly as complex
     as the implementation. Many small exported types that could be one deep
     module with a simple API.
   - **Convention drift** — areas that follow different patterns than the
     rest of the codebase without clear reason.
   - **Dead weight** — exported symbols with no external callers. Packages
     that exist but are barely used. Compatibility shims for migrations
     that already completed.
   - **Inverted dependencies** — core domain packages depending on
     infrastructure. High-level policy importing low-level detail.

4. **Report.** Present each finding as:
   - **Problem** — what is wrong, with concrete code evidence (`path:line`
     references, type names, function signatures).
   - **Cost** — what this problem makes harder. Slower to change, harder to
     test, easier to introduce bugs, forces unnecessary coordination.
   - **Thread** — a copy-pastable `/architect` prompt in a fenced code block.
     It should describe the problem with enough context (package names, type
     names, file paths, the specific friction) that `/architect` can start
     exploring immediately without re-discovering the issue. Do not suggest
     a solution — describe the problem and its boundaries.

## Rules

- Be opinionated. If something is wrong, say it's wrong and why. Do not
  hedge with "this might be fine depending on context."
- Be specific. "This package is too complex" is not a finding.
  "`pkg/auth` has 14 exported types but only `Verifier` and `Claims` are
  used outside the module — the rest are implementation detail leaking
  through the API" is a finding.
- Do NOT propose solutions. That's what `/architect` is for. Describe the
  problem and its cost, not the fix.
- Do NOT rank or prioritize. The user decides what matters — present
  findings in exploration order.
- Keep each finding to 3-5 lines.
