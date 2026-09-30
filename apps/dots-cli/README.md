# dots

`dots` is a general-purpose workflow CLI for coding agents. It bridges local
tools, services, and host integrations through a consistent command interface.
Each workflow provides explicit context, structured results, and useful next
steps so an agent can spend less time assembling plumbing.

Native tools remain available. The CLI packages recurring workflows; it does
not try to replace every command offered by an integration.

## Install and discover

From the repository root:

```sh
make cli              # build, generate skills, install executable and skills
dots --help           # discover installed command groups
dots github ci --help # help for the first available workflow
```

The executable lives at `~/.local/bin/dots`. Its Stow package is `cli/`; source
and this README live here in `apps/dots-cli/`. Building with `make cli-build`
stages the executable and regenerates its skill without installing anything.

GitHub CI diagnosis is the first workflow. It accepts PRs, remote refs, commits,
workflow runs/attempts, and direct job/check URLs. See [CI usage](../../docs/github-ci.md).
Future commands belong under their integration or domain; only implemented
capabilities appear in help and skill discovery.

## Agent discovery

The command tree in [command.rs](src/command.rs) defines arguments, help, and
workflow descriptions. The same metadata generates:

- `skills/.agents/skills/dots/SKILL.md`: a concise workflow index and guidance.
- Its `references/commands.md`: full help for the available commands.
- `skills/.claude/skills/dots`: a relative discovery link to that canonical skill.

Codex and Pi use the shared `~/.agents/skills` path; Claude uses
`~/.claude/skills`. Their personal instructions point to the `dots` skill and
help, rather than carrying a separately maintained command list. A running
harness may need to reload skills or start a new session after installation.

```sh
make cli-skills                # regenerate checked-in skill artifacts
dots skills render            # print SKILL.md, offline
dots skills render --reference # print the complete help reference, offline
make cli-check                # reject stale artifacts; check CLI and libraries
```

No model generation, network access, credentials, or live session data is used
to generate skills. General operating guidance is authored in `src/skills.rs`;
available commands and their reference text come from the parser. The checked-in
artifacts make skills available before a binary is built. Installed help remains
authoritative if a skill and executable come from different versions.

## Architecture and extension

The root Cargo workspace owns the lockfile, toolchain, and lint policy.

- `apps/dots-cli`: command parsing, discovery metadata, host context, rendering,
  and exit behavior. Keep the binary thin.
- `crates/`: reusable provider and domain libraries. They return typed results
  without requiring a harness or printing command output.
- `herdr-plugins/`: Herdr adapters, UI presentation, scheduling, and lifecycle.
- `pi-plugins/dots`: Pi-specific integration. It can consume CLI output
  without duplicating provider logic.
- `cli/`: executable installation layout; `skills/`: shared skill installation.

To add a workflow, implement its operations in a focused library, declare its
command in `src/command.rs`, and wire its result into the frontend. Give commands
useful descriptions: when to use them, required context, effects, and how to
continue from partial results. Run `make cli-skills` and add behavior tests.
The generator traverses the command tree, so new groups and subcommands enter
the skill and reference without a second command registry.

The CI library separates validated repository/target selection, a tagged subject
(PR, commit, or pinned run attempt), and immutable evidence references. PR context
is optional provenance. All subjects share filtering, summaries, snapshots,
evidence budgets, and rendering. JSON output exposes the subject
explicitly; consumers should not infer it from a PR field.

Commands must document their own effects rather than inheriting a blanket
read-only promise. Use bounded operations and output, explicit missing-data
states, and stable references when later calls need to address the same object.
Help and discovery must work offline. Prefer shared domain operations over
invoking another frontend to reconstruct its behavior.

`make rust-check` validates the complete workspace, including Herdr adapters;
`make herdr-check` remains an entrypoint to that gate. Plain `make` retains its
existing base Stow scope.
