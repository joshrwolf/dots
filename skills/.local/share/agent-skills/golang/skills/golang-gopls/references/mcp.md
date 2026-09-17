# gopls MCP server & native `LSP` tool reference

## Table of contents

- [Starting the server](#starting-the-server)
- [Registering the MCP server](#registering-the-mcp-server)
- [MCP tools](#mcp-tools)
- [Editor and harness LSP integrations](#editor-and-harness-lsp-integrations)
- [What the MCP server can and cannot do](#what-the-mcp-server-can-and-cannot-do)

## Starting the server

A standalone gopls instance speaking MCP over stdin/stdout, launched fresh per session, no LSP client involved:

```bash
gopls mcp
```

Only sees files as they exist **on disk** — an edit made through a different tool but not yet saved is invisible to it. This is the right mode for an agent-only workflow with no attached editor.

## Registering the MCP server

The underlying command that starts the server (`gopls mcp`) is harness-agnostic — any MCP-capable host can point at it. Claude Code registers it via its own CLI:

```bash
claude mcp add gopls -- gopls mcp
```

Other MCP-capable harnesses (Cursor, Windsurf, and others) each have their own MCP server registration — an entry in their respective settings file pointing at `gopls mcp` as the launch command, not a shared config format.

## MCP tools

The following tools are exposed by gopls v0.23; inspect the connected server's tool list for the installed version. They are keyed by name/path/query rather than cursor position — this is the main ergonomic difference from the native `LSP` tool.

| Tool | Purpose | Example |
| --- | --- | --- |
| `go_workspace` | Learn the workspace's overall structure — module, multi-module workspace, or GOPATH project. Useful when the module layout is unknown. | `go_workspace({})` |
| `go_vulncheck` | On-demand reachability check: which known vulnerable symbols may be reached in the current build, subject to static-analysis limits. | `go_vulncheck({"pattern":"./..."})` |
| `go_search` | Fuzzy search for a type, function, or variable by name across the workspace — use when you don't know the exact location. | `go_search({"query":"server"})` |
| `go_file_context` | Summarize a file's dependencies on other files in the _same package_. Useful when related package declarations are missing from the current context. | `go_file_context({"file":"/path/to/server.go"})` |
| `go_package_api` | Show a package's public API — most valuable for third-party dependencies or sibling packages in a monorepo you haven't read file-by-file. | `go_package_api({"packagePaths":["example.com/internal/storage"]})` |
| `go_symbol_references` | Find every reference to a symbol — use when assessing affected callers. | `go_symbol_references({"file":"/path/to/server.go","symbol":"Server.Run"})` |
| `go_diagnostics` | Build/analysis errors for the given files — use to validate affected code. | `go_diagnostics({"files":["/path/to/server.go"]})` |
| `go_rename_symbol` | Return semantic rename edits for known references; the client must apply them. The same build-scope and dynamic-use limits as LSP rename apply. | `go_rename_symbol({"file":"/path/to/server.go","symbol":"Server.Run","new_name":"Serve"})` |

See [scope and validation](../SKILL.md#scope-and-validation) before relying on a refactor.

## Editor and harness LSP integrations

Client setup and available operations vary. Use the host's existing Go language
server integration when available. The server supports more LSP operations than
some agent tools expose; see [matrix.md](matrix.md). Automatic diagnostic delivery
is a client feature, not a guarantee provided by every LSP tool.

For an attached editor session, gopls can expose MCP with
`gopls serve -mcp.listen=localhost:8092`. Connect the MCP client to that session's
endpoint (for example `/sessions/1`) using the editor's configured session.
Unlike detached `gopls mcp`, this mode can see unsaved buffers.

## What the MCP server can and cannot do

The gopls MCP server wraps LSP functionality with these boundaries:

- **Can**: read files from the filesystem and return their contents; execute `go` commands to load package metadata (which may reach `proxy.golang.org` and write to the local Go module/build cache); write to gopls's own cache/configuration files; upload telemetry if the user has opted in.
- **Cannot**: make arbitrary writes to the source tree outside of the edits a tool call explicitly returns; make arbitrary network requests; package loading and vulnerability checks can contact the configured module proxy and vulnerability database.

Both integrations analyze the locally resolved build. Read API documentation with `go doc` and use module tools to inspect versions and replacements.

Source: [official gopls MCP documentation](https://go.dev/gopls/features/mcp).
