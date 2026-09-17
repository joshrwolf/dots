# Skills in dots

Personal skills live in `skills/.agents/skills/`. Third-party collections are
registered in `skill-sources.toml` and imported as squashed Git subtrees under
`skills/.local/share/agent-skills/<source>/`. Everything is tracked in dots;
a fresh clone needs no submodule setup. Preserve upstream licenses and attribution.

## Install and edit

Run `make skills` to regenerate discovery links and restow the skills package.
Plain `make` also regenerates these links. Neither command fetches updates.
Python 3.11+ and GNU Stow are required. Both agents use the same source through
`~/.agents/skills/<name>` and `~/.claude/skills/<name>`.

Edit imported files directly in their subtree and commit changes in dots.
Keep imported-content edits separate from unrelated dotfiles changes where
practical; this makes reviewing updates and extracting contributions easier.
Keep repository instructions in canonical `AGENTS.md` files with sibling
`CLAUDE.md -> AGENTS.md` links, following this repository's convention.

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
