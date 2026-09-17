---
name: golang-dependency-management
description: "Manage Go modules and dependencies. Use for go.mod/go.sum changes, version selection, replace directives, vendoring, workspaces, and vulnerability checks."
user-invocable: true
license: MIT
compatibility: Designed for Claude Code, Codex or similar harness, and for projects using Golang.
metadata:
  author: samber
  version: "1.3.1"
  openclaw:
    emoji: "📦"
    homepage: https://github.com/samber/cc-skills-golang
    requires:
      bins:
        - go
        - govulncheck
    install:
      - kind: go
        package: golang.org/x/vuln/cmd/govulncheck@latest
        bins: [govulncheck]
allowed-tools: Read Edit Write Glob Grep Bash(go:*) Bash(golangci-lint:*) Bash(git:*) Agent Bash(govulncheck:*) AskUserQuestion
---

**Dependencies:**

- govulncheck: `go install golang.org/x/vuln/cmd/govulncheck@latest`

# Go Dependency Management

## Choose dependencies within the task

Prefer the standard library when it meets the requirement. Reuse suitable existing
dependencies before adding another. A dependency needed for the authorized feature,
or explicitly requested by the user, does not require a separate confirmation.
Respect project dependency restrictions and ask only about unresolved choices with
material consequences. Explain the selected package and why it is needed.

Keep additions and upgrades scoped. Inspect `go.mod` and `go.sum` changes and verify
affected code; an existing dependency is not automatically safe to upgrade. Avoid
broad `go get -u` runs as a side effect of an unrelated implementation task.

Before proposing a dependency, evaluate:

- Does the standard library already cover the use case?
- Is the license compatible?
- Are there well-known alternatives?
- What it does and why it's needed?

The `golang-popular-libraries` skill lists candidates, not a security certification. Verify suitability, maintenance, and compatibility for the project's resolved version.

## Key Rules

- Commit `go.sum` when present. Downloads are checked against recorded hashes and, for eligible public modules, the checksum database. `go mod verify` detects changes to downloaded module-cache content; it does not validate vendored files or certify the code as safe
- `govulncheck ./...` or `go tool govulncheck ./...` before every release — catches known CVEs in your dependency tree before they reach production
- Maintenance status, license compatibility, and stdlib alternatives are important considerations before adding a dependency — dependencies can increase attack surface and maintenance burden; linked code, not every module in go.mod, affects binary size
- `go mod tidy` before every commit that changes dependencies — removes unused modules and adds missing ones, keeping go.mod honest

## go.mod & go.sum

### Essential Commands

| Command           | Purpose                                      |
| ----------------- | -------------------------------------------- |
| `go mod tidy`     | Add missing deps, remove unused ones         |
| `go mod download` | Download modules to local cache              |
| `go mod verify`   | Check cached module content has not changed since download |
| `go mod vendor`   | Copy deps into `vendor/` directory           |
| `go mod edit`     | Edit go.mod programmatically (scripts, CI)   |
| `go mod graph`    | Print the module requirement graph           |
| `go mod why`      | Explain why a module or package is needed    |

### Vendoring

Use `go mod vendor` when the project needs checked-in dependency source or builds without module-proxy access. Vendoring alone does not make builds hermetic: toolchains, generators, cgo libraries, and other inputs still need control. Regenerate and review `vendor/` after dependency changes when the project tracks it.

## Installing & Upgrading Dependencies

### Adding a Dependency

```bash
go get github.com/google/uuid          # Latest version
go get github.com/google/uuid@v1.6.0   # Specific version
go get github.com/google/uuid@latest   # Explicitly latest
go get github.com/google/uuid@abcdef123456 # Specific commit (pseudo-version)
```

Inspect versions with `go list -m -versions`, resolved APIs with `go doc`, and known vulnerabilities with `govulncheck`. Public package pages provide additional context.

### Upgrading

```bash
go get -u ./...            # Upgrade dependencies of matched packages (use -t for test dependencies)
go get -u=patch ./...      # Upgrade to latest patch only (safer)
go get github.com/pkg@v1.5 # Upgrade specific package
```

**Prefer `go get -u=patch`** for routine updates. Patch and minor updates are usually lower risk than major upgrades, but still require review. For dependency updates, run:

```bash
go get -u=patch ./...
go mod tidy
go test ./...
go vet ./...
govulncheck ./...   # or: go tool govulncheck ./...
```

Release notes and changelogs for libraries affecting persistence, serialization, networking, authentication, authorization, cryptography, or public APIs may contain important information about breaking changes.

### Removing a Dependency

