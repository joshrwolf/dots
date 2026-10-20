# Go development tools

- Use `go doc` for package documentation and `gopls` for semantic navigation,
  diagnostics, and refactoring.
- Run `go test`, `go test -race`, and `go vet` for the relevant packages.
- Use `go tool pprof` and `go tool trace` for runtime evidence; use `benchstat`
  when comparing benchmark samples.
- Use the repository's `golangci-lint` configuration for additional checks.
- Inspect dependencies with `go list -m`, `go mod why`, and `govulncheck`.
- Keep GoReleaser for projects that use it; follow their existing release config.

These tools have distinct responsibilities. Semantic analysis does not replace
runtime debugging or project-specific generators.
