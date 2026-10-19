# Issue workspaces

Linear's desktop Custom script hands the selected directory, readable issue
identifier, suggested branch and full prompt to `herdr-issue linear`. Agents use
`herdr-issue open` after resolving the issue through their own MCP. Neither path
calls Linear's API or stores credentials. The command is a standalone Rust tool
in the plugin Cargo workspace; it does not need a plugin manifest or daemon.

## Setup and use

Run `mise run //src/herdr:install` from the repository root to build and install the command with
the optional Herdr Stow package. Run `mise run //skills:install` to install the shared agent
skill. With the target session already running:

```sh
herdr-issue configure-linear --session default --herdr "$HOME/.local/bin/herdr"
```

This atomically updates only `openIssue` in `~/.linear/coding-tools.json`, pointing
to the installed executable. Other top-level settings are preserved. Linear
passes `LINEAR_WORK_DIR`, `LINEAR_ISSUE_IDENTIFIER`, `LINEAR_ISSUE_BRANCH_NAME`,
and `LINEAR_PROMPT` as environment data. Choose Work on issue → Custom script
and a repository directory. The launcher opens/focuses the worktree workspace,
then exits successfully. It never attaches another Herdr client. Linear itself
starts a terminal window to execute custom scripts; that transient window is
separate from the workspace focused in the existing Herdr session. Configuration
pins the absolute Herdr executable so GUI launches need no interactive-shell PATH.

An external session is explicit and must already be running; a missing or
stopped session produces an actionable error. Inside Herdr, agent commands
inherit the injected socket. Explicit `--session` overrides it. Session
registry discovery belongs to `herdrkit::connection` and uses the installed
CLI's bounded, read-only `session list --json`; socket-directory conventions
are not duplicated.

```sh
herdr-issue open --repo /src/example --issue ENG-123 --branch eng-123-fix --no-focus
herdr-issue context --repo /src/example --branch eng-123-fix
```

`open` returns JSON with actual workspace, tab and pane IDs, checkout path, and
state database path. `context` returns the saved identifier, branch, optional
UUID and prompt. From the opened workspace shell, `herdr-issue codex` or `herdr-issue claude`
starts that interactive agent in the same pane with the saved issue prompt.
The launcher resolves the current checkout and branch through Herdr, and replaces
itself with the native agent executable. Existing agent settings and MCP remain
intact. Prompts over 32 KiB use a short instruction to retrieve the complete
stored context, avoiding platform argv limits. Missing context fails before
starting an agent. Reopening from Linear never starts or prompts an agent.

## Identity and recovery

The selected directory is resolved through Herdr's repository listing, then its
root is canonicalized. The repository root and exact branch locate the checkout.
The readable issue identifier and optional MCP-provided UUID guard against
accidental rebinding. Linear's documented custom-script variables do not include
its immutable issue UUID. Branches and readable identifiers are not immutable;
conflicts fail instead of silently creating another workspace. For an intentional
rename, reconcile the checkout, then explicitly `unbind --repo PATH --branch OLD`
and open the new binding. Unbinding deletes only local context.

Private SQLite state lives under `~/.local/state/herdr-issue` (directory 0700,
database 0600), outside Git. A FULL-synchronous pending reservation commits
before workspace mutation. A SQLite immediate transaction serializes mutations
from launchers sharing this state directory. The shared worktree operation
re-lists while holding that lock, opens an existing checkout or creates it,
and recovers a failed creation response by listing the same branch again.
The returned checkout is saved only after the operation succeeds. Missing new
prompt/UUID values preserve existing context. Stored context is a snapshot;
MCP remains responsible for fresh Linear information.

An interrupted process leaves a recoverable pending binding. SQLite and Herdr
cannot commit atomically: a later retry resolves the branch against live Herdr
state. The API has no atomic create-or-open operation, so unrelated clients can
still race; no retry invents a different branch. A competing launcher waits up
to ten seconds for the state lock, then reports that the active launch should
finish before retrying. A moved checkout is resolved by its current branch;
the stored checkout path is a record of the last successful handoff.

## Ownership

- `herdrkit::connection`: validated session selection and bounded discovery.
- `herdrkit::worktree`: typed create-or-reopen operation and topology results.
- `issue`: Linear environment/agent argv adapters, issue validation, durable
  bindings and configuration writer.
- `find`: ordinary local workspaces/worktrees; there is no remote Linear source.

The reusable dynamic picker foundation remains available for other providers.
The old Linear GraphQL client, OAuth/keyring dependencies, authorization UI,
and remote picker cache were removed.

## Validation

`mise run rust-check` covers input validation, exact API requests, lost-response
recovery, unavailable/ambiguous checkouts, repository drift, durable bindings,
identity conflicts, context preservation, and configuration merging. Fixtures
are hand-authored using neutral repository names. The build stages the command
atomically in `dist/bin`; explicit installation copies it into the Herdr Stow package.
