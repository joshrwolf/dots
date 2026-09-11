# GitHub status service

`herdrkit::service` owns one pane-less process per concrete Herdr server and
plugin service identity. Startup and workspace hooks ensure readiness and exit.
The status service never invokes an agent or mutates a GitHub resource.

## Scheduling

The plugin subscribes to workspace topology and reconciles local checkout/branch
identity at least every 15 seconds. Focus hooks are only recovery/wake signals;
they do not run GitHub queries. Repeated wake signals coalesce, with local
inventory at most every five seconds. All open workspaces participate, including
unfocused ones; this API does not expose exact sidebar row visibility.

PR discovery is separate from status fetching. Negative discovery is retried
after five minutes; known branch mappings are rechecked after thirty minutes.
Explicit PR URLs key shared snapshots. Multiple workspaces subscribed to one PR
share one fetch and receive independent metadata publication. Local identity is
rechecked before publication; a changed branch cannot receive a previous branch's
status. Detached checkouts include their commit OID in their identity.

The scheduler starts at most two network jobs and spaces launches by at least
ten seconds. A sidebar fetch uses explicit PR targets for metadata and all-check
counts, without requesting required-check classification or logs. The full CI
inspection used by autofix remains a separate data path. A gh invocation can
make multiple HTTP requests; this is a conservative job budget, not an exact
GitHub request counter or an account-wide quota coordinator.

Running CI is refreshed every 45 seconds, settled open PRs every three minutes,
and terminal PRs every thirty minutes, subject to the shared budget. Oldest due
work wins so discovery is not starved by running CI. Manual refresh requests use
the same queue, join in-flight work, and cannot bypass rate-limit blocking.
Failures back off exponentially to thirty minutes. Detected rate-limit errors
pause all network launches for thirty minutes. No broad PR searches run.

## State and presentation

SQLite in the plugin state directory stores successful PR snapshots and
server-scoped workspace mappings. In-flight jobs are memory-only: restarting
reconciles live workspaces and resumes useful cached snapshots, without replaying
a durable work queue. Service control state is separate and belongs to herdrkit.

Status tokens are retained until replaced/cleared or the workspace closes.
`gh_updated` records an absolute UTC date/time; `gh_health` reports delayed
updates or retry/lookup failures. A failed fetch never becomes a no-PR result.
The service health action reports active jobs, next eligible network launch, and
the latest error. A stopped service cannot update its own warning, so the retained
fetch timestamp remains the honest fallback, not an inferred claim of freshness.

## Lifecycle

The shared service layer owns private control framing, an OS singleton lock,
serialized launch, an eight-second readiness deadline, and a thirty-second
launch cooldown. Status and wake requests use its private socket. Slow work runs
off the control loop. Server identity, plugin enablement, and plugin root are
checked every ten seconds. Owner lookup failures suspend ticks and cause exit
after thirty seconds. Normal shutdown joins outstanding bounded worker jobs.

This is plugin-owned lifecycle management, not Herdr supervision: crash recovery
occurs on the next startup/event/action hook. There is no tab or external OS
service to manage. Building does not activate a service in the user's session.

## Extension boundary

Other plugins reuse `Service` and implement `Handler::{request,tick,status}`.
Handlers enqueue slow work and keep callbacks bounded; their domains own
scheduling, persistence, and cancellation. Never add provider policy to herdrkit
or start an independent refresh path from a hook.
