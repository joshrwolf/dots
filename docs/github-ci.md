# GitHub CI workflow

`dots github ci` inspects PRs, remote refs, commits, and exact workflow run attempts.
It retrieves exact-job logs and GitHub check-run output.
It uses `gh` for GitHub authentication and queries; `gh` remains available for
general GitHub operations. No Herdr session is required.

```sh
make cli                 # build and Stow ~/.local/bin/dots
dots github ci           # current branch: checks and bounded failure excerpts
dots github ci --pr https://github.com/example/project/pull/7
dots github ci --repo example/project --ref main
dots github ci --repo example/project --sha FULL_SHA
dots github ci --run https://github.com/example/project/actions/runs/20 --attempt 2
dots github ci logs https://github.com/example/project/actions/runs/20/job/30
dots github ci output https://github.com/example/project/runs/40
dots github ci --no-logs --json
dots github ci --all --limit 20
dots github ci --name build --no-logs
dots github ci logs REF   # copy the exact command from inspection output
dots github ci logs REF --start 1 --lines 100
dots github ci logs REF --search '--- FAIL:' --lines 30
dots github ci logs REF --test TestExample/subtest --lines 30
dots github ci output REF # copy the check-output command from inspection
dots github ci output REF --search 'undefined'
dots github ci output REF --annotations
```

Targets are mutually exclusive. With no selector, inspection still resolves the
current branch's PR. `--ref` resolves a remote branch or tag once, then queries
that exact SHA; it does not select the latest historical run on the branch.
Ambiguous branch/tag names require `refs/heads/NAME` or `refs/tags/NAME`.
Annotated tags are peeled by immutable object ID. `--sha` accepts a full commit
ID. A run URL covers any trigger (push, schedule, manual dispatch, release,
merge queue); `--attempt` pins an older attempt. Otherwise the current attempt
is resolved once. Follow-up commands never switch to a newer attempt.

Numeric PR/run/job/check IDs work with `--repo [HOST/]OWNER/REPO`. URLs carry
their repository and must agree with `--repo` when both are supplied. Ref/SHA
commands may infer a repository only when the checkout has one unambiguous
GitHub repository identity. Full URLs work outside a checkout. Copied job/check
URLs with query strings or fragments are normalized before emitting commands.

A commit summary covers reported check runs and the latest legacy status per
context on that commit. Distinct returned check-run IDs are preserved even when
names match. A run summary covers jobs from one attempt, and separately reports
the run's own state/conclusion/event/revision. A failed or cancelled run with no
jobs remains explicit; zero job counts never imply success. Requiredness is only
evaluated in PR scope. No target claims repository-wide health or merge eligibility.

Use `gh run list` for historical run/workflow discovery, then pass a run URL to
`dots`. Dispatch, rerun, cancellation, watching, artifacts, and general API access
remain with `gh`.

The summary covers every check. Details default to failed, cancelled, unknown
and pending checks, ordered with failures first and limited to twenty. `--all`
includes passed/skipped checks but still respects `--limit` and `--offset`.
`--name TEXT` matches a case-insensitive literal substring across all states.
Filtering happens before pagination. Follow the returned command to continue;
it preserves the filter. When caching is available, the
continuation uses `--snapshot REF` to page the same observation without API
requests. Saved observations expire after five minutes and always display their
original observation time. Run the ordinary inspection command for fresh status.

`--directory PATH` selects a checkout. Explicit PR URLs work outside a checkout.
Queries are read-only, noninteractive, and never launch or prompt an agent.
Exit zero means the query completed, including failing/pending CI or no PR;
query and usage errors exit one. JSON output has a tagged
`subject` for PR, commit, or run scope. Evidence carries `observed_head`,
optional PR provenance, and exact run/job/check identity. Output and opaque
references have one current format; there is no compatibility layer for prior
development formats.

Inspection reports its scope, observation time, revision, local head when a configured remote matches the target repository,
check counts, states, and requiredness. Unavailable requiredness is never treated
as optional: it is unknown when status remains available, or an explicit query
error when GitHub omits the check. No checks is not a passing result. GitHub check runs also carry an exact check-output reference, including checks
backed by external providers. Legacy commit statuses without check-run IDs
retain their provider links.

