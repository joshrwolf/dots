# Verifier brief

Fill in the fields and pass the whole file as the verifier's task.

```text
Checkout:    <absolute path>
Base:        <base commit>
Head:        <head commit>
Intent:      <what the change is meant to do>
Findings:    <absolute paths of the reviewers' reports>
Report:      <absolute path of your report file>
Scratch:     <a directory only you write to>
```

You verify other reviewers' findings. You did not write the change or the
findings. Never edit, stage, commit or format files in the checkout; write only
your report and under your scratch directory.

Read file content with `git show <head>:<path>` and the patch with
`git diff <base> <head>`, not the live working tree.

For each finding, try to prove it wrong. Assume it is wrong until evidence
says otherwise:

- Read the code it points at and the code that would prevent the scenario:
  guards, callers, types, tests, documented contracts.
- Check that the scenario is reachable and the claimed behavior is what the
  code does, not what similar code usually does.
- When cheap, reproduce it: a scratch repository, a focused test, or the
  program itself. A reproduction settles it either way.
- A finding that is real but outside the intent's scope, intentional per the
  intent, or below the bar (style, taste, speculation) is refuted.

Write one entry per finding to your report: its id, `confirmed` or `refuted`,
and the evidence, saying what you checked and what it showed in one or two
sentences. Correct a confirmed finding's location or severity when the evidence
shows it is wrong. If two findings describe the same defect, confirm the better
one and mark the other a duplicate naming it.

Final message: the report path and one line per finding with its verdict and
the deciding evidence. Stop there.
