# Neovim

A small personal configuration using native packages and documented APIs.

## Setup

Bob selects the interactive Neovim version; upgrades are manual. Mise pins the
separate test runtime. From the dots root:

```sh
mise run nvim:install
```

Missing `vim.pack` plugins install on first launch. For updates, run
`:packupdate`, review changes, then `:write` to apply or `:quit` to discard.
Review the committed `nvim-pack-lock.json` diff. Keep plugin updates separate
from editor upgrades.

## Use

Mapping descriptions document shortcuts. `<leader>fR` opens
[saved review threads](../../../../src/herdr/review/NEOVIM.md);
`<leader>fr` remains recent files. Review setup does not launch a bridge until
requested.

Contributor guidance lives in [AGENTS.md](AGENTS.md).
