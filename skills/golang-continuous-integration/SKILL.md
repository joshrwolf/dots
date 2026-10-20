---
name: golang-continuous-integration
description: "Configure CI and release workflows for Go projects. Use for GitHub Actions, test and lint gates, dependency automation, or GoReleaser."
user-invocable: true
license: MIT
compatibility: Designed for Claude Code, Codex or similar harness, and for projects using Golang.
metadata:
  author: samber
  version: "1.4.2"
  openclaw:
    emoji: "🚀"
    homepage: https://github.com/samber/cc-skills-golang
    requires:
      bins:
        - go
        - goreleaser
        - gh
    install:
      - kind: brew
        formula: goreleaser
        bins: [goreleaser]
      - kind: brew
        formula: gh
        bins: [gh]
      - kind: npm
        package: skills
        bins: [skills]
allowed-tools: Read Edit Write Glob Grep Bash(go:*) Bash(golangci-lint:*) Bash(git:*) Agent WebFetch Bash(goreleaser:*) Bash(gh:*) AskUserQuestion
---

When extending an existing pipeline, inspect its workflows and propose targeted
additions without duplicating existing steps.

**Dependencies:**

- goreleaser: `go install github.com/goreleaser/goreleaser/v2@latest`
- gh: `brew install gh`

# Go Continuous Integration

Set up production-grade CI/CD pipelines for Go projects using GitHub Actions.

## Action Versions

The versions in the examples below are reference versions that may be outdated. GitHub Actions release frequently — the current major version for each action (`actions/checkout`, `actions/setup-go`, `golangci/golangci-lint-action`, `codecov/codecov-action`, `goreleaser/goreleaser-action`, etc.) may differ from what is shown here.

## Quick Reference

| Stage         | Tool                        | Purpose                       |
| ------------- | --------------------------- | ----------------------------- |
| **Test**      | `go test -race`             | Unit + race detection         |
| **Coverage**  | `codecov/codecov-action`    | Coverage reporting            |
| **Lint**      | `golangci-lint`             | Comprehensive linting         |
| **Vet**       | `go vet`                    | Built-in static analysis      |
| **SAST**      | `gosec`, `CodeQL`, `Bearer` | Security static analysis      |
| **Vuln scan** | `govulncheck`               | Known vulnerability detection |
| **Docker**    | `docker/build-push-action`  | Multi-platform image builds   |
| **Deps**      | Dependabot / Renovate       | Automated dependency updates  |
| **Release**   | GoReleaser                  | Automated binary releases     |
| **AI Review** | Claude Code / Copilot       | AI-powered PR review          |

---

## Testing

`.github/workflows/test.yml` — see [test.yml](./assets/test.yml)

Adapt the Go version matrix to match `go.mod`:

```
go 1.23   → matrix: ["1.23", "1.24", "1.25", "1.26", "1.27", "stable"]
go 1.24   → matrix: ["1.24", "1.25", "1.26", "1.27", "stable"]
go 1.25   → matrix: ["1.25", "1.26", "1.27", "stable"]
go 1.26   → matrix: ["1.26", "1.27", "stable"]
go 1.27   → matrix: ["1.27", "stable"]
```

Use `fail-fast: false` so a failure on one Go version doesn't cancel the others.

Go 1.27 raises the Darwin floor to macOS 13 (Ventura). `macos-latest`/`macos-14`+ runners are unaffected; only pin an older `macos-12` runner if a project still needs it, and note it can no longer build with a Go 1.27 toolchain.

Test flags:

- `-race`: run on a supported OS/architecture with cgo available; it detects races on exercised paths. Go races are bugs, but the memory model does not classify all racy behavior as unconstrained undefined behavior
- `-shuffle=on`: Randomize test order to catch inter-test dependencies
- `-coverprofile`: Generate coverage data
- `git diff --exit-code`: Fails if `go mod tidy` changes anything

### Coverage Configuration

Use the project's chosen coverage policy; Configure thresholds in `codecov.yml` at the repo root — see [codecov.yml](./assets/codecov.yml)

---

## Integration Tests

`.github/workflows/integration.yml` — see [integration.yml](./assets/integration.yml)

Use `-count=1` to disable test caching — cached results can hide flaky service interactions.

---

## Linting

`golangci-lint` MUST be run in CI on every PR. `.github/workflows/lint.yml` — see [lint.yml](./assets/lint.yml)

### golangci-lint Configuration

Create `.golangci.yml` at the root of the project. See the `samber/cc-skills-golang@golang-lint` skill for the recommended configuration.

---

## Security & SAST

