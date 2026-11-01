# Go skill evaluations

Neutral tasks exercise ordinary implementation, delegated work, cancellation,
modernization, review accuracy, and restraint on a typo fix. Use
[skill-evaluation](../../skill-evaluation/SKILL.md) to freeze a setup and compare
revisions; these cases do not require a runner.

Give each run a fresh copy of `fixture/`. Keep model and effort matched across
configurations and implementation children. Retain the exact rendered prompt,
including any concrete model and effort substituted into the case template.

The fixture contains deliberate defects and older idioms as task inputs.
`acceptance/` holds functional checks for completed implementation tasks.
Install runtime skill copies without `evals/`, assertions or hidden checks;
otherwise record the exposure and audit for contamination.
After execution, add only the target package's checks to a separate acceptance
copy and run them with the repository's test commands. Poller acceptance tests
use `testing/synctest`; run each test with a bounded timeout so one broken
implementation cannot block the suite. Review cases are graded from the review
and original source; acceptance tests do not grade prose.

Keep discovery and output quality separate. A correct result that skips guidance
is not automatically a code failure. Include additional unseen cases before
drawing broad conclusions from this small set.
