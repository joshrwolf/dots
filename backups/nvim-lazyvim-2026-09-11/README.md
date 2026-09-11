# Neovim configuration

LazyVim-based configuration managed by the dots repository.

- `lua/config/`: editor options, keymaps, autocommands, and lazy.nvim setup.
- `lua/plugins/core.lua`: plugin configuration and overrides.
- `lua/herdr_review/`: the Herdr review UI and its tests.
- `lazy-lock.json`: pinned plugin revisions.

Run `make check` from the dots repository root to validate the configuration and
Herdr review integration.
