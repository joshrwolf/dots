---
name: golang-refactoring
description: "Navigate and restructure Go code with gopls and Go tooling. Use for semantic navigation (definitions, references, implementations, call graphs), safe renames, extract/inline, moving code across packages, breaking import cycles, bulk rewrites with go fix or eg, and sequencing structural changes."
license: MIT
---

# Go refactoring and semantic navigation

A refactor changes structure without changing observable behavior. Semantic tools
catch many static conflicts but do not prove behavior is preserved, so verify
with focused tests.

## Defaults

- Prefer gopls Rename, Inline and Extract over hand edits. Rename checks name
  conflicts and visible interface satisfaction. Inline preserves evaluation order
  and side effects. Review every diff anyway: Extract is best-effort, and comments
  can be lost.
- Keep code movement separate from behavior or performance changes, so each can
  be checked on its own.
- Move declarations and update every caller in the same change. Remove the
  obsolete declarations and imports. Add compatibility aliases, forwarding
  wrappers or deprecation stages only when actual external consumers impose that
  contract. Resolve such a contract before changing the public API.
- Break an import cycle with a consumer-side interface first. The producer never
  imports it. Split packages or extract a shared leaf only when that is not enough.
- After a rename, grep for what the compiler cannot see: struct tags, templates,
  reflection, serialized names, generated inputs, `//go:linkname`. Explicit
  `json`/`db` tags can intentionally keep a wire name stable.
- Use `go fix ./...` (Go 1.26+) for modernization rewrites. It runs the modernize
  analyzers (`rangeint`, `minmax`, `stringscut`, `newexpr`, `errorsastype`, and
  others; `go tool fix help` lists them). Review the result like any other
  generated diff.

## Gotchas

- gopls `references` reflects only the build configuration of the queried file.
  Re-query under other `GOOS` values or build tags when cross-platform code matters.
- `call_hierarchy` shows only static calls. Calls through function values and
  interface methods need `references` or `implementation`.
- Files marked `DO NOT EDIT` get no code actions. Change the generator input
  instead.
- gopls diagnostics do not replace `go build`, `go vet` or the tests.

## Risk

| Risk | Transforms | Before landing |
| --- | --- | --- |
| Low | Rename, extract/inline variable, organize imports, local `refactor.rewrite.*` | Build, vet, test |
| Medium | Extract function, cross-package inline, add/remove a parameter, introduce generics | Targeted tests over the affected code first |
| High | Signature change across many callers, cross-package moves, package split/merge, cycle breaking, exported-API or major-version changes | Tests over affected callers; compatibility requirements resolved |

## References

- [gopls.md](references/gopls.md): reaching gopls through MCP, LSP or CLI, plus
  features and limits. Its links cover [MCP setup](references/gopls/mcp.md), the
  [CLI](references/gopls/cli.md), [features](references/gopls/features.md),
  [settings](references/gopls/settings.md) and the
  [capability matrix](references/gopls/matrix.md).
- [structural.md](references/structural.md): import cycles, package boundaries,
  cross-package moves, exported-API and major-version moves.
- [go-tooling.md](references/go-tooling.md): `gofmt -r`, `eg`, `go/analysis`
  fixers, `//go:fix inline`, `deadcode`, and how they differ.
- [catalog.md](references/catalog.md): the Fowler catalog mapped to Go, with
  triggers, tools and risk.
- [safety-net.md](references/safety-net.md): characterization tests, test seams,
  verification commands.

The `golang` skill covers what the resulting code should look like.
`golang-project-layout` covers the target package layout.
