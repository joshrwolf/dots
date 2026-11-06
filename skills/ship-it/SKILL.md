---
name: ship-it
description: Commit, push and create or update PRs when authorized, including when the user says "ship it". "Ship it and rip it" adds CI/review iteration until ready for human review.
---

# Ship it

Publish the relevant work to the agreed finish line. Read the actual user
request and repository instructions; loading this skill grants no authority.
For goal work, use `goals` for the mandate, task state, attention and handoff.
Do not ask again for actions already authorized. Small shipments need no goal.

Load `shipping-context` when available for private environment and company
guidance. Use `dots` for supported evidence, review and PR watch workflows;
installed help owns command syntax. Repository policy may call for direct
publication without a PR—do not manufacture one.

## Finish line

- **Ship it:** authorizes relevant commits, push and PR creation/update. Publish,
  enroll any PR in the dots watch list, report its link and stop.
- **Ship it and rip it:** additionally authorizes relevant fixes, CI retries,
  automated-review enrollment, factual feedback replies and resolution of
  verified-fixed threads where repository policy permits. Continue through
  [monitored shipping](references/monitored-flow.md) until ready for human review.

A goal mandate or explicit request can select either endpoint without a trigger
phrase. Watching CI alone does not authorize fixes or replies. Neither mode
permits remote PR merge, auto-merge, deployment or policy changes. Do not ping
people or request reviewers without separate authorization.

## Route publication

Confirm repository identities from remotes, push URLs and repository metadata.
In a fork workflow, push to the user's fork and target its parent; conventional
names are `origin` and `upstream`. Never push to upstream merely because access
allows it. A missing remote does not establish repository identity; ask only
when the destination remains ambiguous.

Use explicit push and PR destinations. Resolve the base from the request or
repository conventions, otherwise use the target's default branch. With `gh`,
specify target repository, base and source owner/branch instead of relying on
automatic selection or push behavior.

Update an existing PR from that source branch rather than duplicating it.
Follow-up work sharing a write set with an open PR extends that PR; separate
PRs must be independently mergeable. Default new PRs to ready for review unless
asked for a draft, and preserve an existing PR's draft status.

## Commit and verify

Inspect the working tree and full change against the intended base. Commit only
relevant work, preserve unrelated changes and run appropriate repository checks,
reusing applicable results. Prefer coherent commits: amend small corrections
when appropriate and retain substantive review responses or distinct changes.
Use `--force-with-lease` for authorized rewrites against a verified destination;
reconcile unexpected remote updates first.

Honor configured signing for every commit-producing operation, including
amendments and rebases. Do not enable signing elsewhere, disable it, substitute
another method or change identity to evade a failure. Missing access needs the
user or environment owner, not improvised login or credential transfer.

A yielded tool with a live commit process is not a timeout; wait on that process.
If committing or signing times out, abort any in-progress rebase, stop
publication and wait for an explicit retry instruction. Before retrying, inspect
process and Git state for a late completion.

Gate pushes on successful commit creation and required signature verification.
Scripts fail closed: use `set -e`, explicit non-zero verification failures and
success-dependent chaining. Before creating a PR or claiming a published fix,
verify push success and the intended remote head. A failed commit must never
lead to a reply or thread resolution naming an old SHA as the fix.

## Describe and publish

Use actual issue context and repository conventions for links and closing
keywords. Explain the final change's purpose, why it matters and material
limitations—not file inventories, conversation history or local-test lists.
Include a testing section only when explicitly requested. For an existing PR,
review the whole diff and replace stale title/description text while preserving
required policy trailers.

Prefix AI-generated descriptions and authorized comments/replies with `:robot:`.
For multiline `gh` descriptions, write exact Markdown to a temporary file and
pass `--body-file`. Verify the published target, base, source branch, head and
description.

Enroll created or updated PRs through the dots watch workflow. If enrollment
fails, report that separately from publication and retry only enrollment, never
PR creation. Watch enrollment does not authorize monitoring.

## Stop or hand off

For access, decisions, human-only dispositions or exhausted recovery, use goal
attention when this work has a goal; otherwise ask directly in chat. Follow
`goals` rather than duplicating its attention lifecycle here.

Report the published link and material caveats at the agreed endpoint.
Distinguish published from CI/review-ready; monitored work continues until its
readiness check passes or a concrete blocker requires the user.
