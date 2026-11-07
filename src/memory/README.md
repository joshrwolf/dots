# Knowledge storage

`dots-memory` stores shared project identities and immutable knowledge snapshots
on the [cross-machine substrate](../substrate/README.md). It keeps no Git
repository of its own.

A snapshot is a manifest blob: canonical compact JSON with its parent snapshots,
the store identity, the shared project registry, and every document's path,
SHA-256 hash and size. The snapshot ID is the manifest's hash (`sha256:<hex>`).
Documents are blobs; the substrate ref `memory` names the latest manifest.

Reads check the remote first. `context` reuses a listing, or a failed attempt
to list, up to 30 seconds old; explicit reads and searches always list. A
listing that takes over 5 seconds counts as unreachable. When the remote is
unreachable, reads use the last snapshot this machine saw and label it stale. A
machine that never listed has nothing to read.

A publication changes global and project documents against one expected
snapshot and lands with a synchronous compare-and-swap. When the ref moved on,
the store merges three ways against the unique merge base found by walking
manifest parents. Changes to different documents produce a two-parent snapshot
(incoming first, local second). Divergent edits to one document, file/directory
collisions and case collisions are retained as a local candidate with an
integration plan; `memory reconcile` publishes it with an explicit choice for
every conflict, even when text could merge. A publication made while the remote
is unreachable, or whose outcome is unknown, is retained the same way and
published by the next online publication or reconcile.

Retained candidates and their blobs live on this machine only, under
`$XDG_DATA_HOME/dots/memory`, beside the local project bindings. Snapshots stay
bounded: at most 4096 documents and 64 MiB per snapshot; oversized publications
fail rather than prune. Removing a document changes the next snapshot; earlier
snapshots keep it.

`convert` turns a store in the earlier native Git layout (`repo.git`,
`store.json`, `bindings.json`) into manifests with the same parents and
byte-identical documents. It reads the source with isolated read-only Git
plumbing, publishes only when no `memory` value exists, and verifies every
document after publishing.
