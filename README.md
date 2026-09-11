# Home sweet $HOME

Personal dotfiles for macOS, managed with GNU Stow.

Bootstrap a new machine, then link the base dotfiles:

```sh
brew bundle --file=brew/.Brewfile
make
```

Plain `make` only restows `aerospace`, `brew`, `codex`, `ghostty`, `theme`, and
`zsh`. Run `make herdr` explicitly to build the Herdr plugins, restow the
`herdr` and `nvim` packages, and link the plugins. `make herdr-check` runs the
Rust and Neovim checks without installing or reloading plugins.

Run `make nvim` to link only the minimal Neovim configuration, independently
of the Herdr plugins. Its setup notes live in `nvim/.config/nvim/README.md`.

Shared application themes live under `theme/.config/theme/themes`. The
`theme/.config/theme/current` symlink selects one theme directory. Ghostty
includes its adapter from there. Herdr's theme uses the corresponding application
palette so all of its UI surfaces match the selected terminal theme.
