# Home sweet $HOME

Personal dotfiles for macOS, managed with GNU Stow. Mise owns pinned build tools
and local tasks; Cargo and npm retain their native dependency graphs.
See [build and activation](docs/build.md) for the complete lifecycle.

Bootstrap a new machine, then link the base dotfiles:

```sh
brew bundle --file=home/brew/.Brewfile
mise trust
mise install --locked
mise run install
```

Codex settings and trust records stay machine-local in `~/.codex/config.toml`,
not Stow-managed. Optional private Brew additions can live in `~/.Brewfile.local`;
apply them with `brew bundle --file="$HOME/.Brewfile.local"`.

`mise run install` only restows `aerospace`, `brew`, `codex`, `ghostty`, `pi`, `skills`,
`theme`, and `zsh`. Run `mise run //src/herdr:install` explicitly to build the Herdr plugins, restow the
`herdr` and `nvim` packages, and link the plugins. `mise run rust-check` runs the
Rust and Neovim checks without installing or reloading plugins.

Run `mise run nvim:install` to link only the minimal Neovim configuration, independently
of the Herdr plugins. Its setup notes live in `home/nvim/.config/nvim/README.md`.

Optional gitsign credential caching uses a LaunchAgent and a per-repository
signing wrapper. See [gitsign setup and preview](docs/gitsign.md); `mise run gitsign:install`
explicitly installs and enables it.

Optional browser-profile routing uses Finicky. `mise run finicky:install` links its config;
see [Finicky setup](docs/finicky.md) for activation and the signing-link rule.

Shared application themes live under `home/theme/.config/theme/themes`. The
`home/theme/.config/theme/current` symlink selects one theme directory. Ghostty
includes its adapter from there. Herdr's theme uses the corresponding application
palette so all of its UI surfaces match the selected terminal theme.

## Pi

Pi uses the official managed installer, with its own Node.js runtime; it does
not depend on Homebrew or system npm. Run `mise run pi-cli:install` to install it,
and `mise run pi-cli:update` to update it later. Restart Pi after CLI updates
(`pi -c` continues the previous session). These targets do not update pinned
plugins.

The `pi` package installs independent personal instructions at
`~/.pi/agent/AGENTS.md` (no `CLAUDE.md`) and a `dots` theme linked through
`~/.config/theme/current/pi.json`. Shared skills are discovered automatically
from `~/.agents/skills`; no additional copies or links are needed.

After `mise run install`, select `dots` in Pi's `/settings` once. To preview the theme without saving:

```sh
pi --use-theme dots
```

Pi's `settings.json` stays machine-local: model preferences and runtime
bookkeeping are not Stow-managed. Credentials, sessions, caches, and trust
records also stay outside dots. Ctrl+G uses the shell's existing `EDITOR=nvim`.
No extensions or custom keybindings are required for this base setup.

Optional Pi extensions use pinned upstream npm releases and our companion in
`src/pi/`. `mise run //src/pi:upstream-install` installs upstream pins;
`mise run //src/pi:install` activates a prepared companion release.
Neither is part of base Stow installation. See [Pi setup](docs/pi.md).

Edit `home/theme/.config/theme/themes/github-light/pi.json` to tune the palette;
Pi hot-reloads edits to the active theme. After changing the central `current`
symlink, use `/reload` or restart Pi. New theme directories should provide a
`pi.json` adapter with the stable name `dots`.

## Agent CLI

`dots` is a general-purpose workflow CLI for coding agents. Commands bridge local
tools and integrations through a shared interface with discoverable help and
structured results. GitHub CI inspection is its first workflow.

`mise run //src/dots:install` installs the executable and its generated discovery skill for Codex,
Claude, and Pi. Start with `dots --help`.

See [the CLI home](src/dots/README.md) for architecture, command discovery,
and how to add new workflows.

## Claude mods

Claude Code mods live in `src/claude/`. Committed links in the skills package
install them into `~/.claude/skills`, where Claude loads and hot-reloads them.
`mise run //src/claude/codex-agent:check` validates and tests them. See [Claude mods](docs/claude-mods.md).
