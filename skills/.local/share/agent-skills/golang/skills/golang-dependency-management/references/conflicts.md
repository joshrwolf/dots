# Dependency Conflicts & Resolution

## Diagnosing Conflicts

```bash
# See why a module is in your build
go mod why -m github.com/some/module

# See which version is selected
go list -m github.com/some/module

# See the full requirement graph
go mod graph

# List all modules in the build
go list -m all
```

## Resolution Strategies

**Replace the source for a selected version** (for a deliberate patch or fork):

```bash
go mod edit -replace=example.com/pkg@v1.2.0=example.com/pkg@v1.3.1
```

```go
// go.mod
replace example.com/pkg v1.2.0 => example.com/pkg v1.3.1
```

This version-specific replacement applies only if v1.2.0 is selected. It does not force MVS to select that version. For ordinary version changes, prefer `go get module@version`.

**Use a local fork** (for debugging or patching):

```go
replace example.com/pkg => ../my-local-fork
```

**Block a problematic version**:

```bash
go mod edit -exclude=example.com/pkg@v1.3.0
```

Since Go 1.16, requirements on an excluded version are ignored; `go get` or `go mod tidy` may add a higher version to satisfy imports. Inspect the resulting build list.

**Force upgrade a transitive dependency**:

```bash
go get github.com/transitive/dep@v1.5.0
```

This asks `go get` to select that version and update related requirements. If it is lower than an existing requirement, Go may downgrade or remove other modules; a `require` directive alone is only a minimum, not an MVS override.

## Resolution Workflow

1. Run `go mod graph` and `go mod why -m <module>` to understand the dependency chain
2. Identify which of your direct dependencies pulls in the conflicting version
3. Try upgrading the direct dependency first: `go get github.com/direct/dep@latest`
4. If that doesn't resolve it, use `replace` or `exclude` as a temporary fix
5. Run `go mod tidy` to clean up
6. Verify with `go build ./...` and `go test ./...`

**Important**: `replace` and `exclude` directives only take effect in the **main module's** `go.mod`. They are ignored when your module is used as a dependency. Ensure a published library builds with its declared dependencies and does not require consumers to inherit local replacements. Workspace go.work replacements can override main-module replacements.

## Retract (For Module Authors)

Mark versions as broken or accidentally published:

```go
// go.mod
retract v1.0.0         // Contains critical bug in auth
retract [v1.1.0, v1.2.0] // Range of broken versions
```

Retracted versions are still downloadable but `go get` will not select them by default, and `go list -m -u` warns about them.
