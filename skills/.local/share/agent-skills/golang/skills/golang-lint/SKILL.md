---
name: golang-lint
description: "Configure and interpret Go linting. Use for golangci-lint settings, linter selection, warning fixes, and justified suppressions."
user-invocable: true
license: MIT
compatibility: Designed for Claude Code, Codex or similar harness, and for projects using Golang.
metadata:
  author: samber
  version: "1.4.1"
  openclaw:
    emoji: "🧹"
    homepage: https://github.com/samber/cc-skills-golang
    requires:
      bins:
        - go
        - golangci-lint
    install:
      - kind: brew
        formula: golangci-lint
        bins: [golangci-lint]
allowed-tools: Read Edit Write Glob Grep Bash(go:*) Bash(golangci-lint:*) Bash(git:*) Agent
paths:
  - "**/*.go"
  - ".golangci.yml"
---

**Dependencies:**

- golangci-lint: `go install github.com/golangci/golangci-lint/v2/cmd/golangci-lint@latest`

# Go Linting

Use the repository's existing golangci-lint configuration and lint targets.
The [example configuration](./assets/.golangci.yml) is a starting point when
setting up linting, not a required linter set for every project.

## Quick Reference

```bash
# Run all configured linters
golangci-lint run ./...

# Auto-fix issues where possible
golangci-lint run --fix ./...

# Format code (golangci-lint v2+)
golangci-lint fmt ./...

# Run a single linter only
golangci-lint run --enable-only govet ./...

# List all available linters
golangci-lint linters

# Verbose output with timing info
golangci-lint run --verbose ./...
```

## Configuration

The [example .golangci.yml](./assets/.golangci.yml) illustrates configuration options. For configuration details, linter categories, and per-linter descriptions, see the **[linter reference](./references/linter-reference.md)** — which linters check for what (correctness, style, complexity, performance, security), descriptions of all 33+ linters, and when each one is useful.

## Suppressing Lint Warnings

Use `//nolint` directives sparingly — fix the root cause first.

```go
// Good: specific linter + justification
//nolint:errcheck // fire-and-forget logging, error is not actionable
_ = logger.Sync()

// Bad: blanket suppression without reason
//nolint
_ = logger.Sync()
```

Rules:

1. **//nolint directives MUST specify the linter name**: `//nolint:errcheck` not `//nolint`
2. **//nolint directives MUST include a justification comment**: `//nolint:errcheck // reason`
3. **Configure `nolintlint` with `require-specific: true` and `require-explanation: true` to enforce both rules** — it flags bare `//nolint` and missing reasons
4. **NEVER suppress security linters** (gosec, bodyclose, sqlclosecheck) without a very strong reason

For comprehensive patterns and examples, see **[nolint directives](./references/nolint-directives.md)** — when to suppress, how to write justifications, patterns for per-line vs per-function suppression, and anti-patterns.

## Interpreting Output

Each issue follows this format:

```
path/to/file.go:42:10: message describing the issue (linter-name)
```

The linter name in parentheses tells you which linter flagged it. Use this to:

- Look up the linter in the [reference](./references/linter-reference.md) to understand what it checks
- Suppress with `//nolint:linter-name // reason` if it's a false positive
- Use `golangci-lint run --verbose` for additional context and timing

## Common Issues

| Problem | Solution |
| --- | --- |
| "deadline exceeded" | Set or increase `run.timeout` in `.golangci.yml`; golangci-lint v2 defaults to no timeout (`0`) |
| Too many issues on legacy code | Use `issues.new-from-merge-base: main` for branch changes, or `new-from-rev` for an explicit baseline; ensure that history is fetched |
| Linter not found | Check `golangci-lint linters` — linter may need a newer version |
| Conflicts between linters | Disable the less useful one with a comment explaining why |
| v1 config errors after upgrade | Run `golangci-lint migrate` to convert config format |
| Slow on large repos | Inspect timings and memory pressure before tuning `run.concurrency`; path exclusions filter reports and do not avoid package analysis |

## Organizing Legacy Codebase Cleanup

For a requested legacy lint cleanup, group findings by category. Apply fixes sequentially when they touch the same files, and inspect automatic fixes before proceeding:

- Run `golangci-lint run --fix ./...` for auto-fixable issues
- Fix security linter findings (bodyclose, sqlclosecheck, gosec)
- Fix error handling issues (errcheck, nilerr, wrapcheck)
- Fix style and formatting (gofumpt, goimports, revive)
- Fix code quality (gocritic, unused, ineffassign)

## Cross-References

- → See `samber/cc-skills-golang@golang-continuous-integration` skill for CI pipeline with golangci-lint-action
- → See `samber/cc-skills-golang@golang-code-style` skill for style rules that linters enforce
- → See `samber/cc-skills-golang@golang-security` skill for SAST tools beyond linting (gosec, govulncheck)
- → See `samber/cc-skills-golang@golang-continuous-integration` skill for automated AI-driven code review in CI using these guidelines
