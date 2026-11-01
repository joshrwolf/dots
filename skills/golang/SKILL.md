---
name: golang
description: "Go language, API and documentation reference. Use when checking error contracts, collection ownership, nil behavior, context propagation, types, design, resource lifecycles, package comments or executable examples. Specialist skills cover testing, concurrency and tooling."
license: MIT
---

# Go technical reference

Personal standing instructions carry the required Go defaults. This reference
adds technical detail when a choice needs checking; reading it is not a
prerequisite for every Go change.
Use `golang-testing` for test design, fixtures, `synctest`, fuzzing, HTTP tests,
flaky tests and go-cmp options. No prerequisite bundle is required.

## Errors

- Handle errors according to the API contract, and give a reason for each
  deliberate ignore. With `io.Reader.Read`, use the `n` bytes before checking `err`.
- Add context that identifies the operation, not a prefix at every layer. Use `%w`
  when callers may need to match the cause, such as `context.Canceled` or
  `fs.ErrNotExist`. Where an API deliberately hides its implementation, translate
  the cause to a domain sentinel or use `%v` to block unwrapping (`%v` does not
  redact).
- Use `errors.Is` for sentinels and `errors.AsType[T]` (Go 1.26+) for typed
  inspection. Use `errors.As` on older targets or for non-error interface
  targets, and `errors.Join` for independent failures.
- One layer reports each failure: return it or log it, not both. Retry
  diagnostics are the exception.
- Panic for violated invariants or a documented panic API such as `Must*`. Do not
  turn an internal defect into a successful zero or default result.
- Error strings start lowercase, keep acronyms, and have no trailing punctuation.

## Ownership and nil contracts

- Know who owns slices and maps that cross an API. Return `slices.Clone` or
  `maps.Clone` when callers must not alias internal state; otherwise document
  that the value is shared.
- Preserve nil versus empty wherever callers can observe it, such as JSON `null`
  versus `[]`. Do not change it as a cleanup.
- Return an untyped `nil` for interface results. A typed nil pointer stored in an
  interface is not `== nil`.
- `append` may write into a shared backing array. Clone, or use `s[:len(s):len(s)]`,
  before appending to a slice someone else holds.
- Writing to a nil map panics. Make the zero value usable or the constructor
  mandatory.
- `defer` runs at function exit, so in loops, move the body into a function.
  Propagate writer `Close`/`Flush` errors; use `Sync` when durability is part of
  the contract.
- Integer conversions truncate silently. Check bounds at the boundaries.

## Context and goroutines

- Make `ctx context.Context` the first parameter and propagate the caller's
  context. `context.Background()` mid-request detaches the work from the caller's
  deadline. Pass context per call rather than storing it, unless the value's
  lifetime is the operation and the API says so.
- Every goroutine needs an owner, a stop signal and a way to wait for it.
  `golang-concurrency` covers goroutine, channel, synchronization and
  cancellation design.

## Current Go

Check the module's `go` directive, and its CI and consumers, before using a
version-sensitive API. On Go 1.27+, `go test` runs the `stdversion` vet check,
which flags APIs newer than that directive. Within the target, prefer `any`;
`min`, `max` and `clear`; `slices`, `maps` and `strings.Cut`; `for i := range n`;
`wg.Go` (1.25+); `t.Context` and `b.Loop` (1.24+); and `new(expr)` (1.26+)
instead of pointer helpers (`new(int64(30))` when the type matters). Keep
`&existing` when identity with that variable matters; `new(existing)` copies its
current value into a distinct allocation.
`omitzero` is not a drop-in for `omitempty`: it keeps empty non-nil slices and
maps and drops zero structs, so change a tag only when that wire output is the
intended contract.
`go fix ./...` (1.26+) applies the modernize rewrites; review its diff. Use
`golang-modernize` for requested modernization passes, release-specific details,
migration hazards and target upgrades.

## Tools

- Run `go doc pkg.Symbol` inside the module so the docs match the resolved
  dependencies. Do not add a dependency just to read its docs.
- Use gopls for references and renames (`golang-refactoring`).

## References

Read a reference only when its condition applies:

- [style.md](references/style.md): a requested style or naming review, or when
  local code sets no convention for names, declarations or control flow.
- [documentation.md](references/documentation.md): package and API comments,
  executable examples, and project documentation.
- [types.md](references/types.md): structs, embedding, receivers, type switches,
  struct tags (`omitempty` versus `omitzero`).
- [errors.md](references/errors.md): designing an error API, panic/recover
  boundaries, log grouping.
- [nil-safety.md](references/nil-safety.md), [collections.md](references/collections.md),
  [pitfalls.md](references/pitfalls.md): chasing a panic, nil, aliasing or
  representation bug, or reviewing for correctness hazards.
- [design.md](references/design.md): constructor configuration, functional
  options, `init` and globals, enums, timeouts and limits, iterators and streaming.
- [resources.md](references/resources.md): graceful shutdown, pools, `runtime.AddCleanup`.
- [generics.md](references/generics.md): writing a generic type or function.
- [samber-oops.md](references/samber-oops.md), [samber-mo.md](references/samber-mo.md):
  only when the project uses those libraries.
