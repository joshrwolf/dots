---
name: pr-review
description: Review, understand, or discuss the PR prepared by Herdr-review in the agent's checkout. Loads the captured comparison and local conversation, saves findings and replies in their threads, and follows the user's requested depth and focus. Editing or posting requires an explicit request.
---

# PR review

Help me review the PR opened through Herdr-review. Herdr prepares the checkout
and opens an ordinary shell; neither Neovim nor a particular tab layout is
required. This skill works the same in Claude and Codex.

## Start from the actual checkout

Read [references/review-context.md](references/review-context.md) and load the
review context using the agent's working directory. Do not use the UI-focused
workspace, infer the PR from a generated branch name, or query SQLite directly.
The saved PR identity and captured comparison are authoritative for this review.
If no unambiguous PR binding exists, explain what is missing and ask me to open
the intended PR through Herdr-review; do not create a new checkout or guess.

Check the captured base/head against the checkout and report local edits or a
different HEAD before treating working-tree contents as the reviewed snapshot.
Read the local captured comparison; do not silently fetch, switch, reset, or
update it. If GitHub has a newer head, distinguish that from what is checked out.
Use the explicit bound PR URL for description, discussion, or CI queries, and
fetch only what the requested activity needs. Never resolve it from the branch.

Existing local findings are part of the context. Use their IDs to continue or
revise them rather than recreating the same observations on every invocation.
They are not a transcript of every earlier agent conversation; do not claim to
remember discussion that is not available.

## Let my prompt choose the activity

- **Review**: investigate the requested scope, verify concerns, save local
  findings, then summarize them and their placement.
- **Orient or explain**: explain the change or answer the question. Do not make
  me wait for an unrelated full review or automatically create findings.
- **Discuss or continue**: work through the existing threads and my new messages.
  For a delivered agent request, save answers with `thread.reply` using its
  request, dispatch attempt, and thread IDs. Do not replace a finding to answer
  a question.

`<leader>rS` sends new reviewer messages, not an instruction to implement
every open finding. Earlier messages are context. Answer questions as questions;
edit code only when the new messages explicitly ask for changes. New messages
typed during your turn belong to the next exchange. Request/message IDs are
stable across retries; dispatch attempt IDs change. Inspect existing replies
before repeating any action, answer only missing threads, and use the current
attempt ID when saving. A request being delivered or an agent turn finishing
does not prove that answers were saved.

Depth and focus are conversational, not rigid modes. A quick pass should be
short; a deep review should explain the relevant design and investigate its
risks. If I simply ask for a review, use a medium-depth pass and briefly state
the scope. Delegation is optional when available and authorized; do not require
a particular agent, model, or an independent review before answering a question.

## Review judgment

Read surrounding code and callers, not just the diff. Consider correctness,
design fit, test adequacy, and security where relevant. CI is evidence, not proof
that a suspected bug is impossible. Prefer targeted, non-destructive checks
that resolve a concrete uncertainty over rerunning an expensive suite. Do not
edit the branch or tracked test files as part of this review.

Before presenting a finding, try to refute it: check the intended behavior,
affected callers, actual consequence, and supporting location. Distinguish a
confirmed defect from a question or design preference. Never claim a repro or
test result that was not observed. Avoid nits already handled by tooling.

## Findings have explicit placement

For each actionable finding, make clear:

- **Inline**: repository-relative path, comparison side, and line/range where
  the comment belongs. Anchor to the captured snapshot, not a guessed current
  line. Choose the smallest location that explains the concern.
- **General**: a review-level concern with no honest single comment location.
  Include supporting code references without inventing an inline anchor.
- **Related locations**: evidence, callers, or suggested fix sites, clearly
  separate from the primary comment location.

State the consequence, evidence and uncertainty, and suggested change when
known. A fix site may differ from the comment site; say so. For example:

> **Inline · `internal/release.go:142` · target side**
> Issue cleanup can repeat a successful release. Release success is recorded
> only after cleanup; a cleanup error makes the next wake publish again.
> Suggested change: record success independently of retryable cleanup.
> Related location: the caller's retry loop in `internal/worker.go:88`.

Save verified review findings through the supported interface in the context
reference. A saved finding starts a local thread, not a comment in my voice or
a GitHub draft. My replies can continue that thread through the same tool. Tell me what
was saved and where; if saving fails, retain the findings in chat and disclose
the failure rather than claiming they are visible in Neovim.

Neovim is an optional view of the same state: `<leader>fr` finds inline and
general findings; inline annotations and `[r` / `]r` navigate anchored threads.
Do not open or control a Neovim instance just to save findings.

## GitHub posting is a separate, explicit request

Do not draft comments in my voice without explicit consent for that batch.
When requested, draft terse, lowercase, first-person observations or questions,
not directives. Read [references/gh-comments.md](references/gh-comments.md) only
when preparing GitHub comments. Preserve the distinction between
general and inline placement and verify that inline locations are commentable
against the intended PR head.

Do not post or submit a verdict automatically. If I explicitly ask you to post
on my behalf, follow that request and its scope; otherwise everything stays
local. No code changes or GitHub actions are implied by reviewing or discussing.
