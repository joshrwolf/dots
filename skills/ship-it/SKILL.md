---
name: ship-it
description: Publish authorized changes through commits, pushes and pull requests. Use when the user says "ship it" and before creating or updating a PR. "Ship it and rip it" includes opt-in CI/review iteration until ready for human review. Compose with goals, dots and optional private shipping-context guidance.
---

# Ship it

Carry authorized work to its agreed publication finish line. "Ship it" authorizes
committing the relevant work, pushing it and creating or updating its PR.
"Ship it and rip it" also authorizes the monitored flow below. Neither authorizes
merging a remote PR, enabling auto-merge, deployment or changing policy.
Loading a skill does not authorize publication.

## Establish scope and context

Read repository instructions and the actual user request. If this work belongs
to a goal, use `goals` to read its recorded mandate, finish line and outstanding
attention before acting. Do not ask again for actions already authorized, and
do not expand authority during a skill handoff. Small shipments need no goal.
Repository policy may call for direct publication without a PR; follow that
policy rather than manufacturing one.

If the `shipping-context` skill is available, load it for private environment,
identity and company guidance. Otherwise use repository instructions and actual
Git/authentication configuration. Do not infer ambient credentials from Linux
alone or interactive authentication from macOS alone. Unclear signing identity
or push routing is a blocker, not permission to choose a new configuration.

Use `dots` and installed command help for supported evidence, review threads,
watch lists and goal attention. Native tools remain available for unsupported
operations. Keep task results current when coordinating a goal; publication is
not proof that the goal's verification or monitored finish line is complete.

## Choose the finish line

- **Ship it:** publish, add any PR to the watch list, report the published link,
  and stop. Do not monitor CI or post additional comments.
- **Ship it and rip it:** publish and continue until the current revision is
  ready for the user's review. This authorizes relevant fixes, commits, pushes,
  appropriate CI retries, automated-review enrollment, factual replies and
  resolution of verified-fixed threads where repository policy permits.
  Read [the monitored flow](references/monitored-flow.md).

An existing goal mandate or explicit request can select either endpoint without
a trigger phrase. A request merely to watch CI does not authorize fixes or
replies. Default new PRs to ready for review; create a draft only when asked and
preserve an existing PR's draft status unless asked to change it.

## Route publication explicitly

Inspect remotes, including push URLs, and confirm repository identities. In a
fork workflow, push to the user's fork and target the parent repository's PR
base. With conventional remotes this means push to `origin`, PR to `upstream`;
never push to upstream merely because credentials permit it. A missing
`upstream` remote does not prove a repository is not a fork: consult repository
metadata. Ask if routing remains ambiguous.

Use explicit destinations for every push and PR operation. Resolve the intended
base from the task/repository conventions, otherwise use the target's default
branch. With `gh`, specify target repository, base and source owner/branch;
do not rely on automatic remote selection or push behavior. Check for an
existing PR from that source branch and update it instead of duplicating it.
When follow-up work shares a write set with an open PR, extend that PR rather
than creating a dependent stack. Separate PRs must be independently mergeable.

## Prepare and verify commits

Inspect the working tree and full change against the intended base. Commit only
relevant work, preserve unrelated changes, and run appropriate repository checks,
reusing applicable results. Prefer coherent commits: amend small corrections
when appropriate, preserve substantive review responses and distinct changes.
For authorized history rewrites, use `--force-with-lease` against the verified
destination and reconcile unexpected remote updates first.

Honor the repository's configured signing mechanism for every commit-producing
operation, including amendments and rebases. Do not enable signing elsewhere,
disable it, substitute another method or change identity to evade a failure.
Use the environment's configured authentication path; missing access needs the
user or environment owner, not an improvised login or credential transfer.

A tool yielding while a process remains alive is not a signing timeout; wait on
that same process. If signing or committing times out, abort any in-progress
rebase, stop publication and request attention. Wait for an explicit retry
instruction, then inspect process and Git state before retrying to avoid a late
completion producing duplicate work.

Gate every push on successful commit creation and required signature
verification. Scripts must fail closed: use `set -e`, explicit non-zero exits
for failed verification and success-dependent chaining. A verification loop
that merely prints failure is not a gate. Before PR creation, replies or thread
resolution, verify push success and that the intended remote head matches.
Never name an old SHA as the fix after a failed commit or push.

## Write for the reviewer

Use actual issue context and repository conventions for issue links and closing
keywords; do not invent associations. Write a concise title and description
explaining what the final change accomplishes, why and any material limitation.
Prefer short prose over file inventories, conversation history or local-test
lists. Include a testing section only when explicitly requested. Preserve
required policy trailers. For an existing PR, inspect the whole current diff
and replace stale title/description text rather than appending a change log.

Prefix AI-generated descriptions and authorized comments/replies with `:robot:`.
For multiline `gh` descriptions, write exact Markdown to a temporary file and
pass `--body-file`. Verify the published target, base, source branch, head and
description after creation or update.

Add every created or updated PR to the watch list from its working checkout:
`dots github watch add <full-pr-url> --json`. Adding is local and idempotent;
report failure separately from successful publication and retry only the add,
never recreate the PR. Watch enrollment is not monitoring authorization.

## Blockers and handoff

Use `goals` and durable attention for authentication, decisions, human-only
dispositions or exhausted recovery under an active goal. Block only affected
work, use a stable question key, and ask the same self-contained question in
chat. Keep credentials out of records. Preserve actual human answers separately
from interpretation and reconcile them before resuming; a saved confirmation
permits checking the result, not claiming authentication succeeded. Without a
goal, ask directly; do not create a goal just to request login.

At the authorized endpoint, report the published link and material caveats in
chat. Distinguish published from CI/review-ready. In monitored mode, continue
through the readiness check rather than handing routine iteration to the user.
Do not imply unattended work continues without a running coordinator.
