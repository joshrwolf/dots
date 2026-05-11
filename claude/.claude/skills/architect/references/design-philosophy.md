# Design Philosophy

These principles define what "good architecture" means. They apply regardless
of language. Both `/survey` (finding problems) and `/architect` (designing
solutions) are grounded in these values.

The intellectual lineage: Parnas (information hiding, 1972) → Liskov (abstract
data types, 1974) → Pike (composition over taxonomy) → Kennedy ("Type Is
Life") → Ousterhout (deep modules). The core idea is unchanged: **data and
its operations are one unit, and that unit is the primary organizing structure
of code.**

---

## Type Is Life

Types are the organizing unit of code. A well-designed system is a set of
types, each encapsulating data and the operations that give that data meaning.

**When to recognize a missing type:**
- The same cluster of arguments appears across multiple function signatures —
  that's a struct you haven't defined yet.
- A function's primary argument is always the same type — that should be a
  method. Kennedy's test: "Methods are valid when it is practical or
  reasonable for a piece of data to have a capability."
- A function keeps gaining parameters — those parameters are fields waiting
  for a type.
- State is configured once and used repeatedly — that's a constructor +
  methods (the `NewEncoder(w)` pattern).

**When something is NOT a type:**
- A pure transformation with clear inputs and outputs is a function.
  Name test: if you'd name it after *what it returns* (`ParseInt`,
  `ReadAll`), it's a function. If you'd name it after *an action*
  (`.Scan()`, `.Process()`), it's a method.

**The anti-pattern: anemic types.** Structs with exported fields but no
methods, operated on entirely by external functions. If external functions
always take the same struct as their first argument, those functions are
methods that haven't moved home yet. Data and behavior live together — a
type without behavior is just a data bag, and a function that always
operates on one type is a method in exile.

---

## Deep Modules

A good type (or package, or module) has a **small interface hiding
significant implementation**. Three exported methods backed by 500 lines of
internal logic is a deep module. Twenty exported methods each doing one
trivial thing is a shallow module.

Depth is earned by hiding complexity. The consumer sees a simple API; the
implementation handles the hard parts. This is Ousterhout's primary
quality metric and Parnas's original information-hiding principle.

**Shallow modules are a design smell.** When a type's interface is nearly
as complex as its implementation, it isn't hiding anything — it's just
reorganizing code without reducing cognitive load.

---

## Earn Your Abstractions

A function, type, interface, or layer must earn its existence. One caller
is not enough — inline it. A wrapper that renames without adding value is
ceremony. A layer that exists for "separation of concerns" but adds no
actual abstraction is noise.

Duplication is far cheaper than the wrong abstraction. Three similar blocks
of code are fine until a genuine second use case reveals the right seam.
Premature extraction creates coupling to an abstraction that doesn't yet
represent a real pattern.

---

## Interfaces at the Consumer

Define interfaces where they are consumed, not where types are implemented.
Write concrete types first; interfaces emerge naturally from shared method
sets. One to three methods is ideal — the bigger the interface, the weaker
the abstraction.

**Accept interfaces** for flexibility and testability. **Return concrete
types** for clarity and richness. The producer doesn't know what the
consumer needs to abstract over — let the consumer decide.

This is composition over taxonomy: no type hierarchies, no up-front
interface design. Interfaces are discovered from concrete usage, not
declared in anticipation of it.

---

## Testability Is Design Quality

If code is hard to test through its public API, the design is wrong —
refactor the code, not the tests. If you need a mock framework, the
dependency boundary is in the wrong place.

Well-designed types accept interfaces for dependencies (making fakes
trivial), return values instead of mutating state (making assertions
straightforward), and avoid global state (making tests independent).

---

## No Ceremony

- **No bag-of-functions packages.** Name things after what they *provide*,
  not what they *contain*. `util`, `helpers`, `common` are symptoms of
  missing types.
- **No premature interfaces.** Don't abstract until there's a concrete
  second consumer.
- **No wrapper types that add nothing.** If calling the underlying thing
  directly is equally clear, remove the layer.
- **No indirection for a single concrete case.** Generics and interfaces
  exist to serve multiple implementations, not to look extensible.
- **No single-use helpers.** A function must earn its existence through
  reuse. Three lines of code called once is not a function — it's an
  indirection that forces the reader to jump somewhere else for no reason.

---

## Boundaries Follow Change

Separate concerns that change for different reasons. Co-locate things that
change together. Decompose by *what changes*, not by flowchart steps or
layer diagrams.

Business logic should not depend on transport or infrastructure — invert
the dependency so core domain types are self-contained and infrastructure
adapts to them.

A missing boundary (two concerns tangled in one module) is a problem. A
gratuitous boundary (one concern split across modules for organizational
aesthetics) is also a problem. Both create friction — the first by coupling,
the second by scattering.
