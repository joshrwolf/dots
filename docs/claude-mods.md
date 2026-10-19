# Claude mods

Claude Code plugins under `src/claude/` provide tools and interface changes
through function hooks.

## Setup

`mise run //skills:install` links authored mods into `~/.claude/skills/`.
Claude loads them as `<name>@skills-dir`; use `/reload-plugins` in an existing
session. Loaded edits hot-reload, and `claude plugin list` shows status.

## Use

The `codex-agent` mod runs `codex exec` through a `codex` tool using your
ChatGPT subscription. Authenticate with `codex login`.

Runs default to read-only. Use `session_id` to continue a thread or
`background: true` for a later result. `/codex` opens live progress and a Stop
button. Reloading the mod stops running jobs.

## Development

Follow [the mod rules](../src/claude/AGENTS.md), including discovery links and
import boundaries. Run `mise run //src/claude/codex-agent:check` for native
validation/tests and `mise run //home:check` for links.

Editor types are generated only with `--plugin-dir`, not skills discovery:

```sh
claude --plugin-dir src/claude/codex-agent
```

Machine-local `local-*` mods and their links remain ignored.
