---
name: gotools
description: Go development toolchain for Claude Code — automatic syntax checking, import management, compilation diagnostics, modernization analysis, and LSP-powered navigation. MUST be loaded for ANY Go work.
allowed-tools: Bash(gopls *), Bash(go list:*), Bash(go doc:*), Bash(go test:*), Bash(go mod tidy:*), LSP, Read, Edit, Write, MultiEdit, Glob, Grep
---

# Go Toolchain

## What happens automatically

Imports, formatting, and diagnostics are handled for you. Never think about
them — never run `goimports`, `gofmt`, `go vet`, `go build`, or `gopls check`.

**After every edit to a `.go` file** the PostToolUse hook:
1. Validates syntax — exits with the parse error if broken, fix before continuing
2. Auto-fixes imports and formatting in-place (adds missing, removes unused)

The file on disk may differ from what you wrote. If your next `old_string`
doesn't match, re-read the file — auto-formatting changed it.

**When you finish a turn** the Stop hook checks all `.go` files you edited:
1. `go vet` on affected packages (~1s) — compilation errors and vet diagnostics
2. If vet is clean: `gopls check` for modernize, staticcheck, unused params (~4s)
3. On retry turns: only `go vet` (skips the slower gopls pass)

Output is filtered to files you edited — pre-existing issues don't appear.

## go doc

`go doc` resolves packages through the module in the current directory.
Run it from the module that imports the package:

```bash
cd /path/to/module && go doc pkg.Symbol
```

If you get "no such package", you're in the wrong module. Find one that
imports it (`grep -r "import-path" --include='*.go' -l`), then cd to
its module root.

## When to run go mod tidy

If the Stop hook reports "no required module provides package X", the fix is
`go mod tidy` from the module root, not a code change. Run it, then let the
hook re-check.

## Navigation

Load the LSP tool first: `ToolSearch query="select:LSP"`

The LSP tool provides navigation only — no diagnostics.

| Operation | Use |
|---|---|
| `goToDefinition` | Jump to where a symbol is defined |
| `findReferences` | All call/use sites |
| `goToImplementation` | Concrete types behind an interface |
| `documentSymbol` | All symbols in a file (faster than reading) |
| `workspaceSymbol` | Find a symbol by name across the workspace |
| `hover` | Type signature and docs |
| `incomingCalls` | Who calls this function |
| `outgoingCalls` | What this function calls |

Start with `workspaceSymbol` to find line:col, then drill in.

## Rename

```bash
gopls rename -w <file>:<line>:<col> <NewName>
```

Symbol-aware — updates all references, qualified names, embedded fields.
Never use grep+Edit to rename Go symbols.
