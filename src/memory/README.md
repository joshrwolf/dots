# Memory storage

`dots memory` manages machine-local Markdown knowledge without a service or
search index. Use `dots memory --help` for commands and the shared
[memory skill](../../skills/memory/SKILL.md) for recall and publication guidance.

Reads and searches create nothing. Writes create directories as needed, compare
a full-file SHA-256 revision under a short cooperative lock, and atomically
replace one file. Missing files use the revision `missing`. Removal uses the
same revision check. New file contents are private to the user on Unix.

Search matches case-insensitive literal text in live files.
It returns bounded path/line excerpts, identifies its backend, and paginates each
scope independently. Sources and the change log are opt-in. These results are
retrieval candidates, not evidence that a conclusion is current. There is no
semantic ranking, and `dots` never invokes QMD.

## Constraints

- Multi-file edits are not transactions. Publish topics before the index and
  repair interrupted updates. A crash can leave `.write-lock`; coordinate with
  its owner before recovery. Never remove locks based solely on a reused PID.
- Locks and symlink checks protect cooperating agents from mistakes, not hostile
  concurrent filesystem mutation. Ordinary editors can bypass revision checks.
- Paths retain Pi's checkout-key convention: linked worktrees share a scope,
  separate clones do not. Normalization can collide; moving a checkout requires
  moving its memory directory explicitly.
- Reads cap files at 256 KiB and pages at 16 KiB. Search caps the corpus at 1,000
  Markdown files and eight directory levels, failing explicitly when exceeded.
- Deleting a file does not erase backups, Git history, or external search indexes.
