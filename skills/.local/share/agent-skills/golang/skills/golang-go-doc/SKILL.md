---
name: golang-go-doc
description: Read Go package and symbol documentation with go doc. Use when checking API signatures, behavior, or source for the project's resolved dependencies.
license: MIT
---

# Go package documentation

Run `go doc` from the relevant module or workspace so documentation matches the
project's selected dependencies and replacements.

```sh
go doc net/http.Client
go doc net/http.Client.Do
go doc -all net/http
go doc -src net/http.Client.Do
```

Start with the specific package or symbol. Use `-all` for a broader API inventory
and `-src` when implementation details matter. For semantic references and safe
refactoring, use `golang-gopls`.

If a package cannot be resolved, inspect `go env GOMOD GOWORK` and
`go list -m -json all`. Do not add a dependency or change the module merely to
read documentation. Consult the official versioned package documentation or
repository source if it is not available locally. Check `go help doc` before
using toolchain-specific flags.

Documentation does not establish maintenance status or vulnerability exposure.
Use `go list -m -u all` for available module updates and `govulncheck` for reachable
known vulnerabilities. Check license files in the resolved module separately.

Reference: [go doc](https://pkg.go.dev/cmd/doc).
