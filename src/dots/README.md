# dots CLI

`dots` packages recurring agent workflows with structured results and explicit
follow-up commands; native tools remain available for everything else.

## Setup

```sh
mise run //src/dots:install
dots --help
```

Installation links `~/.local/bin/dots` and its generated discovery skill.
`mise run //src/dots:build` only stages the executable in `dist/bin/`.

## Use

- [Inspect CI](../../docs/github-ci.md) on PRs, commits, refs, and run attempts.
- [Read review threads](../../docs/github-reviews.md).
- [Track shared project tasks](../../docs/tasks.md).
- [Associate PRs with Herdr workspaces](../herdr/github/README.md).

Agents should request `--json`; structured follow-ups preserve that mode.
Commands describe their own effects—there is no blanket read-only guarantee.

Installed help is authoritative if the executable and skill versions differ.
Codex and Pi discover skills through `~/.agents/skills`; Claude uses
`~/.claude/skills`. A running harness may need a reload or new session.

## Development

[src/command.rs](src/command.rs) owns parsing, help, and generated command
discovery. [src/skills.rs](src/skills.rs) supplies workflow guidance. Regenerate
after changing either:

```sh
mise run //src/dots:skills
mise run //src/dots:check
```

Generation is offline and uses no model, credentials, or session data.
Contributor rules live in [AGENTS.md](AGENTS.md).
