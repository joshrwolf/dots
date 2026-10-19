# Local review in Neovim

Normally opened editors can read and update saved discussions in an existing
Herdr review binding.

## Setup

Open a review from Herdr's review picker, then start Neovim yourself in that
workspace. Install the [Neovim configuration](../../../home/nvim/.config/nvim/README.md)
and install `herdr-review` with `mise run //src/herdr:install`; the client
finds `~/.local/bin/herdr-review` or an executable on PATH. Set `vim.g.herdr_review_binary` to
select one explicitly.

Setup is passive. A private stdio bridge starts on the first review operation;
review content remains in the repository's registry, not the editor process.

## Use

- `:ReviewThreads` / `<leader>fR`: choose a saved thread, including resolved ones.
- `:ReviewReply` / discussion-local `<leader>rp`: add a reviewer reply.
- `:ReviewResolve` / discussion-local `<leader>rr`: resolve the thread locally.
- `:ReviewRefresh`: reload saved state.
- `q`: close the read-only discussion split.

`<leader>fr` remains recent files. Existing CodeDiff mappings are unchanged.

## Constraints

Replies use a single prompt. An uncertain save is never retried automatically;
the draft remains until the view closes. Refresh and inspect saved messages
before retrying.

Views show findings, evidence, General threads, and saved anchor coordinates.
They do not jump to working-tree coordinates: the saved comparison may differ
from the current file. This surface does not create anchored threads, dispatch
agent work, or publish GitHub feedback.

Caller discovery refreshes pane/workspace context instead of trusting inherited
coordinates. The bridge validates binding scope, but pane movement can still
race the final lookup.

`mise run //src/herdr/review:deps` restores locked Neovim test dependencies;
`mise run //src/herdr/review:check` runs the isolated Lua suite and bridge
checks, and `mise run //src/herdr/review:verify` adds the editor-restart
acceptance test. Neither loads the user's configuration or contacts a live server.
