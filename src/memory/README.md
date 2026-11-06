# Knowledge storage

`dots-memory` owns a private bare Git repository, shared project identities and
immutable knowledge snapshots. A publication changes global and project documents
against one expected snapshot. Ordinary reads select published ancestry, never an
incoming candidate or partial batch.

The authority initiates native Git exchange over an explicitly selected Tailscale
SSH client. Pulls and full-bundle imports validate in private quarantine. Authority
returns stage under a separate Git-ref namespace; the replica validates history
and fences publication against its node identity and expected head. Divergent
edits to one document need explicit reconciliation even when text could merge.

Enrolled peers and this tool's exports are trusted producers. Integrity, lineage,
schema and practical resource bounds do not make Git a hostile-file sandbox.
Fetch cursors track retained history; delivery cursors record acknowledged accepted
snapshots, not an unfetched newer remote tip. Exactly one active authority
integrates a dataset; retire it before using an authority restored for recovery.

[History](src/history.rs) and [transfer](src/incoming.rs) budgets bound exchange.
Oversized history fails explicitly rather than triggering pruning or rewriting;
this is bounded knowledge storage, not an unlimited archive.

Installed Git runs through `dots-process` with isolated configuration, bounded I/O
and explicit durability settings. There is no working tree, automatic pruning,
network activity during ordinary reads, or replicated task/session data.
Filesystem document utilities belong to `dots-documents`.
