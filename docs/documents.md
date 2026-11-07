# Retained evidence

Dots keeps exact task evidence outside checkouts. Briefs remain editable
Markdown; published outputs preserve the bytes used for acceptance.
Publication records provenance, not truth, and does not accept work.

## Use

Publish a UTF-8 file with `dots output publish --create`, inspect the returned
slug and digest with `dots output show`, then select that exact pair with
`--evidence SLUG DIGEST` when recording a task result. Repeat `--evidence` for
multiple revisions. Small work can record a result without any output.

Results, status changes and evidence commit together. Each write checks that
the selected revisions are intact and replaces the current result; the earlier
result stays in history. Purged evidence is identified separately.

Task inspection exposes current evidence and a report-discovery command.
The discovery command preserves the project and output mode; its reports are
not necessarily accepted evidence. `dots task result-event` opens an
older outcome and its exact selections. Output associations provide discovery
before acceptance; detaching one does not erase historical evidence. Use CLI
help for publication, pagination, associations, availability and integrity checks.

In Pi, `/goal evidence TASK [EVENT|current] [PROJECT]` opens recorded results
and exact evidence without following a goal or starting work. The inspector also
browses task discovery attachments, which do not imply acceptance. Select an
explicit revision; use N/B for content pages. Availability is checked on each
read. Automatic goal context includes counts and event pointers, not reports.

An output slug fixes its filename and media type. Extending it publishes another
content digest, not a mutable latest version. Publishing A, B, then A returns
the original A. Retry metadata never overwrites the first committed metadata.
Payloads are UTF-8 Markdown, plain text or JSON, at most 1 MiB each. Native edits
to retained files are unsupported and detected as corruption.

## Files and retention

The catalog is `$XDG_DATA_HOME/dots/workspace.sqlite3`. Project files live in
`dots/workspaces/<project-id>/`:

- `briefs/` contains current agreements.
- `outputs/<slug>/sha256-<hex>/<filename>` contains immutable evidence.
- `scratch/` contains unretained drafts and probes.

Ordinary relative Markdown links resolve from their containing file. Publish
linked evidence first, then write links relative to the report's final output
directory before publishing it. Reports are single files, not self-contained
bundles. Links into scratch or checkouts remain unretained sources. Dots does not
copy link targets or automatically read another project's evidence.

Outputs survive task closure, worker shutdown and checkout removal. Nothing
expires automatically. Default memory recall excludes outputs and scratch.
Purging a project digest requires an explicit user request: Dots tombstones it
before removing matching managed output bytes, including staging and orphan
copies. Retry finishes interrupted removal. Sources, scratch, other projects,
backups and session transcripts remain outside that erasure guarantee.

Integrity inspection reports missing, corrupt, unreadable and pending-purge
payloads, unregistered files, staging remnants and SQLite sidecars. It never
adopts or deletes them. A catalog staging file may hard-link the live database;
do not open it as another catalog or blindly delete it.

## Offline backup and restore

Knowledge lives on the shared substrate remote, not in the workspace; the
[knowledge store](../src/memory/README.md) describes its format and limits. Back
up the remote repository itself. A new machine recovers knowledge with
`dots substrate init` against that remote; local project bindings and retained
unpublished candidates under `$XDG_DATA_HOME/dots/memory` stay machine-local.
A workspace backup alone does not contain knowledge.

For the machine-local catalog, briefs and outputs, stop all cooperating writers, including Pi parents/workers and Superfans bridges,
for the entire backup or restore. A file-by-file online copy is not a consistent
workspace backup. Retain the source until restoration is verified.

1. Inspect integrity for every registered project and resolve or explicitly record
   outstanding issues. Create a new backup directory outside the dots root.
2. Use SQLite's snapshot facility, for example
   `sqlite3 /absolute/dots/workspace.sqlite3 "VACUUM INTO '/absolute/backup/workspace.sqlite3';"`.
   The destination database must not exist. Copy the complete `workspaces/`
   directory beside that snapshot while writers remain stopped. Preserve native
   current files as well as retained outputs. Record project IDs and each task's
   `result_event` for comparison. Do not copy live SQLite sidecars as a replacement for
   the snapshot.
3. Restore into a separate empty data root first. Put the snapshot and
   `workspaces/` under its `dots/` directory; do not overlay a running catalog or
   reuse a live mutation lock. Point `XDG_DATA_HOME` at that isolated data root.
4. Run `dots project list`, inspect known tasks and historical result events, and
   run `dots output integrity --project ID` for every project. Compare IDs,
   associations, exact digests and current document bytes. Empty evidence is not
   proof that omitted historical reports were preserved.
5. Only after approval, with writers still stopped, select the verified restored
   root and restart current consumers. Never start old and new consumers against
   different stores as a dual-write arrangement.

Native current documents have per-file integrity, not transactional multi-file
history. Offline snapshots preserve their selected bytes. Backup erasure and
copies outside this root need separate handling.
