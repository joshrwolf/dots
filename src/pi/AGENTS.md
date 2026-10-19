# Pi packages in dots

- Stock third-party npm releases are pinned in `../../pi-packages.json` and
  installed by Pi. Do not vendor them or edit Pi's managed package cache.
- This directory is our private companion package. List resource entrypoints
  explicitly in `package.json`; do not also install discovery symlinks. Add
  extensions under `extensions/<name>/index.ts` for actual functionality.
- Personal workflow/UI belongs here; consume upstream public APIs.
  Prefer an upstream release for compatibility fixes, not a local fork.
- Read the installed Pi documentation before using its extension APIs. Declare
  imported Pi host libraries as peers, with exact development versions when
  needed for checks; do not bundle another Pi runtime. Commit npm lockfiles.
- Use `mise run //src/pi:deps`, `mise run //src/pi:check`, and
  `mise run //src/pi:verify-host`. Explicit `mise run //src/pi:install` runs the
  full verification gate and installs an immutable runtime through the public
  settings API. Use the install/rollback tasks, not ad hoc activation scripts.
  Neither reloads a session; companion updates use `/reload` when jobs/workers
  are idle, while CLI/SDK updates require restart. Use `upstream-install` for
  pinned third-party npm packages through Pi. Do not install merely to test;
  use isolated temporary homes.
- Serialize preparation, verification, and installation within a checkout: they
  share `dist/` staging. The installer lock protects the final switch, not builds;
  use separate checkouts for independent builds.
- Updates are explicit: change the exact npm pin, remove the old registration
  with `pi remove`, then run `mise run //src/pi:upstream-install`. Preserve resource filters when
  replacing a registration. Never install an unpinned tracking version.
- After an SDK or source update, run the full `mise run //src/pi:check` and a bounded
  native async child launch/result against the installed host. Command discovery
  and development-SDK tests alone do not prove host compatibility. Keep smoke
  artifacts outside the repository; don't switch execution modes to hide failures.
- Model catalogs do not establish account entitlement. Verify a proposed default
  with a bounded live request before saving it.
- Keep `docs/pi.md` concise: setup, updates, usage, and file locations. Put agent
  development guidance here, not release history or session findings in the guide.
- Check fixtures must be hand-authored with neutral names. Credentials, runtime
  state, transcripts, captures, package caches, and node_modules stay untracked.
- New instruction scopes use AGENTS.md directly; no CLAUDE.md compatibility links.
  See [Pi setup](../../docs/pi.md) for installation and use.

## Presentation code

- The public library is `@joshrwolf/pi-dots/ui`; types and consumer tests own its
  API reference. Keep lifecycle constraints in `src/README.md`. Importing it must
  not register extensions or start resources.
- Pi owns rendering, layout primitives, input, themes, scheduling, and persistence.
  Use public APIs; do not build a parallel UI framework or inspect private child arrays.
- Project recorded tool data into readonly models, then present them. Rendering
  never changes execution, stored results, or model-facing content, and never
  reads current files to reconstruct missing history.
- Fix renderer ownership and shell mode for a row. Unknown or mixed content
  stays visible through conservative fallback; images remain host-owned.
- Preserve failures, recovery paths, missing-result distinctions, and truncation
  notices. Display limits are not tool-result truncation. Never invent durations,
  success, cancellation, or nested results absent from the record.
- Sanitize untrusted text before styling; use native Unicode/width helpers.
  Keep original records separate from their display representation.
- Bound preparation, retained caches, and rendered output. Render callbacks do
  no I/O, process execution, history scans, or repeated patch parsing.
- Share one lazy syntax service per runtime. It must work before session binding;
  guard asynchronous completion by document revision and runtime lifetime.
  Tool rows have no disposal contract: no per-row timers or watchers.
- Footer/header setters do not provide compare-and-restore ownership. Do not
  reset a later extension's surface during cleanup. Editor ownership is identity
  checked through the public API; preserve native input and working status.
- Pi owns theme, startup, and editor settings. Validate our own preferences and
  cancel pending dialogs at shutdown; never use an old context after reload.
- Use `dev/` for opt-in, neutral fixture scenarios built from shipping components,
  not parallel mock implementations. Run `mise run //src/pi:verify-terminal` for
  presentation changes; real scrolling, focus, resize, and reload are contracts.
- Use strict, erasable TypeScript and typed consumer tests. Verify real host
  transitions and HTML export, including cold custom-tool exports; builtin
  export templates can bypass registered renderers. Test release switching and
  rollback through `/reload`, including changed transitive imports and version
  reporting. Keep compatibility tests
  separate from production imports.
