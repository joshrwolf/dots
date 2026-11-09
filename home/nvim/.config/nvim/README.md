# Neovim

A small personal configuration using native packages and documented APIs.

## Setup

Mise selects the latest interactive Neovim release; upgrades are deliberate.
The repository SDK selects its test runtime separately. [Machine setup](../../../../bootstrap/README.md)
deploys this configuration, its language servers and Rustup editor components
as part of the portable base.

Missing `vim.pack` plugins install on first launch. For updates, run
`:packupdate`, review changes, then `:write` to apply or `:quit` to discard.
Review the committed `nvim-pack-lock.json` diff. Keep plugin updates separate
from editor upgrades.

## Use

Mapping descriptions document shortcuts. In Rex terminals, clipboard operations
use native OSC52 to reach the viewing client rather than the server's desktop
clipboard. Outside Rex, Neovim selects the host's native clipboard provider.

Contributor guidance lives in [AGENTS.md](AGENTS.md).
