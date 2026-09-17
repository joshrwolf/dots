# Structural Constraints: Import Cycles, Package Boundaries, Type Moves, API Evolution

Go enforces a handful of structural rules at compile time that other languages leave to convention or linting. This file covers import cycles, package boundaries, moving declarations with their callers, and identifying real compatibility requirements when changing a published API.

## Table of Contents

- [Breaking Import Cycles](#breaking-import-cycles)
  - [1. Consumer-side interface (dependency inversion)](#1-consumer-side-interface-dependency-inversion)
  - [2. Extract shared types to a new/lower package](#2-extract-shared-types-to-a-newlower-package)
  - [3. `internal/` packages](#3-internal-packages)
  - [4. Mediator/bridge package](#4-mediatorbridge-package)
- [Package Boundary Design](#package-boundary-design)
  - [Splitting a god package](#splitting-a-god-package)
- [Moving Types Across Packages](#moving-types-across-packages)
- [Exported API Surface and Versioning](#exported-api-surface-and-versioning)
- [`init()`, Global State, and Package-Level Vars as a Refactoring Target](#init-global-state-and-package-level-vars-as-a-refactoring-target)
- [Generics — When a Refactor Toward Them Is Warranted](#generics--when-a-refactor-toward-them-is-warranted)
- [Common Mistakes](#common-mistakes)
- [Cross-References](#cross-references)

## Breaking Import Cycles

- Go compiles packages leaf-to-root in dependency order: before compiling package `X`, the compiler must have already finished compiling everything `X` imports, because it needs their compiled type information to type-check `X`.
- An import cycle — `X` imports `Y`, `Y` imports `X` (directly or transitively) — has no valid compilation order, so `go build` rejects it outright as `import cycle not allowed`. This is not a style preference; there is no fallback behavior to fall back to.

Four strategies fix a cycle, in preference order:

| # | Strategy | Call-site cost | Best when |
| --- | --- | --- | --- |
| 1 | Consumer-side interface | Often unchanged direct calls; function types and construction may need edits | The consumer only calls one or two methods on the producer's type |
| 2 | Extract shared type to a leaf package | Import path changes on both sides | Both packages genuinely need the _same concrete type_, not just its behavior |
| 3 | `internal/` package | Import path changes for the shared code only | The shared code should never become part of the public API |
| 4 | Mediator/bridge package | New package, both sides delegate to it | 1–3 don't fit the shape of the coupling |

### 1. Consumer-side interface (dependency inversion)

- A narrow interface can preserve direct call syntax, but changing a parameter type changes the function type and may affect assignments, wrappers, and construction.
- If package `x` only _uses_ behavior from package `y` — it calls a method or two on `y`'s concrete type, it doesn't need the type itself — define a small interface in `x` naming just those methods.
- Go's implicit interface satisfaction means `y`'s existing type already satisfies that interface without `y` importing anything from `x` or even being aware `x`'s interface exists:

```go
// package x (consumer)
type Storer interface {
    Store(ctx context.Context, key string, val []byte) error
}

func Process(s Storer) error { /* uses s.Store, no import of package y */ }
```

```go
// package y — unchanged, already satisfies x.Storer implicitly
type Store struct{ /* ... */ }
func (s *Store) Store(ctx context.Context, key string, val []byte) error { /* ... */ }
```

- `x` no longer imports `y` for this dependency; `y` may continue importing `x`. That removes one edge from the original cycle, provided the interface signatures do not still mention y-owned types.
- The cycle is gone because one direction of the dependency graph was never real — `x` never needed `y`'s concrete type, only a name for the behavior it called.

### 2. Extract shared types to a new/lower package

- Works because Go's package model is flat within a module — a nested subdirectory is still a fully distinct, independently importable package — so pulling the types both `x` and `y` need into a small new leaf package that both can import breaks the cycle by construction: the leaf package imports neither.
- Be honest about the cost: in a real codebase a single cycle can span five or more packages once you trace every type both sides share, so this is the "correct but sometimes painful" option next to the surgical, call-site-free consumer-side interface above.

### 3. `internal/` packages

- Share code between related packages without widening the public API.
- Anything rooted under an `internal/` directory is importable only by packages rooted at the parent of that `internal/` directory — this lets two sibling packages share implementation detail through a common `internal/` package without either becoming part of the module's public surface, and without pulling either sibling into the other.

### 4. Mediator/bridge package

- A last resort when 1–3 don't fit the shape of the dependency: a new package holding the shared functionality that both `x` and `y` delegate to, absorbing the coupling neither side wants to own.
- Reach for this only after confirming a consumer-side interface can't express the relationship — it usually can.

An `internal/` layout for strategy 3 looks like this — note that both `billing` and `shipping` can import `order/internal/model`, but nothing outside the `order` tree can:

```
order/
├── billing/
│   └── billing.go        // imports order/internal/model
├── shipping/
│   └── shipping.go        // imports order/internal/model
└── internal/
    └── model/
        └── order.go        // shared type, invisible outside order/
```

## Package Boundary Design

- **Choose parameter and return types for the public contract.**
  - Accepting a narrow interface as a parameter maximizes what a caller can pass — including a test fake that implements only the one or two methods the function actually calls — without the function ever needing to import the caller's concrete types.
  - Returning a concrete struct preserves full type information at the call site, which matters because _that_ caller's own consumers get to define their own narrow interface later, on their side, without the original producer ever having had to anticipate what subset of behavior a future caller would need.
- **Use consumer-owned interfaces for narrow dependency seams.** Package-owned public interfaces are also appropriate when they intentionally hide implementation details. Follow repository API conventions.
  - A consumer package declares the interface naming only the methods it calls; the producer package never imports it, never knows it exists, and satisfies it purely because Go's interface satisfaction is structural.
  - This is exactly the mechanism in the consumer-side-interface fix above — it isn't a separate rule, it's the same rule applied proactively during design instead of reactively during a cycle break.
- An interface return can preserve a deliberately private implementation; a concrete return is useful when callers need that concrete API.
- **Caveat: this is a heuristic, not dogma.**
  - Don't mechanically split every struct into an interface-plus-implementation pair on the theory that it's "more testable" — a concrete struct with no interface is simpler to read, and an interface with exactly one implementation and no test-double need is pure indirection.
  - Don't return an interface from a constructor just because "it might be more flexible later" — that flexibility has a name (YAGNI) and a cost (the caller loses type information it might have wanted).
  - → See `samber/cc-skills-golang@golang-project-layout` skill for the directory/package layout conventions this section assumes, and `samber/cc-skills-golang@golang-design-patterns` skill for judging when introducing an interface is the right call versus premature abstraction.

### Splitting a god package

- Before reaching for a full package split, gopls's `refactor.extract.toNewFile` code action handles the lighter-weight case: moving a top-level declaration to a new file in the _same_ package, which is often enough to make a bloated package navigable without touching its import graph at all.
- gopls also has an experimental `source.splitPackage` code action that opens a browser visualization of package components and their dependencies. It does not rewrite files; use the graph to plan boundaries and then perform the moves explicitly. See the [official web features reference](https://go.dev/gopls/features/web#sourcesplitpackage-split-package-into-components).

## Moving Types Across Packages

Move the declaration, update imports and references across affected callers, and
remove the old declaration. Check package cycles, method ownership, generated code,
reflection, and external consumers. Build and test affected packages together.

Do not leave compatibility aliases or forwarding wrappers for ordinary refactors.
An alias is a tool for an explicitly required compatibility contract, not a mandatory
migration stage. Resolve real external-consumer requirements before changing a
published API; do not invent compatibility requirements for internal moves.

## Exported API Surface and Versioning

- **Follow the actual published compatibility contract.** When a supported public API requires a deprecation period, a doc comment beginning `// Deprecated: ...` is recognized by tooling and IDEs and surfaces as a strikethrough or warning at every call site, which gives callers time to migrate before the symbol disappears — deleting an exported identifier outright breaks every downstream module at their next `go build` with no warning beforehand.
- **Version published APIs according to their compatibility contract.** Internal refactors do not need additive APIs or deprecation scaffolding. For supported external consumers, assess the impact of both additions and removals before choosing a migration and release.
- **Semantic import versioning** is how Go expresses that: a v2+ module carries a `/vN` suffix in both its module path and every importer's import path (e.g. `example.com/mod/v2`).
  - This is what lets v1 and v2 of the same module coexist in the same build — the two are, to the toolchain, simply different packages with different import paths.
  - It's what lets callers migrate one package at a time rather than all at once, the same gradual-migration property a type alias gives you _within_ one version, now applied _across_ major versions.
  - During the transition, the v2 implementation can be written as a thin wrapper over v1 (or vice versa, whichever side holds the canonical logic) to avoid maintaining two divergent copies of the same behavior.
- **`retract` directives** mark an already-shipped defective version as unfit for use in `go.mod` — `go get` and `go list -m -u` surface the retraction to anyone who depends on it, without requiring the broken version to be deleted from the module proxy:

```
module example.com/mod

go 1.24

retract (
    v1.2.0 // published with a data-loss bug, see #123
    [v1.2.1, v1.2.3] // range retraction — a whole span of bad releases
)
```

## `init()`, Global State, and Package-Level Vars as a Refactoring Target

- `init()` ordering and mutable package-level state are a common source of hidden coupling: a caller of a function has no way to see, from the call site, that the function's behavior depends on some other package's `init()` having already run, or on a global variable some unrelated code path mutated earlier in the program's lifetime.
- That coupling doesn't show up in a signature, so it doesn't show up in a diff, and it's exactly the kind of dependency that makes a piece of code unsafe to move or test in isolation.
- The refactor is toward explicit construction: a constructor function that returns a struct, with dependencies passed in as parameters rather than reached for through a package-level variable or `init()`-populated singleton.
  - This doesn't remove the dependency — the code still needs what it needed before — it makes the dependency an explicit, visible seam in the function signature instead of an implicit one buried in the package's `init()`.
  - → See `samber/cc-skills-golang@golang-design-patterns` skill for constructor and dependency-injection patterns, and [safety-net.md](safety-net.md) in this skill for how this same seam is what a Feathers-style characterization test exploits to get coverage before a risky change.

## Generics — When a Refactor Toward Them Is Warranted

- Narrower case, briefly: introduce a type parameter only when the _logic_ is genuinely identical across types, not merely similar.
- When the _behavior_ differs per type — even by one branch — that's an interface, not a generic; a generic with a type switch inside it is usually an interface wearing a disguise.
- A practical litmus test: reach for a generic when a type parameter would eliminate a type assertion that's currently in the code, and the constraint it needs stays narrow — `comparable`, `cmp.Ordered`, or a small one-method interface.
- Write the concrete version first; refactor to generic only once the duplication is real and already committed in two or more places, not anticipated for a future third caller that may never arrive.
- As of this writing, Go has no method-level type parameters, which blocks fluent generic method chaining (`Map` returning a differently-typed receiver) as a design option — plan around that limitation rather than discovering it mid-refactor.
- Verify a generics migration the same way as any other refactor, no special-casing: `go build ./... && go vet ./...` plus the full test suite for the touched packages.

## Common Mistakes

| Mistake | Fix | Why |
| --- | --- | --- |
| Confusing a defined type with an alias | `type A B` creates a distinct type; `type A = B` preserves identity | Ordinary moves update callers and remove the old declaration; aliases are only for an explicit compatibility contract |
| Breaking a cycle by moving the _producer's_ concrete type into the consumer's package | Define the interface in the consumer instead, leave the producer's type where it is | Moving the concrete type usually just relocates the cycle to whatever else the producer's type depends on |
| Assuming exported always means externally supported | Check actual consumers and compatibility requirements | Internal moves need no automatic deprecation stage; published contracts may |
| Bumping a module to v2 without adding `/v2` to the module path | Add the `/v2` suffix to both `go.mod`'s `module` line and every import path | Without the suffix, Go's module resolution can't tell the new major version apart from the old one, and a module-aware v2 tag without the required suffix is generally invalid, rather than silently upgrading existing v1 importers |
| Reaching for a generic the first time a second, similar-looking function appears | Wait for a third real occurrence with identical logic, or an existing type assertion the generic would remove | Two occurrences are often coincidentally similar rather than logically identical; a premature generic ossifies an abstraction around a coincidence |

## Cross-References

- → See `samber/cc-skills-golang@golang-project-layout` skill for directory/package layout conventions.
- → See `samber/cc-skills-golang@golang-design-patterns` skill for when an interface is the right design choice versus premature abstraction, and for constructor/DI patterns.
- → See [catalog.md](catalog.md) in this skill for the Fowler catalog entries (Extract Interface, Change Function Declaration, Move Function) these structural moves build on.
