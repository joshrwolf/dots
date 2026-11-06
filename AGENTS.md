# dots

Native mise machine setup: source trees under `home/` mirror `$HOME`; `bootstrap/` owns their installed modules and `bootstrap/source/` owns source acquisition.

## Design

- dots is greenfield with one user. Breaking changes are allowed: choose the
  architecturally correct design and update callers, config, and docs in the
  same change instead of keeping shims, aliases, or deprecated paths. The goal
  is the simplest correct system, not the smallest diff.
- Files describe only the current state. Whatever a change replaces is removed
  completely: code, config, tests, docs, comments, and instruction lines. No
  legacy or compatibility paths, "formerly"/"no longer"/"replaces X" notes,
  migration guides, or tombstones. Git history and dots tasks keep the record;
  commit messages and tasks may describe the change, files may not. Machine-local
  data (task store, session records) is migrated or reset once, by decision; the
  migration code then goes too.

## Agent-facing tools and instructions

Applies to Pi extensions, Claude mods, skills, and `AGENTS.md` files.

- Build on the host (Pi, Claude Code, the CLIs they drive). Add a tool only for
  what an agent cannot do reliably with existing ones.
- Constrain what has one right answer: schemas, enums, validation, and errors
  that list valid choices. Leave judgment to the model, with short criteria and
  the real data it needs to decide.
- Read facts from the host at runtime (models, catalogs, settings) rather than
  hardcoding lists that go stale.
- State goals, constraints, and stopping points, not step-by-step scripts. Drop
  guidance that a tool's schema or description already carries.
- Try new tools in the real host. When an agent repeats a mistake, fix the
  tool's shape before adding prose.
- This guidance is written for current models and hosts. Base new work on
  current upstream practice, not on this file's examples; when a newer model or
  host release makes a tool, rule, or workaround unnecessary, remove it.

## Documentation

- Use plain English, present tense, and active voice. Start with purpose; add
  Setup, Use, or Constraints sections only when useful.
- Keep READMEs user-facing and contributor rules in the relevant `AGENTS.md`.
  Explain decisions and limits that code does not make clear; link to CLI help,
  types, tests, or manifests instead of copying their inventories.
- Keep one authoritative explanation. Remove research reports and unused designs;
  do not expand architecture docs for their own sake.
- Leave upstream-owned docs and generated references to their owners/generators.
  Keep skills focused on their workflows rather than applying prose-wide rewrites.

## Instructions and skills

- Use native `AGENTS.md` discovery; do not add `CLAUDE.md` compatibility links.
  Claude personal instructions use its native user rules directory.
- Keep skills flat at `skills/<name>/`, with `name` and `description` frontmatter
  in `SKILL.md` and relative supporting paths. Commit skills and mods in their
  public or private owning repository; names carry no ownership prefix.
- Native groups own deployment and shared agent discovery; add no per-agent
  per-skill links or private links in public source. Keep other files in the
  namespace to what every host ignores. See [skill setup](docs/skills.md).
  The ignored `rex` skill belongs to `rex skill setup`; do not edit it.
- Keep shared guidance portable. A symlink does not make tool-specific hooks,
  commands, metadata, or permissions portable.
- Keep upstream licenses for adapted skills, such as
  `skills/LICENSE-cc-skills-golang`.

## Upstream code

- Consume upstream by default: pinned releases through their native manager and
  public APIs, unedited. Check upstream before working around a defect, and
  remove workarounds once upstream makes them unnecessary.
- Forking is the user's decision. Never fork, vendor, or copy upstream code into
  dots without the user's explicit approval for that project; propose it with
  the reason and the kind of fork.
- An approved fork lives in-tree under `src/<name>/`, brought in with
  `git subtree add --squash`, keeps upstream's license, and has an `AGENTS.md`
  naming its upstream and kind:
  - tracked: keeps upstream's layout and formatting, carries minimal changes,
    and takes upstream releases with `git subtree pull --squash`;
  - owned: ours to change freely, with no upstream pulls.
- A fork installs through bootstrap like any other source. Binaries it needs
  come from pinned mise tools, never from downloads at runtime.

## Tooling

