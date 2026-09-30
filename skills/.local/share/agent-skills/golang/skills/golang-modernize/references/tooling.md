# Tooling Modernization

Tool upgrades can change configuration, diagnostics, generated output, and
platform requirements. Include the necessary companion changes and verify them.

## Go target upgrades

Prefer the latest stable release. Check the project's support policy and module
consumers before raising its minimum version; an old directive alone does not
establish a continuing requirement for old Go. Update the `go` directive,
`toolchain` selection when present, CI matrix, and build images consistently.
Verify the selected release against the official release notes rather than
copying a version number from this guide.

Use `go version` to identify the running toolchain and inspect `go.mod` / `go.work`
for language targets. `go get go@<version>` selects a target version; update an
explicit toolchain pin if the project uses one. Re-run the project's checks and
review module/dependency changes after an upgrade.

## golangci-lint v2 _(golangci-lint v2.0.0+, March 2025)_

Upgrade to golangci-lint v2. **Migration**: Run `golangci-lint migrate` to convert v1 config to v2. See the `samber/cc-skills-golang@golang-lint` skill for the recommended configuration.

## govulncheck _(works best with Go 1.22+)_

`govulncheck` scans Go code for known vulnerabilities using the Go vulnerability database at `vuln.go.dev`. It uses static call-graph analysis to distinguish potentially reachable vulnerable symbols from affected dependencies; this is not proof that a vulnerable path executes or that an unreported path is safe.

Use a govulncheck release compatible with the project's Go toolchain and review source/binary analysis limitations.

```bash
# Pin in the module (Go 1.24+)
go get -tool golang.org/x/vuln/cmd/govulncheck@latest

# Scan source code
go tool govulncheck ./...

# Scan a compiled binary
go tool govulncheck -mode=binary ./myapp
```

## Profile-Guided Optimization (PGO) _(Go 1.21+)_

PGO is generally available since Go 1.21. Gains depend on whether the CPU profile represents the production workload:

```bash
# 1. Build and run with CPU profiling
go test -run='^$' -cpuprofile=default.pgo -bench=. ./internal/engine

# 2. Prefer a representative service CPU profile; a single-package benchmark
#    profile is useful only when it exercises the production hot paths.
#    Place default.pgo in the main package directory
# 3. Rebuild — PGO is applied automatically
go build ./...
```

Go 1.22+ expanded PGO to devirtualize more interface calls. Go 1.23+ reduced PGO build time overhead to single digits.

## `go fix` and `go doc` changes _(Go 1.27+)_

Use `go fix` or the project's configured modernize linter on affected packages to catch routine older forms. Review the patch and preserve behavior; diagnostics supplement judgment. `go fix` now applies more of the `modernize` fixer suite automatically. Coverage shifted between Go 1.26 and 1.27:

- **Added**: `atomictypes`, `embedlit`, `slicesbackward`, `unsafefuncs`
- **Removed**: `fmtappendf` (superseded by other fixers)
- **Renamed**: `waitgroup` → `waitgroupgo`

```bash
go fix -diff ./path/to/affected/package  # preview; nonzero status also means changes available
go fix ./path/to/affected/package        # apply and inspect the resulting diff
go tool fix help                        # check installed fixer coverage
```

`go doc` accepts `package@version` to look up a specific module version without changing `go.mod`, and `-ex` lists executable examples:

```bash
go doc golang.org/x/tools/cmd/stringer@v0.30.0
go doc -ex net/http.Client
```

`go mod tidy` for modules with `go 1.27` or newer auto-merges duplicate `require` blocks and enforces a two-block layout (direct dependencies, then indirect), preserving existing comments.

`go test -json` output gained an optional `OutputType` field (`"error"`, `"error-continue"`, `"frame"`) for consumers that need to distinguish test-output kinds programmatically.

`go tool trace -http` now binds to localhost by default; pass `-http=0.0.0.0:6060` explicitly to expose the trace viewer beyond the local machine (relevant when tracing inside a container or remote dev environment).

## AI-Driven Code Review in CI

Add an AI agent as a PR reviewer alongside traditional static analysis. When configured with this skill plugin, the agent loads the relevant Go skills — `golang-security` for security review, `golang-concurrency` for concurrency issues, `golang-error-handling` for error handling, and so on — providing project-specific context. It may find issues beyond linting, but can miss bugs or produce false positives; it is optional and does not replace tests or review.

See the `samber/cc-skills-golang@golang-continuous-integration` skill for ready-to-use GitHub Actions assets for both Claude Code and GitHub Copilot.
