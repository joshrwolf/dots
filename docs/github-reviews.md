# Review threads

Use `dots github reviews` to discover unresolved inline/file review threads and
read exact conversations. General PR discussion and review summaries remain
available through `gh pr view`; posting, replying, resolving, and merging are
outside this command's scope. Unresolved does not necessarily mean a code change
is needed. Outdated does not mean resolved. An empty thread index does not rule
out review-level feedback: use `gh pr view PR --json reviews,comments` for that.

```sh
dots github reviews --json
dots github reviews --pr https://github.com/example/project/pull/7 --json
dots github reviews --repo example/project --pr 7 --all --json
dots github reviews thread THREAD_REF --json
```

The list defaults to ten unresolved threads, including outdated ones. `--all`
includes resolved threads. Summary counts cover the complete traversal; `matching`,
`offset`, and `returned` describe the displayed page. `complete` means this page
contains the entire matching collection, not that pagination is finished.
`has_more` is true exactly when `next_command` provides another page; a final
page can have `complete: false` and `has_more: false`. A preview includes
only the opening comment, with `comments_total` and `conversation_complete` making
that distinction explicit. Follow `next_command` to read the conversation. JSON
follow-ups preserve JSON mode.

A thread reference includes repository, PR, observed revision, and exact thread
identity. Reading it does not select another thread after a PR update. The result
reports the current observed head and warns if it changed. Original and current
line locations remain separate and nullable; the CLI does not guess where
outdated feedback belongs in today's checkout. Diff context records the original
reviewed revision as `original_commit_id`. `commit_id` is GitHub's current
comment association, which can point at a later commit; it is not evidence that
the later revision was originally reviewed. Deleted authors are represented as null.

Conversation reads fetch replies separately from the index, in API order, then
return at most five comments by default (`--limit` accepts 1–20). Opening previews
are bounded to 512 bytes; full comment bodies and the reviewed diff hunk are
bounded to 4 KiB each, preserving grapheme boundaries. Every shortened text has
`truncated: true`. Comment URLs and exact native `gh api` commands provide access
to full bodies and diff hunks when needed. `full_diff_hunk_command` restores the
untruncated review hunk, not surrounding source; use the checkout or `gh` for
broader implementation context. Diff context appears only at offset zero. Later
pages provide `context_command` to recover the opening page from the same saved
observation, avoiding repeated context in reply-only pages.

Returned continuation commands use `--snapshot` when saving succeeds. Saved
index and conversation pages make no GitHub requests and retain their observation
time. Local credential/keychain access is still required to identify the private
cache. They expire after five minutes; use `refresh_command` for current state.
Refresh preserves `--all` and page size while resetting the offset to zero.
Missing, expired, corrupt, or differently authenticated observations fail without
silently querying GitHub. `--no-cache` disables saving and reuse. The private
credential-scoped cache is shared with CI under `$XDG_CACHE_HOME/dots/github`
(or `~/.cache/dots/github`).

Fresh index queries fetch up to 50 threads and one opening comment per page.
Exact conversation queries fetch up to 25 comments per page. Limits of twenty
index requests and eight conversation requests bound each traversal; the shared
45-second operation deadline and 2 MiB response limit also apply. GraphQL errors,
missing nodes, duplicate IDs, changing totals/revisions, and incomplete pagination
fail explicitly rather than reporting a partial result as complete. These
observations are not an atomic snapshot of GitHub while a multi-page query runs.
