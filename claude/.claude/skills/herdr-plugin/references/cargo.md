# Cargo, build, and validation

Use this reference for workspace membership, dependencies, Make targets,
release staging, and checks. Read the files themselves before changing exact
versions or lint settings.

## Workspace shape

`herdr-plugins/Cargo.toml` is one resolver-v3 workspace:

```toml
[workspace]
resolver = "3"
members = ["contracts", "crates/herdrkit", "find", "github", "nav", "nvim"]

[workspace.package]
edition = "2024"
rust-version = "1.89"
license = "MIT"
publish = false
```

Dependencies and lints are declared at the workspace root. Every member uses
`edition.workspace`, `rust-version.workspace`, `license.workspace`, and:

```toml
[lints]
workspace = true
```

Plugin packages are named `herdr-<directory>`, disable automatic binaries, and
declare an explicit library and binary:

```toml
[package]
name = "herdr-example"
autobins = false

[lib]
path = "src/lib.rs"

[[bin]]
name = "herdr-example"
path = "src/main.rs"
```

Keep `main.rs` at the environment/exit-code boundary and put behavior in the
library. Add `herdrkit` as a normal dependency. When tests use the fake server,
enable it only under dev dependencies:

```toml
[dev-dependencies]
herdrkit = { workspace = true, features = ["testing"] }
```

Do not add a direct `crossterm` dependency; use `ratatui::crossterm`, which is
the version Ratatui was built against. Keep direct dependencies for crates the
code uses directly even when another crate also happens to pull them in.

## Lints and toolchain

The stable toolchain includes `clippy` and `rustfmt`. Workspace lints forbid
unsafe code and deny panic-prone shortcuts including `unwrap_used`,
`expect_used`, `panic`, `panic_in_result_fn`, `indexing_slicing`, `todo`,
`unimplemented`, and `dbg_macro`. `pedantic` and `rust_2018_idioms` use
`priority = -1` so targeted exceptions can win.

`clippy.toml` permits unwrap, expect, and panic in tests, but not `dbg!`. Prefer
`#[expect(lint, reason = "...")]` for a justified item-level exception; it
becomes stale loudly if the lint stops applying.

## Per-plugin build primitive

Every plugin manifest must have exactly one build command:

```toml
[[build]]
command = ["make", "-C", "../..", "plugin-build-example"]
```

The root Makefile derives `plugin-build-<name>` targets from directories that
contain `herdr-plugin.toml`. Each target:

1. verifies the manifest exists;
2. builds only `--package herdr-<name>` in release mode;
3. verifies the built file is executable;
4. copies it to `<plugin>/bin/herdr-<name>.tmp.<pid>`;
5. sets the executable bit and renames it over the destination in the same
   directory.

The same-directory rename is the publication boundary: a running Herdr process
must see either the old complete executable or the new complete executable,
never a partially copied file. Do not replace this with helper build scripts or
a broad build-and-copy loop.

Useful targets from the repository root:

```sh
make plugin-build-example  # build and atomically stage one plugin
make plugins-build         # all manifest-backed plugins
make plugins-link          # link every manifest-backed plugin
make                       # build, restow, then link plugins last
```

`[[build]]` runs during plugin installation, not local `plugin link`, so linking
must follow successful staging. `plugins-link` is idempotent and treats any
link failure as fatal. It skips cleanly only when the `herdr` executable is not
installed.

## Complete validation gate

Run from the repository root:

```sh
make check
```

This is intentionally one gate:

1. dry-run every derived `plugin-build-<name>` target;
2. `cargo fmt --all --check`;
3. `cargo clippy --workspace --all-features --all-targets -- -D warnings`;
4. `cargo nextest run --workspace --all-features --all-targets`;
5. `cargo test --workspace --all-features --doc`, because nextest does not run
   doctests;
6. `cargo doc --workspace --all-features --no-deps` with
   `RUSTDOCFLAGS="-D warnings"`.

Do not substitute plain `cargo test` for the workspace nextest gate. Update the
`contracts` crate whenever a manifest's action/pane IDs, plugin set, build
primitive, or lifecycle contract changes.
