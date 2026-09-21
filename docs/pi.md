# Pi packages in dots

Pi's Stow package (`pi/`) owns personal instructions and the theme discovery
link. `pi-plugins/` owns executable extension source, dependencies, and tests,
like `herdr-plugins/` does for Herdr. Plain `make` remains a Stow operation:
no npm installs, package registration, upstream fetches, or live reloads.

## Layout and ownership

- `pi-sources.toml`: upstream HTTPS URLs, exact full Git commit SHAs, and npm
  check scripts for third-party packages.
- `pi-plugins/vendor/<source>`: editable squashed Git subtrees. Source content
  and our patches are fixed by the dots commit; retain upstream licenses.
- `pi-plugins/dots`: our private companion package, with explicit Pi resource
  entrypoints in `package.json`. It starts empty; no placeholder extension runs.
- `pi-plugins/Makefile`: package setup and checks, delegated to by root targets.
- `scripts/pi-packages.py`: source validation, subtree operations, locked
  dependency installation, and local registration through Pi's own CLI.

Keep general-purpose child execution changes in pi-subagents. Keep personal
placement, navigation, and workflow policy in our companion package. Herdr-side
Rust features still belong in `herdr-plugins/` and use `herdrkit`.

The initial registry selects pi-subagents v0.70.1 at
`1ac7b5e2652e9571164847ac2905ab4aded92791`. The initial scaffold does not contain
that subtree yet: importing it is a separate operation that creates commits.

## Initial import (once, by a maintainer)

First commit the setup and finish any unrelated working-tree changes. Import
and update refuse dirty trees, including untracked files; they never stash or
commit unrelated work for you.

```sh
make pi-import SOURCE=pi-subagents
```

This uses `git subtree add --squash` against the registered exact SHA. Review
the imported source and license before executing it. Audit it for the public
repo's privacy requirements. Where necessary, normalize imported instruction
scopes to canonical `AGENTS.md` plus `CLAUDE.md -> AGENTS.md` in a separate
customization commit. Do not discard upstream instructions or attribution.

After the import is committed, fresh clones contain the source and need no
submodule initialization or repeat import. A GitHub fork is not required.

## Dependencies, checks, and installation

Prerequisites: GNU Make, Git with subtree support, Python 3.11+, npm/Node, and
Pi. Use the Node requirements of the pinned upstream package when changing its
revision. npm lifecycle scripts are disabled during dependency installation;
review any dependency that requires enabling one rather than removing the
protection globally.

```sh
make pi-deps       # npm ci --ignore-scripts in each package; includes dev deps
make pi-check     # manager tests, upstream typecheck/test:all, companion checks
make pi-install   # npm ci --ignore-scripts, then register local paths with Pi
```

`pi-check` neither downloads dependencies nor registers packages. Before the
initial subtree import, `pi-deps`, the full `pi-check`, and `pi-install` fail
with an import instruction, before package setup side effects. Scaffold checks
can still run independently:

```sh
make -C pi-plugins check-manager
npm --prefix pi-plugins/dots run check
```

Installation registers absolute paths for the imported packages and the
companion package using `pi install --no-approve <path>`. It does not copy source
or create extension symlinks. It changes only Pi-managed registration through
Pi's CLI, not a Stow-managed settings file. Existing local registrations and
resource filters are preserved. Dependency failure prevents new registrations;
if a later Pi registration fails, rerun installation to finish it.

Model preferences, credentials, sessions, trust, package caches, and other
runtime bookkeeping remain machine-local. The helper honors
`PI_CODING_AGENT_DIR` for the settings location. Moving the dots checkout requires
removing its old local registrations and rerunning installation.

Use `/reload` or restart Pi when ready to activate changes. Setup never reloads
an active session. It also does not install Herdr's agent integration or modify
a live Herdr session.

### Avoid duplicate installations

