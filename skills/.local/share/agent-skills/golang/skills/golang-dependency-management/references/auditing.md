# Auditing Dependencies

## Test-Only vs Binary Dependencies

Go's `go.mod` does **not** distinguish between test-only and production dependencies. All modules appear together, with `// indirect` marking transitive dependencies.

### What Gets Included in Your Binary

- `*_test.go` files are **never** compiled by `go build` — only by `go test`
- Packages imported only by test files are not linked into the final binary
- However, their modules still appear in `go.mod`

### Module Graph Pruning (Go 1.17+)

With `go 1.17` or higher in `go.mod`, Go prunes the module graph: transitive dependencies needed only for tests of other modules are excluded from the build graph. This reduces `go.mod` size and avoids downloading unnecessary modules.

### Upgrading With or Without Test Dependencies

```bash
go get -u ./...       # Upgrade deps, EXCLUDING test-only deps
go get -u -t ./...    # Upgrade deps, INCLUDING test-only deps
```

### Impact on Binary Size

Use `go list -deps ./cmd/myapp` to inspect the build's package graph and
`go version -m ./myapp` for embedded module metadata. These do not measure each
module's linked size. For an unstripped binary, `go tool nm -size ./myapp` shows
symbol sizes; compare binaries built with the same flags to assess a change.

## Vulnerability Scanning with govulncheck

`govulncheck` reports known vulnerabilities that affect your code. It uses static analysis to narrow reports to vulnerabilities in code paths your project actually calls — unlike generic CVE scanners that flag every dependency regardless of usage.

```bash
# Scan source code (most common)
govulncheck ./...
# Or, when govulncheck is pinned with a Go 1.24+ tool directive:
go tool govulncheck ./...

# Scan a compiled binary
govulncheck -mode=binary ./bin/myapp

# JSON output (for CI integration)
govulncheck -format json ./...

# Include test code in analysis
govulncheck -test ./...
```

Output shows the vulnerability ID, affected module, fixed version, and the call trace from your code to the vulnerable function. If a vulnerability exists in a dependency but your code never calls the affected function, `govulncheck` does not flag it.

For CI pipeline integration, see the `samber/cc-skills-golang@golang-continuous-integration` skill.

## Available dependency updates

```sh
go list -m -u all
go list -m -u -json all
```

Review the selected versions and replacements before updating. This reports
available updates without changing the module. Use `go mod why -m module/path`
to investigate why a dependency is present.