`.github/workflows/security.yml` — see [security.yml](./assets/security.yml)

Use `govulncheck` for known Go vulnerabilities. Its static analysis identifies potentially reachable symbols for the selected build, not proof of actual runtime calls. The sample SAST jobs report findings; configure required checks or code-scanning merge protection for the desired gate.

- CodeQL results appear in the repository's Security tab.
- Bearer is good at detecting sensitive data flow issues.

### CodeQL Configuration

Create `.github/codeql/codeql-config.yml` to use the extended security query suite — see [codeql-config.yml](./assets/codeql-config.yml)

Available query suites:

- **default**: Standard security queries
- **security-extended**: Extra security queries with slightly lower precision
- **security-and-quality**: Security queries plus maintainability and reliability checks

### Container Image Scanning

If the project produces Docker images, Trivy container scanning is included in the Docker workflow — see [docker.yml](./assets/docker.yml)

---

## Dependency Management

### Dependabot

`.github/dependabot.yml` — see [dependabot.yml](./assets/dependabot.yml)

Minor/patch updates are grouped into a single PR. Major updates get individual PRs since they may have breaking changes.

#### Auto-Merge for Dependabot

`.github/workflows/dependabot-auto-merge.yml` — see [dependabot-auto-merge.yml](./assets/dependabot-auto-merge.yml)

