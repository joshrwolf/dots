# Home sweet $HOME

Personal macOS dotfiles managed with GNU Stow, with mise tasks for development
and explicit installation.

## Setup

Install Homebrew and Apple Command Line Tools, then run from this checkout:

```sh
brew bundle --file=home/brew/.Brewfile
mise trust
mise install --locked
mise run install
```

Base installation links shell, terminal, theme, and agent configuration. It does
not install application binaries, activate Herdr plugins, or enable optional
services. See [build and activation](docs/build.md) for checks and installation
boundaries.

Codex settings stay machine-local in `~/.codex/config.toml`. Private Brew additions
can live in `~/.Brewfile.local`; apply them with
`brew bundle --file="$HOME/.Brewfile.local"`. Keep credentials and runtime state
outside this public repository.

## Use

- [dots CLI](src/dots/README.md): agent workflows for GitHub, Herdr, and shared tasks.
- [Pi](docs/pi.md): managed CLI, companion extensions, and themes.
- [Claude mods](docs/claude-mods.md) and [shared skills](docs/skills.md).
- [Herdr navigation](src/herdr/nav/README.md),
  [PR status](src/herdr/github/README.md),
  [issue workspaces](src/herdr/issue/README.md), and
  [local review](src/herdr/review/NEOVIM.md).
- [Neovim](home/nvim/.config/nvim/README.md): independently installed editor config.
- [Commit signing](docs/gitsign.md) and [Finicky routing](docs/finicky.md).

Shared themes live in `home/theme/.config/theme/themes/`; the `current` symlink
selects a palette. Applications consume their own adapters from that directory.
