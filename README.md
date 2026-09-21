# Home sweet $HOME

Personal dotfiles for macOS, managed with GNU Stow.

Bootstrap a new machine, then link the base dotfiles:

```sh
brew bundle --file=brew/.Brewfile
make
```

Plain `make` only restows `aerospace`, `brew`, `codex`, `ghostty`, `pi`, `skills`,
`theme`, and `zsh`. Run `make herdr` explicitly to build the Herdr plugins, restow the
`herdr` and `nvim` packages, and link the plugins. `make herdr-check` runs the
Rust and Neovim checks without installing or reloading plugins.

Run `make nvim` to link only the minimal Neovim configuration, independently
of the Herdr plugins. Its setup notes live in `nvim/.config/nvim/README.md`.

Shared application themes live under `theme/.config/theme/themes`. The
`theme/.config/theme/current` symlink selects one theme directory. Ghostty
includes its adapter from there. Herdr's theme uses the corresponding application
palette so all of its UI surfaces match the selected terminal theme.

## Pi

The `pi` package installs independent personal instructions at
`~/.pi/agent/AGENTS.md` (no `CLAUDE.md`) and a `dots` theme linked through
`~/.config/theme/current/pi.json`. Shared skills are discovered automatically
from `~/.agents/skills`; no additional copies or links are needed.

After `make`, select `dots` in Pi's `/settings` once. To preview the theme without saving:

```sh
pi --use-theme dots
```

Pi's `settings.json` stays machine-local: model preferences and runtime
bookkeeping are not Stow-managed. Credentials, sessions, caches, and trust
records also stay outside dots. Ctrl+G uses the shell's existing `EDITOR=nvim`.
No extensions or custom keybindings are required for this base setup.

Optional Pi extensions live in `pi-plugins/`: pinned, editable upstream subtrees
and our own companion package. `make pi-install` installs locked dependencies
and registers local packages; `make pi-check` runs their checks. Neither is part
of plain `make`. See [Pi package setup and maintenance](docs/pi.md), including
the initial subtree import prerequisite.

Edit `theme/.config/theme/themes/github-light/pi.json` to tune the palette;
Pi hot-reloads edits to the active theme. After changing the central `current`
symlink, use `/reload` or restart Pi. New theme directories should provide a
`pi.json` adapter with the stable name `dots`.
