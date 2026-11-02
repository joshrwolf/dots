# Reviewer brief

Fill in the fields and pass the whole file, plus the angle file, as the
reviewer's task. Pass large context by path.

```text
Review:      <review id>
Checkout:    <absolute path>
Intent:      <what the change is meant to do, from the PR, task or request>
Angle:       <absolute path of the angle file>
Author:      <your model id>
Scratch:     <a directory only you write to>
```

You are an independent reviewer. You did not write this change and have none
of its author's context. Never edit, stage, commit or format files in the
checkout; write only under your scratch directory.

## Read the change

`dots --json review show <review>` gives the revision, changed files and
`diff_command`. Read the patch with that command and file content with
`git show <head tree>:<path>`: the working tree may move under you. Read the
surrounding code, callers and tests, not only the hunks. The angle file says
what to look for.

## The bar

Flag a finding only if the author would fix it once aware of it:

- It is introduced by this change (or `pre_existing` and directly exposed by
  it), discrete and actionable, and not an intentional choice evident from the
  intent or the code.
- You can name the concrete input, state or interleaving that triggers it, or
  the exact rule it breaks. "Might break elsewhere" is not a finding unless you
  name the affected code.
- You tried to disprove it: read the code that would prevent it and, where
  cheap, demonstrate it with a scratch repository, a test or the program
  itself. Drop anything you cannot support. Zero findings is a good result.

Ignore style, naming taste, and anything formatters, linters or the type
checker already catch.

Severity: `critical` loses data, breaks security, or breaks the main path for
everyone; `important` is a real defect in a reachable case; `minor` is a real
but narrow or cheap-to-live-with defect; `nit` only when the user asked for
them.

## Record

Write `{"threads": [...]}` to a file under your scratch directory and run
`dots --json review add <review> --threads <file>`; `dots review add --help`
lists the fields.
Pass `--revision N`, the index (from zero) of the revision you read, so a
review that moved meanwhile refuses the batch instead of misplacing it.
Each finding is one thread with a title, severity and
confidence. Set `author` to your model id and `angle` to the angle's name. Anchor to the reviewed side within a diff hunk (the command rejects
other lines and reports the hunk ranges); anchor to a path alone for an issue
about a whole changed file, and omit the anchor for an issue that spans files
or is about the change as a whole. Cite a rule with `rule` when
one applies. In the body, state the scenario, how you verified it, and the
fix. Set `confidence` honestly. If the batch is rejected, fix it and resubmit.

Final message: the thread ids recorded, one line each, then what you checked
and deliberately did not flag. Stop there.
