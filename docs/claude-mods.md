# Claude mods

Mods are Claude Code plugins built from function hooks: tools, panes, slash
commands, and other interface changes. Ours live in `src/claude/<name>/`.

## Install and edit

Each mod has a committed relative link in the skills package:

```sh
ln -s ../../../../src/claude/<name> home/skills/.claude/skills/<name>
```

`mise run install` (or `mise run //skills:install`) stows it to `~/.claude/skills/<name>`, and Claude
loads it as `<name>@skills-dir`: run `/reload-plugins` to pick up a new mod in
a running session. Saving a loaded mod reloads it, with no per-session prompt.
`claude plugin list` shows each mod's status.

`mise run //codex-agent:check` validates and tests the mod.
`mise run //home:check` checks discovery links.
Claude writes editor types into `.claude-plugin/types/` only for mods loaded
with `--plugin-dir`; run `claude --plugin-dir src/claude/<name>` when you
want them.

## Sharing code

A mod can import only files inside its own folder. Shared behaviour lives in a
kit mod: it adds a noun to `$` and declares it in `types/index.d.ts`. Other mods
list it under `dependencies` in `plugin.json` and call it as `$.<noun>`.

## Machine-local mods

`src/claude/local-<name>/` and its link stay out of Git. Create or
remove the link by hand, as above.

## Mods

- `codex-agent`: a `codex` tool that runs a task on an OpenAI model with
  `codex exec`, billed to your ChatGPT subscription. Log in once with
  `codex login`. It runs read-only by default; pass `session_id` to continue
  a thread, or `background: true` to get the result later as a message.
  `/codex` opens a pane with each run's live progress and a Stop button.
  Reloading the mod stops its running jobs.
