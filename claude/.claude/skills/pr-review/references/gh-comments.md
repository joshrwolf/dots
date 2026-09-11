# gh comment recipes

Templates for an explicitly requested GitHub comment batch. By default, emit
them for me to run. Execute them only when I explicitly ask you to post on my
behalf, within that request's scope. Fill the bodies with the drafted comments
(my voice: terse, lowercase, first-person). Leave the `event`/verdict for me to
choose; permission to post comments is not permission to approve or request
changes.

Use the repository, PR number, URL, and captured target SHA returned by the
review context. Never derive them from the shell's branch. Before drafting an
inline command, compare GitHub's current head with that captured SHA; if it has
changed, stop and discuss which snapshot the feedback should address. A local
finding outside the PR diff cannot be posted as an inline review comment.

Fill these values from the context (the names below are examples):

```sh
PR_URL=https://github.com/example/project/pull/42
REPO=example/project
PR=42
SHA='<captured-target-sha>'
```

## Top-level review (summary + verdict)

Write the body to a file so it survives newlines and review:

```sh
gh pr review "$PR_URL" --comment --body-file /tmp/pr-$PR-review.md # plain comment
# or --approve / --request-changes — my call, not yours
```

## Inline comments (line-anchored)

`gh pr review` can't place inline comments; use the API. One comment:

```sh
gh api "repos/$REPO/pulls/$PR/comments" \
  -f body='this leaks the conn if Decode errors — defer the close above?' \
  -f commit_id="$SHA" \
  -f path='internal/server/handler.go' \
  -F line=142 \
  -f side=RIGHT
```

- `line` is the line number in the file's new version; `side=RIGHT` is the new
  side (use `LEFT` to comment on a removed line).
- For a multi-line range, add `-F start_line=<n> -f start_side=RIGHT`.

## One review carrying many inline comments + a summary

Batches all inline comments into a single submitted review. Prepare a JSON body
for the user to inspect; do not rely on repeated array flags to group fields:

```json
{
  "commit_id": "<captured-target-sha>",
  "event": "COMMENT",
  "body": "overall: solid. a few things below.",
  "comments": [
    {
      "path": "internal/server/handler.go",
      "line": 142,
      "side": "RIGHT",
      "body": "this leaks the conn if Decode errors — defer the close above?"
    },
    {
      "path": "internal/server/router.go",
      "line": 88,
      "side": "RIGHT",
      "body": "this map read races with the goroutine on line 60."
    }
  ]
}
```

```sh
gh api --method POST "repos/$REPO/pulls/$PR/reviews" \
  --input /tmp/pr-$PR-review.json
```

`event`: `COMMENT` (no verdict), `APPROVE`, or `REQUEST_CHANGES` — my choice.

Contract references: [GitHub review API](https://docs.github.com/en/rest/pulls/reviews#create-a-review-for-a-pull-request)
and [gh pr review](https://cli.github.com/manual/gh_pr_review).
