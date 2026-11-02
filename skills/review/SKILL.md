---
name: review
description: Review a change with independent, fresh-context reviewers and a falsifying verifier, recording findings as threads with `dots review`. Use to check your own work before handing it over, to review a changeset before pushing, or to review a pull request (including from a PR URL). Arguments are an optional profile name and a target.
---

# Review

Reviews produce few, verified findings anchored to exact content. Precision
beats recall: a wrong finding costs the user more than a missed nit, and model
judges asked "is this valid?" accept almost everything, so every finding must
survive an attempt to disprove it with evidence.

`dots review --help` and each subcommand's help own the CLI contract (targets,
thread fields, statuses, refresh). Read them before first use in a session.

## Profiles

| Profile | Use | Reviewers | Verifier |
|---|---|---|---|
| `quick` | An agent checking its own work before reporting | one: correctness and guidance angles together | none; the reviewer falsifies its own findings |
| `standard` (default) | A finished changeset, a colleague's PR | correctness from two model families, guidance from one | yes |

Run each reviewer and the verifier as a fresh-context subagent (in Pi,
`worker_spawn` with tools `read` and `bash`). Model family means provider:
Anthropic and OpenAI. Pick each model from the host's catalog for the role: the
strongest model priced for routine workers, at high effort for correctness and
verification, lower effort for guidance. Do not use the premium tier unless the
user asks. `quick` uses one reviewer from the family other than your own when
one is available, with both angle files.

## Run a review

1. **Target and intent.** Each worktree has one living review: its branch,
   through the working tree, against the base `dots review open` chooses and
   reports. That is the target for your own work and for a branch or PR checked
   out in a worktree. For a PR URL, read `gh pr view <url> --json
   title,body,baseRefName,headRefOid`, fetch the base, confirm `HEAD` is the PR
   head, and pass `--base <remote>/<baseRefName>` when the chosen base differs.
   Use `dots review start --uncommitted` or `--base REF` only for an ad hoc
   review beside the living one. Intent is the PR description, the task, or
   the user's request; write it down in a sentence or two. If the target is
   ambiguous, ask.
2. **Open.** `dots --json review open`, from your own session rather than a
   worker. Check the reported base, and keep the review id for the commands
   below.
3. **Reviewers.** For each reviewer in the profile, fill in
   [reviewer.md](reviewer.md) and give it one angle file from `angles/`
   ([correctness](angles/correctness.md), [guidance](angles/guidance.md)).
   Run them in parallel. Reviewers never see your session, your reasoning, or
   your opinion of the change. They record their findings as threads
   themselves.
4. **Verify.** When all reviewers finish, run one verifier with
   [verifier.md](verifier.md) over the open threads. It marks each
   `accepted` or `dismissed` with its evidence as the body. Merge duplicates
   by dismissing the weaker copy with a body naming the kept one.
5. **Report.** `dots review show <id>`. Give the user accepted findings, most
   severe first, each with `path:line`, the failing scenario, and the fix;
   then one line on what was dismissed and why. Say plainly when nothing
   blocks. Do not post to GitHub.
6. **Hand over.** In Rex, open the review beside your pane:
   `rex split -s "$REX_SESSION" -b "$REX_BLOCK" --cwd <worktree> --label
   review -- nvim +Review`. Without those variables, find your pane as the
   rex skill describes.

## After fixes

When the change moves (fixes, new commits), run `dots review open` again. It
follows the worktree, re-anchors threads, and returns a delta `diff_command`
(`dots review refresh <id>` does the same for an ad hoc review). Continue the same
reviewer sessions when the host allows it (in Pi, `message` to the finished
worker): give them the delta, ask them to confirm which of their findings are
fixed and to review only the delta. Mark verified fixes `fixed` with a body
giving the evidence.
Re-review until a round adds nothing new, or the user stops it.

`dots review remove <id>` deletes an ad hoc review once the user is done with
it; reviews also disappear with their worktree.

## Submission notices

The agent session that last opened the worktree's review (or started an ad
hoc one) owns it; open it yourself, or pass `--owner`, rather than from a
worker. The user's comments stay private drafts until the user submits them. In
Pi, each submission reaches the owner as one notice with its summary and each
comment's thread and location. Read the threads with `dots review show`, then
decide: answer them yourself, fix the change, or resume the worker that owns
that change or finding with `message` and give it the threads. Reply on each
thread with the outcome (`dots review reply`, or `dots review set` with evidence
on threads agents started), so the user sees it in the review. Only the user
settles a thread the user started; the CLI refuses an agent status there.
