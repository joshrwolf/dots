# Filesystem documents

Shared bounded text, path validation, atomic file publication and cooperative
mutation locks for task briefs, output evidence and machine-local documents.

Reads create nothing. Writes compare the expected revision under a short
kernel-held lock, then replace one staged file atomically. A missing file and an
empty file have distinct revisions. Multi-file writes are not transactions.
Locks coordinate cooperating writers; path checks are not a hostile-process
sandbox. Never remove the stable lock file while another process may use it.
