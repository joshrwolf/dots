# Build and activation

Mise selects tools and orders tasks; Cargo and npm own their dependency graphs.

## Setup

Use mise 2026.10.12 or newer with Homebrew and Apple Command Line Tools:

```sh
mise trust
mise install --locked
mise run doctor
mise run deps
mise run check
mise run build
mise run verify
```

The SDK lock supports macOS arm64. Checks need the installed Claude host; full
acceptance also needs Stow, Herdr, and a Pi host matching the companion's exact
development SDK. Host applications keep their own installers and runtime state.

`check` validates source. `build` stages artifacts in `dist/`. `verify` also
exercises installation and hosts in disposable homes and checkouts. None
activates against your real home; provider-free tests do not prove live-model
compatibility.

## Development

Use `mise tasks --all` for available tasks and scoped paths for focused work:

```sh
mise run //src/dots:check
mise run //src/herdr/nav:check
mise run //src/pi:check
mise run rust-check
mise run --affected --affected-json '//...:check'
mise tasks graph --explain
```

Rust leaf checks include upstream dependencies. The root Rust gate checks the
workspace in one pass. Root/configuration changes conservatively affect all
projects. Use `verify` at integration checkpoints.

Code lives under named owners in `src/`, Stow payloads in `home/`, canonical
skills in `skills/`, and repository contracts in `tests/`.

To update SDKs, edit `mise.toml`, align `rust-toolchain.toml` when applicable,
then run `MISE_LOCKED=false mise lock --platform macos-arm64`. Review the lock,
install, and verify. Cargo's MSRV is a separate compatibility requirement.

Only the Pi tarball uses mise's local artifact cache; Cargo caches compilation.
Checks, dependency installs, acceptance, updates, and activation are uncached.
The repository disables remote task caching.

## Activation

Run installation only when ready to change the live setup:

```sh
mise run install                    # base Stow packages
mise run //src/dots:install         # CLI and generated discovery skill
mise run //src/herdr/nav:install    # navigation plugin only
mise run //src/pi:upstream-install  # pinned third-party Pi packages
mise run //src/pi:install           # verify and install the companion update
```

Base Stow packages are listed in root `mise.toml`; `unstow` removes only those
links. Neovim, Finicky, and gitsign have separate `<name>:install` tasks.
Stow uses `--no-folding` to keep runtime directories out of the checkout.

`mise run //src/herdr:install` installs all Herdr plugins and the Herdr/Neovim
configuration. It requires the [patched client and server](../src/herdr/host/README.md)
for GitHub workspace resources. Stock Herdr users should install supported
plugins individually and configure only their bindings.

Herdr registers immutable copies under `~/.local/share/dots/herdr/releases/`.
Registration is idempotent, but not transactional across plugins: inspect and
retry a partial failure. Old releases remain available. Installing a binary
does not upgrade a running server; never restart it as a build or test step.

[Pi installation](pi.md#updates) verifies and installs an immutable companion
snapshot. Use `/reload` afterward when jobs and workers are idle. Restart for Pi
CLI/SDK or startup environment changes. Coordinate package writers across sessions.
Installation and rollback never reload sessions automatically.
