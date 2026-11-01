# Home sweet $HOME

Portable workstation configuration and tooling, composed with native mise.

## Setup

Use mise 2026.10.25 or newer. From reviewed source, select the committed
repository to install:

```sh
cd bootstrap/source
mise trust
mise vars "dots_public_url=$(git rev-parse --show-toplevel)"
mise bootstrap --only files,repos,dotfiles,task
```

The source project clones into persistent home, installs native configuration
modules, and applies their selected resources. It does not install uncommitted
changes. Use `-E desktop` for macOS desktop configuration and `-E private` for
explicit private composition.

See [machine setup](bootstrap/README.md) for source selection, optional modules
and SSH setup. Repository
development and checks have separate [SDK setup](docs/build.md).

Credentials, sessions and device enrollment remain machine-local. No setup
operation enrolls file synchronization or prunes unrelated packages.

## Use

- [dots CLI](src/dots/README.md): GitHub, memory and shared tasks.
- [Pi](docs/pi.md): managed CLI and companion packages.
- [Shared skills](docs/skills.md) and [Claude mods](docs/claude-mods.md).
- [Rex](home/rex/.config/rex/README.md): sessions and fuzzy navigation.
- [Neovim](home/nvim/.config/nvim/README.md).
- [Commit signing](docs/gitsign.md) and [Finicky routing](docs/finicky.md).

Shared themes live under `home/theme/.config/theme/themes/`; the `current`
link selects their adapters.
