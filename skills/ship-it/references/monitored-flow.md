# Monitored shipping

Carry the authorized change to a current revision ready for the user's review,
or surface a concrete blocker that requires them. Routine pending checks,
valid findings and skill handoffs are agent work, not reasons to ask the user
to supervise another iteration. This mode does not authorize merge, auto-merge,
policy changes or deployment. Preserve the agreed scope and finish line.

## Discover effective repository policy

Read the target repository's current instructions, ownership rules, workflow
and reviewer configuration, selected PR controls and runtime check evidence.
Determine the applicable gates for the changed paths, expected publishers,
automated-review enrollment and human-only dispositions. An announcement,
stale README, label or another path's rollout does not establish effective
policy. If sources disagree, inspect the owning implementation and current
results before making a consequential assumption.

Preserve the selected review path and explicit opt-outs. Enroll in the ordinary
automated review only when supported and authorized. Do not invent design
associations, reviewer commands or approval thresholds; do not switch paths,
remove controls or weaken checks to evade a blocker. Repository-owned policy
stays in the repository, not a copied snapshot in this skill.

## Inspect, fix and repeat

Use the `dots` skill and installed command help for CI evidence, review threads
and the PR watch list. Inspect complete conversations, top-level reviews, bot
summaries and carried findings, not only unresolved inline threads or the
latest round. Use native tools for unsupported operations.

Track the remote head and the revision/run attempt each observation evaluated,
including merge revisions where applicable. Refresh status while monitoring;
a saved snapshot or a watcher exiting is not fresh readiness evidence. Include
external providers, change-selected jobs and delayed downstream checks. Use
bounded native wakes rather than model-turn polling; about two minutes between
active change checks is a useful starting point, with backoff when appropriate.
Report meaningful changes, not routine ticks.

Evaluate feedback against the code and intended behavior. Fix valid findings,
run appropriate checks and publish coherent corrections. Confirm the commit,
required signing and push succeeded and the remote head matches before claiming a fix,
replying or resolving a thread. A failed commit must not lead to a reply naming
an old SHA. Keep the description current and preserve required trailers.

Reply factually within this mode's authorization, prefixed `:robot:`, with the
fix or evidence for disagreement. Resolve a thread only after verifying the
concern is addressed and repository policy permits that actor to resolve it.
A reply, outdated diff or new commit is not proof. For discussions without a
resolution control, establish an explicit disposition. A waiver, disputed
finding or reviewer-only disposition needs the appropriate person; never
acknowledge accepted risk just to make a gate pass.

After a push, inspect the current revision's checks/reviews and carried findings.
Distinguish code failures, infrastructure failures and base drift. Retry a
plausibly transient failure selectively; repeated unchanged deterministic
failures need diagnosis. A remote commit-signing verification flake does not
justify re-signing an already signed commit. Workflow reruns keep their original
SHA/ref and cannot repair base drift. Honor the main skill's signing-timeout
stop rule and use each actual provider's retry mechanism.

## Ready means verified now

Take a fresh observation and confirm:

- The remote head is the revision inspected. Expected required checks and
  applicable automated-review gates pass for that revision from the expected
  publishers. Missing, pending, cancelled, stale or incomplete evidence is not
  success. Accept neutral/skipped results only when effective policy allows it.
- Relevant advisory failures have a supported disposition. Do not describe
  all CI as green while a shadow/advisory failure remains unexplained.
- Relevant human/bot findings, summary ledgers, threads and review bodies have
  an appropriate disposition; no actionable feedback remains. A clean newest
  round or zero unresolved inline threads does not clear carried findings.
- There are no conflicts or stale approvals hiding a requirement. Verify any
  selected automated-approval path separately from ordinary CI. Distinguish
  ready for the user's requested review from approved or authorized to merge.

Recheck the remote head at the end; if it moved, refresh the evidence. At the
agreed endpoint, report the PR link, inspected revision, CI/review result and
material caveats in a self-contained chat handoff. Assume the user has not read
goal briefs, task entries, agent reports or review threads; explain what matters
rather than making those artifacts a reading assignment. Link any relevant open
discussion and name whose input it needs; an actionable unresolved requirement is a blocked handoff, not an
unqualified ready claim. Do not ping people or request reviewers without
separate authorization.

## When blocked

Continue useful work within the mandate. For credentials, permissions,
human-only dispositions, scope changes or exhausted recovery, record a durable
request under the active goal when one exists; otherwise ask directly in chat.
Ask the smallest self-contained question.
Name the affected work and recommendation; do not make the user read internal
notes or identify task IDs. Preserve actual human input separately from your
interpretation. A saved login confirmation permits checking/retrying within
the mandate, not claiming authentication or a signed push succeeded.

Wait on real events when appropriate; do not silently abandon unfinished
monitoring or imply a watcher will perform remaining fixes. If the coordinator
cannot continue, state what remains and preserve an explicit resumption point.
Signing timeout still stops the workflow until the user explicitly asks to
retry. Neither another wake nor an unrelated message supplies that instruction.