Log references pin the observed revision, exact Actions job, and resolved attempt. A later
rerun or PR update cannot redirect the reference. Logs include the job's own
revision and attempt, steps, and line-numbered excerpts; PR and job revisions
can differ for workflows testing a merge commit. Missing/expired logs are
reported as unavailable, never as an explanation of the failure. Each command
has a 45-second network-query budget; individual processes have at most thirty
seconds. Metadata and stderr are capped at 2 MiB; raw job logs have a separate
512 MiB cap, streamed into a private disk spool rather than a whole-log memory buffer. Larger or unavailable logs always include an exact-job `gh` fallback. Log pages contain at most 500 lines, with a 16 KiB
text budget and a 2 KiB per-line limit. Shortening is explicit and includes a
`gh` command for the full log. Benign ANSI color styling is removed; other terminal controls are escaped in readable output;
JSON preserves them as escaped string data.


Default excerpts prefer structured failure markers over incidental error words in
embedded prose. For Go failures, diagnostics attributed to failing leaf tests are
preferred over parent summaries and setup messages. Explicit Actions errors and
date/time-prefixed ERROR records from the failed Run step outrank its generic
process-exit wrapper. At most three preceding lines keep the chosen diagnostic
inside the excerpt byte budget. These
are navigation heuristics, not root-cause verdicts.

`--search TEXT` performs a literal, case-sensitive search and returns a compact
index without an automatic context excerpt. It reports both total matches and
matches at/after `--start`, at most ten previews, visible preview shortening, and
commands for context or the next ten matches. Context defaults to twenty lines, including up to three preceding lines for
headers and setup. The leading margin also respects `--lines`, so even a one-line
window includes the match. Per-line bounds keep those preceding lines from
consuming the byte budget before the diagnostic. `--lines` controls the size of
those follow-up windows. Search JSON includes only unsuccessful steps.

Go failure matches also return a `--test NAME` command. This joins regions
attributed by RUN/NAME/CONT markers, prefers a diagnostic as the default start,
and preserves original line numbers in `line_numbers`, even across disjoint
regions. `--start 1` retrieves from the first attributed region; Continue resumes
at the first omitted matching line. Concurrent package output and unmarked logs
can make attribution ambiguous. Inspect surrounding raw lines when necessary.

All navigation uses the cached log bytes; standalone commands still refresh job
metadata and, when PR provenance exists, the current PR head unless using a saved observation. Missing matches or test regions return explicit empty
results. Excerpts stop at the 16 KiB budget and report individual line shortening
separately. Full log commands remain available for pathological single lines.

Automatic inspection targets two substantive failures, with separate hard caps
of ten evidence attempts and four content-retrieval attempts. External check
output uses the same budget; annotations are only fetched explicitly. Recognized aggregate steps
skip downloads and do not consume substantive slots or log-attempt slots. Log
markers can also identify aggregates, including `needs results: ... failure`,
without depending on job names. Classification is explicitly heuristic. The
45-second operation budget still bounds queries.


See [the general-purpose CLI home](../apps/dots-cli/README.md) for installation,
agent discovery, architecture, and adding workflows.

## GitHub check-run evidence

`output` reads the check run's title, summary and text stored in GitHub. This
covers external checks without integrating with their provider APIs. Each
reference pins the repository, check ID and observed revision; the result also
reports the check's own head and warns about revision differences. Actions
checks expose both their job logs and check-output commands. Automatic evidence
prefers Actions logs when available and check output for other failed checks.

Text uses the same bounded windows and literal search as job logs. Default
windows prefer recognizable compiler diagnostics and failure markers over
links/setup; this is a navigation heuristic, not a root-cause verdict. If no
marker is recognized, output begins at line one. `--start 1` always reads from
the beginning. After automatic retrieval, More reuses the saved observation;
Refresh explicitly fetches current output. Follow its
returned `--snapshot` commands for search/context/paging with zero GitHub calls.
Check output can change after completion, so there is no implicit long-lived
cache: fresh output reads query the check and, when PR provenance exists, the
current PR head. Automatic output reuses inspection's head observation. Explicit snapshots preserve the original
output for five minutes and fail with a refresh command when unavailable.

