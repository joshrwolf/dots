---
name: golang-modernize
description: "Modernize Go code for its supported toolchain. Use when adopting language features, replacing deprecated APIs, or upgrading Go and related tooling."
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

**Modes:**

- **Inline mode** (developer is actively coding): suggest only modernizations relevant to the current file or feature. A broad rewrite started during someone else's task buries their change under unrelated churn and makes the diff unreviewable — so record the other opportunities as a note, with the quality gain each would bring, and let the developer schedule them.
- **Full-scan mode** — for a requested broad audit, inspect deprecated APIs, language features, standard library changes, testing patterns, and tooling within the supported Go version. Report and prioritize findings; implement a migration when requested.

# Go Code Modernization Guide

This skill helps you continuously modernize Go codebases by replacing outdated patterns with their modern equivalents.

**Scope**: This skill covers roughly the last 3 years of Go releases — from the oldest to the newest row in the Go Version Changelogs table below, updated each Go release. Projects targeting an older `go.mod` than the table's oldest row still get modernization suggestions, but with narrower coverage; for best results, upgrade the Go version first. Some older modernizations (e.g., `any` instead of `interface{}`, `errors.Is`/`errors.As`, `strings.Cut`) are included because they are still commonly missed, but many pre-1.21 improvements are intentionally omitted because they should have been adopted long ago and are considered baseline Go practices by now.

Keep modernization within the requested scope. Do not turn an unrelated implementation task into a modernization campaign.

## Workflow

When invoked:

1. **Check the project's `go.mod` or `go.work`** to determine the current Go version (`go` directive)
2. **Use the supported Go version** as the modernization target; propose toolchain upgrades only within the requested scope
3. **Read `.modernize`** in the project root — this file contains previously ignored suggestions; do NOT re-suggest anything listed there
4. **Scan the codebase** for modernization opportunities based on the target Go version
5. **Run `golangci-lint`** with the `modernize` linter if available, and `go test ./...` — Go 1.27+ runs the `stdversion` vet check by default, flagging APIs newer than the module's `go` directive; bump the directive or revert the suggestion, don't ignore the hit
6. **Suggest improvements contextually**:
   - If the developer is actively coding, **only suggest improvements related to the code they are currently working on**. Do not refactor unrelated files. Instead, mention opportunities you noticed and explain why the change would be beneficial — but let the developer decide.
   - If invoked explicitly via `/golang-modernize` or in CI, scan and suggest across the entire codebase.
7. **Group findings by affected package and prerequisite**. Keep changes coherent and follow the agreed workflow; a broad scan does not require multiple agents or worktrees.
8. **Before applying a dependency update**, review its changelog and compatibility with the supported toolchain. Scope module changes and run affected tests. Clarify only unresolved compatibility requirements.
9. **Respect ignored suggestions** for the session. Update `.modernize` only when persistent tracking is requested or established by project instructions.

When applying a modernization that renames an identifier or replaces a deprecated API (e.g. `reflect.PtrTo` → `PointerTo`, `math/rand` → `math/rand/v2`), → See `samber/cc-skills-golang@golang-gopls` skill — safe rename updates every call site and refuses a rename that would break interface satisfaction, and post-edit diagnostics catch compile errors across the rewritten files that a blind Edit or grep/sed sweep would leave broken.

### `.modernize` file format

```
# Ignored modernization suggestions
# Format: <date> <category> <description>
2026-01-15 slog-migration Team decided to keep zap for now
2026-02-01 math-rand-v2 Legacy module requires math/rand compatibility
```

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

When the project's `go.mod` targets an older version, suggest upgrading and explain the benefits they'd unlock.

## Using the modernize linter

The `modernize` linter (available since **golangci-lint v2.6.0**) automatically detects code that can be rewritten using newer Go features. It originates from `golang.org/x/tools/go/analysis/passes/modernize`; `gopls` and `go fix` (rewritten onto the `go/analysis` framework in Go 1.26, with fixer coverage still growing in Go 1.27 — see [Tooling modernization](./references/tooling.md) for the exact fixer list) cover overlapping modernization checks, but exact coverage differs by tool version. See the `samber/cc-skills-golang@golang-lint` skill for configuration.

## Version-specific modernizations

For detailed before/after examples for each Go version (1.21–1.27) and general modernizations, see [Go version modernizations](./references/versions.md).

## Tooling modernization

For CI tooling, govulncheck, PGO, golangci-lint v2, and AI-powered modernization pipelines, see [Tooling modernization](./references/tooling.md).

## Deprecated Packages Migration

