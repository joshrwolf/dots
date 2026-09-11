# Home sweet $HOME

Personal dotfiles for macOS, managed with GNU Stow.

Run `make` to build the Herdr plugins, restow the dotfile packages into `$HOME`,
and link the plugins. `make check` runs the Rust and Neovim checks without
installing or reloading plugins.

Each top-level directory is a Stow package except the project directories listed
in `PROJECTS` in the Makefile. Neovim configuration lives under `nvim/`, and the
Rust plugin workspace lives under `herdr-plugins/`.
