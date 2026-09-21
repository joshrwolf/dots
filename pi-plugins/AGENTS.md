# Pi packages in dots

- `vendor/<source>` holds editable, squashed Git subtrees registered in
  `../pi-sources.toml`. Edit them here, never in Pi's managed package cache.
  Preserve upstream licenses. Keep import/update commits and local patches
  separate from unrelated dots changes.
- `dots/` is our private companion package. List resource entrypoints explicitly
  in its `package.json`; do not also install discovery symlinks. Add extensions
  under `dots/extensions/<name>/index.ts` when there is actual functionality.
- Keep general child-execution changes in pi-subagents and personal workflow/UI
  policy in the companion package. Consume public APIs, not vendor internals.
  Separately registered Pi packages are not automatically Node dependencies.
- Read the installed Pi documentation before using its extension APIs. Declare
  imported Pi host libraries as peers, with exact development versions when
  needed for checks; do not bundle another Pi runtime. Commit npm lockfiles.
- Use root `make pi-deps`, `make pi-check`, and `make pi-install`. Plain `make`
  only restows dotfiles. `pi-install` changes machine-local package registration,
  not source pins, and never reloads a session. Do not install merely to test.
- `make pi-import SOURCE=<name>` and `make pi-update SOURCE=<name>` require a
  clean working tree and create commits. Commit a full SHA in the source
  registry first. Updates are explicit; no tracking branches or automatic pulls.
- Check fixtures must be hand-authored with neutral names. Credentials, runtime
  state, transcripts, captures, package caches, and node_modules stay untracked.
- New instruction scopes use canonical AGENTS.md with CLAUDE.md -> AGENTS.md.
  See ../docs/pi.md for setup, update, conflict, and contribution workflows.