Do not also install npm/Git copies of the same packages or symlink their
entrypoints into `~/.pi/agent/extensions`. The installer rejects ordinary
same-name npm, Git, and alternate local-package registrations before dependency
installation. This is a guard, not exhaustive alias/extension discovery: inspect
`pi list` and `pi config` when migrating another setup, including project-local
registrations and manually configured extensions.

Use `pi remove <old-source>` for the obsolete registration, then rerun
`make pi-install`. The helper never removes an existing package automatically.
Use `pi config` to disable unwanted package prompts/skills; those resource
filters remain local and are preserved on repeated installation.

## Pins and compatibility

There are three independent version concerns:

1. The source registry selects a full upstream commit, not a moving branch or
   tag. The subtree's checked-in content also contains our local patches.
2. Each package's committed `package-lock.json` fixes dependency resolution.
   Installation uses `npm ci`; it does not regenerate lockfiles or update source.
3. Pi itself supplies the extension runtime. Homebrew currently installs Pi
   without an exact version pin. A dependency lockfile is **not** a host Pi pin.

The setup was designed using installed Pi 0.85.1 documentation. The selected
pi-subagents revision's development dependencies target Pi 0.86.1. Compatibility
with the actual host must be smoke-tested before relying on it; passing upstream
tests against its own development dependencies does not establish host
compatibility. Local Herdr-native subagent execution is a future implementation,
not provided by this package scaffold.

Never develop inside `~/.pi/agent/git/` or `npm/`: Pi can reset/clean managed Git
clones during reconciliation. Pi updates do not manage these local source paths.

## Updating upstream while keeping our changes

1. Finish and commit local work.
2. Review the desired upstream revision; replace `ref` in `pi-sources.toml`
   with its full SHA and commit that registry change.
3. Run the explicit merge:

   ```sh
   make pi-update SOURCE=pi-subagents
   ```

4. Review the resulting subtree merge, refresh dependencies with `make pi-deps`,
   and run `make pi-check` plus a host Pi smoke test before reloading.

This uses `git subtree pull --squash`. Nonconflicting committed customizations
survive. Overlapping changes require normal Git conflict resolution. Finish the
merge with `git commit` after resolving conflicts, or cancel with
`git merge --abort`. Preserve Git's subtree trailers in generated commits.

The registry is the desired import pin. Between committing a new pin and
successfully merging it, it can intentionally differ from the currently
imported revision; do not ship that intermediate state. If abandoning an
update, revert the registry change too. Never reset the entire dots working
tree as an update or rollback procedure.

Keep upstream imports and local patches in separate commits where practical.
`pi-update` never installs, reloads, pushes, or publishes anything.

## Our own extensions

Add an actual extension under `pi-plugins/dots/extensions/<name>/index.ts` and
list its exact entrypoint in the package's `pi.extensions` array. Helpers and
tests should not be automatically discovered as extensions. Add tests and, when
TypeScript source is introduced, a typecheck script to the package's `check` gate.

Declare imported Pi host libraries as peer dependencies; use exact matching
development versions for typechecking and tests. Put other runtime dependencies
in `dependencies`, and commit lockfile changes. Do not bundle a second Pi runtime.
The current empty companion package deliberately has no unused SDK dependencies.

Prefer pi-subagents' public event/API contracts over imports from its internals.
Two separately registered Pi packages are not automatically Node dependencies;
using an event contract avoids accidentally installing a second subagent owner.
If a direct runtime import becomes necessary, explicitly design its dependency
resolution rather than relying on a sibling package being installed.

Shared personal skills remain in the existing `~/.agents/skills` setup. Do not
copy them into this package just to make Pi discover them.

## Contributions and removal

To extract our dependency changes for a separately authorized contribution:

```sh
git subtree split --prefix=pi-plugins/vendor/pi-subagents -b pi-subagents-export
```

Review that branch before pushing it to a fork. Subtree operations themselves
never contact other people or open PRs.

To remove a source, remove its Pi registration with `pi remove <absolute-path>`,
remove the registry entry and subtree with ordinary Git operations, and review
the removal. Removing a source must be explicit; no synchronization target
silently deletes imported code or local runtime state.
