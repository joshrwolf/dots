# Skills in dots

Skills live flat in `skills/<name>/`, each with a `SKILL.md` and relative
supporting files. Codex, Pi and Claude all read that directory directly: the
`skills` Stow package links `~/.agents/skills` (Codex, Pi) and
`~/.claude/skills` (Claude) to it. Edits in the stowed checkout are live; a
running harness may need a reload or a new session.

The `golang-*` skills began as [samber/cc-skills-golang](https://github.com/samber/cc-skills-golang)
and are now maintained here; `skills/LICENSE-cc-skills-golang` keeps the upstream
MIT notice. There is no upstream sync.

## Setup

```sh
mise run //home:check      # skill frontmatter, generated CLI skill, links, Stow simulation
mise run install           # restow the base packages, including skills
```

`~/.agents/skills` and `~/.claude/skills` must be absent or links before
installation; given an existing directory, Stow would link its contents file by
file, so `install` refuses. Move any unmanaged entries into `skills/` first.

## Add a skill

Create `skills/example/SKILL.md` with `name: example` and a useful `description`
in YAML frontmatter, then run `mise run //home:check`. No link or restow is needed.

Everything in `skills/` is visible to every tool. Tools ignore entries without a
`SKILL.md`, so the license file is harmless there. Claude also loads plugin
directories: Claude mods appear as links such as
`skills/codex-agent -> ../src/claude/codex-agent` (see
[Claude mods](claude-mods.md)); Codex and Pi skip them.

Machine-local skills and mod links use `local-<name>`, which Git ignores.
Never force-add private workflows or machine captures.

## Generated CLI skill

`mise run //src/dots:skills` generates the `dots` skill and reference from its Rust
command tree; `dots skills check` validates them. See [the CLI home](../src/dots/README.md).
