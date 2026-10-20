# Issue workspaces

`herdr-issue` opens or resumes a Herdr worktree from a Linear desktop action or
an agent-resolved issue, without calling Linear's API or storing credentials.

## Setup

The [aggregate Herdr installation](../../../docs/build.md#activation) installs
the command. With the target session already running:

```sh
herdr-issue configure-linear --session default --herdr "$HOME/.local/bin/herdr"
```

This updates only `openIssue` in `~/.linear/coding-tools.json`. In Linear, choose
**Work on issue → Custom script** and a repository directory. The launcher focuses
the worktree in the existing session and exits; Linear's transient terminal
window is separate. The configured absolute Herdr path avoids GUI PATH issues.

## Use

Agents resolve the issue through their own MCP, then use the
`herdr-issue` skill or CLI help:

```sh
herdr-issue open --repo /src/example --issue ENG-123 --branch eng-123-fix --no-focus
herdr-issue context --repo /src/example --branch eng-123-fix
```

Inside Herdr, commands use the injected socket; explicit `--session` overrides
it. External sessions must already be running.

From the workspace shell, `herdr-issue codex` or `herdr-issue claude` starts that
agent in the same pane with saved issue context. Opening/reopening from Linear
never starts or prompts an agent. Stored context is a snapshot; MCP supplies
fresh issue information.

## Constraints

Bindings use the canonical repository and exact branch, guarded by issue
identifier and optional UUID. Branches and readable issue IDs are not immutable;
conflicts fail rather than silently rebinding. For an intentional rename,
reconcile the checkout, then `unbind --repo PATH --branch OLD` and reopen.
Unbinding removes only local context.

Private state lives under `~/.local/state/herdr-issue/`. A durable reservation
precedes workspace mutation. SQLite and Herdr cannot commit atomically, so a
retry reconciles the same branch with live state instead of inventing a new one.
Unrelated clients can still race the launcher's lock.

Run `cargo nextest run -p herdr-issue` for scoped validation.
