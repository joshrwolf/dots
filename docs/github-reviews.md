# GitHub review threads

`dots github reviews` reads inline/file review discussions without posting,
replying, resolving, or merging.

## Use

```sh
dots github reviews --json
dots github reviews --pr https://github.com/example/project/pull/7 --json
dots github reviews --repo example/project --pr 7 --all --json
dots github reviews thread THREAD_REF --json
```

The index defaults to unresolved threads, including outdated ones. `--all`
includes resolved threads. Follow the returned commands to read complete
conversations and additional pages; index previews contain only opening comments.
Use `--help` for filtering and page limits.

General discussion and review summaries are separate:

```sh
gh pr view PR --json reviews,comments
```

## Constraints

Unresolved does not imply a code change is needed; outdated does not mean resolved.
An empty index does not rule out review-level feedback.

References pin exact threads and the observed revision. Reads warn when the PR
head changes rather than selecting another thread. Original and current locations
remain distinct; the CLI does not guess where outdated feedback belongs in the
current checkout. `original_commit_id`, not the later `commit_id` association,
identifies the originally reviewed revision.

Bodies and diff hunks are bounded with explicit truncation. Returned native
`gh api` commands retrieve full text. A full diff hunk is still not surrounding
source; inspect the checkout when needed.

Counts cover the full traversal, while `complete` describes whether the displayed
page contains the entire matching collection. Use `has_more` and `next_command`
to continue, not `complete` alone.

Saved pages share the [CI snapshot policy](github-ci.md#saved-observations):
five-minute lifetime, private credential-scoped storage, and explicit refresh.
Multi-page GitHub queries are not atomic snapshots; detected revision changes,
incomplete traversal, and API failures produce errors rather than false completeness.
