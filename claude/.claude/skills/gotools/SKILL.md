---
name: gotools
description: Tools for working with Go code beyond Read/Edit/Bash basics — symbol-aware rename across workspace, gopls-driven format and imports, and the LSP tool for navigation (definitions, references, implementations, call hierarchy, symbol search). MUST be loaded for ANY Go work — reading, editing, refactoring, or navigating .go files, or running Go tooling. Load this skill before touching Go code, not after.
allowed-tools: Bash(gopls *), Bash(go list:*), Bash(go doc:*), Bash(go test:*), LSP, Read, Edit, Write, MultiEdit, Glob, Grep
---

# gotools

Operations on Go code use specialized tools that beat Read+Edit+Grep.

## Diagnostics — automatic, deferred to end of turn

A Stop hook runs `gopls check` on all `.go` files you edited during the turn.
Diagnostics surface once after all your edits complete, not after each
individual edit. This means you will NOT see transient errors mid-refactor —
edit freely, and diagnostics arrive when you stop.

Do not manually run `gopls check`, `go vet`, or `go build` to verify your
own edits — the hook output is authoritative. Only run them if explicitly asked.

## Navigation — use LSP, not grep

The `LSP` tool wraps gopls navigation. It is **deferred** — you must load
its schema first:

```
ToolSearch query="select:LSP"
```

Then call with `operation`, `filePath`, `line`, `character` (1-based).

| Operation | When |
|---|---|
| `goToDefinition` | Find where a symbol is defined |
| `findReferences` | All call/use sites of a symbol |
| `goToImplementation` | Concrete types implementing an interface |
| `documentSymbol` | All symbols in one file (faster than reading + parsing) |
| `workspaceSymbol` | Find a symbol by name across the workspace |
| `hover` | Type signature + docs at a position |
| `prepareCallHierarchy` → `incomingCalls` | Who calls this function |
| `prepareCallHierarchy` → `outgoingCalls` | What does this function call |

Typical flow: `workspaceSymbol` (find the line:col) → other ops.

## Mutation — use gopls CLI, not Edit

The LSP plugin is read-only. For these, use `Bash`:

### Rename a symbol across the workspace

```bash
gopls rename -w <file>:<line>:<col> <NewName>
```

Symbol-aware. Updates all references, qualified names, and embedded fields
correctly. **Never use grep+sed/Edit to rename Go symbols** — it misses
qualified references and matches strings/comments.

To find `<line>:<col>`: use `LSP workspaceSymbol` or `documentSymbol`, or
`grep -n` for the declaration line and count the column.

Validate first if unsure:
```bash
gopls prepare_rename <file>:<line>:<col>
```

### Organize imports (add missing, remove unused, sort)

```bash
gopls imports -w <file>
```

Project-aware (uses gopls's view). Prefer over `goimports`.

### Format a file (gofmt-equivalent)

```bash
gopls format -w <file>
```

Project-aware. Prefer over standalone `gofmt`.

### Other useful gopls subcommands

- `gopls signature <file>:<line>:<col>` — function signature at a position
- `gopls codeaction <file>` — list quick-fixes available (e.g. missing
  imports, extract function/variable). Pair with `gopls execute` to apply.

## Anti-patterns

- Running `go build` / `go vet` / `gopls check` to verify your own edits —
  the PostToolUse hook does this. Wastes a tool call and tokens.
- Using grep+Edit to rename a symbol — silently misses references and may
  match comments/strings. Use `gopls rename`.
- Using `goimports` or `gofmt` directly — they don't see project-local
  imports the way `gopls imports`/`gopls format` do.
- Using `Read` to enumerate a file's symbols — `LSP documentSymbol` is
  faster and gives structured output.
- Using `Grep` to find callers of a function — `LSP findReferences` is
  symbol-aware and skips comments/strings.
