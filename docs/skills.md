# Skills in dots

Authored skills live once at `skills/<name>/`, with a `SKILL.md` and relative
supporting files. Stow installs discovery links from `home/skills/` into
`~/.agents/skills` for Pi/Codex and `~/.claude/skills` for Claude.

Imported skills remain editable Git subtrees under
`skills/.local/share/agent-skills/`. This is their Git-recorded source prefix,
not an installation payload. Preserve upstream licenses and instruction files;
do not replace source or discovery links with copies.

## Setup

```sh
mise run //skills:check       # canonical definitions and generated CLI docs
mise run //home:check         # discovery links and isolated Stow behavior
mise run //skills:install
```

Installation only restows authored links. It never fetches upstream, generates
aliases or rewrites source. Claude's native validator checks imported definitions
at their real paths because it intentionally does not follow directory symlinks.

## Add a personal skill

Create `skills/example/SKILL.md` with `name: example` and a useful `description`
in YAML frontmatter, then add both discovery links from the repository root:

```sh
ln -s ../../../../skills/example home/skills/.agents/skills/example
ln -s ../../.agents/skills/example home/skills/.claude/skills/example
mise run //skills:check ::: //home:check
mise run //skills:install
```

For machine-local skills, use `local-<name>` for the directory and both aliases.
Git ignores all three. Never force-add private workflows or machine captures.
Installation leaves existing private skills alone. To remove one, remove its
source and both discovery aliases, then restow.

## Imported collections

The Go subtree at `skills/.local/share/agent-skills/golang` tracks
`https://github.com/samber/cc-skills-golang.git`. Native Git operations are explicit:

```sh
mise run //skills:golang:import  # only when initially adding the collection
mise run //skills:golang:update # fetch and merge upstream main
```

These create local commits. Start with a clean tree, review upstream changes and
resolve conflicts normally; `git merge --abort` abandons a conflicted merge.
Neither operation pushes or installs anything.

After an update, add discovery links for new skills and remove obsolete aliases:

```sh
ln -s ../../../../skills/.local/share/agent-skills/golang/skills/golang-example home/skills/.agents/skills/golang-example
ln -s ../../.agents/skills/golang-example home/skills/.claude/skills/golang-example
```

`//home:check` detects missing or misdirected aliases. Review and commit source/link
changes together, then restow. Do not overwrite a personal skill to accommodate
an imported name collision. Another collection needs explicit subtree tasks and
reviewed links, not a new registry or generator.

For an explicitly requested upstream contribution, extract the collection's history:

```sh
git subtree split --prefix=skills/.local/share/agent-skills/golang -b go-skills-export
```

## Generated CLI skill

`mise run //src/dots:skills` generates the `dots` skill and reference from its Rust
command tree. Validation uses `dots skills check`. Discovery links are authored,
not generated. See [the CLI home](../src/dots/README.md).
