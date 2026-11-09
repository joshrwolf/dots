---
name: review
description: Review a change with independent, fresh-context reviewers and a falsifying verifier, each writing a report file the parent accepts findings from. Use to check your own work before handing it over, to review a changeset before pushing, or to review a pull request (including from a PR URL). Arguments are an optional profile name and a target.
---

# Review

Reviews produce few, verified findings anchored to exact content. Precision
beats recall: a wrong finding costs the user more than a missed nit, and model
judges asked "is this valid?" accept almost everything, so every finding must
survive an attempt to disprove it with evidence. When review is part of an
agreed execution mandate, return findings to the coordinator for fixes and
verification. A skill handoff does not require renewed approval or expand the
mandate.

## Profiles

| Profile              | Use                                             | Reviewers                                              | Verifier                                      |
| -------------------- | ----------------------------------------------- | ------------------------------------------------------ | --------------------------------------------- |
| `quick`              | An agent checking its own work before reporting | one: correctness and guidance angles together          | none; the reviewer falsifies its own findings |
| `standard` (default) | A finished changeset, a colleague's PR          | correctness from two model families, guidance from one | yes                                           |

Run each reviewer and the verifier as a fresh-context subagent (in Pi,
`worker_spawn` with tools `read`, `bash` and `write`). Model family means
provider: Anthropic and OpenAI. Pick each model from the host's catalog for the
role: the strongest model priced for routine workers, at high effort for
correctness and verification, lower effort for guidance. Do not use the premium
tier unless the user asks. `quick` uses one reviewer from the family other than
your own when one is available, with both angle files.

## Run a review

1. **Target.** Pin the change to commits so reviewers read the same bytes
   while the checkout moves: a base commit and a head commit. For a worktree's
   branch, the base is the merge base with the branch it builds on (its
   upstream when that is another branch, else the default branch, the base
   dots-diff shows); commit uncommitted work first, or, where you may not
   commit, use `git stash create` (tracked changes only). For a PR URL, read
   `gh pr view <url> --json title,body,baseRefName,headRefOid`, fetch the PR
   into a worktree (`gh pr checkout` there, or fetch `headRefOid`), confirm
   `HEAD` is the PR head, and take the merge base with the fetched base
   branch. If the target is ambiguous, ask.
2. **Intent.** The PR description, the task, or the user's request, in a
   sentence or two.
3. **Scratch.** One directory for this review's reports, outside the
   checkout and durable across restarts, such as
   `~/.local/state/<topic>/review/`; a new round gets its own subdirectory.
4. **Reviewers.** For each reviewer in the profile, fill in
   [reviewer.md](reviewer.md) with one angle file from `angles/`
   ([correctness](angles/correctness.md), [guidance](angles/guidance.md)) and
   its own report path in the scratch directory. Run them in parallel.
   Reviewers never see your session, your reasoning, or your opinion of the
   change.
5. **Verify.** When all reviewers finish, run one verifier with
   [verifier.md](verifier.md) over their reports. It writes a verdict per
   finding, confirmed or refuted with evidence, to its own report, and names
   duplicates.
6. **Accept and report.** Read the verdicts and judge the confirmed findings
   yourself; you accept, not the verifier. Report accepted findings in chat,
   most severe first, each with `path:line`, the failing scenario, and the fix;
   then one line on what was refuted and why. Say plainly when nothing blocks.
   The user reads chat, not report files; paths are supporting evidence. In an
   ongoing effort this is an internal result: continue authorized fixes and
   verification rather than handing routine work back to the user. Do not post
   to GitHub.
7. **Pin to code when a viewer is open.** When a dots-diff viewer shows the
   checkout and its files still match the reviewed head, pin each accepted
   finding that points at code as a note
   (`dots-diff ctl note --file ABS --line N --end M "…"`; see the
   [dots-diff skill](../dots-diff/SKILL.md)). Internal review loops do not
   require a user-facing viewer.

## After fixes

When the change moves, re-review its new state: pin the new head and run the
reviewers and verifier again with a new round directory. Continue the same
reviewer sessions when the host allows it (in Pi, `message` to the finished
worker), giving them the new head and their previous report: they confirm
which of their findings are fixed and review what changed since. Re-review
until a round adds nothing new, or the user stops it. The conversation is the
record of what was raised and resolved; comments the user sends from dots-diff
arrive there too.