> **Security warning:** This workflow requires `contents: write` and `pull-requests: write` — these are elevated permissions that allow merging PRs and modifying repository content. The `if: github.actor == 'dependabot[bot]'` guard restricts execution to Dependabot only. Do not remove this guard. Actor identity is supplied by GitHub, but identity alone does not establish the safety of a change. **Required checks and repository merge rules enforce the merge policy**. Ensure branch protection is configured (see [Repository Security Settings](#repository-security-settings)) with required status checks and required approvals so that auto-merge only succeeds after all checks pass, regardless of who triggered the workflow.

### Renovate (alternative)

Renovate is a more mature and configurable alternative to Dependabot. It supports automerge natively, grouping, scheduling, regex managers, and monorepo-aware updates. If Dependabot feels too limited, Renovate is the go-to choice.

Install the [Renovate GitHub App](https://github.com/apps/renovate), then create `renovate.json` at the repo root — see [renovate.json](./assets/renovate.json)

Key advantages over Dependabot:

- **`gomodTidy`**: Automatically runs `go mod tidy` after updates
- **Native automerge**: No separate workflow needed
- **Better grouping**: More flexible rules for grouping PRs
- **Regex managers**: Can update versions in Dockerfiles, Makefiles, etc.
- **Monorepo support**: Handles Go workspaces and multi-module repos

---

## Release Automation

GoReleaser automates binary builds, checksums, and GitHub Releases. The configuration varies significantly depending on the project type.

### Release Workflow

`.github/workflows/release.yml` — see [release.yml](./assets/release.yml)

> **Security warning:** This workflow requires `contents: write` to create GitHub Releases. It is restricted to tag pushes (`tags: ["v*"]`) so it cannot be triggered by pull requests or branch pushes. Use tag rulesets and a protected release environment where the project requires controls on who may publish.

### GoReleaser for CLI/Programs

Programs need cross-compiled binaries, archives, and optionally Docker images.

`.goreleaser.yml` — see [goreleaser-cli.yml](./assets/goreleaser-cli.yml)

### GoReleaser for Libraries

Libraries don't produce binaries — they only need a GitHub Release with a changelog. Use a minimal config that skips the build.

`.goreleaser.yml` — see [goreleaser-lib.yml](./assets/goreleaser-lib.yml)

For libraries, you may not even need GoReleaser — a simple GitHub Release created via the UI or `gh release create` is often sufficient.

### GoReleaser for Monorepos / Multi-Binary

When a repository contains multiple commands (e.g., `cmd/api/`, `cmd/worker/`).

`.goreleaser.yml` — see [goreleaser-monorepo.yml](./assets/goreleaser-monorepo.yml)

### Docker Build & Push

For projects that produce Docker images. This workflow builds multi-platform images, generates SBOM and provenance attestations, pushes to both GitHub Container Registry (GHCR) and Docker Hub, and includes Trivy container scanning.

`.github/workflows/docker.yml` — see [docker.yml](./assets/docker.yml)

> **Security warning:** Permissions are scoped per job: the `container-scan` job only gets `contents: read` + `security-events: write`, while the `docker` job gets `packages: write` (to push to GHCR). This ensures the scan job cannot push images even if compromised. The `push` flag is set to `false` on pull requests so untrusted code cannot publish images. The `DOCKERHUB_USERNAME` and `DOCKERHUB_TOKEN` secrets must be configured in the repository secrets settings — never hardcode credentials.

Key details:

- **Buildx**: Supports multi-platform builds; QEMU is one option when executing foreign-architecture binaries. Native builders or cross-compilation can avoid emulation. The example targets (`linux/amd64,linux/arm64`). Remove platforms you don't need.
- **`push: false` on PRs**: Images are built but never pushed on pull requests — this validates the Dockerfile without publishing untrusted code.
- **Metadata action**: Automatically generates semver tags (`v1.2.3` → `1.2.3`, `1.2`, `1`), branch tags (`main`), and SHA tags.
- **Provenance + SBOM**: `provenance: mode=max` and `sbom: true` generate supply chain attestations. BuildKit attaches these to registry images; these options alone do not need GitHub's `attestations: write` or `id-token: write` permissions. GitHub artifact attestations are a separate integration.
- **Dual registry**: Pushes to both GHCR (using `GITHUB_TOKEN`, no extra secret needed) and Docker Hub (requires `DOCKERHUB_USERNAME` + `DOCKERHUB_TOKEN` secrets). Remove the Docker Hub login and image line if not needed.
- **Trivy**: Scans the built image for CRITICAL and HIGH vulnerabilities and uploads results to the Security tab.
- Adapt the image names and registries to your project. For GHCR-only, remove the Docker Hub login step and the `docker.io/` line from `images:`.

---

## Repository Security Settings

Repository security settings (branch protection, workflow permissions, secrets, environments) form the security foundation for the CI pipeline — these are documented in [repo-security.md](./references/repo-security.md).

---

## AI-Driven Code Review

When requested, AI review can complement traditional static analysis. It can miss bugs and produce false positives. When loaded with this skill plugin, the agent applies the relevant Go skills per review area — catching architectural drift, logic bugs, missing error context, and concurrency hazards that linters cannot detect.

> **Cost note:** AI review agents run concurrently per PR. For cost control, remove jobs you don't need or raise the PR trigger filter to specific branches only.

Each subsection below is a generated artifact targeting one specific reviewer — the linked asset file runs on a CI runner, not the developer's local harness, so its tool names and permission flags are deliberately literal rather than capability prose.

### Claude Code

`.github/workflows/ai-review.yml` — see [claude-code-review.yml](./assets/claude-code-review.yml)

The workflow runs parallel jobs, each scoped to a set of review areas and priority level:

| Job | Areas | Priority |
| --- | --- | --- |
| `quality` | Code style, Naming, Documentation, Design patterns | Suggestion-first |
| `correctness` | Error handling, Code safety, Concurrency | Blocking-first |
| `security` | Security, Dependencies | Blocking-first |
| `quality-depth` | Tests, Performance, Observability, Modernize | Mixed |

Additional skills that may be relevant depending on the project: `golang-cli`, `golang-context`, `golang-data-structures`, `golang-database`, `golang-dependency-injection`, or any library-specific skill.

The Claude Code GitHub App integration is configured via the `/install-github-app` command, which sets up the required API secrets.

### GitHub Copilot

Use the project's approved, pinned skills in the repository, then adapt [copilot-review-instructions.md](./assets/copilot-review-instructions.md) to `.github/copilot-instructions.md`:

Copilot review support for loading skills differs from an interactive agent. Keep critical review guidance in `.github/copilot-instructions.md`; do not assume a symlink causes referenced files to be loaded.

---

## Common Mistakes

| Mistake | Fix |
| --- | --- |
| Race checks absent on concurrent code | Add `go test -race` on a supported CI target |
| No `-shuffle=on` | Randomize test order to catch inter-test dependencies |
| Caching integration test results | Use `-count=1` to disable caching |
| `go mod tidy` not checked | Add `go mod tidy && git diff --exit-code` step |
| Missing `fail-fast: false` | One Go version failing shouldn't cancel other jobs |
| Not pinning action versions | Use a verified full commit SHA for immutable pinning; major tags are readable examples but can move |
| No `permissions` block | Follow least-privilege per job |
| Ignoring govulncheck findings | Fix or suppress with justification |
| Assuming AI review proves correctness | Keep tests and human review; AI review is optional |

## Related Skills

See `samber/cc-skills-golang@golang-lint`, `samber/cc-skills-golang@golang-security`, `samber/cc-skills-golang@golang-testing`, `samber/cc-skills-golang@golang-dependency-management`, `samber/cc-skills-golang@golang-modernize` skills.
