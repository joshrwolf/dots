# Angle: correctness

Name: `correctness`

Does the change do what its intent says, in every case it can reach?

- Trace the changed code paths with real inputs, including empty, missing,
  duplicate, very large, and malformed values.
- Error paths: failures after partial progress, cleanup, swallowed or
  misreported errors, success returned for a violated invariant.
- State and concurrency: races between processes or tasks, ordering
  assumptions, stale reads, lost updates, resources that outlive their owner.
- Contracts: callers and consumers of changed functions, formats, files,
  commands and flags; behavior the change breaks for them.
- Security where input crosses a trust boundary: injection, path traversal,
  secrets in output or logs.
- Tests: a changed behavior with no test of its failure path is a finding only
  when you can show the untested case is wrong or likely to regress.
