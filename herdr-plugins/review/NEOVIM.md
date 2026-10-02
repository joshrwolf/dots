# Review discussions in ordinary Neovim

Open Neovim yourself in a Herdr workspace with an existing review binding.
Review content stays in `review-core`'s repository-local SQLite registry, not in
an editor process. There is no persistent-editor/controller dependency.

The configured surface is deliberately small:

- `:ReviewThreads` / `<leader>fR`: choose a saved thread, including resolved ones.
- `:ReviewReply` / buffer-local `<leader>rp`: add a local reviewer reply.
- `:ReviewResolve` / buffer-local `<leader>rr`: resolve the selected thread locally.
- `:ReviewRefresh`: reload persisted discussion state.
- `q`: close the read-only discussion split.

`<leader>fr` remains recent files. The existing CodeDiff mappings are unchanged.
Reply input is currently a single prompt. An uncertain save is never retried
automatically; its draft remains in the discussion view until it closes. Refresh
and inspect the saved messages before deciding whether to retry.

Setup is passive: the private `herdr-review --stdio <checkout>` process starts
only when a review operation is requested. Build the review plugin through the
root `make herdr-build` target; the client finds the staged binary or one on PATH.
`vim.g.herdr_review_binary` can explicitly select an executable.

The client is adapted from the previous configuration's review adapter, including
bounded JSON frames, request IDs, capability negotiation, and revision-consistent
pagination. Current-pane lookup supplies the bridge child's workspace/pane context;
stale inherited coordinates are not reused after a move. A changed caller/socket
identity retires the cached connection, and the repository bridge validates the
requested binding's server/workspace scope. This is not an atomic transaction
with pane movement after the final lookup.

Discussion views show General threads, findings/evidence, messages, and saved
anchor coordinates. They intentionally do **not** jump to those coordinates in
the working tree: a saved comparison is not necessarily the current file.

## Next slice

Restore/adapt the existing CodeDiff integration for opening the exact saved
comparison, jumping to resolved anchors, and creating new anchored threads.
Inline annotations, per-file navigation, multiline composition, and review-agent
commands from the old adapter are not enabled in this surface yet. No command
here dispatches agent work or publishes GitHub feedback.

## Evidence

`tests/neovim.rs` starts disposable ordinary Neovim instances, drives the real
Lua client and compiled stdio bridge, saves a reply and resolution, then reopens
Neovim and verifies the same durable discussion. It also checks that stale
inherited coordinates are replaced and a moved workspace cannot mutate the old
binding. Only current-pane discovery and interactive UI choices are supplied by
the fixture; database/IPC operations are real. The editor mini.test suite covers
passive setup, General rendering, late responses, cancelled input and uncertain
saves. The tests do not load the user's config or contact a live Herdr server.
