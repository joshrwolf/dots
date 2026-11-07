# Knowledge storage

`dots-memory` stores shared project identities and immutable knowledge snapshots
on the [cross-machine substrate](../substrate/README.md). It keeps no Git
repository of its own.

A snapshot is a manifest blob: canonical compact JSON with its parent snapshots,
the store identity, the shared project registry, and every document's path,
SHA-256 hash and size. The snapshot ID is the manifest's hash (`sha256:<hex>`).
Documents are blobs; the substrate ref `memory` names the latest manifest.

Reads use this machine's substrate copy and never contact the remote. Each
reports when the copy was last confirmed current and whether sync is failing;
the CLI labels a copy over two minutes old stale. The sync agent keeps the copy
current; `--fresh` runs one sync pass first. A machine that has never synced
has nothing to read.

A publication runs one sync pass, then changes global and project documents
against one expected snapshot and lands with a synchronous compare-and-swap.
After a lost race it waits a random, doubling time, syncs and tries again. When
the ref moved on,
the store merges three ways against the unique merge base found by walking
manifest parents. Changes to different documents produce a two-parent snapshot
(incoming first, local second). Divergent edits to one document, file/directory
collisions and case collisions are retained as a local candidate with an
integration plan; `memory reconcile` publishes it with an explicit choice for
every conflict, even when text could merge. A publication made while the remote
is unreachable, whose outcome is unknown, or that loses every race is retained
the same way and published by the next online publication or reconcile.

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
