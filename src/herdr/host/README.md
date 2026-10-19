# Herdr workspace resources

The GitHub plugin requires `workspace-resources.patch` on upstream Herdr
[`d6b40d4edd550ccea081f089605a64314f8c8b27`](https://github.com/herdrdev/herdr/commit/d6b40d4edd550ccea081f089605a64314f8c8b27).
Both client and server need the patch for dynamic, selectable PR children.

From the dots root:

```sh
git clone https://github.com/herdrdev/herdr.git /tmp/herdr-workspace-resources
git -C /tmp/herdr-workspace-resources checkout --detach d6b40d4edd550ccea081f089605a64314f8c8b27
git -C /tmp/herdr-workspace-resources apply "$PWD/src/herdr/host/workspace-resources.patch"
cd /tmp/herdr-workspace-resources
cargo build --release --locked
```

Use the checkout's pinned Rust and Zig versions. Install `target/release/herdr`
atomically, then use `server.live_handoff` with `import_exe` pointing to the new
binary to preserve running panes. Do not stop the active server to update it.
Run `mise run //src/dots:install` and `mise run //src/herdr:install` from dots to install the CLI, plugins and config.
An upstream host update will require reapplying or replacing this patch.

## Contract

- Workspace JSON includes a persistent UUID `identity`; short IDs remain handles.
  Legacy snapshots receive identities on restore, persisted by the next save.
- `workspace.resources.get` reads collections. `workspace.resources.replace`
  accepts `workspace_id`, `identity`, `plugin_id`, `expected_revision`, `ttl_ms`
  and `resources`. Revision checks prevent stale writes.
- Resources carry `id`, `label`, `status`, `tone`, `completed`, `action` and
  `reference`, plus optional `detail: {text, tone}` for a second line. Both lines
  share one selection and click target. Actions belong to the enabled producer; the reference is passed as
  `clicked_url`. Invocation checks identity, revision and expiry.
- Collections expire and are rebuilt by their producer after restart. Bounds are
  512 items per producer, 32 producers per workspace and a 24-hour maximum TTL.
- The optional `endpoint.workspace-resources.v1` frame carries resources without
  changing frozen bincode layouts. Desktop spaces show selectable children and a
  collapsible completed group; compact/mobile navigation remains workspace-only.

Keep UUID guards and dynamic collections; fixed status-token slots are not a
compatible fallback.
