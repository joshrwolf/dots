# dots

GNU stow-managed dotfiles. Package contents mirror `$HOME`, so a file such as
`home/zsh/.zshrc` here becomes `~/.zshrc`. Repo-specific rules belong in this file.

## Shared agent instructions and skills

- Keep repository instructions in `AGENTS.md`. Current Claude Code, Codex, and
  Pi discover these natively; do not add `CLAUDE.md` compatibility links.
  Leave third-party instruction files upstream-owned. Shared personal instructions
  reach Claude through its native user rules directory, not a CLAUDE.md shim.
- Keep each skill in one directory with a standard `SKILL.md` containing
  `name` and `description` frontmatter. Use relative paths for supporting files.
- Keep canonical personal skills at `skills/<name>`. Author relative discovery
  links in `home/skills/.agents/skills/` and `home/skills/.claude/skills/`;
  Stow installs them at the corresponding home paths. Add both when creating
  a skill. See `docs/skills.md` for link targets and validation.
- Machine-local skills live in ignored `skills/local-*/`
  directories. Add both ignored discovery links explicitly with `ln -s`;
  `mise run //skills:install` only restows authored files.
  Never commit these skills or their links; reserve `local-` names for them.
- Write shared instructions for both tools. Keep tool-specific metadata or
  integrations optional and separate; do not assume a symlink makes
  tool-specific commands, hooks, or permissions portable.
- Third-party skills are editable Git subtrees under
  `skills/.local/share/agent-skills/<source>`, with explicit mise import/update
  tasks invoking Git directly. Edit source files in dots; do not replace discovery
  links with copies. `mise run //skills:install` restows locally;
  `mise run //skills:golang:update` explicitly merges upstream. See `docs/skills.md`.

## Third-party subtrees

Stay in sync with upstream and keep local patches minimal. Check upstream before
patching compatibility issues; prefer an upstream update, and remove superseded
patches when updating. Keep personal features outside vendored code.

## Stow

`mise run install` restows the explicit base package list in root `mise.toml`:
`aerospace`, `brew`, `codex`, `ghostty`, `pi`, `skills`, `theme`, and `zsh`.
`mise run //src/herdr:install` explicitly prepares immutable plugin releases,
restows `herdr` and `nvim`, then registers plugins. Build/check never activate.

Mise owns SDK pins and orchestration; native tools own dependency/compiler graphs.
Put ordinary native commands directly in mise tasks, not Python/shell dispatchers.
Do not translate wrappers into another language: remove redundant behavior first.
Keep substantial application logic and acceptance tests in their owning toolchain.
Keep `rust-toolchain.toml` aligned with `mise.toml`; run `mise run doctor`.
Use `mise run deps`, `mise run check`, and `mise run verify`; see `docs/build.md`.
Only proven distributable tasks may use local artifact caching. Dependency
installation, checks, smoke tests, updates, and activation are always uncached.

## Rust applications and shared libraries

- Root `Cargo.toml` owns one workspace across the named owners in `src/` and
  repository contracts in `tests/`. Root `Cargo.lock`, `rust-toolchain.toml`,
  and `clippy.toml` are canonical.
- `src/dots` owns the CLI; `src/github/{client,inspect}` owns shared GitHub code.
  Herdr libraries live at `src/herdr/{kit,prs,review/core}` alongside their
  feature owners. See `src/dots/README.md`.
- `src/tasks` owns the shared task store. Readiness is derived, claims
  are single transactions, and callers pass the project key and time explicitly.
- Use `mise run //src/dots:build`, `mise run //src/dots:check`, and `mise run //src/dots:install` to stage, check, and
  explicitly install local commands. Base Stow installation does not install the CLI.
- `mise run rust-check` is the complete Rust gate. Leaf `:check` tasks check
  their package and upstream dependencies; libraries need no artificial build tasks.

## Pi packages

- Third-party Pi packages use stock npm releases pinned in `pi-packages.json`.
  Our companion package lives in `src/pi`. See `docs/pi.md`.
- Use root `mise run //src/pi:deps`, `mise run //src/pi:check`, and `mise run //src/pi:install`. Installation
  uses prepared releases and Pi's public settings API; upstream packages use Pi's
  package manager. Settings and runtime state remain machine-local.
  Do not register additional local/Git copies of upstream packages.
- Keep personal features in the companion package and consume public APIs.
  Do not patch Pi's managed package cache or vendor upstream packages.

## Claude mods

- Claude Code mods live in `src/claude/<name>/` and install through a
  committed `home/skills/.claude/skills/<name>` link. Write mods there, not in a
  session's dev-mods folder. See `src/claude/AGENTS.md` and
  `docs/claude-mods.md`.
- Use root `mise run //src/claude/codex-agent:check`.

## This repo is public

Never commit an employer name, an internal repository or project name, or an
internal hostname or URL — not in code, comments, docs, config, or test
fixtures. (`joshrwolf/...` is my own public namespace and is fine.)

The trap is captured output. Never commit a live capture of anything that
observed this machine — an API snapshot, a process listing, a log file, shell
history. Read the real thing to get its shape right, then hand-author the
fixture with neutral names. That also covers the cases the live session
happened not to contain, which a capture never does.
