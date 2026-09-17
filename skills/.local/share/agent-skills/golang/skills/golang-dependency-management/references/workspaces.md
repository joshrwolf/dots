# Go Workspaces (go.work)

## go.work vs go.mod

| Scenario                                       | Use       |
| ---------------------------------------------- | --------- |
| Single module project                          | `go.mod`  |
| Developing multiple related local modules      | `go.work` |
| Monorepo with separate Go modules              | `go.work` |
| Testing local changes across module boundaries | `go.work` |
| Published library consumed by others           | `go.mod`  |

## Workspace Commands

```bash
go work init                    # Initialize workspace
go work use ./services/auth     # Add module to workspace
go work edit -dropuse=./old-module    # Remove module from workspace
go work sync                    # Write workspace-selected dependency versions back to each module
```

## Key Points

- Workspaces eliminate the need for `replace` directives during local development — the workspace automatically resolves local modules
- Keep personal workspaces local. If a repository intentionally shares a workspace, track its go.work and generated go.work.sum together under the project's policy
- A workspace affects commands run inside it, including CI. Consumers do not inherit it; test independently published modules with `GOWORK=off` as well
- For workspace directory structure examples, see the `samber/cc-skills-golang@golang-project-layout` skill
