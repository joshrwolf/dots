# dots

GNU Stow dotfiles: package contents under `home/` mirror `$HOME`.

## Documentation

- Use plain English, present tense, and active voice. Start with purpose; add
  Setup, Use, or Constraints sections only when useful.
- Keep READMEs user-facing and contributor rules in the relevant `AGENTS.md`.
  Explain decisions and limits that code does not make clear; link to CLI help,
  types, tests, or manifests instead of copying their inventories.
- Keep one authoritative explanation. Remove completed migration plans, research
  reports, and unused designs; do not expand architecture docs for their own sake.
- Leave upstream-owned docs and generated references to their owners/generators.
  Keep skills focused on their workflows rather than applying prose-wide rewrites.

## Instructions and skills

- Use native `AGENTS.md` discovery; do not add `CLAUDE.md` compatibility links.
  Claude personal instructions use its native user rules directory.
- Keep canonical skills at `skills/<name>/`, with `name` and `description`
  frontmatter in `SKILL.md` and relative supporting paths.
- Author both discovery links in `home/skills/.agents/skills/` and
  `home/skills/.claude/skills/`; installation only restows them.
  See [skill setup](docs/skills.md) for targets and validation.
- Reserve `local-*` for ignored machine-local skills and links; never commit them.
- Keep shared guidance portable. A symlink does not make tool-specific hooks,
  commands, metadata, or permissions portable.
- Imported skills are editable Git subtrees under
  `skills/.local/share/agent-skills/`. Preserve upstream instructions and licenses;
  edit source, not copied discovery files. Updates are explicit mise/Git tasks.
- Check upstream before compatibility patches. Prefer upstream releases, keep
  local patches minimal, and remove superseded patches. Personal features stay
  outside vendored code.

## Tooling

- Before porting a script, look for native capabilities of the surrounding
  tools (host app, package manager, OS services, upstream main) that make it
  obsolete, and question assumptions that drive outsized complexity. Porting
  to Rust is the last resort.
- Language follows the artifact: Rust for executables, repository and
  installation contracts, and automation logic; TypeScript/Node only inside the
  Pi and Claude packages; Lua only for Neovim. No Python. Shell appears only as
  one-line inline mise commands; a host-required executable shim must justify
  itself.
- Tests spawn only real dependencies or our own fixtures: test-binary re-exec,
  or `[[bin]]` fixtures behind a `testing` feature. No interpreters and no
  detached socket call-backs.
- Keep one implementation per capability (for example, `dots-process` is the
  only subprocess runner), tested once where it lives.
- Mise selects tools and orders tasks. Plain cargo/npm/stow calls stay inline;
  other logic lives in Rust: `cargo xtask` (in-repo, never installed, created
  only on demonstrated need) for repository build, install, and contracts;
  `dots` only for user commands.
- Each artifact has one build path: build once into `dist/` (a Herdr plugin
  into its own directory, Herdr's native layout), then install or link from
  there.
- `check` is fast and side-effect free (format, lint, unit tests, typecheck)
  and is what agents run. `verify` holds process-lifecycle, PTY, host,
  installation, and bootstrap acceptance. Place tests by the structural
  markers in [build and activation](docs/build.md#check-and-verify).
- Delete tests that serve no purpose, such as tests of mise's own graph or of
  task-file shape, rather than porting them.

## Build and activation

- Mise owns SDK pins and orchestration; native tools own dependency/compiler
  graphs. Put ordinary native commands directly in tasks, not dispatch wrappers.
  Remove redundant behavior before translating a wrapper into another language.
- Keep substantial application logic and acceptance tests in their owning
  toolchain. Keep `rust-toolchain.toml` aligned with `mise.toml`.
- Use `mise run doctor`, `deps`, `check`, and `verify` as documented in
  [build and activation](docs/build.md). Builds/checks never activate.
- Base Stow packages are explicit in root `mise.toml`. Herdr, CLI, and Pi
  activation are separate; do not install or reload merely to validate changes.
- Only proven distributable tasks may use local artifact caching. Dependencies,
  checks, smoke tests, updates, and activation are always uncached.

## Source ownership

- Root Cargo workspace, lockfile, toolchain, and lint policy govern Rust under
  `src/` and repository contracts in `tests/`. Use `mise run rust-check` for the
  complete fast gate and `mise run rust-verify` for Rust acceptance; leaf checks include upstream dependencies. Libraries need no
  artificial build tasks.
- Keep shared operations with their named owners. The task store derives
  readiness, claims transactionally, and accepts project key and time explicitly.
- Pi uses pinned stock npm releases and a companion in `src/pi/`. Consume public
  APIs; never patch managed caches, vendor upstream packages, or register duplicate
  copies. Settings and runtime state remain machine-local.
- Claude mods live in `src/claude/<name>/`, not session dev-mods folders.
  Follow [their contributor rules](src/claude/AGENTS.md).

## Public repository

Never commit employer names, internal project/repository names, or internal
hostnames/URLs, including in comments, docs, config, and fixtures.
`joshrwolf/...` is the owner's public namespace.

Never commit captures of this machine: API snapshots, process lists, logs,
transcripts, or shell history. Inspect real data for its shape, then hand-author
neutral fixtures, including cases the capture did not contain.