`--annotations` fetches fresh source diagnostics, ten per page. Messages are
bounded to 2 KiB each with explicit shortening; returned commands page forward
or retrieve the exact raw annotation page through `gh`. Annotations do not share
the text snapshot. Missing text, inaccessible annotations and provider links
remain explicit; absence of evidence is never a root-cause explanation.

## Request and cache policy

PR inspection traverses the check connection once, fetching requiredness in
that same query. Explicit URLs need one HTTP request per hundred raw check
contexts; branch/number resolution adds one lookup. The HTTP connection is
reused across pages. The observed PR head and commit are checked on every page;
an incomplete traversal or changing head fails instead of returning mixed data.
Reruns are grouped like `gh pr checks` by check name, workflow, and event, keeping
the latest started run. Legacy status contexts keep the newest creation time.

Commit inspection uses a small GraphQL projection over the pinned SHA, including
check runs and legacy statuses in one traversal. Ref resolution adds branch/tag
lookups; exact SHAs skip them. Run inspection fetches run metadata once, then
pages jobs for its pinned attempt. Inline excerpts reuse those job records,
avoiding another job-metadata request. Direct job/check commands need no PR
lookup and reuse the metadata fetched during target resolution.

Automatic PR excerpts reuse that PR observation. Standalone log commands without `--snapshot` fetch
current job metadata and, when PR provenance exists, the current PR head before
reading cached logs. Completed exact-job
logs are cached for 24 hours, scoped to host, repository, job, attempt, and a
fingerprint of the active credential. Tokens are never stored. Each cache directory retains
at most 64 entries and 1 GiB, with private file permissions and integrity
checks. Missing credentials, corrupt files, and unwritable cache paths are
cache misses; errors and incomplete jobs are never cached.

The CLI uses `$XDG_CACHE_HOME/dots/github-ci/logs`, or
`$HOME/.cache/dots/github-ci/logs`. `--no-cache` bypasses reads, writes, and
credential lookup. Log output reports its source and original fetch time.
Fresh inspection never reads saved status implicitly. `--snapshot` explicitly
reads the original saved observation. Inspection snapshots disable automatic
excerpts; job/output snapshots navigate saved evidence without fresh metadata. Expired, missing, or differently
authenticated snapshots fail with a refresh command instead of silently fetching
new data. Diagnostics distinguish missing credentials, inaccessible/private
storage, absent entries, expiration, and integrity failures where detectable.
Missing entries cannot distinguish eviction from a different credential/cache
location. Credential failures suggest checking authentication and sandbox/keychain
access without exposing credential output. Snapshot reads use the active credential fingerprint but do not query
GitHub to recheck its current permissions. Snapshots live alongside logs in `dots/github-ci/snapshots`, with the
same size/count and permission bounds but a five-minute lifetime. When saving is
unavailable, the continuation falls back to explicit fresh `--pr` inspection.
`--offset` without `--snapshot` still queries fresh status. Rate limits and other API failures do not
trigger automatic retries. Raw-log compatibility fallback only retries a flag
rejected locally by older `gh`, avoiding a second network download.

### Saved job evidence

Returned job-log navigation commands include `--snapshot REF` when both job
metadata and log bytes were saved. Search, context, test attribution, and paging
then make no GitHub requests and retain the original observation time. Local
credential access still selects the correct private cache. Observations expire
after five minutes; log bytes remain cached for 24 hours. Omit `--snapshot` or
follow Refresh to fetch current job/PR metadata. Missing, expired, or evicted
evidence fails with a refresh command instead of silently querying GitHub.

Automatic job excerpts prefer diagnostics within failed-step timestamps.
Timestamp boundaries have GitHub's precision, so selection remains heuristic.
Without a recognized diagnostic, the CLI labels its failed-step or job-tail
fallback explicitly. Explicit windows and searches still cover the whole log.
