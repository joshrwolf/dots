---
name: golang-project-layout
description: "Organize Go projects and workspaces. Use for new project setup, package boundaries, cmd/internal conventions, module layout, and repository configuration."
user-invocable: true
license: MIT
compatibility: Designed for Claude Code, Codex or similar harness, and for projects using Golang.
metadata:
  author: samber
  version: "1.4.2"
  openclaw:
    emoji: "📁"
    homepage: https://github.com/samber/cc-skills-golang
    requires:
      bins:
        - go
    install: []
allowed-tools: Read Edit Write Glob Grep Bash(go:*) Bash(golangci-lint:*) Bash(git:*) Agent AskUserQuestion
---

# Go Project Layout

## Choose structure and wiring

Follow the requested architecture and existing repository conventions. For a new
project, start with the simplest structure that meets its needs; a small CLI does
not need layers or a DI framework. Use ordinary constructors where wiring is needed
and retain an established DI library when appropriate. Ask only when missing
requirements would materially change the architecture or lifecycle design.

See `golang-design-patterns` for architectural tradeoffs and
`golang-dependency-injection` for wiring choices when the task needs them.

## 12-Factor App

For applications (services, APIs, workers), follow [12-Factor App](https://12factor.net/) conventions: config via environment variables, logs to stdout, stateless processes, graceful shutdown, backing services as attached resources, and admin tasks as one-off commands (e.g., `cmd/migrate/`).

## Quick Start: Choose Your Project Type

| Project Type | Use When | Key Directories |
| --- | --- | --- |
| **CLI Tool** | Building a command-line application | `cmd/{name}/`, `internal/`, optional `pkg/` |
| **Library** | Creating reusable code for others | Root package or named subpackages, `internal/` for private code |
| **Service** | HTTP API, microservice, or web app | `cmd/{service}/`, `internal/`, `api/`, `web/` |
| **Monorepo** | Multiple related packages/modules | `go.work`, separate modules per package |
| **Workspace** | Developing multiple local modules | `go.work`, replace directives |

## Module Naming Conventions

### Module Name (go.mod)

Your module path in `go.mod` should:

- **MUST match your repository URL**: `github.com/username/project-name`
- **Use lowercase only**: `github.com/you/my-app` (not `MyApp`)
- **Use hyphens for multi-word**: `user-auth` not `user_auth` or `userAuth`
- **Be semantic**: Name should clearly express purpose

**Examples:**

```go
// ✅ Good
module github.com/jdoe/payment-processor
module github.com/company/cli-tool

// ❌ Bad
module myproject
module github.com/jdoe/MyProject
module utils
```

### Package Naming

Packages MUST be lowercase, singular, and match their directory name. → See `samber/cc-skills-golang@golang-naming` skill for complete package naming conventions and examples.

## Directory Layout

Preserve existing layout unless restructuring is requested. A small command can have a root `main` package; use `cmd/<name>` when separating binaries is useful. Keep entrypoints focused and extract substantial reusable or testable logic as needed. Prefer `internal/` for implementation packages not intended for external import. Public packages need no `pkg/` wrapper.

See [directory layout examples](references/directory-layouts.md) for universal, small project, and library layouts, plus common mistakes.

## Essential Configuration Files

Follow project requirements, including required package documentation and examples. Reuse existing build and lint configuration; add files where they serve the task. These are available templates, not a mandatory scaffold:

- **Makefile** — build automation. See [Makefile template](assets/Makefile)
- **.gitignore** — git ignore patterns. See [.gitignore template](assets/.gitignore)
- **.golangci.yml** — linter config. See the `samber/cc-skills-golang@golang-lint` skill for the recommended configuration

For application configuration with Cobra + Viper, see [config reference](references/config.md).

## Tests, Benchmarks, and Examples

Co-locate `_test.go` files with the code they test. Use `testdata/` for fixtures. See [testing layout](references/testing-layout.md) for file naming, placement, and organization details.

## Go Workspaces

Use `go.work` when developing multiple related modules in a monorepo. See [workspaces](references/workspaces.md) for setup, structure, and commands.

## Initialization Checklist

When starting a new Go project:

- [ ] Use the requested architecture or choose a simple structure appropriate to scope
- [ ] Follow existing wiring conventions; use constructors where needed
- [ ] Decide project type (CLI, library, service, monorepo)
- [ ] Right-size the structure to the project scope
- [ ] Choose module name (matches repo URL, lowercase, hyphens)
- [ ] Run `go version` to detect the current go version
- [ ] Run `go mod init github.com/user/project-name`
- [ ] Choose a root entrypoint or `cmd/{name}` to fit the project
- [ ] Use `internal/` when implementation needs separate private packages
- [ ] Place public packages according to existing conventions; `pkg/` is optional
- [ ] For monorepos: Initialize `go work` and add modules
- [ ] Run `gofmt -s -w .` to ensure formatting
- [ ] Add `.gitignore` with `/vendor/` and binary patterns

## Related Skills

- → See `samber/cc-skills-golang@golang-cli` skill for CLI tool structure and Cobra/Viper patterns.
- → See `samber/cc-skills-golang@golang-dependency-injection` skill for DI approach comparison and wiring.
- → See `samber/cc-skills-golang@golang-lint` skill for golangci-lint configuration.
- → See `samber/cc-skills-golang@golang-continuous-integration` skill for CI/CD pipeline setup.
- → See `samber/cc-skills-golang@golang-design-patterns` skill for architectural patterns.
- → See `samber/cc-skills-golang@golang-refactoring` skill for safely moving or splitting existing code into the layout above by updating affected callers and verifying the resulting dependency graph.
- → Use `golang-how-to` Configure mode only when project skill configuration is requested.
