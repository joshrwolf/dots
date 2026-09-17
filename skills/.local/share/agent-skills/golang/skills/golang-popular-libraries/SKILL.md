---
name: golang-popular-libraries
description: "Select libraries for Go projects. Use when comparing dependencies against the standard library and assessing maintenance, complexity, and suitability."
user-invocable: true
license: MIT
compatibility: Designed for Claude Code, Codex or similar harness, and for projects using Golang.
metadata:
  author: samber
  version: "1.2.2"
  openclaw:
    emoji: "📚"
    homepage: https://github.com/samber/cc-skills-golang
    requires:
      bins:
        - go
    install: []
allowed-tools: Read Edit Write Glob Grep Bash(go:*) Bash(golangci-lint:*) Bash(git:*) Agent WebFetch WebSearch AskUserQuestion Bash(gopls:*) LSP mcp__gopls__*
---

# Go Libraries and Frameworks Recommendations

## Core Philosophy

When recommending libraries, prioritize:

1. **Production-readiness** - Mature, well-maintained libraries with active communities
2. **Simplicity** - Go's philosophy favors simple, idiomatic solutions
3. **Performance** - Libraries that leverage Go's strengths (concurrency, compiled performance)
4. **Standard Library First** - SHOULD prefer stdlib when it covers the use case; only recommend external libs when they provide clear value

## Reference Catalogs

- [Standard Library - New & Experimental](./references/stdlib.md) — v2 packages, promoted x/exp packages, golang.org/x extensions
- [Libraries by Category](./references/libraries.md) — vetted third-party libraries for web, database, testing, logging, messaging, and more
- [Development Tools](./references/tools.md) — debugging, linting, testing, and dependency management tools

Find more libraries here: <https://github.com/avelino/awesome-go>

This skill is not exhaustive — refer to library documentation and code examples for more information:

- Consult `go doc` for the resolved package API; see `golang-go-doc` for lookup guidance.
- Once a candidate is added to your build, → See `samber/cc-skills-golang@golang-gopls` skill (`gopls`) to browse its actual resolved source and compare candidates side by side.

## Dependency Fit

Prefer an existing dependency or the standard library when it already covers the required behavior. For a new dependency, assess the API and transitive footprint against the concrete gap it fills; maintenance and license constraints can rule out an otherwise suitable choice. Make those tradeoffs explicit rather than requiring a separate approval merely because a library is old or unfamiliar.

## Cross-References

- → See `samber/cc-skills-golang@golang-dependency-management` skill for adding, auditing, and managing dependencies
- See `golang-go-doc` for API inspection. Maintenance and license review are separate checks.
- → See `samber/cc-skills-golang@golang-samber-do` skill for samber/do dependency injection details
- → See `samber/cc-skills-golang@golang-samber-hot` skill for samber/hot in-memory caching details
- → See `samber/cc-skills-golang@golang-samber-oops` skill for samber/oops error handling details
- → See `golang-go-cmp` skill for structured comparisons with go-cmp
- → See `samber/cc-skills-golang@golang-grpc` skill for gRPC implementation details
