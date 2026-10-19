# Herdr workspace resources

The GitHub plugin requires `workspace-resources.patch` on upstream Herdr
[`d6b40d4edd550ccea081f089605a64314f8c8b27`](https://github.com/herdrdev/herdr/commit/d6b40d4edd550ccea081f089605a64314f8c8b27).
Both client and server need the patch for dynamic, selectable PR children.

## Setup

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

## Constraints

Persistent workspace UUIDs guard against recycled short IDs. Resource writes and
invocations validate identity, revision, and expiry; fixed status-token slots are
not a compatible fallback. Collections expire and are rebuilt by their producer
rather than restored as authoritative state.

The optional endpoint frame preserves frozen bincode layouts. Desktop spaces
support selectable PR children; compact/mobile navigation remains workspace-only.
The patch and typed client define the wire contract.
