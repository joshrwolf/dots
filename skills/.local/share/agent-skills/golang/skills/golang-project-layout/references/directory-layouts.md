# Directory Layouts

Follow existing project conventions and required files. These examples illustrate
choices, not scaffolds to generate automatically. Preserve build and lint setup.

## Small command

```text
my-tool/
├── main.go
├── main_test.go
└── go.mod
```

A root main package is sufficient for a small command. Split files or extract
packages when that improves cohesion, reuse, or testing; no fixed file count applies.

## Multiple commands

```text
project/
├── cmd/
│   ├── server/main.go
│   └── client/main.go
├── internal/
│   ├── config/
│   └── service/
└── go.mod
```

Each command has its own main package. Keep entrypoints focused on configuration,
wiring, and lifecycle; share implementation through focused internal packages.
Use `go build ./cmd/...` to check all command builds or `go build ./cmd/server`
for one command. A main package may contain multiple files and tests.

## Library

```text
my-library/
├── client.go
├── client_test.go
├── internal/
│   └── protocol/
└── go.mod
```

Public APIs can live at the module root or in named subpackages. A `pkg/` directory
is optional; follow existing conventions. Use `internal/` for packages that should
not be externally imported, and expose only symbols callers need.

Add package documentation, executable examples, build files, and lint configuration
as required by the project. Use descriptive package names instead of catch-all
`utils` or `common` packages. Avoid introducing directories without a concrete role.
