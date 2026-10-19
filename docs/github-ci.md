# GitHub CI

`dots github ci` inspects checks and retrieves exact-job logs or check-run output
using `gh` authentication, without a Herdr session.

## Use

Install the [dots CLI](../src/dots/README.md), authenticate `gh`, then:

```sh
dots github ci --json
dots github ci --pr https://github.com/example/project/pull/7
dots github ci --repo example/project --ref main
dots github ci --repo example/project --sha FULL_SHA
dots github ci --run https://github.com/example/project/actions/runs/20 --attempt 2
dots github ci logs https://github.com/example/project/actions/runs/20/job/30
dots github ci output https://github.com/example/project/runs/40
```

With no selector, inspection resolves the current branch's PR. Selectors are
mutually exclusive. A ref resolves to one remote commit, not the latest historical
run on that branch. Use `refs/heads/NAME` or `refs/tags/NAME` for ambiguous names.
A run target pins one attempt; returned evidence commands never switch to a rerun.

Follow the returned commands for pagination and exact evidence. Logs and check
output support bounded windows and literal search; Go logs also support
`--test NAME`. Use `--annotations` for source diagnostics stored by a check.
Run `dots github ci --help` and subcommand help for options.

Use `gh` for historical run discovery, dispatch, reruns, cancellation, watching,
artifacts, and general API access.

## Interpreting results

- Exit zero means the query completed, not that CI passed. No checks is not a
  passing result, and an unavailable requiredness result is not optional.
- Requiredness is evaluated only for PRs. No target establishes repository-wide
  health or merge eligibility.
- Evidence pins its repository, revision, and job/check identity. A job testing
  a merge commit may have a different revision from the PR head.
- Default excerpts and Go test attribution are navigation heuristics, not
  root-cause verdicts. Inspect surrounding lines when attribution is ambiguous.
- Missing or expired evidence is explicit. Truncation includes an exact native
  `gh` fallback; provider links remain available for external checks.
- Check output is the text stored in GitHub, not a query to the external provider.
  It can change even after completion.

Queries are read-only and noninteractive. Network work and output are bounded;
rate limits and API errors do not trigger automatic retries. Follow-up commands
are safer than reconstructing references or guessing which attempt failed.

## Saved observations

Returned `--snapshot` commands page the same observation without GitHub
requests. They retain the original observation time and expire after five
minutes. Follow Refresh or rerun inspection for current state. Missing,
expired, corrupt, or differently authenticated snapshots fail rather than
silently fetching new evidence.

Completed exact-job logs are cached for 24 hours. The private cache lives under
`$XDG_CACHE_HOME/dots/github` (default `~/.cache/dots/github`) and is scoped to
the active credential without storing tokens. Local credential/keychain access
is still needed for snapshots; it does not recheck current GitHub permissions.
`--no-cache` bypasses saving and reuse.

Fresh inspection never implicitly reuses saved status. Standalone log reads
refresh metadata while reusing cached bytes; snapshot navigation retains both
the saved metadata and bytes.
