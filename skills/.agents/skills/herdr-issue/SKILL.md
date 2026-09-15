---
name: herdr-issue
description: Open or resume a Herdr worktree workspace for a Linear issue, or retrieve its saved issue context. Use when the user asks to pick up an issue in Herdr; Linear lookup uses the agent's existing MCP integration.
---

# Herdr issue workspaces

`herdr-issue` is the local workspace handoff. It has no Linear credentials.
Run `herdr-issue --help` for the installed command contract.

For an issue lookup, use the existing Linear MCP to obtain its identifier,
suggested Git branch, and optional UUID. Use the repository the user specified;
do not infer it from the Linear project name. Pass the exact suggested branch.

```sh
herdr-issue open --repo /src/example --issue ENG-123 --branch eng-123-fix --no-focus
```

Inside Herdr the command inherits the caller's session. Outside it, require an
explicit `--session NAME`; sessions must already be running. Omit `--no-focus`
when the user asks to switch to the workspace. Add `--uuid` when MCP provides the
immutable issue UUID. Supply issue context through `--prompt-stdin` using a
literal input file or safely quoted stdin; never interpolate issue text as shell
code. An omitted prompt preserves previously stored context.

The JSON result contains `workspace_id`, `tab_id`, `pane_id`, `checkout`, and
`state_database`. Use the returned IDs. Existing workspaces and their running
agents are preserved; this operation does not start an agent or submit a task.
From a human-controlled shell in the issue checkout, `herdr-issue codex` or
`herdr-issue claude` starts the chosen interactive agent with saved issue context
in that same pane. These commands replace the launcher process; do not run them
inside an agent tool call expecting a finite response.
When coordinating agents programmatically, use the installed Herdr CLI help
and operating skill to start or prompt the appropriate agent separately.

A retry uses the same repository and branch. Do not invent a new branch to
bypass a creation timeout or binding conflict. If the command reports another
launch is active, wait for it to finish before retrying. A binding conflict
requires reconciling the issue/branch identity; `unbind` explicitly removes only
local context and leaves the checkout and workspace intact.

Read saved context with:

```sh
herdr-issue context --repo /src/example --branch eng-123-fix
```

Context is a snapshot supplied at handoff, not a live Linear fetch. Use MCP for
fresh issue details, status changes, or comments within the user's requested
scope. Do not read or alter another agent's MCP credentials or configuration.
