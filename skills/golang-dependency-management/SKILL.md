---
name: golang-dependency-management
description: "Choose and manage Go dependencies. Use for evaluating libraries against the standard library, go get and go mod tidy, upgrades and version selection, go.mod tool directives, replace/exclude/retract, vendoring, go.work, dependency automation, and govulncheck audits."
license: MIT
---

# Go dependencies

## Choosing a dependency

- Use the standard library when it covers the requirement. Next, reuse a suitable
  dependency already in `go.mod`. Before adding one, check the gap it fills, its
  transitive footprint, maintenance and license.
- A dependency that the authorized feature needs, or that the user requested,
  needs no separate confirmation. Say which package you chose and why.
- Read the resolved API with `go doc` from inside the module. Package lists in
  [library-catalog.md](references/library-catalog.md) are candidates, not
  endorsements. Check the resolved version before relying on them.
- Recent standard-library additions often replace a dependency: `math/rand/v2`,
  `log/slog`, `iter`, `unique`, `crypto/hkdf`/`pbkdf2`/`sha3`, `uuid` (Go 1.27).
  `encoding/json/v2` (Go 1.27) is a new API with different defaults, not a
  drop-in replacement for v1 code or third-party JSON libraries; `encoding/json`
  keeps v1 behavior on the v2 implementation. See
  [stdlib-additions.md](references/stdlib-additions.md) and `golang-modernize`.

## Changing go.mod

- Keep additions and upgrades scoped to the task. An unrelated implementation
  task is not the place for a broad `go get -u ./...`. Prefer `go get -u=patch`
  for routine updates. Read release notes before upgrading anything that touches
  persistence, serialization, networking, auth, crypto or public APIs.
- After a change, run `go mod tidy`, then `go vet`, `go test` and `govulncheck` on
  the affected packages. Review the `go.mod`/`go.sum` diff. Commit `go.sum`.
- Remove a dependency with `go get mod@none` followed by `go mod tidy`.
- On Go 1.24+, pin executable tools with `tool` directives (`go get -tool …`)
  and run them with `go tool name`. Use a `//go:build tools` file only for older
  targets. Do not raise the `go` directive just to add tools.
- A newer local toolchain does not change the module's target. Raise the `go`
  directive deliberately, together with CI and the toolchain choice.
- In modules whose `go` directive is 1.27 or newer, `go mod tidy` merges
  duplicate `require` blocks into the standard direct/indirect layout.
- `go mod verify` checks the module cache against `go.sum`. It does not validate
  `vendor/` or certify that code is safe.

## References

- [versioning.md](references/versioning.md): semver, minimal version selection,
  major-version suffixes.
- [conflicts.md](references/conflicts.md): diagnosing conflicts, `replace`,
  `exclude`, `retract`.
- [auditing.md](references/auditing.md): govulncheck, update checks, test-only
  versus binary dependencies.
- [workspaces.md](references/workspaces.md): `go.work` versus `go.mod`, and what
  not to commit.
- [automated-updates.md](references/automated-updates.md): Dependabot and
  Renovate policy. Workflow files live in `golang-continuous-integration`.
- [visualization.md](references/visualization.md): `go mod graph`, `go mod why`,
  finding bloat.
- [dev-tools.md](references/dev-tools.md): the standard development toolchain.
