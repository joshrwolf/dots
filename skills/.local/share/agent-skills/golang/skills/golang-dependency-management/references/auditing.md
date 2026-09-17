# Auditing Dependencies

## Test-Only vs Binary Dependencies

Go's `go.mod` does **not** distinguish between test-only and production dependencies. Modules appear together. `// indirect` means no package in the main module directly imports a package from that module; direct test imports count too.

### What Gets Included in Your Binary

- `*_test.go` files are **never** compiled by `go build` — only by `go test`
- Packages imported only by test files are not linked into the final binary
- However, their modules still appear in `go.mod`

### Module Graph Pruning (Go 1.17+)

Go 1.17+ can prune transitive module requirements because each modern module records the dependencies needed by its packages and tests explicitly. This reduces graph loading, while often increasing the main go.mod's explicit indirect requirements. It is not simply a filter for dependencies used by other modules' tests.

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

`govulncheck` reports known vulnerabilities that affect your code. Its default source scan uses a static call graph to distinguish potentially reachable vulnerable symbols from affected imported packages and modules. Reachability is an approximation, not proof of execution or absence of risk.

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

Source output can include call paths to vulnerable symbols and summaries of affected packages or modules without detected calls. Binary analysis has less call-graph detail. Results depend on build configuration, analysis limits, and the vulnerability database.

For CI pipeline integration, see the `samber/cc-skills-golang@golang-continuous-integration` skill.

## Available dependency updates

```sh
go list -m -u all
go list -m -u -json all
```

Review the selected versions and replacements before updating. This reports
available updates without changing the module. Use `go mod why -m module/path`
to investigate why a dependency is present.
