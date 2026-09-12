# dots

GNU stow-managed dotfiles. Package contents mirror `$HOME`, so a file such as
`zsh/.zshrc` here becomes `~/.zshrc`. Repo-specific rules belong in this file.

## Shared agent instructions and skills

- Keep instructions in `AGENTS.md`, with a sibling relative symlink
  `CLAUDE.md -> AGENTS.md` at every instruction scope. Edit the canonical file;
  do not maintain separate copies.
- Keep each skill in one directory with a standard `SKILL.md` containing
  `name` and `description` frontmatter. Use relative paths for supporting files.
- For shared personal skills, use Stow to install the canonical directories at
  `~/.agents/skills/<name>` and individual relative symlinks at
  `~/.claude/skills/<name>`. For repo-local skills, use the equivalent paths
  under the repository. Add both discovery paths when creating a skill.
- Write shared instructions for both tools. Keep tool-specific metadata or
  integrations optional and separate; do not assume a symlink makes
  tool-specific commands, hooks, or permissions portable.

## Stow

The root Makefile lists packages explicitly. Plain `make` restows only the base
`aerospace`, `brew`, `codex`, `ghostty`, `skills`, `theme`, and `zsh` packages. `make
herdr` owns the optional Herdr setup and restows `herdr` and `nvim` after
building the plugins.

Use the root Make targets so package scope remains visible. Herdr plugin build
and check implementation lives in `herdr-plugins/Makefile`; root targets delegate
to it.

## This repo is public

Never commit an employer name, an internal repository or project name, or an
internal hostname or URL — not in code, comments, docs, config, or test
fixtures. (`joshrwolf/...` is my own public namespace and is fine.)

The trap is captured output. Never commit a live capture of anything that
observed this machine — an API snapshot, a process listing, a log file, shell
history. Read the real thing to get its shape right, then hand-author the
fixture with neutral names. That also covers the cases the live session
happened not to contain, which a capture never does.
