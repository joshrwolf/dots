# Pi packages in dots

- Stock third-party npm releases are pinned in `../pi-packages.json` and
  installed by Pi. Do not vendor them or edit Pi's managed package cache.
- `dots/` is our private companion package. List resource entrypoints explicitly
  in its `package.json`; do not also install discovery symlinks. Add extensions
  under `dots/extensions/<name>/index.ts` when there is actual functionality.
- Personal workflow/UI belongs in `dots/`; consume upstream public APIs.
  Prefer an upstream release for compatibility fixes, not a local fork.
- Read the installed Pi documentation before using its extension APIs. Declare
  imported Pi host libraries as peers, with exact development versions when
  needed for checks; do not bundle another Pi runtime. Commit npm lockfiles.
- Use root `make pi-deps`, `make pi-check`, and `make pi-install`. Plain `make`
  only restows dotfiles. `pi-install` changes machine-local package registration,
  not source pins, and never reloads a session. Do not install merely to test.
- Updates are explicit: change the exact npm pin, remove the old registration
  with `pi remove`, then run `make pi-install`. Preserve resource filters when
  replacing a registration. Never install an unpinned tracking version.
- After an SDK or source update, run the full `make pi-check` and a bounded
  native async child launch/result against the installed host. Command discovery
  and development-SDK tests alone do not prove host compatibility. Keep smoke
  artifacts outside the repository; don't switch execution modes to hide failures.
- Model catalogs do not establish account entitlement. Verify a proposed default
  with a bounded live request before saving it.
- Keep `docs/pi.md` concise: setup, updates, usage, and file locations. Put agent
  development guidance here, not release history or session findings in the guide.
- Check fixtures must be hand-authored with neutral names. Credentials, runtime
  state, transcripts, captures, package caches, and node_modules stay untracked.
- New instruction scopes use canonical AGENTS.md with CLAUDE.md -> AGENTS.md.
  See ../docs/pi.md for setup, update, conflict, and contribution workflows.
