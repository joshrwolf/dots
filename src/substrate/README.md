# Cross-machine substrate

`dots-substrate` stores content-addressed blobs and compare-and-swap refs on a
shared backend. Domains such as memory and tasks own their formats and conflict
rules; they reach other machines only through this crate.

A backend lists refs, fetches one ref with its blobs, and updates refs in a
batch with an independent outcome per ref: applied, conflict, or unknown. Updates
are never atomic across refs, and a rejected update may still have stored its
blobs. An optional watch only reduces listing; it never changes results.

`Substrate` adds the per-machine parts. A state directory holds a blob cache
that verifies every read, and one SQLite database (WAL, versioned schema)
shared by every process on the machine: the listing record, the known value of
each ref, an outbox that coalesces deferred updates per ref, stranded entries,
the token-bucket push budget, sync health, request counters and the machine
identity. Each change is one transaction. A binary that finds a newer schema
refuses the state rather than rewriting it.

Reads serve the local copy with the time it was last confirmed current, and
never contact the backend; a machine that has never synced says so. A sync
pass lists every ref, fetches changed values into the cache, flushes the outbox
in one batch within the budget and records the outcome. Passes are idempotent
and run one at a time per state directory: a scheduled pass skips while another
runs, an explicit one waits. A failed or offline pass backs off scheduled
passes with jittered exponential delays. A deferred update that another writer
pre-empts stays as a stranded entry for manual reconciliation. Writers touch the
state directory's `changes` marker so a sync agent or a reader can react
without polling. Callers pass the current time; `dots sync run` is the agent
that runs passes on each machine.

Backends:

- `memory`: the in-process reference implementation, with a fake watch that can
  drop, duplicate and delay hints.
- `git`: one SHA-1 Git remote such as a private GitHub repository, with refs
  under `refs/dots/` and a private local bare cache repository. Git runs without
  user or system configuration; credentials come only from configuration the
  caller supplies. Listing has a 5 second deadline. The cache keeps a local ref
  for every value it fetched or pushed, so garbage collection keeps history, and
  maintenance packs it once it holds a few thousand loose objects.

Every backend runs the same [conformance suite](src/conformance.rs). `check`
runs it against the memory backend and a local `file://` Git remote. The
acceptance test runs it against a real remote and prints list, update and fetch
latency; it skips unless `DOTS_SUBSTRATE_TEST_REMOTE` names a disposable remote:

```sh
DOTS_SUBSTRATE_TEST_REMOTE=https://github.com/OWNER/REPO.git \
DOTS_SUBSTRATE_TEST_CREDENTIAL_HELPER='!gh auth git-credential' \
  cargo nextest run -p dots-substrate --all-features --profile acceptance --no-capture
```
