# Build and activation

Mise selects SDKs and orders tasks. Cargo owns the Rust workspace and compiler
graph; npm owns Pi dependencies. Native lockfiles remain authoritative alongside
`mise.lock`. There is no cloud CI or remote task cache.

## Setup

Use mise 2026.10.12 or newer, review the configuration, then:

```sh
mise trust
mise install --locked
mise run doctor
mise run deps                 # explicit Cargo, npm and Neovim dependency installs
mise run check                # complete checks, without activation
mise run build                # dist/bin executables and dist/pi-package tarball
mise run verify               # checks, builds and isolated installation/host tests
```

The SDK lock supports macOS arm64. Brew supplies machine applications; Apple
Command Line Tools supply the C compiler/SDK. Optional signing also uses Brew
Python at runtime. Repository tests use mise's Python. Native prerequisites are
not hermetic. Add and verify other platforms explicitly before claiming support.

Checks require the installed Claude host. Full acceptance also requires Stow,
Herdr, and a Pi host matching the companion's exact development SDK. Hosts retain
their own installers, credentials and runtime state. Aggregate Herdr activation
also requires the [patched client and server](../src/herdr/host/README.md) for
GitHub workspace resources; a version number alone does not establish support.

To change SDKs, edit `mise.toml`, update the Rust mirror in `rust-toolchain.toml`
when applicable, run `MISE_LOCKED=false mise lock --platform macos-arm64`, review
the lockfile, then install and rerun the checks. Cargo's MSRV is a separate
compatibility requirement.

## Source and task ownership

- `src/` groups code by owner: `dots`, `github`, `tasks`, `herdr`, `pi`, `claude`,
  `aerospace` and `gitsign`. Manifests define packages, not directory categories.
- `home/` contains Stow payloads and links to source-owned runtime code.
- `skills/` contains canonical skills and imported subtree source.
- `tests/` contains repository contracts and cross-component acceptance tests.
- `dist/` and `target/` are ignored build output. Building never activates it.

Use `mise tasks --all` to discover tasks. Scoped paths work from anywhere inside
the repository:

| Task | Purpose |
| --- | --- |
| `rust-build` / `rust-check` | All native executables / complete Rust workspace gate |
| `//dots:build` / `//dots:check` | CLI build / CLI and upstream dependency checks |
| `//dots:skills` | Regenerate checked-in CLI skill documentation |
| `//src/herdr/find:build` / `//src/herdr/find:check` | One Herdr feature and its dependencies |
| `//herdr:deps` | Restore locked Neovim test dependencies |
| `//pi:check` | Pi typechecks and unit tests |
| `//pi:package` / `//pi:prepare` | Native npm tarball / verified runtime in `dist/pi-runtime` |
| `//pi:verify-host` / `//pi:verify-terminal` | Provider-free host / regular and fullscreen PTY tests |
| `//pi:verify` | Complete companion checks and acceptance |
| `//codex-agent:check` | Native Claude validation and tests |
| `//skills:check` | Skill validation and generated CLI documentation consistency |
| `//home:check` | Stow isolation and discovery-link contracts |

Rust leaf checks include upstream checks through mise's `^check`; compilation
ordering and reuse remain Cargo's responsibility. Explicit project edges cover
non-Cargo consumers, including skill generation, home links and manifest tests.
The root gate uses one efficient Cargo workspace pass rather than repeating every
leaf gate. `verify` shares prerequisites in a single mise task graph.

Inspect affected selection without executing tasks:

```sh
mise run --affected --affected-json '//...:check'
mise tasks graph --explain
```

Root/configuration changes conservatively select all projects. Executable graph
tests verify exact affected sets in a disposable Git fixture. Use focused checks
while editing and `verify` at integration checkpoints.

Only the Pi tarball uses mise's bounded 256 MiB local artifact cache. Cargo owns
native compilation caching. Checks, dependency installation, acceptance,
updates and activation are uncached. If your global mise configuration enables
remote caching, constrain the outer runner explicitly:

```sh
MISE_TASK_CACHE=local-only mise run verify
```

## Explicit activation

For stock Herdr, install only supported features with their scoped tasks (for
example, `mise run //src/herdr/nav:install`), then configure their bindings as
[documented](../src/herdr/nav/README.md). The aggregate below includes the
patch-dependent GitHub plugin and the complete home configuration.

```sh
mise run install                     # base Stow packages only
mise run //dots:install              # CLI and checked generated discovery skill
mise run //herdr:install             # Herdr binaries, home config and registration
mise run //src/herdr/find:install    # only this plugin's immutable registration
mise run //pi:upstream-install       # exact third-party npm pins through Pi
mise run //pi:install                # verified immutable companion release
```

Optional `nvim:install`, `//skills:install`, `finicky:install` and `gitsign:install`
retain separate scopes. `unstow` removes only base Stow links. `pi-cli:install`
and `pi-cli:update` use Pi's official installer/updater.

Stow uses `--no-folding`, keeping runtime directories out of the source tree.
Cargo stages activated CLI binaries inside ignored Stow package paths; its
tracking files are never linked into the home directory. Herdr source-install
callbacks use each plugin's explicit `:prepare` task, separately from normal
builds and managed immutable deployment.

Pi registration points at an atomically replaced `current` link under its agent
directory. Existing resource filters survive explicit source migration; conflicts
are rejected. A failed settings write restores the prior pointer.
`//pi:rollback` verifies and reactivates the previous release without reloading a
session. Close Pi sessions and avoid concurrent package writers during activation:
the SDK preserves unrelated settings, but its package list is last-writer-wins.
See [Pi setup](pi.md) for explicit source replacement.

Herdr registers immutable copies under `~/.local/share/dots/herdr/releases`.
Registration is idempotent, not transactional across plugins: earlier links may
succeed before a later failure. Inspect and retry explicitly; old releases remain.
Installed hosts own their live reload/restart decisions.

Acceptance tests use disposable source trees and homes for Cargo/Stow/Herdr, and
isolated Pi agent directories. Pi acceptance covers packaged children, RPC and
real PTYs (menu, resize, theme, reload and shutdown), not live-model requests.
No build or verification task activates against your real home.
