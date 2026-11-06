# Claude mods

Claude Code plugins under `src/claude/` provide tools and interface changes
through function hooks.

## Setup

Each mod has a link in `skills/`, which Claude reads as `~/.claude/skills/`.
Claude loads them as `<name>@skills-dir`; use `/reload-plugins` in an existing
session. Loaded edits hot-reload, and `claude plugin list` shows status.

## Development

Follow [the mod rules](../src/claude/AGENTS.md), including discovery links and
import boundaries. Run the mod's `check` task for native validation/tests and
`mise run //home:check` for links.

Editor types are generated only with `--plugin-dir`, not skills discovery:

```sh
claude --plugin-dir src/claude/<name>
```

Commit public mods and discovery links in this repository. Private mods and
their discovery entries belong to private source and deploy through private
groups; they do not write links into public source. Names carry no ownership
prefix.
