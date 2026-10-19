# dots

`dots` is a general-purpose workflow CLI for coding agents. It bridges local
tools, services, and host integrations through a consistent command interface.
Each workflow provides explicit context, structured results, and useful next
steps so an agent can spend less time assembling plumbing.

Agents should request --json. Structured follow-up commands preserve JSON mode;
provider text and native-tool fallback commands are left intact.

Native tools remain available. The CLI packages recurring workflows; it does
not try to replace every command offered by an integration.

## Install and discover

From the repository root:

```sh
mise run //src/dots:install              # build, verify skills, install executable and skills
dots --help           # discover installed command groups
dots github ci --help # help for the first available workflow
```

The executable lives at `~/.local/bin/dots`. Its Stow package is `home/cli/`; source
and this README live here in `src/dots/`. Building with `mise run //src/dots:build`
stages `dist/bin/dots` without installing anything. Skill generation is an
explicit source update through `mise run //src/dots:skills`.

GitHub CI diagnosis is the first workflow. It accepts PRs, remote refs, commits,
workflow runs/attempts, and direct job/check URLs. See [CI usage](../../docs/github-ci.md).
Review-thread inspection lists unresolved feedback and reads exact conversations.
See [review usage](../../docs/github-reviews.md). Shared project tasks coordinate
multi-PR work across sessions. See [task usage](../../docs/tasks.md).
Future commands belong under their integration or domain; only implemented
capabilities appear in help and skill discovery.

## Agent discovery

The command tree in [command.rs](src/command.rs) defines arguments, help, and
workflow descriptions. The same metadata generates:

- `skills/dots/SKILL.md`: a concise workflow index and guidance.
- Its `references/commands.md`: full help for the available commands.
- `home/skills/.claude/skills/dots`: a relative discovery link to that canonical skill.

Codex and Pi use the shared `~/.agents/skills` path; Claude uses
`~/.claude/skills`. Their personal instructions point to the `dots` skill and
help, rather than carrying a separately maintained command list. A running
harness may need to reload skills or start a new session after installation.

```sh
mise run //src/dots:skills                # regenerate checked-in skill artifacts
dots skills render            # print SKILL.md, offline
dots skills render --reference # print the complete help reference, offline
mise run //src/dots:check                # reject stale artifacts; check CLI and libraries
```

No model generation, network access, credentials, or live session data is used
to generate skills. General operating guidance is authored in `src/skills.rs`;
available commands and their reference text come from the parser. The checked-in
artifacts are available before a binary is built. Installed help remains
authoritative if a skill and executable come from different versions.

## Architecture and extension

The root Cargo workspace owns the lockfile, toolchain, and lint policy.

- `src/dots`: command parsing, discovery metadata, host context, rendering,
  and exit behavior. Keep the binary thin.
- `src/github/` and `src/tasks/`: reusable provider and domain libraries. They return typed results
  without requiring a harness or printing command output.
- `src/herdr/`: Herdr adapters, UI presentation, scheduling, and lifecycle.
- `src/pi`: Pi-specific integration. It can consume CLI output
  without duplicating provider logic.
- `home/cli/`: executable staging; `home/skills/`: discovery links;
  `skills/`: canonical skill source.

To add a workflow, implement its operations in a focused library, declare its
command in `src/command.rs`, and wire its result into the frontend. Give commands
useful descriptions: when to use them, required context, effects, and how to
continue from partial results. Run `mise run //src/dots:skills` and add behavior tests.
The generator traverses the command tree, so new groups and subcommands enter
the skill and reference without a second command registry.

The shared github-inspect library owns CI and review-thread inspection over the
same provider and credential-scoped snapshot cache. CI separates validated repository/target selection, a tagged subject
(PR, commit, or pinned run attempt), and immutable evidence references. PR context
is optional provenance. All subjects share filtering, summaries, snapshots,
evidence budgets, and rendering. JSON output exposes the subject
explicitly; consumers should not infer it from a PR field.

Commands must document their own effects rather than inheriting a blanket
read-only promise. Use bounded operations and output, explicit missing-data
states, and stable references when later calls need to address the same object.
Help and discovery must work offline. Prefer shared domain operations over
invoking another frontend to reconstruct its behavior.

`mise run rust-check` validates the complete Rust workspace, including Herdr adapters.
`mise run install` retains the explicit base Stow scope.
