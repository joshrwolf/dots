---
name: golang-gopls
description: "Navigate and refactor Go code with gopls. Use for semantic definitions and references, diagnostics, safe renames, code actions, or resolved dependency APIs."
user-invocable: true
license: MIT
compatibility: Designed for Claude Code, Codex or similar harness. Requires the gopls binary (go install golang.org/x/tools/gopls@latest) v0.20+ on PATH.
metadata:
  author: samber
  version: "1.1.1"
  openclaw:
    emoji: "🛰️"
    homepage: https://github.com/samber/cc-skills-golang
    requires:
      bins:
        - go
        - gopls
    install:
      - kind: go
        package: golang.org/x/tools/gopls@latest
        bins: [gopls]
    skill-library-version: "0.22.0"
allowed-tools: Read Edit Write Glob Grep Bash(go:*) Bash(golangci-lint:*) Bash(git:*) Agent Bash(gopls:*) LSP mcp__gopls__*
paths:
  - "**/*.go"
---

Use the project's installed `gopls` version. Install or configure an integration
only when needed for the task. MCP support requires gopls v0.20+.

`gopls` analyzes the workspace and its resolved dependencies. Use `go doc`
for package documentation; use module tools for version and replacement details.

## Ways to reach gopls

- **MCP** exposes symbol names, paths, and search queries. A standalone `gopls mcp`
  process sees saved files; an attached editor session can include unsaved buffers.
  See [MCP setup and tools](references/mcp.md).
- **Editor or harness LSP integration** provides position-based navigation and
  refactors. Operations and automatic diagnostic delivery depend on the client;
  inspect its exposed capabilities rather than assuming a fixed tool surface.
- **CLI** is useful for one-shot checks and scripted edits. It is an experimental,
  incomplete interface. Check `gopls <command> -h` for the installed release.
  See [CLI commands](references/cli.md).

Choose an available interface that supports the operation. The
[capability map](references/matrix.md) distinguishes the server's LSP features
from the subset a particular agent host exposes.

## Use cases

- **Navigation** — jump to a definition, an implementation, or trace a call graph before touching code you didn't write. Details: [references/features.md](references/features.md#navigation).
- **Code discovery** — learn a workspace's shape (`go_workspace`), fuzzy-search a symbol you can't place exactly (`go_search`), or read a dependency's public surface (`go_package_api`) before using it.
- **Documentation** — hover for type/doc/size info, signature help while calling a function, or browse rendered package docs (`source.doc`, including internal packages pkg.go.dev never sees).
- **Diagnostics & safety** — compiler and analyzer findings for affected code (`go_diagnostics` or client diagnostics). Use `go_vulncheck` for a requested vulnerability check or relevant dependency work; it is static analysis, not a runtime guarantee.
- **Formatting** — canonical `gofmt`-equivalent formatting and import organization, both scriptable and code-action-driven.
- **Refactoring** — semantic rename (checks known references and interface relationships), extract/inline, and the full `refactor.rewrite.*` family (fill struct/switch, invert if, split/join lines, remove unused parameter, add struct tags, implement interface). Full catalog with gotchas: [references/features.md](references/features.md#transformation).

## Scope and validation

Use references to inspect affected callers when changing an API. Review returned
edits and verify the affected packages and build configurations. Diagnostics do
not replace compilation or tests; choose test scope from the change and project
requirements, including whole-module tests when appropriate.

**Limits worth checking:**

- Renames cannot prove behavior preserved across reflection, dynamic assertions, external consumers, generated inputs, or unexamined build configurations.
- `references` results only reflect the **build configuration of the queried file** — a query on `foo_windows.go` will not surface matches in `bar_linux.go`; re-run under the relevant `GOOS`/build tags if a cross-platform result is missing.
- `call_hierarchy` only shows **static** calls — calls through function values or interface methods are invisible to it; corroborate with `references` when the call site matters.
- Review refactoring output: comments are sometimes dropped, and generated files marked `DO NOT EDIT` receive no code actions at all.
- `refactor.rewrite.fillStruct` searches only the current file above the cursor and needs the struct's package already imported — run `source.organizeImports` first if the type was just typed in.

## Documentation and runtime evidence

Use `golang-go-doc` for API documentation and `golang-security` for vulnerability
checks. Use reproductions, stack traces, the race detector, pprof, and execution
traces for runtime evidence. Gopls does not step through a running process.
