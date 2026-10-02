---
name: ship-it
description: Ship changes when the user says "ship it". Also use before creating or updating any pull request on the user's behalf, even without that phrase. Covers fork routing, signing, commit history, and PR writing.
---

# Ship it

"Ship it" is consent to commit the relevant work, push it, and create or update
the PR. Do not ask for that consent again. When invoked through other requests,
apply these rules within the authorization already given; discovering the skill
does not itself authorize publication.

Default new PRs to ready for review. Create a draft only when the user asks;
preserve an existing PR's draft status unless asked to change it. Stop after
publishing the PR and reporting its link. Do not monitor CI or merge unless the
user explicitly asks. Do not post additional comments or messages without an
explicit request.

## Route the push and PR explicitly

The user normally works in forks: `origin` is their fork and `upstream` is the
target repository. Inspect the remotes, including push URLs, and confirm the
repository identities before publishing. In a fork workflow, **always push to
`origin` and always open the PR against `upstream`**. Never create remote branches
or push commits directly to upstream, even if credentials allow it.

A missing `upstream` remote does not prove the repository is not a fork. Resolve
the fork's parent from repository metadata when necessary. If routing is still
ambiguous, ask instead of guessing. Only when the repository is confirmed not
to be a fork may you push to its primary remote and target that repository.

Use explicit destinations for every push and PR operation. Select the intended
base branch from the task or repository conventions; use the target repository's
default branch when no other base is intended. When using `gh`, specify the
target repository, base branch, and source owner/branch explicitly rather than
relying on its remote selection or automatic push behavior. Check for an
existing PR from that source branch in the target repository before creating
one. Update that PR rather than opening a duplicate.

## Prepare commits and preserve signing

Inspect the working tree and the full change against the PR base. Commit work
belonging to the task; leave unrelated changes out. Perform checks appropriate
to the change and follow repository requirements, reusing relevant results
already obtained. Local checks do not belong in the PR description by default.

Prefer fewer, coherent commits. Small corrections such as lint fixes can be
amended into the relevant commit. Preserve meaningful commits for substantive
review responses or distinct changes so reviewers can follow the history.
Use judgment rather than always squashing or always adding a commit. When
rewriting the task branch is appropriate, use `--force-with-lease` against the
verified push destination, never an unconditional force push. Reconcile an
unexpected remote update before overwriting anything.

Most repositories use gitsign. Honor the configured signing mechanism for all
commit-producing operations, including amendments and rebases. Let gitsign open
its browser OAuth flow and tell the user when their interaction is needed.
Never complete authentication on the user's behalf, substitute another signing
method, disable signing, or otherwise bypass the flow. Repositories without
gitsign should retain their existing signing configuration.

A tool yielding while the commit process remains alive is not a signing timeout;
continue waiting on that same process. If the commit or signing flow times out,
**stop the shipping workflow, notify the user, and wait for an explicit request
to retry**. Do not retry automatically or continue to push or publish. When the
user asks to retry, inspect the process and Git state first so a late completion
does not produce a duplicate commit.

## Write for the reviewer

Use the conversation to identify the Linear issue or issues. Consult available
issue context when needed to establish scope. If the association is unclear,
ask which issues apply or whether there is no associated issue; do not invent a
link or silently omit an unclear association. Infer `Closes` for issues this
change completes and `Refs` for partial or related work. Include the appropriate
Linear links with those keywords, treating each issue separately.

Write a concise title and description around the final scope. Explain what the
PR accomplishes, why it is needed, and any supplemental context the reviewer
needs to assess it. Prefer short prose; include a concrete behavior example or
material limitation when it helps. Do not recount the conversation, list files
or code changes as a substitute for explaining the purpose, or enumerate local
tests and validation. Include a testing section only if the user explicitly
asks, even when a repository PR template requests one.

For an existing PR, review the entire current diff and update its title and
description to reflect the current scope. Replace stale explanations instead
of appending a history of the conversation or abandoned approaches.

Prefix AI-generated PR descriptions and any explicitly authorized comments or
replies with `:robot:`. When using `gh` for multiline descriptions, write the
exact Markdown to a temporary file and pass it with `--body-file`.

After publication, verify the PR's target repository, base, source branch, and
published description. Return the PR link with a brief statement of what was
created or updated.

## Register every shipped PR in Herdr

When the invoking environment supplies Herdr workspace/pane context, discover
`dots herdr prs register --help` and register the verified full PR URL after
creation or update: `dots herdr prs register <url> --workspace <verified-id> --json`.
Verify the workspace against the task and `herdr api snapshot`. Never copy inherited
Herdr IDs or use UI focus or checkout similarity as ownership evidence: shared
agent daemons can carry another pane's context. If ambiguous, ask for the target;
report publication as successful and registration as pending. Do this for every PR,
including an already-open PR and each PR in a sequence. Registration is local,
idempotent, and does not replace earlier workspace associations.

Outside Herdr, skip registration. Within Herdr, report a registration failure
separately from successful publication and retry only the local registration;
never recreate a PR to repair the association. The background plugin watches
status. Stop after publication and registration; do not monitor CI or merge.
