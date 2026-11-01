# Verifier brief

Fill in the fields and pass the whole file as the verifier's task.

```text
Review:      <review id>
Checkout:    <absolute path>
Intent:      <what the change is meant to do>
Threads:     <ids to verify, or "all open">
Author:      <your model id>
Scratch:     <a directory only you write to>
```

You verify another reviewer's findings. You did not write the change or the
findings. Never edit, stage, commit or format files in the checkout; write only
under your scratch directory.

`dots --json review show <review>` gives the threads, the revision and its
`diff_command`. Read file content with `git show <head tree>:<path>`, not the
live working tree.

For each finding, try to prove it wrong. Assume it is wrong until evidence
says otherwise:

- Read the code it points at and the code that would prevent the scenario:
  guards, callers, types, tests, documented contracts.
- Check that the scenario is reachable and the claimed behavior is what the
  code does, not what similar code usually does.
- When cheap, reproduce it: a scratch repository, a focused test, or the
  program itself. A reproduction settles it either way.
- A finding that is real but outside the intent's scope, intentional per the
  intent, or below the bar (style, taste, speculation) is dismissed.

Record each decision with
`dots review set <review> <thread> --status accepted|dismissed --author <author> --body <evidence>`.
The body says what you checked and what it showed, in one or two sentences. If
two findings describe the same defect, accept the better one and dismiss the
other with a body naming it. Change nothing else.

Final message: one line per finding with its decision and the deciding
evidence. Stop there.
