# Go Tooling for Refactoring

This file is the tool reference for `samber/cc-skills-golang@golang-refactoring`: semantic editing, bulk rewrites, and reachability analysis, with the capabilities and limitations that distinguish them. See [catalog.md](catalog.md) for which tool maps to which Fowler refactoring.

## Table of Contents

- [1. gopls — the Primary Actuator](#1-gopls--the-primary-actuator)
- [2. Bulk Mechanical Rewrite Tools](#2-bulk-mechanical-rewrite-tools)
  - [`gofmt -r` — syntactic, single-expression](#gofmt--r--syntactic-single-expression)
  - [`eg` — type-aware, example-based](#eg--type-aware-example-based)
  - [`go/analysis` + SuggestedFixes — bespoke, testable](#goanalysis--suggestedfixes--bespoke-testable)
  - [`go fix` — the `go/analysis`-based fixer suite](#go-fix--the-goanalysis-based-fixer-suite)
  - [`dave/dst` — comment- and formatting-preserving AST edits](#davedst--comment--and-formatting-preserving-ast-edits)
  - [Import Cleanup and Reachability](#import-cleanup-and-reachability)
- [4. Structure-Discovery Tools (blast-radius mapping)](#4-structure-discovery-tools-blast-radius-mapping)
- [Cross-References](#cross-references)

## 1. gopls — the Primary Actuator

- gopls performs most of this skill's Low- and Medium-risk transforms — Rename, Inline, Extract, and the `refactor.rewrite.*` family.
- See the Risk Stratification table in [SKILL.md](../SKILL.md) and the `Tool:` line on each entry in [catalog.md](catalog.md) for which gopls action maps to which refactoring.
- The full code-action reference, CLI invocation, safety behavior, and MCP server setup belong to `samber/cc-skills-golang@golang-gopls` — this file covers the tools that skill doesn't.

## 2. Bulk Mechanical Rewrite Tools

Choose tooling based on whether the change needs textual, syntactic, or type-aware matching. A generated rewrite can help with many repeated edits; direct edits can be simpler for a small, well-understood set of sites.

### `gofmt -r` — syntactic, single-expression

- Purely syntactic and type-unaware: it matches expression shape, not the types involved, and a rewrite is limited to a single expression.
- Wildcards are single lowercase identifiers that match any sub-expression.

```bash
gofmt -r 'bytes.Compare(a, b) == 0 -> bytes.Equal(a, b)' -w file.go
gofmt -r 'bytes.Compare(a, b) != 0 -> !bytes.Equal(a, b)' -w file.go
gofmt -s -w file.go    # -s additionally simplifies (e.g. s[a:len(s)] -> s[a:])
gofmt -l .             # list non-conforming files — useful as a CI gate
gofmt -d file.go       # show the diff without writing
```

- Because it can't see types, it cannot target "every `bytes.Compare` call on a `[]byte`, but not a look-alike function of the same name from another package" — that distinction needs `eg`.

### `eg` — type-aware, example-based

`golang.org/x/tools/cmd/eg` rewrites by example, à la Refaster: a template file declares `before`/`after` functions of identical type, each with a single-expression body.

```go
// template.go
package template

import (
	"errors"
	"fmt"
)

func before(s string) error { return fmt.Errorf("%s", s) }
func after(s string) error  { return errors.New(s) }
```

```bash
eg -t template.go -w ./...
```

- Matching is semantic, not textual — `func(x int)` in the template also matches `func(y int)` at the call site, since `eg` matches by type and structure.
- Limits: expressions only, no statements or function-literal patterns; a rewrite can't change the expression's type; imports are added but never removed (run `goimports` afterward); and duplicating a wildcard variable in the `after` template duplicates whatever side effect the matched expression had.


### `go/analysis` + SuggestedFixes — bespoke, testable

- For a rewrite that needs custom type-aware matching, write an `analysis.Analyzer`. Its `Run(pass)` walks the type-checked AST and reports `analysis.Diagnostic{SuggestedFixes: [...]}` at each match.
- Test it against `.golden` files with `analysistest.RunWithSuggestedFixes`, then ship it as a `singlechecker`/`multichecker -fix` binary, or run it through `go vet -vettool=<path>`.

### `go fix` — the `go/analysis`-based fixer suite

- As of Go 1.26, `go fix` is rewritten onto the `go/analysis` framework and has converged with `go vet` — this is where the `modernize` fixer suite lives (`rangeint`, `mapsloop`, `minmax`, `any`, `stringscut`, `omitzero`, and more). Coverage keeps shifting release to release — Go 1.27 added `atomictypes`, `embedlit`, `slicesbackward`, `unsafefuncs`, removed `fmtappendf`, and renamed `waitgroup` to `waitgroupgo`; → See `samber/cc-skills-golang@golang-modernize` skill for the idiom-by-idiom breakdown.
- On an older toolchain without this convergence, run the equivalent analyzers through `singlechecker`/`multichecker -fix` instead of `go fix`.
- The `//go:fix inline` directive marks a function or constant so that `go fix` inlines every call site into its replacement — a machine-executable way to complete a deprecation migration once the replacement exists:

```bash
go fix ./...
go run golang.org/x/tools/go/analysis/passes/inline/cmd/inline@latest -fix ./...
```

### `dave/dst` — comment- and formatting-preserving AST edits

- `go/ast` stores comments in a side table keyed by byte offset, so reordering, moving, or deleting nodes desyncs comments from the code they were attached to — the root cause of the Extract/Inline comment-loss caveat above.
- `github.com/dave/dst` (Decorated Syntax Tree) attaches comments and blank-line spacing as node-local decorations instead, so a hand-rolled AST rewrite round-trips them correctly via `decorator.Parse` / `decorator.Print`.
- `dstutil.Apply` mirrors `astutil.Apply`'s visitor API, so existing `go/ast` rewrite logic ports over directly.
- Reach for this only when a bespoke `go/analysis` fixer needs to preserve comments that `go/ast`-based rewriting would otherwise scatter.

### Import Cleanup and Reachability

If a rewrite leaves unused or missing imports, use import organization on the affected files. A tool that already maintains imports does not require a second pass.

```bash
goimports -w changed.go    # organizes imports added/left dangling by gofmt -r, eg, or a hand-rolled fixer
```

Reachability analysis can help after removal-heavy refactors; it is not a required check for every edit.

```bash
deadcode ./...                     # find code orphaned by a removal-heavy refactor
deadcode -test ./...                # include test binaries — unreached public API here signals a coverage gap, not dead code
deadcode -whylive=funcName ./...    # shortest reachability path proving a function is still live
```

- `golang.org/x/tools/cmd/deadcode` builds its reachability graph with Rapid Type Analysis from `main`/`init`, so it is unsound with respect to assembly, `go:linkname`, and reflection-driven dispatch — treat a "dead" verdict as a strong hint, not a proof, on code that uses any of those.
- Text substitutions do not understand Go scope, types, comments, or string literals. Use semantic tools when those distinctions determine the match; bounded textual edits are reasonable when their matches can be reviewed directly.

## 4. Structure-Discovery Tools (blast-radius mapping)

These queries identify affected callers and dependencies beyond the edited declaration.

| Tool | What it answers |
| --- | --- |
| `golang.org/x/tools/go/callgraph` (`rta`/`cha`/`static`/`vta` algorithms) | Who can reach this function, statically, across the whole build — algorithms trade precision for speed differently |
| gopls call hierarchy (`textDocument/prepareCallHierarchy`) | Incoming/outgoing calls for one symbol, interactively |
| `go_references` / `go_symbol_references` (gopls MCP) | Every reference to a symbol, from an agent context, without a full callgraph build |
| `go mod graph` | Module-level dependency edges — which modules require which, for blast radius that crosses module boundaries |

## Cross-References

- [catalog.md](catalog.md) — the Fowler refactoring catalog mapped to Go, with the tool from this file that mechanizes each entry.