- Before porting a script, look for native capabilities of the surrounding
  tools (host app, package manager, OS services, upstream main) that make it
  obsolete, and question assumptions that drive outsized complexity. Porting
  to Rust is the last resort.
- Language follows the artifact: Rust for executables, repository and
  installation contracts, and automation logic; TypeScript/Node only inside
  Pi-based packages and Claude
  packages, plus JavaScript configuration required by Finicky's host;
  Lua for Neovim and for Rex, whose configuration, actions, and
  `rex do` scripts own Rex automation. No Python. Shell appears only as
  one-line inline mise commands; a host-required executable shim must justify
  itself.
- Tests spawn only real dependencies or our own fixtures: test-binary re-exec,
  or `[[bin]]` fixtures behind a `testing` feature. No interpreters as test
  fixtures and no detached socket call-backs; Lua a host runs (Rex, Neovim) is
  tested in that host's own interpreter.
- Keep one implementation per capability (for example, `dots-process` is the
  only subprocess runner), tested once where it lives.
- Mise selects tools and orders tasks. Plain Cargo, pnpm and native mise commands stay inline;
  other logic lives in Rust: `cargo xtask` (in-repo, never installed, created
  only on demonstrated need) for repository build, install, and contracts;
  `dots` only for user commands.
- Each artifact has one install path. User commands have no build step: `cargo install --locked --path .` places them in Cargo's
  bin directory; native mise groups carry configuration. Source-loaded packages, like
  the Pi companion, have no build step.
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
  toolchain.
- Use `mise run doctor`, `deps`, `check`, and `verify` as documented in
  [build and activation](docs/build.md). Builds/checks never activate.
- Machine configuration groups live in `bootstrap/`, separate from the root
  development SDK. Do not apply setup, install or reload merely to validate
  changes. Use disposable source trees and homes for acceptance. The Pi
  companion runs from the checkout that registered it: register only from the
  primary checkout, never from an agent worktree.
- Add a project `mise.toml` only for real tasks (build, install, deps, non-Cargo
  checks); Cargo crates need none to be checked.

## Source ownership

- Rust follows [Google's Rust Style Guide](https://google.github.io/styleguide/rustguide.html),
  including its scope and exceptions. Operational and external-input failures
  are errors; violated internal invariants must not become successful defaults.
- TypeScript follows [Google's TypeScript Style Guide](https://google.github.io/styleguide/tsguide.html),
  applied through the shared `idiomatic-typescript` skill. Use Effective TypeScript
  for type/API design judgment. Host-required exports and Node's erasable syntax
  govern runtime constraints; Google-internal tooling assumptions do not apply.
  Validate external values at their boundary; assertions do not validate data.
- Root Cargo workspace, lockfile, toolchain, and lint policy govern Rust under
  `src/`. Root `check` and `verify` run the whole workspace; scope a run with
  `cargo nextest run -p <crate>`.
- Keep shared operations with their named owners. The task store derives
  readiness, claims transactionally, and accepts project key and time explicitly.
- Pi uses pinned stock npm releases and a companion in `src/pi/`. Consume public
  APIs; never patch managed caches, vendor upstream packages, or register duplicate
  copies. Native Pi groups merge tracked preferences from `home/pi/` into the
  user settings file. Authentication, last-selected model/provider, device
  identity, package registrations and runtime state remain machine-local.
- Pi-based packages share one pnpm workspace and lockfile at the root, with
  optional private packages under `private/src/`. The native workspace catalog
  owns exact Pi release pins; `src/pi-kit/` owns shared APIs. Move common code
  there when a second package first needs it, never copy it. Public checks must
  run without a private checkout.
- Claude mods live in `src/claude/<name>/`, not session dev-mods folders.
  Follow [their contributor rules](src/claude/AGENTS.md).

## Public repository

Never commit employer names, internal project/repository names, or internal
hostnames/URLs, including in comments, docs, config, and fixtures.
`joshrwolf/...` is the owner's public namespace.

Never commit captures of this machine: API snapshots, process lists, logs,
transcripts, or shell history. Inspect real data for its shape, then hand-author
neutral fixtures, including cases the capture did not contain.
