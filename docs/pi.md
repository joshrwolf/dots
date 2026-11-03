# Pi

The companion adds workers, finite background jobs, wakes, memory, goal
inspection, review submission notices and Rex status to stock Pi.

## Setup

[Machine setup](../bootstrap/README.md) installs Pi and the companion's runtime
dependencies, then registers its durable source. From HOME:

```sh
mise run pi:packages
pi list
```

Keep one companion registration. Pi identifies local packages by path, so
registering another checkout loads a second copy. Remove the old path before
registering a replacement. Skills use the shared [skill namespace](skills.md).

## Preferences

Machine setup merges the [public preferences](../home/pi/.pi/agent/settings.json)
into `~/.pi/agent/settings.json` and the [web search routing](../home/pi/.pi/agent/web-search.json)
into pi-web-access's `~/.pi/agent/web-search.json`, leaving other fields
untouched. Search tries OpenAI through the Codex login first, whatever the
active model, and falls back to keyless Exa. API keys, credentials,
last-selected model/provider, device identity, package registrations and sessions
stay machine-local.

Pi saves preference edits locally, not back to dots. Change the tracked file to
make a preference portable; explicit machine apply restores its owned values.
Restart idle Pi sessions to use the applied preferences.

## Updates

The registered source is `~/.local/share/dots/checkouts/dots/src/pi`.
Changes in a development worktree do not change the running setup. Update the
acquired source through [machine setup](../bootstrap/README.md#inspect-and-update),
then load it:

| Change                         | Action                                               |
| ------------------------------ | ---------------------------------------------------- |
| Companion source               | `/reload`                                            |
| Runtime dependencies           | `mise run runtime:deps` from HOME, then `/reload`    |
| Pi host or startup environment | Restart Pi; `pi -c` resumes the last project session |
| Third-party package            | `pi update npm:pi-web-access`, then `/reload`        |

Reload only when background work is idle: it shuts down this session's extensions,
jobs and workers. Other sessions keep their loaded code until their own reload
or restart. Pi and the development SDK must match the workspace catalog pin.

## Presentation

- `/ui` selects compact/native displays, thinking, header, footer and editor
  styling. **Apply and reload** saves; Cancel leaves settings unchanged.
- Ctrl+T toggles thinking; Ctrl+O expands tool details. Compact displays retain
  errors, truncation and full-output links without changing recorded results.
- `/goal`, `/workers`, `/jobs` and `/wakes` inspect activity without replacing
  the editor draft. Scrolling a transcript pauses follow; F resumes it.
- `/settings` owns theme selection. The `dots` theme follows
  `~/.config/theme/current/pi.json`; theme-source changes need `/reload`.
- `/context` shows reported usage and prompt-source estimates, not billing or
  cache-hit measurements. Its overlapping estimates must not be added together.

UI preferences live in `~/.pi/agent/dots-ui.json`. Disable the UI resource through
`pi config` to restore native surfaces. Third-party custom renderers remain
owned by their packages.

## Workers

Workers are headless Pi sessions managed by their parent. An interactive Pi
opened in a Rex pane is independent, not a managed worker.

Use `/workers` to inspect progress, transcripts and results; M messages an owned
worker and X offers stop confirmation. Descendants are read-only. Completion
means a result is ready, not that its process exited or its work was accepted.
Worker costs are shown separately from the parent's session cost. A `~` marks a
catalog estimate; an unmarked total is provider-billed for every response.

Workers belong to a session, not a conversation branch. `/tree` preserves them;
`/fork` and `/clone` copy only read-only history. Reload/exit stops owned workers;
`message` can resume their saved sessions. Stop retained worker processes before
removing their worktrees.

Agents use `worker_spawn` for bounded work, `message` to steer or resume it, and
completion events rather than polling. Tool schemas supply models, thinking
levels, skill handoff and task-assignment arguments. Task association does not
claim, complete or accept a task.

Native Pi settings own compaction. The companion displays context pressure and
warns near supported workers' native triggers; it neither changes model capacity
nor replaces a worker automatically. Unknown or stale context remains explicit.

## Goals

Use `/goal new IDEA` to plan an outcome, or `/goal follow ID` to follow an existing
one. Following adds its brief and task outline to context; it does not authorize
implementation or start work. Use `/goal` to inspect, `/goal refresh` to refresh,
and `/goal unfollow ID` to remove its future context.

For another project, supply its exact `project` key from `dots task --json`,
not a guessed path. Several goals may be followed at once; selecting one changes
only the display. The followed set survives session reload/resume.

Briefs hold agreements, tasks hold progress, and retained outputs hold evidence.
The agent manages claims through `dots`; direct CLI use needs an explicit
`--owner` rather than an inherited session identity. See [tasks](tasks.md) and
[documents](documents.md).

## Jobs

`/jobs` inspects finite session-owned Bash commands, outcomes and captured output.
Jobs support neither PTYs nor stdin, service supervision or reload survival.
They require a live TUI/RPC session, not one-shot print mode.

Aborting an agent response does not stop an admitted job; use `job_stop`.
Root exit, stop intent, cleanup and capture completeness are separate facts.
Process-group cleanup cannot contain every detached descendant. Historical jobs
are inspection-only, never resumed. Output retains a bounded prefix and tail
under `~/.pi/agent/dots/jobs/`; check capture warnings before treating it as complete.

## Wakes

`/wakes` inspects scheduled turns. Agents use `wake` for a one-shot reminder, a
finite recurring check, or a Rex pane reaching a status or exiting; native
schemas describe the triggers. Use trusted finite checks, not daemons, and
cancel schedules whose purpose is over.

Pane wakes follow one `rex events --all` stream per Rex host while any exist,
never autostart a server, and reconcile with snapshots after a reconnect or
restart. A pane wake ends early when its program stops, its block or session
closes, or the Rex server restarts, and gives up after its `for` deadline.

Wakes persist but do not fire while Pi is closed. A missed wake fires once on
return. `/tree` preserves them; `/fork` and `/clone` copy inactive history.
A fire waits until the session is idle; after a cancelled run it is recorded
without starting a turn until the next prompt.

## Memory

`/memory` shows global and project indexes. Linked worktrees share project memory;
project notes take precedence over global notes. Memory is learned context, not
an instruction override, task list or transcript archive. Nothing is written
automatically. The [memory skill](../skills/memory/SKILL.md) owns upkeep through
revision-checked `dots memory` commands.

## Review submissions

A review records its owner: the session that started it, or the agent session
that last opened the worktree's review (`dots review open` and `start` take the
invoking agent session unless given `--owner`). That session watches the reviews it
owns across the repository's worktrees. The user's comments stay private drafts
until the user submits them; each submission reaches the owner as one notice
with its summary and comments. Like a wake fire, a notice waits until the
session is idle and does not restart a cancelled run. Notices on the current
branch are the receipts, so reload and resume neither repeat a submission nor
lose one that arrived while Pi was closed.

## Anthropic request guard

Direct Anthropic streams have 90 seconds after headers to produce
`message_start`; keepalive pings do not extend it. Timeout uses Pi's normal retry
budget. User cancellation never retries, and thinking after the response starts
is not limited.

`/request-guard` shows counts and timings. Records contain metadata, not prompts,
response text or credentials. The guard uses Pi's simple-stream override: custom
Anthropic full-stream options are normalized as simple-stream options. Disable
it through `pi config` and restart Pi when workers are idle; reload alone does
not remove provider overrides.

## Rex

Rex supplies session-start orientation. The companion reports OSC7501 status
for the parent and its jobs/workers; Rex owns notifications. A child finishing
does not announce parent completion. See [Rex configuration](../home/rex/.config/rex/README.md).

## Development

Use the [SDK/check setup](build.md), [contributor rules](../src/pi/AGENTS.md) and
[presentation library](../src/pi/src/README.md). Checks use disposable hosts and
never activate the live setup. After a host upgrade, also exercise a native child
launch/result in a fresh session; provider-free checks do not establish live-model access.
