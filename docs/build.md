# Build and activation

Mise selects tools and orders tasks; Cargo and npm own their dependency graphs.

## Setup

Use mise 2026.10.14 or newer with Homebrew and Apple Command Line Tools:

```sh
mise trust
mise install --locked
mise run doctor
mise run deps
mise run check
mise run build
mise run verify
```

The SDK lock supports macOS arm64. Checks need rustup, Stow, and the installed
Claude host; full acceptance also needs Herdr and a Pi host matching the
companion's exact development SDK. Host applications keep their own installers and runtime state.

`build` stages Herdr plugin binaries in each plugin's `bin/`; user commands
have no build step. Check, build, and verify never activate against your real home; provider-free tests do not prove live-model compatibility.

## Check and verify

`check` is fast and side-effect free: formatting, lints, typechecks, unit tests,
and tests that spawn only short-lived real dependencies such as Git in a
temporary repository. Agents run it routinely.

`verify` runs `check` and `build`, then acceptance: process lifecycle (deadlines,
kills, detached or cross-process children), PTYs, and host applications in
disposable homes. Use it at integration checkpoints.

Place each test by structure, not by a list of names:

- Rust: acceptance lives in a crate's `tests/acceptance.rs` integration binary,
  or in a `#[cfg(test)] mod acceptance` when it needs private items.
  [`.config/nextest.toml`](../.config/nextest.toml) excludes both from the
  default profile and selects only them in the `acceptance` profile.
  `check` uses the default profile; `verify` uses `acceptance`.
- Node: acceptance lives in the Pi package's `tests/acceptance/`, which only
  its `verify` task runs.
- Stow: `//home:check` simulates every package into an empty temporary target,
  so conflicts between packages surface without touching `$HOME`.

Host acceptance uses the installed application, such as the `herdr` on `PATH`,
in an isolated temporary home and session; it never contacts the live server.

## Development

Use `mise tasks --all` for available tasks. Root `check` and `verify` run the
whole Cargo workspace in one pass; a project keeps a `mise.toml` only for real
tasks such as build, install, deps, or non-Cargo checks. Scope focused work with
the native tools:

```sh
cargo nextest run -p herdr-nav
cargo nextest run -p dots-process --profile acceptance
mise run //src/pi:check
```

Code lives under named owners in `src/`, Stow payloads in `home/`, and canonical
skills in `skills/`.

Rustup selects Rust from `rust-toolchain.toml`; mise pins the other SDKs in
`mise.toml`. After editing `mise.toml`, run
`MISE_LOCKED=false mise lock --platform macos-arm64`, review the lock, install,
and verify. Cargo's MSRV is a separate compatibility requirement.

## Machine setup

The [machine-setup entry point](../bootstrap/README.md) separates host setup from
this checkout's development SDKs. Select portable agent configuration or the
existing macOS desktop configuration without installing host packages. The root
activation tasks below remain available.

## Activation

Run installation only when ready to change the live setup:

```sh
mise run install                    # base Stow packages
mise run //src/dots:install         # dots CLI
mise run //src/herdr:install        # Herdr plugins, CLIs, and configuration
mise run //src/herdr/nav:install    # navigation plugin only
mise run //src/pi:packages          # pinned third-party Pi packages
mise run //src/pi:install           # register the companion from this checkout
```

Base Stow packages are listed in root `mise.toml`; `unstow` removes only those
links. Neovim and Finicky have separate `<name>:install` tasks.
Stow uses `--no-folding` to keep runtime directories out of the checkout.

Stow carries configuration only. `dots`, `herdr-issue`, and `herdr-review`
install with `cargo install --locked --path .` into Cargo's bin directory
(`~/.cargo/bin`, on `PATH` through mise and the zsh configuration); Cargo
owns their tracking and replacement.

`mise run //src/herdr:install` links all Herdr plugins, installs the
`herdr-issue` and `herdr-review` commands, then stows the Herdr/Neovim
configuration, for stock Herdr 0.9.1 or newer. To use only some plugins,
install them individually and configure only their bindings.

A plugin's `install` runs its `build`, which stages `bin/` in the plugin
directory, then `herdr plugin link` on that directory. Relinking is harmless
and replaces any earlier registration of the same plugin. A linked plugin runs
from the checkout, so every later `build` there, including `mise run build`
and `verify`, changes the live plugin. Link only from the primary checkout,
never from an agent worktree.

[Pi installation](pi.md#setup) works the same way: `pi install` registers
`src/pi` by path, so sessions load the checkout's files on their next `/reload`
or start. Install it only from the primary checkout, never from an agent
worktree. Installation never reloads sessions.