```bash
go get github.com/google/uuid@none  # Mark for removal
go mod tidy                          # Clean up go.mod and go.sum
```

### Installing CLI Tools

For Go 1.24+ modules, pin executable tools in `go.mod` with `tool` directives. Do not create a new `tools.go` blank-import file unless the module must support Go <1.24.

```bash
# Add tools to the current module.
go get -tool github.com/golangci/golangci-lint/v2/cmd/golangci-lint@latest
go get -tool golang.org/x/vuln/cmd/govulncheck@latest
go get -tool golang.org/x/perf/cmd/benchstat@latest

# Run pinned tools reproducibly.
go tool golangci-lint run ./...
go tool govulncheck ./...
go tool benchstat old.txt new.txt

# Install all module-pinned tools into GOBIN/PATH when needed.
go install tool

# Update pinned tools deliberately, then review go.mod/go.sum.
go get -u tool
go mod tidy
```

`go.mod` shape for a module targeting Go 1.27 or newer. This is an example target, not a cap; keep the project's actual `go` directive and do not change it just to add tools.

```go.mod
module example.com/project

go 1.27

tool (
    github.com/golangci/golangci-lint/v2/cmd/golangci-lint
    golang.org/x/vuln/cmd/govulncheck
    golang.org/x/perf/cmd/benchstat
)
```

For `go 1.27` or newer, `go mod tidy` auto-merges duplicate `require` blocks and enforces a two-block layout (direct dependencies, then indirect), preserving existing comments — no manual cleanup needed after a merge that introduces a second `require` block.

For Go <1.24, an existing `tools.go` convention remains an option. Pin tool versions compatible with that toolchain; current tool releases may require newer Go:

```go
//go:build tools

package tools

import (
    _ "github.com/golangci/golangci-lint/v2/cmd/golangci-lint"
    _ "golang.org/x/vuln/cmd/govulncheck"
)
```

Rule: Go 1.24+ = `tool` directives. Go <1.24 = `tools.go` fallback.

### Module target note

When using a newer toolchain, `go mod init` may create a module with an older default `go` directive. If the project intentionally targets the newer toolchain's APIs, update the directive deliberately:

```bash
go mod edit -go=1.27
go mod tidy
```

For future Go versions, use the project's intended target version. Do not use APIs newer than the module's `go` directive until the project explicitly agrees to upgrade it.

## Deep Dives

- **[Versioning & MVS](./references/versioning.md)** — Semantic versioning rules (major.minor.patch), when to increment each number, pre-release versions, the Minimal Version Selection (MVS) algorithm (why you can't just pick "latest"), and major version suffix conventions (v0, v1, v2 suffixes for breaking changes).

- **[Auditing Dependencies](./references/auditing.md)** — Vulnerability scanning with `govulncheck`, inspecting module updates and build dependencies, and distinguishing test-only vs binary dependencies to keep `go.mod` clean.

- **[Dependency Conflicts & Resolution](./references/conflicts.md)** — Diagnosing version conflicts (what `go get` does when you request incompatible versions), resolution strategies (`replace` directives for local development, `exclude` for broken versions, `retract` for published versions that should be skipped), and workflows for conflicts across your dependency tree.

- **[Go Workspaces](./references/workspaces.md)** — `go.work` files for multi-module development (e.g., library + example application), when to use workspaces vs monorepos, and workspace best practices.

- **[Automated Dependency Updates](./references/automated-updates.md)** — Setting up Dependabot or Renovate for automatic dependency update PRs, auto-merge strategies (when to merge automatically vs require review), and handling security updates.

- **[Visualizing the Dependency Graph](./references/visualization.md)** — `go mod graph` to inspect the full dependency tree, `modgraphviz` to visualize it, and interactive tools to find which dependency chains cause bloat.

## Cross-References

- → See `samber/cc-skills-golang@golang-continuous-integration` skill for Dependabot/Renovate CI setup
- → See `samber/cc-skills-golang@golang-security` skill for vulnerability scanning with govulncheck
- → See `samber/cc-skills-golang@golang-popular-libraries` skill for library candidates

## Quick Reference

```bash
# Start a new module
go mod init github.com/user/project

# Add a dependency
go get github.com/google/uuid@v1.6.0

# Upgrade all deps (patch only, safer)
go get -u=patch ./...

# Remove unused deps
go mod tidy

# Check for vulnerabilities
govulncheck ./...   # or: go tool govulncheck ./...

# Check for outdated deps
go list -m -u all

# Inspect the build package graph
go list -deps ./...

# Understand why a dep exists
go mod why -m github.com/some/module

# Inspect dependency graph
go mod graph

# Verify downloaded module-cache content
go mod verify
```
