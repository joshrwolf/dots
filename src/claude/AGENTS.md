# Claude mods in dots

- Each mod is one plugin folder at `<name>/`, its directory matching the
  `name` in `.claude-plugin/plugin.json`. Write and edit mods here, never in a
  session's `~/.claude/dev-mods/` folder; decline that folder's hot-reload
  prompt. Commit a relative link per mod,
  `ln -s ../../../../src/claude/<name> home/skills/.claude/skills/<name>`, which
  `mise run //skills:install` stows to `~/.claude/skills/<name>`. Sessions load it as
  `<name>@skills-dir` (a running one after `/reload-plugins`) and hot-reload
  its edits.
- Layout per mod: `.claude-plugin/plugin.json`, `hooks/hooks.json`,
  `hooks/register.ts(x)`, and `tests/*.test.ts`. A mod with `$.state` values
  or a noun on `$` adds `types/index.d.ts`, named as `"types"` in its manifest.
- A hooks module may import only files inside its own mod folder; symlinks and
  paths outside it do not load. Share behaviour through a kit mod: it adds a
  noun to `$` in `engine.create` and declares it in its `types/index.d.ts`
  contract. Consumers list the kit under `dependencies` and never copy its
  contract. Extract a kit once a second mod needs the code, not before.
- Load the `plugin-authoring` skill before writing a mod and read the API in the
  types it writes. Run `mise run //codex-agent:check` for native validation and
  tests, and `mise run //home:check` for discovery links. Skills-dir mods get no editor types; for those, run an
  interactive `claude --plugin-dir src/claude/<name>`.
- Tests fake the host with hooks beneath the plugin (`process.spawn`,
  `tool.call`) and hand-authored data. Never commit captured transcripts,
  process output, or tokens; keep credentials in the tools a mod drives.
- `local-*` mods and their links are machine-local and ignored. Reserve the
  prefix for them. Mod names share the Claude skills namespace and cannot reuse
  a skill's name.
