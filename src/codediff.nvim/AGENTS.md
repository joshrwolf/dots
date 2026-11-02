# codediff.nvim

Owned fork of [esmuellert/codediff.nvim](https://github.com/esmuellert/codediff.nvim),
brought in at v4.0.6 with `git subtree add --squash`. It is ours to change
freely; it takes no upstream pulls. Port a later upstream fix by hand when one
is worth having. Upstream's MIT `LICENSE` and `ATTRIBUTION.md` stay.

## Binaries

The plugin is Lua only. Its native pieces are upstream release binaries,
pinned as mise tools in [tools.toml](tools.toml) with per-platform assets for
macOS arm64 and Linux x64/arm64:

- `github:esmuellert/codediff.nvim` installs the diff library as
  `libvscode_diff` on `PATH`; `lua/codediff/core/diff.lua` loads it with
  LuaJIT FFI. The `ffi.cdef` declarations there must match the pinned release;
  `tests/integration/core/diff/ffi_spec.lua` checks the loaded version against
  the pin. Bump the version, declarations and lock together.
- `github:esmuellert/codediff` installs `codediff-watcher`, which
  `lua/codediff/core/watcher/process.lua` spawns for native refresh.

Never download at runtime. A missing library is an error naming its mise tool;
a missing watcher warns and falls back to polling. Linux releases link glibc and
`libgomp.so.1`, which machine setup installs as a system package.

## Install

[Machine setup](../../bootstrap/README.md) includes `tools.toml` and deploys
this directory as the `codediff` dotfile group to
`~/.local/share/nvim/site/pack/dots/start/codediff.nvim`, excluding tests and
development files. The Neovim configuration calls `require("codediff").setup()`
from `init.lua`; Neovim sources `plugin/` afterwards.

## Checks

`mise run check` runs StyLua with this directory's `.stylua.toml`, then the
unit and integration specs in headless Neovim (the repository's pinned
release). `mise run verify` adds the E2E specs, which drive embedded Neovim UIs
and the watcher process. `CODEDIFF_TEST_TARGET=<layer|dir|spec> mise run test`
runs a selection. Tests never need network access once the tools are
installed. See [tests/README.md](tests/README.md) for the test layers.
