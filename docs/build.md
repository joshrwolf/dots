# Build and activation

Mise selects tools and orders tasks; Cargo and pnpm own their dependency graphs.

## Setup

Use mise 2026.10.25 or newer with the platform build prerequisites:

```sh
mise trust
mise install --locked
mise run doctor
mise run deps
mise run check
mise run verify
```

The SDK lock supports macOS arm64. Checks need rustup and the installed
Claude host; full acceptance also needs a Pi host matching the Pi release pinned
in `src/pi-kit`. Host applications keep their own installers and runtime state.

User commands have no build step. Check and verify never activate against your real home; provider-free tests do not prove live-model compatibility.

## Check and verify

`check` is fast and side-effect free: formatting, lints, typechecks, unit tests,
and tests that spawn only short-lived real dependencies such as Git in a
temporary repository. Agents run it routinely.

`verify` runs `check`, then acceptance: process lifecycle (deadlines,
kills, detached or cross-process children), PTYs, and host applications in
disposable homes. Use it at integration checkpoints.

Place each test by structure, not by a list of names:

- Rust: acceptance lives in a crate's `tests/acceptance.rs` integration binary,
  or in a `#[cfg(test)] mod acceptance` when it needs private items.
  [`.config/nextest.toml`](../.config/nextest.toml) excludes both from the
  default profile and selects only them in the `acceptance` profile.
  `check` uses the default profile; `verify` uses `acceptance`.
- Node: unit tests live at `tests/*.test.{ts,mjs}`; acceptance lives in a
  package's `tests/acceptance/`, which only its `verify` task runs. TypeScript
  in both is checked statically, along with fixtures and examples.
- Configuration: native source/file acceptance lives in `src/bootstrap/tests/acceptance.rs`,
  using temporary homes and committed Git sources. It never applies to `$HOME`.

Host acceptance uses the installed application, such as the `pi` on `PATH`,
in an isolated temporary home and session; it never contacts the live server.
Pi SDK fixtures also check that the selected host matches the workspace pin.
To test a candidate Pi release without switching the live installation, install
its dependencies in an isolated checkout and run from that checkout's root:

```sh
PATH="$PWD/node_modules/.bin:$PATH" PI_TEST_HOST_ROOT="$PWD/node_modules/@earendil-works" mise run //src/pi:verify
```

`PI_TEST_HOST_ROOT` selects the SDK installation; `PATH` selects the CLI and
its worker children. Both must target the same pinned release.

Cross-package check and verify tasks build the candidate dots CLI through
`//src/dots:test-cli` and put `target/debug` first on their test processes' PATH.
They never install it. For direct, focused Node tests, run that prerequisite and
select the same candidate path explicitly. Test fixtures isolate `HOME`,
`XDG_DATA_HOME` and `XDG_STATE_HOME` before catalog mutations.

## Development

Use `mise tasks --all` for available tasks. Root `check` and `verify` run the
whole Cargo workspace in one pass; a project keeps a `mise.toml` only for real
tasks such as build, install, deps, or non-Cargo checks. Scope focused work with
the native tools:

```sh
cargo nextest run -p dots-pick
cargo nextest run -p dots-process --profile acceptance
mise run //src/pi:check
```

Code lives under named owners in `src/`, configuration trees in `home/`, and canonical
skills in `skills/`.

TypeScript packages run directly on Node 24 or their Pi host; checks do not emit
JavaScript. Their shared [compiler contract](../tsconfig.base.json) uses Node
APIs, not browser globals. `mise run check:typescript` checks tooling types,
ESLint safety rules, and Prettier formatting without modifying files. Scope a
check with `pnpm exec eslint <paths>` or `pnpm exec prettier --check <paths>`;
use `pnpm --filter <package> run typecheck` for a package's complete program.
Pi's MJS unit tests, acceptance tests, fixtures and dev runner receive type-aware
safety linting; full JS typechecking is opt-in with `// @ts-check`. Tooling JS is
typechecked.

Rustup selects Rust from `rust-toolchain.toml`; mise pins the other SDKs in
`mise.toml`. Install rustup with its own installer (rustup.rs), not Homebrew or
mise: its proxies in `~/.cargo/bin` come first on `PATH` (the zsh config puts
them there), and `mise run doctor` fails if the `rustc` it finds is not the pin.
After editing `mise.toml`, run
`MISE_LOCKED=false mise lock --platform macos-arm64`, review the lock, install,
and verify. Rust builds use the pinned toolchain; no older compiler support is
promised.

## Machine setup

[Machine setup](../bootstrap/README.md) applies selected native configuration
modules separately from this checkout's development SDK. Its [Rust acceptance suite](../src/bootstrap/README.md)
runs under `verify`, using disposable source trees as well as homes.

## Activation

Run installation only when ready to change the live setup:

```sh
mise run //src/dots:install         # dots CLI
mise run //src/pick:install         # dots-pick, for Rex views
mise run //src/diff:install         # dots-diff, for the Rex diff split
mise run pi:packages               # selected third-party Pi resources, from HOME
mise run pi:install                # register the installed module's durable source, from HOME
```

Use [machine setup](../bootstrap/README.md) to deploy native home groups,
including Neovim and selected desktop configuration. Runtime directories stay
outside source. Native status and explicit unapply/prune own removal.

Cargo owns executable installation. `dots`, `dots-pick` and `dots-diff` install with `cargo install --locked --path .` into Cargo's bin directory
(`~/.cargo/bin`, on `PATH` through mise and the zsh configuration); Cargo
owns their tracking and replacement.

[Pi installation](pi.md#setup) works the same way: `pi install` registers
`src/pi` by path, so sessions load the checkout's files on their next `/reload`
or start. Install it only from the primary checkout, never from an agent
worktree. Installation never reloads sessions.
