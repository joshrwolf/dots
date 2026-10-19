# Cargo, build, and validation

Use this reference for workspace membership, dependencies, mise tasks,
release staging, and checks. Read the files themselves before changing exact
versions or lint settings.

## Workspace shape

The repository-root `Cargo.toml` is the authority for workspace membership,
resolver, edition, and minimum Rust version.

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

The exact mise-pinned toolchain includes `clippy` and `rustfmt`. Workspace lints forbid
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
command = ["mise", "run", ":prepare"]
```

Each plugin owns a `prepare` task that runs
`cargo install --frozen --path . --root . --force` from that plugin's directory.
It is an explicit host installation callback, distinct from ordinary builds.
Cargo owns compilation, executable placement, and installation tracking/locking;
do not disable tracking or add another copy/staging dispatcher.

Normal `build` tasks use `--root dist`, not active plugin directories. Each executable package has its own task;
Cargo shares compilation work through the root workspace. Run `mise run deps` to
fetch locked dependencies before an offline build.

Useful targets from the repository root:

```sh
mise run //src/herdr/find:build   # stage one plugin through Cargo
mise run rust-build              # all native executables
mise run //src/herdr:link          # link every manifest-backed plugin
mise run //src/herdr:install                 # build, restow, then link plugins last
```

`[[build]]` runs during plugin installation, not local `plugin link`, so linking
must follow successful staging. The `link` task prepares immutable release directories and treats any link
failure or missing Herdr executable as fatal. Registration is not a transaction
across all plugins: preceding links may have succeeded, so retry explicitly.

## Complete validation gate

Run from the repository root:

```sh
mise run rust-check
```

This is intentionally one gate:

1. `cargo fmt --all --check`;
2. `cargo clippy --locked --workspace --all-features --all-targets -- -D warnings`;
3. `cargo nextest run --locked --workspace --all-features --all-targets`, including
   pinned Neovim dependencies and manifest contracts;
4. `cargo test --locked --workspace --all-features --doc`, because nextest does
   not run doctests;
5. `cargo doc --locked --workspace --all-features --no-deps` with
   `RUSTDOCFLAGS="-D warnings"`.

Do not substitute plain `cargo test` for the workspace nextest gate. Update the
`src/herdr/tests` crate whenever a manifest's action/pane IDs, plugin set, build
primitive, or lifecycle contract changes.
