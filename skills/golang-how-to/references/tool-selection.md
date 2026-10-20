# Go tool selection

| Need | Tool |
| --- | --- |
| Package and symbol documentation for the current build | `go doc`; see `golang-go-doc` |
| Definitions, references, diagnostics, safe refactors | `gopls`; see `golang-gopls` |
| Resolved modules, replacements, available updates | `go list -m`, `go mod why` |
| Known vulnerability reachability | `govulncheck`; see `golang-security` |
| Reproduce runtime failures | Focused tests, logs, race detector, stack traces |
| CPU, memory, blocking, or scheduling evidence | `go tool pprof`, `go tool trace` |

`gopls` provides semantic code analysis. It does not attach to running processes,
step through runtime execution, or generate an OpenAPI specification. API schema
generation should follow the project's established build process when needed.
