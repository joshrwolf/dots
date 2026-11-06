# Monitored shipping

Carry the current PR revision to verified readiness under `ship-it`'s publication
and signing rules. Routine pending checks and valid findings are work to finish,
not reasons to ask the user to supervise another iteration.

## Discover effective policy

Read current repository instructions, ownership/reviewer configuration, workflows
and runtime results for the changed paths. Establish expected checks, publishers,
automated-review enrollment and human-only dispositions. Resolve conflicting
sources against the owning implementation before acting. Keep repository policy
in its repository, not a copied snapshot here.

Preserve the selected review path and explicit opt-outs. Do not invent reviewer
commands, design associations or thresholds, or weaken controls to evade a gate.

## Inspect, fix and repeat

Use dots evidence and review workflows, supplemented by native tools for
unsupported surfaces. Read complete conversations, top-level reviews, bot
summaries and carried findings—not just unresolved inline threads or the newest
round. Track which head, merge revision and run attempt each observation covers.
Include external providers, change-selected jobs and delayed downstream checks.

Use bounded wakes for fresh observations, about two minutes apart during active
monitoring with backoff when appropriate. Saved snapshots and watcher exits are
not readiness evidence. Report meaningful changes, not routine ticks.

Evaluate findings against intended behavior, fix valid concerns and publish
checked corrections through `ship-it`. Reply with the fix or evidence for
disagreement. Resolve a thread only when the concern is verified addressed and
policy permits that actor to resolve it; an outdated diff, reply or new commit
is not proof. Discussions without a resolution control need an explicit
disposition. Waivers, disputed findings and reviewer-only dispositions require
the appropriate person, not an agent acknowledgment of risk.

After each push, reassess current checks/reviews and carried findings. Distinguish
code failure, infrastructure failure and base drift. Retry plausibly transient
failures selectively; repeated deterministic failures need diagnosis. A remote
commit-signing verification flake does not justify re-signing an already signed
commit. Workflow reruns retain their SHA/ref and cannot repair base drift; use
the actual provider's retry mechanism.

## Verify readiness

Take a fresh observation and confirm:

- Expected required checks and applicable automated-review gates pass for the
  current revision from expected publishers. Missing, pending, cancelled, stale
  or incomplete evidence is not success. Neutral/skipped results count only
  where effective policy allows them.
- Advisory failures have supported dispositions. Do not call all CI green while
  a shadow/advisory failure remains unexplained.
- Relevant human/bot findings, summary ledgers, threads and review bodies have
  appropriate dispositions, with no actionable feedback outstanding. A clean
  newest round or zero unresolved inline threads does not clear carried findings.
- No conflicts or stale approvals hide a requirement. Verify any selected
  automated-approval path separately from CI. Ready for human review does not
  mean approved to merge.

Recheck the remote head before handoff; refresh evidence if it moved. Report the
PR link, inspected revision, CI/review result and material caveats. An unresolved
actionable requirement is a blocked handoff, not an unqualified ready claim;
link the discussion and name the input needed. Use `ship-it` and `goals` for
blockers and resumption rather than abandoning monitoring or implying a watcher
will perform the remaining fixes.
