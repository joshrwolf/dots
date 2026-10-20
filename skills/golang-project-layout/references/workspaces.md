<!-- markdownlint-disable ol-prefix -->

# Go Workspaces for Multi-Package Repositories

## Table of Contents

- [When to Use Workspaces](#when-to-use-workspaces)
- [Workspace Structure](#workspace-structure)
- [Creating a Workspace](#creating-a-workspace)
- [Workspace Commands](#workspace-commands)

## When to Use Workspaces

Use Go workspaces (`go.work`) when:

- Developing multiple related modules that import each other
- Building a monorepo with separate Go modules
- Testing local changes across module boundaries
- Avoiding `replace` directives in every module

**Don't use workspaces for:**

- Single-module projects
- Projects that only use external dependencies
- Simple applications

## Workspace Structure

Example monorepo with multiple modules:

```
my-monorepo/
├── go.work                    # Workspace file (see below)
├── pkg/
│   ├── auth/                 # Module 1: github.com/user/my-monorepo/pkg/auth
│   │   ├── go.mod
│   │   ├── cmd/
│   │   │   └── auth-server/
│   │   │       └── main.go
│   │   └── internal/
│   │       └── handler/
│   │           └── auth.go
│   └── user/                 # Module 2: github.com/user/my-monorepo/pkg/user
│       ├── go.mod
│       ├── cmd/
│       │   └── user-server/
│       │       └── main.go
│       └── internal/
│           └── handler/
│               └── user.go
├── cmd/
│   └── api/                 # Module 3: github.com/user/my-monorepo/cmd/api
│       ├── go.mod
│       └── main.go
└── tools/
    └── cli/                  # Module 4: github.com/user/my-monorepo/tools/cli
        ├── go.mod
        └── cmd/
            └── mycli/
                └── main.go
```

## Creating a Workspace

1. **Initialize the workspace:**

```bash
go work init ./pkg/auth ./pkg/user ./cmd/api ./tools/cli
```

For modules requiring Go 1.21, this creates a file shaped like (the go directive follows the toolchain/module requirements):

```go
go 1.21

use (
    ./pkg/auth
    ./pkg/user
    ./cmd/api
    ./tools/cli
)
```

2. **Add modules to workspace:**

```bash
go work use ./pkg/auth
go work use ./pkg/user
go work use ./cmd/api
```

3. **Use modules without replace directives:**

In `pkg/user/go.mod`:

```go
module github.com/user/my-monorepo/pkg/user

go 1.21

require github.com/user/my-monorepo/pkg/auth v0.0.0
```

The workspace resolves pkg/auth to the local module. A placeholder v0.0.0 is only usable with the local workspace; publish and require a real version for builds outside it. Test release builds with GOWORK=off to catch dependencies masked by workspace resolution.

## Workspace Commands

```bash
go work init              # Initialize new workspace
go work use ./path/to/mod # Add module to workspace
go work edit -dropuse=./path # Remove module from workspace
go work sync              # Sync workspace with module changes
```
