# Project Documentation

→ See `samber/cc-skills-golang@golang-continuous-integration` skill for automating changelog generation and release workflows.

## README and contributor docs

Use the existing project structure and setup commands. Optional starting points:
[README](../assets/templates/README.md) and
[CONTRIBUTING](../assets/templates/CONTRIBUTING.md).

## Changelog

For projects using Keep a Changelog, the following example and [template](../assets/templates/CHANGELOG.md) show its categories. Otherwise follow the existing release-note format.

### Format

```markdown
## [1.2.0] - 2026-03-08

### Added

- New `WithTimeout` option for client configuration

### Changed

- Improved retry logic to use exponential backoff

### Fixed

- Race condition in connection pool under heavy load

### Deprecated

- `SetTimeout()` method — use `WithTimeout()` option instead

[1.2.0]: https://github.com/{owner}/{repo}/compare/v1.1.0...v1.2.0
```

### Change Categories

- **Added** — new features
- **Changed** — changes in existing functionality
- **Deprecated** — features that will be removed
- **Removed** — removed features
- **Fixed** — bug fixes
- **Security** — vulnerability fixes

### GitHub Releases as Alternative

For simpler projects, GitHub Releases can replace a CHANGELOG file. GoReleaser auto-generates release notes from git commits.

---

## Distribution

Document the installation methods the project actually supports.

### Dockerfile Best Practices

Use multi-stage builds with a minimal final image:

```dockerfile
# Build stage
FROM golang:1.27-alpine AS builder
WORKDIR /app
COPY go.mod go.sum ./
RUN go mod download
COPY . .
RUN CGO_ENABLED=0 go build -ldflags="-s -w" -o /app/binary ./cmd/server

# Final stage
FROM gcr.io/distroless/static-debian12:nonroot
COPY --from=builder /app/binary /binary
ENTRYPOINT ["/binary"]
```
