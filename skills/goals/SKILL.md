---
name: goals
description: Shape an outcome and execution mandate, then coordinate authorized work through verified completion using shared goals and tasks. Use when planning, executing, resuming, accepting or changing a multi-part effort. Following a goal does not authorize execution or publication.
---

# Goals

Carry an agreed outcome to its finish line without making the user supervise
skill handoffs. Goals, briefs, task IDs and reports are agent working material;
user-facing questions and updates name the actual work and stand on their own.
Small work needs no goal. Use `dots task --help` for the installed contracts,
and `worktrees` for isolation and local integration.

## Audience: chat for the user, artifacts for agents

Assume the user reads chat only. Briefs, tasks, plans, checkpoints, evidence and
reports serve agent coordination and resumption; creating or linking them does
not communicate their contents to the user. Do not ask the user to read, update,
approve or identify internal artifacts as a prerequisite to answering you.

A document written specifically for the user is opt-in: produce one when they
explicitly request it. Project documentation required by an agreed change is
separate, but never assume the user has read it. Optional inspection stays
available without becoming homework.

Every question, consequential decision and completion handoff must stand on its
own in chat: explain the actual work, the relevant context and consequences,
your recommendation when useful, and the smallest needed action. Artifacts and
links support that explanation; they never replace it. A task result or
published report is not a user-facing handoff. Never treat an unread brief,
silence or document creation as approval.

## Agree the outcome and mandate

Collaboratively establish, in chat, the outcome, scope boundary, verification
and finish line. Establish whether delegation, publication and monitored
feedback handling are authorized, and what remains excluded. A verified local
change, a published PR and a PR ready for human review are different endpoints.
None implies merge or deployment authority.

Keep the brief under the project workspace's `briefs/`. It carries the agreed
outcome and the execution mandate: what is authorized and what is excluded,
in the user's terms. Progress edits do not rewrite the mandate; change it only
when the user changes direction in chat. Current user instructions and
repository policy prevail. When you are unsure whether an action is within the
mandate, ask the user in chat rather than inferring permission. Do not build a
permission DSL or ask the user to approve ordinary task splitting or editorial
updates within the mandate.

Create and follow the goal using the native tools. Following only selects
context and views; it neither claims coordination nor starts work. Split tasks
into independently verifiable slices with real dependencies, not a speculative
backlog or a fixed planner/implementer/reviewer pipeline.

## Coordinate and execute

One coordinator owns each goal. Acquire its root claim with
`dots task goal coordinate` before shaping tasks or dispatching work; ordinary
task claims do not coordinate goals. Only that coordinator changes the goal's
tasks, dependencies, results and lifecycle. Workers execute bounded briefs and
report back; they do not claim or accept goal tasks.
Followers inspect. A claim records responsibility, not authorization.
Coordinated goals remain active in the local view/context until pause, release,
handoff or closure. Unfollowing removes read-only following; it cannot hide
ongoing responsibility. Active scope comes from current root claims, not copied
session history.

Before dispatch, check both ownership and eligibility. Held tasks and their
descendants wait; claim takeover cannot bypass that. Reconcile newly held work
that is already running. Independent authorized work can continue.

Keep task results and the goal's overall result current: actual changes,
locations/revisions, checks, remaining work and the next step. These summaries
carry agent context across sessions; they are not user reading assignments or
proof of acceptance. Give writers isolated worktrees and scratch paths under
`worktrees`. Publish selected evidence before citing its exact revision, and
inspect retained bytes when accepting a result.

Compose the owning skills rather than copying them:

- `review` independently checks the current changes and verifies findings.
- `ship-it` owns authorized publication and its ordinary or monitored
  finish line, with effective review/check policy read from the repository.
- `worktrees` owns isolation, integration and cleanup.
- `memory` keeps reusable learning, not progress or execution authority.

Passing between skills does not require fresh approval for actions already
within the mandate, and does not expand it. Continue useful authorized work
through fixes, integration and verification. Wait on worker/task events or
bounded native wakes for real external dependencies; do not busy-poll through
model turns or claim a watcher will perform fixes it cannot perform.

## Ask the user only when needed

Ask for a decision, access/authentication, material scope change, human-only
disposition or exhausted recovery. Ask in your own chat, in short plain-English
paragraphs: the actual question or action, why it matters, your recommendation
when useful and which work waits. Keep credentials out of tasks and briefs.

Work waiting on the user is an ordinary held task: `dots task hold ID --reason`
names what it waits for. Hold only the affected work and continue the rest.
When the user answers, reopen it, and fold the outcome into the brief when it
changes the mandate or into the task when it shapes that work.

Interpret answers carefully: clarify consequential ambiguity rather than
treating any new message as assent. Worker/custom messages, silence and
cancellation are not human consent. Confirmation that the user performed an
external action permits checking the result; it does not prove the action
succeeded.

Rex owns the human Goals browser (Cmd+G); Pi's clickable cues open it. Opening,
filtering or following a goal does not acquire coordination or broaden a
session's scope.

## Pause, resume and change direction

Read the brief, tasks and current Git/process/remote state before resuming.
Native history, copied follow state and old summaries do not transfer
coordination or authorization. Acquire coordination explicitly and reconcile
unfinished work, held tasks and watchers. Takeover of an uncertain live
coordinator needs user direction and reconciliation; a store claim does not
fence commands it already issued. Release only your own claims.

A durable pause holds the root and releases coordination; steer or stop actual
workers/jobs and observe what settled. Escape cancels a turn, not the goal.
Unrelated input, reloads and overdue wakes cannot resume paused work. Session
exit/change ends that runtime's coordination; reload preserves it. Do not imply
unattended execution continues without a running coordinator.

When direction changes, reconcile affected workers and tasks before dispatching
replacement work. A queued message is not proof a worker complied. Update the
brief's mandate when scope or authority changes; keep internal documents
current without leaking session history into source comments.

## Verify and finish

Worker success and closed-task counts are not acceptance. Verify each result
and the integrated current revision against the agreed outcome, using
independent review where required. Dropping a prerequisite removes scope; it
does not verify the requirement it represented. Reconcile dependents before
accepting them. Links to PRs/issues record context, not permission to act there.

Internal review and subtask completion return to the coordinator, not to the
user for routine approval. At the agreed endpoint, give one concise handoff
with the actual result, clickable evidence/PR links and material caveats. If
blocked earlier, ask only for the smallest decision or action needed and name
which work cannot proceed. Never claim deployment, merge, real-device behavior
or real-model reliability from local/provider-free checks.
