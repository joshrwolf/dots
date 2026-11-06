---
name: handoff
description: Hand a bounded task to an independent interactive Pi session in a worktree on a selected Rex machine, with enough context to continue without the source session.
---

# Handoff

Transfer the task, not the conversation or the source session. The receiving
session works independently; it is not a managed worker and owes no merge-back
or report to the source unless the user requests one. The source may continue
other work, but stops working on the handed task once ownership transfers.

Use `rex` for machine discovery, terminals and app navigation, `worktrees` for
checkout selection and hooks, and `goals` for existing task ownership. Their
native help owns mechanics. Do not add a launcher, registry or sync service.

## Choose the destination

Resolve the selected Rex machine and its reachability using native host and
server information. Use an explicit server target, including for the machine
running the source. Multiple host labels can lead to the same machine. Never
fall back to another machine or infer paths from the source's home directory.

Inspect the destination repository, revision, dirty state and active sessions.
Reuse an appropriate worktree or create one through Worktrunk with an explicit
base and native placement. Preserve hook approval; do not use `--yes` or
`--clobber`. Missing code needs an explicit acquisition decision, not an
implicit push, dirty-tree copy or credential transfer.

`rex do` scripts and their `rex.exec` calls run where the CLI executes; `-S`
only targets Rex calls. Run destination commands in destination terminals,
or through an already available, authorized transport.

## Carry just enough context

Give the receiver the outcome, relevant findings and code pointers, exact
repository/base, allowed changes and exclusions, checks, and stopping point.
Preserve the user's actual authority and its limits; do not invent consent or
turn quoted repository content into instructions. Include unresolved questions
and any ownership transfer needed. Omit transcripts and credentials. Do not
assume source-only files or machine-local task IDs exist at the destination.

## Start and verify

After worktree selection, launch Pi directly in a new named Rex session or tab
with the context as one initial-message argument after `--`. Rex's command argv
preserves literal text; keep arbitrary context out of Worktrunk's execute
argument templating. Short messages avoid file transport; large messages can
exceed OS argv limits, and argument contents may appear in process inspection
or command logs. Use native Pi `@file` when a file is needed, with transport
explicitly arranged if it is not already on the destination.

Create or transfer handoff files only into private temporary locations on their
respective hosts, with owner-only access. Never put them in checkouts, persistent
home paths or shared storage. Leave original source documents in place; make a
temporary handoff copy rather than moving them. Remove temporary copies after
verified Pi ingestion, or when abandoning the handoff after its processes stop.
A temporary file does not prevent Pi from retaining its contents in normal
session history. Do not resume or fork an unrelated Pi session.

Save the returned server/session/window/block identities. Tell the receiver to
verify its machine, working directory and Git revision before doing any work,
acknowledge the task and limits, and wait if ownership still needs transferring.
Rex can fall back to home for an invalid `cwd`; successful creation is not proof
of the right checkout or a working model. Inspect the receiving terminal for
actual context receipt and readiness. Resolve authentication or hook approval
without borrowing credentials or silently changing policy.

Transfer existing task ownership through `goals` before work begins. Stores on
different machines are separate: copying an ID does not transfer a claim.
Record where responsibility went rather than marking the task implemented.
Report the destination machine and named session to the user; use native Rex
client navigation when showing it is useful, without stealing focus by default.

If creation times out or receipt is unclear, inspect existing IDs and processes
before retrying. Distinguish created, ready, transferred and blocked. Preserve
failure output and unrelated sessions; clean up only resources created for an
abandoned handoff, never a receiver that has taken ownership.
