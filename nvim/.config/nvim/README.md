# Neovim

Small personal configuration using documented Neovim APIs. Bob selects the
Neovim version; upgrades are manual.

- `init.lua` sets leaders and loads modules explicitly, in order.
- `lua/config/options.lua` contains intentional changes to editor defaults.
- `lua/config/plugins.lua` declares native packages and configures them explicitly.
- `lua/config/keymaps.lua` contains personal mappings.
- `lua/dots/herdr.lua` registers this editor for Herdr file navigation; see the
  [editor connection contract](../../../herdr-plugins/nvim/README.md).
- `lua/dots/review/` connects normally opened editors to saved review discussions.
  `:ReviewThreads` / `<leader>fR` opens the thread list; reply, resolve and refresh
  work in its discussion split. See [review integration](../../../herdr-plugins/review/NEOVIM.md).
  `<leader>fr` remains recent files; setup does not launch a bridge.

Run `make nvim` from the dotfiles root to link this configuration.

Keep defaults unless there is a reason to change them. Add modules when they
have a purpose; avoid automatic module discovery and custom setup wrappers.
Use `after/ftplugin/<filetype>.lua` for buffer-local filetype settings, and
respect project EditorConfig settings.

Plugins use `vim.pack`; keep its generated `nvim-pack-lock.json` in version control.
Missing plugins install on first launch. Updates are manual: run `:packupdate`,
review the changes, then `:write` to apply or `:quit` to discard. Review the
lockfile diff along with updates. Keep updates separate from Neovim upgrades.
Prefer documented APIs already available in releases over newly experimental
nightly features.

Keep this README as a stable overview. Put plugin-specific notes beside their
configuration and use mapping descriptions to document shortcuts.

The previous configuration is preserved in `backups/nvim-lazyvim-2026-09-11`
at the dotfiles root.
