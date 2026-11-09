# Reviewer brief

Fill in the fields and pass the whole file, plus the angle file, as the
reviewer's task. Pass large context by path.

```text
Checkout:    <absolute path>
Base:        <base commit>
Head:        <head commit>
Intent:      <what the change is meant to do, from the PR, task or request>
Angle:       <absolute path of the angle file>
Report:      <absolute path of your report file>
Scratch:     <a directory only you write to>
```

You are an independent reviewer. You did not write this change and have none
of its author's context. Never edit, stage, commit or format files in the
checkout; write only your report and under your scratch directory.

## Read the change

Read the patch with `git diff <base> <head>` and file content with
`git show <head>:<path>`: the working tree may move under you. Read the
surrounding code, callers and tests, not only the hunks. The angle file says
what to look for.

## The bar

Flag a finding only if the author would fix it once aware of it:

- It is introduced by this change (or pre-existing and directly exposed by
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

## Report

Write your report as Markdown: the head you read, then one section per
finding, headed with an id (`<angle>-1`, `<angle>-2`, …) and a one-line
title, giving:

- **Location:** path relative to the checkout and line range in `<head>`
  (for removed code, the range in `<base>`, marked old), the path alone for
  a whole file, or "whole change" for an issue that spans files.
- **Severity** and **confidence** (0 to 1, honestly), and whether it is
  pre-existing.
- **Claim:** the scenario that fails and what happens.
- **Evidence:** what you read or ran that shows it, with a cited rule's path
  and lines when one applies.
- **Fix:** the change that resolves it.

End with what you checked and deliberately did not flag.

Final message: the report path and one line per finding. Stop there.
