---
name: gofix
description: Review and fix Go code for design, modern patterns, and efficiency. Applies opinionated Go standards. Invoke with a scope description (file, package, function, etc.).
argument-hint: "[scope: file, package, function, or description of what to review]"
---

# gofix

Review Go code and fix issues. The user provides a scope (file, package, function, etc.) as the argument.

## Process

1. **Identify the code to review.** Use the argument to find the relevant files. If ambiguous, use Glob/Grep to locate them. Read all relevant code before launching agents.

2. **Launch three review agents in parallel.** Pass each agent the full code under review. Instruct each agent to load the `idiomatic-go` skill for the full standards, and to read relevant reference files (e.g. `references/modern.md`) as needed. Each agent **reports findings only** — it does not edit files.

3. **Review agent findings.** Discard false positives and findings not worth the churn. Apply the remaining fixes directly.

4. **Summarize** what was fixed and what was intentionally skipped.

## Agent 1: Design & Structure

Review for structural problems. The core philosophy: code should be organized around
well-typed data with methods, not chains of stateless functions. Inline code that's
only used once is clearer than an extracted helper.

Check for:

- **Single-use helpers that should be inlined.** A function must earn its existence
  through reuse. If called once, inline it — this includes validation, formatting, and
  conversion helpers. "Duplication is far cheaper than the wrong abstraction."

- **Function chains with growing parameter lists.** When a function has 4+ parameters,
  especially when several travel together across calls, that's data that should be a
  struct with methods. The stdlib models this: `http.Server`, `bufio.Scanner`, `exec.Cmd`.

- **Logic that should be a method.** If a function's primary argument is always the
  same type, it should be a method. Kennedy's test: "Methods are valid when it is
  practical or reasonable for a piece of data to have a capability." Cheney's naming
  test: named after an action → method; named after what it returns → function.

- **Anemic types.** Behavior that belongs to a type living outside it as free
  functions taking that type as their first argument — so the type can't enforce its
  own invariants. Tells: a struct with fields but ~no methods while a cluster of
  functions all take `*T`/`T` first; a stateless `FooManager`/`FooService`/`FooHelper`
  that only operates on `Foo`; invariants enforced by a `validateX(x)` callers must
  remember to call. Make them methods; co-locate behavior with data. **Not anemic** —
  leave alone: pure data carriers with no behavior to own (DTOs, wire/JSON/config
  structs, plain records in transit).

- **Missing `New*` constructor.** Types with dependencies or configuration should have
  a constructor accepting interfaces and returning a concrete struct — the
  `json.NewEncoder(w)` / `bufio.NewScanner(r)` pattern.

- **Constructors that panic on bad input.** A `panic` inside a `New*` for invalid
  arguments is almost always laziness avoiding a signature change — flag it. The fix
  is a validating constructor returning `(*T, error)` and updating the call sites, not
  a panic. Panicking is only correct for genuinely unrecoverable programmer errors with
  no caller-supplied input (the `regexp.MustCompile` convention — a `Must*` wrapper over
  an erroring constructor, used with compile-time constants). If construction validates
  caller input, it must return an error.

- **"Bag of functions" packages.** `util`, `helpers`, `common` — name packages after
  what they provide, not what they contain.

- **Premature or oversized interfaces.** Define where consumed, not where implemented.
  1-3 methods ideal. Write concrete types first; interfaces emerge from shared method sets.

- **Unnecessary abstraction layers and refactor leftovers.** Wrappers or facades that
  add no value beyond renaming — if calling the underlying thing directly is equally
  clear, remove the layer. This is where iterative refactors leave deadweight: flag
  pass-through functions (`func A(x) { return B(x) }` — inline at the call sites and
  delete) and type aliases or named types that add no behavior, type safety, or domain
  meaning (`type Foo = Bar` — collapse to the underlying type). Fix the internals to be
  idiomatic rather than preserving a shim. Keep a named type only when it earns its
  place: it has methods, provides type safety, or bridges a real package-migration
  boundary.

## Agent 2: Modern Go Patterns

Review for outdated patterns where modern replacements exist. The codebase targets
latest Go (1.26). Never preserve legacy patterns when a modern equivalent exists.

Check for:

- `wg.Add(1); go func() { defer wg.Done(); ... }()` → `wg.Go(func() { ... })`
- `var target *T; errors.As(err, &target)` → `errors.AsType[*T](err)`
- `for i := 0; i < b.N; i++` or `for range b.N` → `for b.Loop()`
- `context.Background()` or `context.TODO()` in tests → `t.Context()`
- `omitempty` on struct types or `time.Time` → `omitzero`
- `strings.Split`/`strings.Fields` used only for iteration → `SplitSeq`/`FieldsSeq`/`Lines`
- `ptr()` / `toPtr()` / `&tmp` helper patterns → `new(expr)`
- `tools.go` with blank imports → `tool` directive in go.mod
- `runtime.SetFinalizer` → `runtime.AddCleanup`
- `go.uber.org/automaxprocs` → built-in since Go 1.25
- `golang.org/x/crypto/{hkdf,pbkdf2,sha3}` → `crypto/{hkdf,pbkdf2,sha3}`
- `time.Sleep` in tests for synchronization → `testing/synctest`
- Passing `rand.Reader` to crypto functions → ignored since Go 1.26, remove the arg
- `httputil.ReverseProxy.Director` → `Rewrite`
- `item := item` loop variable captures → unnecessary since Go 1.22

## Agent 3: Efficiency & Correctness

Review for performance, correctness, and robustness issues.

Check for:

- **Unnecessary allocations.** String concatenation in loops (use `strings.Builder`).
  `fmt.Sprintf` in hot paths where `AppendText`/`AppendBinary` avoids allocation.
  Slices grown via append-in-loop when length is known (preallocate with `make`).

- **Missing or swallowed errors.** All errors must be checked and wrapped with context
  using `%w`. Never log-and-return — either handle or propagate, not both.

- **Concurrency bugs.** Data races (maps without synchronization, shared state without
  mutex). Goroutine leaks (missing exit path, missing context cancellation check).
  Missing `errgroup.SetLimit` on unbounded fan-out.

- **N+1 and repeated work.** Database/API calls in loops. Redundant computations that
  could be hoisted. Repeated file reads. Duplicate network calls.

- **Unbounded growth.** Slices or maps growing without bound. Missing resource cleanup.
  Goroutine leaks.

- **TOCTOU patterns.** Checking existence before operating — operate directly and
  handle the error instead.

- **Hot-path bloat.** Expensive operations (reflection, JSON marshal, file I/O) in
  per-request or per-iteration paths that could be hoisted or cached.

- **Missing context propagation.** Functions accepting `context.Context` but not
  passing it downstream. Long operations not checking `ctx.Done()`.