| Deprecated | Replacement | Since |
| --- | --- | --- |
| `math/rand` | `math/rand/v2` | Go 1.22 |
| `crypto/elliptic` (most functions) | `crypto/ecdh` | Go 1.21 |
| `reflect.SliceHeader`, `StringHeader` | `unsafe.Slice`, `unsafe.String` | Go 1.21 |
| `reflect.PtrTo` | `reflect.PointerTo` | Go 1.22 |
| `runtime.GOROOT()` | `go env GOROOT` | Go 1.24 |
| `runtime.SetFinalizer` | `runtime.AddCleanup` | Go 1.24 |
| `crypto/cipher.NewOFB`, `NewCFB*` | AEAD modes or `NewCTR` | Go 1.24 |
| `golang.org/x/crypto/sha3` | `crypto/sha3` | Go 1.24 |
| `golang.org/x/crypto/hkdf` | `crypto/hkdf` | Go 1.24 |
| `golang.org/x/crypto/pbkdf2` | `crypto/pbkdf2` | Go 1.24 |
| `testing/synctest.Run` | `testing/synctest.Test` | Go 1.25 |
| `crypto/rsa.EncryptPKCS1v15` for new encryption use | RSA-OAEP (`rsa.EncryptOAEP` / `rsa.EncryptOAEPWithOptions`) or HPKE/KEM design | Go 1.26 |
| `net/http/httputil.ReverseProxy.Director` | `ReverseProxy.Rewrite` | Go 1.26 |
| `crypto/tls.Config.Rand` | `testing/cryptotest.SetGlobalRandom()` | Go 1.27 |
| `github.com/google/uuid` (simple cases) | `uuid` (stdlib) | Go 1.27 |

## Go 1.27+ version-bump risk checklist

Several Go 1.27 changes need **verification, not a rewrite**, before a `go.mod` bump ships. Most notably: a `godebug` line in `go.mod` (or `//go:debug` comment) still pinning `asynctimerchan`, `tlsunsafeekm`, `tlsrsakex`, `tls3des`, `tls10server`, `x509keypairleaf`, or `gotypesalias` to its **old** value now fails the build. Full checklist in [Go version modernizations](./references/versions.md#go-127-version-bump-risk-checklist-verify-dont-rewrite).

## Migration Priority Guide

When modernizing a codebase, prioritize changes by impact:

### High priority (safety and correctness)

1. Remove loop variable shadow copies _(Go 1.22+)_ — prevents subtle bugs
2. Replace `math/rand` with `math/rand/v2` _(Go 1.22+)_ — remove `rand.Seed` calls
3. Use `os.Root` for user-supplied file paths _(Go 1.24+)_ — prevents path traversal
4. Run `govulncheck` _(Go 1.22+)_ — catch known vulnerabilities
5. Use `errors.Is`/`errors.As` instead of direct comparison _(Go 1.13+)_
6. Migrate deprecated crypto packages _(Go 1.24+)_ — security critical
7. Before bumping to `go 1.27`, resolve removed `GODEBUG` keys and `crypto/tls.Config.Rand` callers _(Go 1.27+)_ — see the risk checklist above; a stale `GODEBUG` value now fails the build

### Medium priority (readability and maintainability)

8. Replace `interface{}` with `any` _(Go 1.18+)_
9. Use `min`/`max` builtins _(Go 1.21+)_
10. Use `range` over int _(Go 1.22+)_
11. Use `slices` and `maps` packages _(Go 1.21+)_
12. Use `cmp.Or` for default values _(Go 1.22+)_
13. Use `sync.OnceValue`/`sync.OnceFunc` _(Go 1.21+)_
14. Use `sync.WaitGroup.Go` _(Go 1.25+)_
15. Use `t.Context()` in tests _(Go 1.24+)_
16. Use `b.Loop()` in benchmarks _(Go 1.24+)_
17. Use generic methods for helpers scoped to one type, and `strings.CutLast`/`bytes.CutLast` instead of `LastIndex` slicing _(Go 1.27+)_
18. Migrate to the `encoding/json/v2` API — the new default since Go 1.27; review its duplicate-key and invalid-UTF-8 strictness against real payloads first _(Go 1.27+)_

### Lower priority (gradual improvement)

19. Migrate to `slog` from third-party loggers _(Go 1.21+)_
20. Adopt iterators where they simplify code _(Go 1.23+)_
21. Replace `sort.Slice` with `slices.SortFunc` _(Go 1.21+)_
22. Use `strings.SplitSeq` and iterator variants _(Go 1.24+)_
23. Move tool deps to `go.mod` tool directives _(Go 1.24+)_
24. Enable PGO for production builds _(Go 1.21+)_
25. Upgrade to golangci-lint v2 with modernize linter _(golangci-lint v2.6.0+)_
26. Add `govulncheck` to CI pipeline
27. Set up monthly modernization CI pipeline
28. Replace `google/uuid`/`gofrs/uuid` with the stdlib `uuid` package, after checking for RFC-variant features the stdlib doesn't cover _(Go 1.27+)_
29. Run `go fix ./...` after a toolchain upgrade to apply the safe automated transformations _(Go 1.27+)_
30. Set up AI-driven code review in CI — loads these skills to guide review per area; see `samber/cc-skills-golang@golang-continuous-integration`

## Related Skills

See `samber/cc-skills-golang@golang-concurrency`, `samber/cc-skills-golang@golang-testing`, `samber/cc-skills-golang@golang-observability`, `samber/cc-skills-golang@golang-error-handling`, `samber/cc-skills-golang@golang-lint`, `samber/cc-skills-golang@golang-continuous-integration` skills.

- → See `samber/cc-skills-golang@golang-refactoring` skill for staging a large modernization sweep as small human-reviewed PRs instead of one big worktree sweep.
