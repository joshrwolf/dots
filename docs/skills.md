# Skills in dots

Personal skills live in `skills/.agents/skills/`. Third-party collections are
registered in `skill-sources.toml` and imported as squashed Git subtrees under
`skills/.local/share/agent-skills/<source>/`. Shared skills are tracked in dots;
a fresh clone needs no submodule setup. Machine-local skills use the ignored
`local-*` namespace described below. Preserve upstream licenses and attribution.

## Install and edit

Run `make skills` to regenerate discovery links and restow the skills package.
Plain `make` also regenerates these links. Neither command fetches updates.
Python 3.11+ and GNU Stow are required. Codex and Pi use
`~/.agents/skills/<name>`; Claude uses `~/.claude/skills/<name>`.

Edit imported files directly in their subtree and commit changes in dots.
Keep imported-content edits separate from unrelated dotfiles changes where
practical; this makes reviewing updates and extracting contributions easier.
Keep repository instructions in canonical `AGENTS.md` files with sibling
`CLAUDE.md -> AGENTS.md` links, following this repository's convention.

## Machine-local skills

Keep company-specific workflows and references inside this checkout under
`skills/.agents/skills/local-<name>/`. Git ignores these directories and their
generated `skills/.claude/skills/local-<name>` discovery links. The `local-`
prefix is reserved for uncommitted skills; imported collections cannot use it.

For example, create `skills/.agents/skills/local-work-context/SKILL.md`:

```markdown
---
name: local-work-context
description: Load company conventions when working on company projects.
---

Describe which projects this applies to and the instructions to follow.
Keep supporting material in relative paths such as references/conventions.md.
```

The frontmatter name must match the directory name. Run `make skills` to create
the Claude discovery link and install the skill for all three agents. This uses
the same Stow package as shared skills, including when a home discovery directory
is itself a Stow symlink. No separate private checkout or package is needed.

Confirm the source and link are excluded with:

```sh
git check-ignore skills/.agents/skills/local-work-context/SKILL.md \
  skills/.claude/skills/local-work-context
```

These files remain on this machine, are absent from fresh clones, and must never
be force-added to Git. Give private skills distinct names; they add to the catalog
and do not merge with existing skills. Remove a local skill directory and rerun
`make skills` to remove its generated discovery link. Restart agent sessions if
needed; Pi also supports `/reload`.

Skills load their full instructions on demand. Use them for company workflows
and reference material; rules that must apply to every task need an instruction
file or an explicit instruction to load the skill.

## Add a source

Add a named table to `skill-sources.toml`, for example:

```toml
[sources.golang]
url = "https://github.com/samber/cc-skills-golang.git"
ref = "main"
skills = ["skills/*"]
```

Choose a unique lowercase, hyphen-separated source name. `skills` contains
non-overlapping directory glob patterns relative to the imported repository.
Use `["."]` for a repository with one root-level skill, or explicit directories
to select only some skills. Each matched directory must contain a `SKILL.md`
with standard frontmatter; its `name` determines the discovery link name.
Duplicate names across collections and collisions with personal skills fail
before any links change. Sources do not need to share a directory layout.

Commit the registry entry and start with a clean working tree, then run:

```sh
make skills-import SOURCE=golang
```

This imports the full repository and creates a subtree commit, then generates
links for the selected skills. Review the imported instructions, normalize any
repository instruction files to the convention above, and commit the discovery
links and customizations. Run `make skills` to install.

Import newly registered sources one at a time. All registered sources must be
present before link generation can succeed.

## Merge upstream updates

Start with a clean working tree, then run:

```sh
make skills-update SOURCE=golang
```

This fetches the selected source's configured ref and performs a squashed subtree
merge. Git preserves committed customizations and reports conflicts for
overlapping changes. The command creates commits; it does not push anything.
Review the resulting diff, including new skills and changes to instructions.

If the merge conflicts, resolve it and finish with `git commit`, or cancel with
`git merge --abort`. Upstream changes to a renamed `CLAUDE.md` may need to be
applied to our canonical `AGENTS.md`.

After resolving a merge, run `make skills-links`. This adds links for new skills
and removes links for deleted imported skills without removing personal skills.
Commit any link changes, then run `make skills` to restow. Restart the agent
session if its skill catalog does not refresh automatically.

To remove a source, remove its registry entry, explicitly remove its subtree
with `git rm -r`, and run `make skills-links` to remove its discovery links.
Review and commit the removal, then run `make skills`. The helper never deletes
imported source files or customizations itself.

## Optional contributions

Dots is the source of truth. The `joshrwolf/cc-skills-golang` fork is only needed
when publishing changes; it is not an installation or update source. To extract
that imported directory's history into a local branch:

```sh
git subtree split --prefix=skills/.local/share/agent-skills/golang -b go-skills-export
```

Substitute the source directory for other collections. Review the extracted
branch before explicitly pushing it to a fork. No installation or update target
publishes changes.

## Generated CLI skill

The `dots` skill and its command reference come from the CLI command tree.
Run `make cli-skills` after changing commands; `make cli-check` and the full
Rust gate reject stale generated files. `make cli` installs the binary and
shared skills together. See [the CLI home](../apps/dots-cli/README.md).
