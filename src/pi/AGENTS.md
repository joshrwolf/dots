# Pi packages in dots

- Stock third-party npm resources are declared in bootstrap's `pi:packages` task.
  `setup.mjs` uses the installed host's public package/settings SDK to register
  selected sources and resolve missing resources without upgrading caches.
  Do not vendor them or edit Pi's managed package cache.
- This directory is our private companion package. List resource entrypoints
  explicitly in `package.json`; do not also install discovery symlinks. Add
  extensions under `extensions/<name>/index.ts` for actual functionality.
- Personal workflow/UI belongs here; consume upstream public APIs.
  Prefer an upstream release for compatibility fixes, not a local fork.
- Read the installed Pi documentation before using its extension APIs. Declare
  imported Pi host libraries as peers; the workspace catalog pins their development
  versions for checks. Do not bundle another Pi runtime.
- Use `mise run deps`, `mise run //src/pi:check`, and
  `mise run //src/pi:verify-host`. The installed machine module owns `pi:install`,
  which registers its durable primary source through the public SDK. Pi loads
  that source in place on each session's next `/reload` or start.
  Never install from an agent worktree, and do not install merely to test; use
  isolated temporary homes. Installation never reloads a session; changes use
  `/reload` when jobs/workers are idle, while CLI/SDK updates require restart.
- Pi does not install local package dependencies. The native `runtime:deps`
  task installs the selected applications' production closure; root `deps`
  adds development tools for checks. The companion's host libraries stay peers;
  standalone applications declare their own runtime dependencies through the
  shared catalog. Disable automatic peer installation and keep the shared pnpm
  lock and supply-chain policy for both install modes.
- Ordinary third-party requests follow latest on first install. Use Pi's native
  update operation for upgrades; repeat setup must preserve installed versions,
  resource filters, unrelated sources and machine-local settings. Only concrete
  compatibility contracts justify exact pins.
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
- See [Pi setup](../../docs/pi.md) for installation and use.

## Request reliability

- `extensions/anthropic-request-guard/` owns only the direct Anthropic
  header-to-message-start deadline; Pi owns retries and worker settlement.
  Keep provider auth, catalog and parsing upstream-owned.
- On Pi upgrades, run the guard host suite and its stock baseline in
  `tests/acceptance/anthropic-request-guard.test.mjs`. The baseline's short
  observation window is not proof of an infinite stall; set
  `PI_GUARD_BASELINE_MS=95000` for a provider-free window beyond our deadline.
  Compare stock and
  guarded startup deadlines, cleanup, compaction and worker outcomes before
  deciding whether the guard is still needed; do not assert upstream must stay
  broken. Remove the guard when upstream covers the contract.
- Live model probes are separate, bounded, and require approval.

## Model context

- Separate automatic snapshots, material events, and explicit inspection. Keep
  complete records with their capability owner; do not reuse inspector text as
  automatic context. Existing tools expose results, evidence and history.
- Use independently replaceable `systemPromptOptions.sections` for companion
  sources. Keep native instruction discovery in `contextFiles`. Pi owns diffing,
  persistence and compaction checkpoints; add no parallel injection cache.
- Render snapshots deterministically from semantic state, not observation clocks,
  heartbeats, event numbers or worker history. Root and slice claims remain
  visible. Keep the source-bounded brief page intact.
  Bound previews, identify omissions, and provide a complete-record retrieval path.
- Preserve working agreements, unavailable observations, same-run tool results,
  ownership, branch-local delivery receipts and cancellation. Test compaction,
  cold resume and tree navigation in the real host, not just render snapshots.
- Measure projected context growth as well as current size. Current inventory and
  historical patches overlap; estimates are not provider billing or wire capture.

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
  It drives Pi's own `InteractiveMode` through its public `terminal` option in
  both TUI modes; tty raw mode and protocol negotiation stay upstream's concern,
  so do not add a PTY harness. Extend `tests/fixtures/terminal-host.mjs` flows.
- Use strict, erasable TypeScript and typed consumer tests. MJS fixtures and
  tests receive static lint checks; semantic checking is opt-in with `@ts-check`.
  Verify real host
  transitions and HTML export, including cold custom-tool exports; builtin
  export templates can bypass registered renderers. Keep compatibility tests
  separate from production imports.
