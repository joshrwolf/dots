# Cross-machine substrate

`dots-substrate` stores content-addressed blobs and compare-and-swap refs on a
shared backend. Domains such as memory and tasks own their formats and conflict
rules; they reach other machines only through this crate.

A backend lists refs, fetches one ref with its blobs, and updates refs in a
batch with an independent outcome per ref: applied, conflict, or unknown. Updates
are never atomic across refs, and a rejected update may still have stored its
blobs. An optional watch only reduces listing; it never changes results.

`Substrate` adds the per-machine parts: a blob cache that verifies every read, a
freshness record shared by every process using one state directory, an outbox
that coalesces deferred updates per ref and flushes them in one batch under a
token-bucket push budget, and the machine identity. A deferred update that
another writer pre-empts stays as a stranded entry for manual reconciliation.
Nothing runs in the background; callers pass the current time.

Backends:

- `memory`: the in-process reference implementation, with a fake watch that can
  drop, duplicate and delay hints.
- `git`: one SHA-1 Git remote such as a private GitHub repository, with refs
  under `refs/dots/` and a private local bare cache repository. Git runs without
  user or system configuration; credentials come only from configuration the
  caller supplies.

Every backend runs the same [conformance suite](src/conformance.rs). `check`
runs it against the memory backend and a local `file://` Git remote. The
acceptance test runs it against a real remote and prints list, update and fetch
latency; it skips unless `DOTS_SUBSTRATE_TEST_REMOTE` names a disposable remote:

```sh
DOTS_SUBSTRATE_TEST_REMOTE=https://github.com/OWNER/REPO.git \
DOTS_SUBSTRATE_TEST_CREDENTIAL_HELPER='!gh auth git-credential' \
  cargo nextest run -p dots-substrate --all-features --profile acceptance --no-capture
```
