---
name: golang-modernize
description: "Choose current Go idioms when writing, reviewing, or refactoring code. Use for version-sensitive API choices, proactive modernization, and Go or tooling upgrades."
user-invocable: true
license: MIT
compatibility: Designed for Claude Code, Codex or similar harness, and for projects using Golang.
metadata:
  author: samber
  version: "1.5.1"
  openclaw:
    emoji: "🔄"
    homepage: https://github.com/samber/cc-skills-golang
    requires:
      bins:
        - go
    install: []
    skill-library-version: "1.27"
allowed-tools: Read Edit Write Glob Grep Bash(go:*) Bash(golangci-lint:*) Bash(git:*) Agent WebFetch WebSearch AskUserQuestion EnterWorktree ExitWorktree
paths:
  - "**/*.go"
---

<!-- markdownlint-disable ol-prefix -->

# Go Code Modernization

Prefer current idioms during ordinary implementation and review. Apply
straightforward, behavior-preserving modernizations in affected code and nearby
uses of the same pattern; an explicit modernization request is not necessary.
Existing older syntax is not by itself a reason to repeat it. Keep unrelated,
large migrations separate so the requested change remains reviewable.

## Version and behavior contracts

Check the module's `go` directive, workspace modules, CI matrix, and documented
consumer requirements. Use the latest stable Go for new projects. For existing
projects, move the target forward when useful and no actual support requirement
blocks it; align module directives, toolchain selection, CI, and relevant build
images as one change. A newer installed compiler alone does not change the
module's language or API contract. Do not invent older-version support, or silently
drop explicitly supported consumers to use a newer convenience API.

Prefer stable language and standard-library replacements that simplify the code.
Check semantic differences: pointer identity, nil versus empty collections, JSON
wire behavior, random sequences, cancellation, and public API types can matter.
Newest is not a reason to break those contracts or enable experimental APIs.
For a requested audit, report findings; for implementation, make the relevant
modernizations and verify them rather than merely suggesting them.

Everyday defaults belong to `golang-code-style`, `golang-testing`,
`golang-concurrency`, and `golang-error-handling`. Use this skill's references for
release-specific details and migration hazards. For semantic renames, use
`golang-gopls` and review dynamic or external consumers separately.

## Go Version Changelogs

Reference the relevant changelog when suggesting a modernization:

| Version | Release       | Changelog                   |
| ------- | ------------- | --------------------------- |
| Go 1.21 | August 2023   | <https://go.dev/doc/go1.21> |
| Go 1.22 | February 2024 | <https://go.dev/doc/go1.22> |
| Go 1.23 | August 2024   | <https://go.dev/doc/go1.23> |
| Go 1.24 | February 2025 | <https://go.dev/doc/go1.24> |
| Go 1.25 | August 2025   | <https://go.dev/doc/go1.25> |
| Go 1.26 | February 2026 | <https://go.dev/doc/go1.26> |
| Go 1.27 | August 2026   | <https://go.dev/doc/go1.27> |

For versions newer than Go 1.27, consult the official Go release notes.

## Using the modernize linter

The `modernize` linter (available since **golangci-lint v2.6.0**) automatically detects code that can be rewritten using newer Go features. It originates from `golang.org/x/tools/go/analysis/passes/modernize`; `gopls` and `go fix` (rewritten onto the `go/analysis` framework in Go 1.26, with fixer coverage still growing in Go 1.27 — see [Tooling modernization](./references/tooling.md) for the exact fixer list) cover overlapping modernization checks, but exact coverage differs by tool version. See the `samber/cc-skills-golang@golang-lint` skill for configuration.

## Version-specific modernizations

For detailed before/after examples for each Go version (1.21–1.27) and general modernizations, see [Go version modernizations](./references/versions.md).

## Tooling modernization

For govulncheck, PGO, golangci-lint v2, and tool configuration, see [Tooling modernization](./references/tooling.md).

## API migration candidates

| API to review | Alternative | Alternative available |
| --- | --- | --- |
| `crypto/elliptic` (most functions) | `crypto/ecdh` | Go 1.21 |
| `reflect.SliceHeader`, `StringHeader` | `unsafe.Slice`, `unsafe.String` | Go 1.21 |
| `reflect.PtrTo` | `reflect.PointerTo` | Go 1.22 |
| `runtime.GOROOT()` | `go env GOROOT` | Go 1.24 |
| `runtime.SetFinalizer` (not formally deprecated) | Consider `runtime.AddCleanup` for fallback cleanup | Go 1.24 |
| `crypto/cipher.NewOFB`, `NewCFB*` | AEAD modes or `NewCTR` | Go 1.24 |
| `golang.org/x/crypto/sha3` | `crypto/sha3` | Go 1.24 |
| `golang.org/x/crypto/hkdf` | `crypto/hkdf` | Go 1.24 |
| `golang.org/x/crypto/pbkdf2` | `crypto/pbkdf2` | Go 1.24 |
| `testing/synctest.Run` | `testing/synctest.Test` | Go 1.25 |
| `crypto/rsa.EncryptPKCS1v15` for new encryption use | RSA-OAEP (`rsa.EncryptOAEP` / `rsa.EncryptOAEPWithOptions`) or HPKE/KEM design | Go 1.26 |
| `net/http/httputil.ReverseProxy.Director` | `ReverseProxy.Rewrite` | Go 1.26 |
| `crypto/tls.Config.Rand` | Leave nil in production; `testing/cryptotest.SetGlobalRandom` for tests | Go 1.27 |
| `github.com/google/uuid` (simple cases) | `uuid` (stdlib) | Go 1.27 |

## Go 1.27+ version-bump risk checklist

Several Go 1.27 changes need **verification, not a rewrite**, before a `go.mod` bump ships. Most notably: a `godebug` line in `go.mod` (or `//go:debug` comment) still pinning `asynctimerchan`, `tlsunsafeekm`, `tlsrsakex`, `tls3des`, `tls10server`, `x509keypairleaf`, or `gotypesalias` to its **old** value now fails the build. Full checklist in [Go version modernizations](./references/versions.md#go-127-version-bump-risk-checklist-verify-dont-rewrite).

## Related Skills

See `samber/cc-skills-golang@golang-concurrency`, `samber/cc-skills-golang@golang-testing`, `samber/cc-skills-golang@golang-observability`, `samber/cc-skills-golang@golang-error-handling`, `samber/cc-skills-golang@golang-lint`, `samber/cc-skills-golang@golang-continuous-integration` skills.

- → See `samber/cc-skills-golang@golang-refactoring` skill for Go-specific risks when moving declarations or changing package boundaries.
