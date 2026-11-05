# Knowledge storage

`dots-memory` owns a private bare Git repository, shared project identities and
immutable knowledge snapshots. A publication changes any number of global and
project documents against one expected snapshot. Readers select a published
snapshot; they never read an incoming candidate or a partial batch.

The store uses installed Git through `dots-process`, with bounded I/O, isolated
configuration, protected references and explicit durability settings. It has no
working tree, automatic pruning, network activity during reads, or machine-local
task data. Filesystem document utilities belong to `dots-documents`.
