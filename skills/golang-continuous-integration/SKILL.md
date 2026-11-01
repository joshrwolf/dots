---
name: golang-continuous-integration
description: "Configure CI, linting, and releases for Go projects. Use for GitHub Actions test, lint, and security workflows, golangci-lint configuration and nolint policy, Go version matrices, coverage, Dependabot or Renovate, GoReleaser, and container image builds."
license: MIT
---

# Go CI, linting and releases

When an existing pipeline or lint configuration is in place, extend it with
targeted additions and do not duplicate steps. The assets are starting points;
check current action and tool versions before copying them.

## Pipeline defaults

- Test with `go test -race -shuffle=on ./...` on a supported OS/arch with cgo.
  Run integration suites with `-count=1` so cached results do not hide flaky
  service interactions.
- Derive the version matrix from the `go` directive: test the minimum it declares
  and `stable`. Add intermediate versions only when consumers really use them. Set
  `fail-fast: false`.
- Fail when `go mod tidy` changes anything (`go mod tidy && git diff --exit-code`).
- Run `go vet`, `golangci-lint` and `govulncheck ./...`. govulncheck reports
  potentially reachable symbols for the selected build; fix findings or record why
  they are suppressed.
- Pin actions to full commit SHAs. Give each job a least-privilege `permissions`
  block. Never run `contents: write` jobs on untrusted pull-request code.

## Linting

- Use the repository's `.golangci.yml`. The [example config](assets/golangci.yml)
  is for new setups only (golangci-lint v2, `version: "2"`; convert v1 configs
  with `golangci-lint migrate`).
- Fix the cause before suppressing. Every `//nolint` names the linter and gives a
  reason: `//nolint:errcheck // best-effort flush on exit`. Enforce this with
  `nolintlint` (`require-specific`, `require-explanation`). Suppressing gosec,
  bodyclose or sqlclosecheck needs a strong reason.
- Enable `modernize` (golangci-lint v2.6+). Apply its fixes, or `go fix`'s, in the
  code you change rather than only reporting them.
- On legacy code, gate only new issues (`issues.new-from-merge-base`) instead of
  disabling linters.

## Assets and references

| Need | Asset |
| --- | --- |
| Test, integration, lint workflows | [test.yml](assets/test.yml), [integration.yml](assets/integration.yml), [lint.yml](assets/lint.yml) |
| govulncheck, CodeQL, SAST | [security.yml](assets/security.yml), [codeql-config.yml](assets/codeql-config.yml) |
| Coverage | [codecov.yml](assets/codecov.yml) |
| Dependency updates | [dependabot.yml](assets/dependabot.yml), [dependabot-auto-merge.yml](assets/dependabot-auto-merge.yml), [renovate.json](assets/renovate.json) |
| Releases | [release.yml](assets/release.yml), GoReleaser for [CLIs](assets/goreleaser-cli.yml), [libraries](assets/goreleaser-lib.yml), [multi-binary repos](assets/goreleaser-monorepo.yml) |
| Container images with SBOM, provenance and Trivy | [docker.yml](assets/docker.yml) |

- Keep the `github.actor == 'dependabot[bot]'` guard in the Dependabot auto-merge
  workflow. Branch protection with required checks is what actually enforces the
  merge policy.
- Libraries often need only a tagged GitHub release, not GoReleaser.
- [lint.md](references/lint.md): golangci-lint commands, interpreting output,
  common configuration problems. Its [linter reference](references/lint/linter-reference.md)
  and [nolint directives](references/lint/nolint-directives.md) go deeper.
- [repo-security.md](references/repo-security.md): branch protection, workflow
  permissions, secrets, environments.
