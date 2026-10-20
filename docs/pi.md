# Pi

Dotfiles provide Pi instructions and themes; the companion adds local memory,
workers, jobs, wakes, and compact presentation.

## Setup

Use the [pinned development tools](build.md), separately from Pi's managed runtime:

```sh
mise run pi-cli:install
mise run install
mise run //src/pi:packages
mise run //src/pi:install
```

`:packages` runs `pi install` for the pinned third-party npm packages.
`:install` runs `mise run //src/pi:deps` (`npm ci`), then
`pi install <checkout>/src/pi`. Pi loads a local package from its path without
copying, so the companion runs from the checkout. Install only from the primary
checkout, never from an agent worktree. Reinstalling is harmless.

Settings, credentials, models, sessions, and package registrations stay
machine-local under `~/.pi/agent/`. Shared skills use `~/.agents/skills/`.
Use `pi list` to inspect packages and `pi config` to select their resources.
Register one companion copy: Pi identifies local packages by path, so a second
checkout's copy loads alongside and conflicts. Use `pi remove <path>` first.

## Updates

Companion changes take effect in the primary checkout's working tree; there is
no install step. A running session keeps the code it loaded until `/reload`.
Use `/reload` when background work is idle: it shuts down extensions and stops
this session's jobs and worker processes. Every session loads the checkout's
current files on its next reload or start, so switching branches or editing
there changes them all.

| Change | Load it |
|---|---|
| Companion source | `/reload` |
| Companion dependencies | `mise run //src/pi:deps`, then `/reload` |
| Pi CLI/SDK or startup environment/options | Restart Pi; `pi -c` resumes the last project session |
| Workbench source | `/reload` in the workbench; relaunch for launcher changes |

Keep the companion's exact development SDK and lockfile aligned with the host.
Pi supplies its own SDK, TUI, and TypeBox to extensions; the development copies
in `node_modules` serve only checks. Use `mise run //src/pi:check` or
`mise run //src/pi:verify`; verification uses isolated provider-free hosts and
does not change the live installation or prove live-model access.

`mise run pi-cli:update` updates Pi itself, not pinned extensions.

Third-party npm pins live in the `:packages` task. To upgrade, edit the pin and
run `mise run //src/pi:packages`; Pi replaces the older registration and keeps
its resource filters. Use `/reload` when idle. Companion checks do not run upstream's
test suite; test a worker launch/result in a fresh session after upgrades.

## Presentation

Use `/ui` to select tool displays, thinking, the header, footer, and editor styling. Changes take
effect only with **Apply and reload**; Cancel leaves settings unchanged.
**Other tools** covers background tools and tools without their own custom renderer;
third-party custom renderers retain ownership. Preferences live in
`~/.pi/agent/dots-ui.json`. Disabling the UI resource through
`pi config` restores native surfaces.

Compact thinking labels short summaries and groups longer Markdown under **Thinking**.
Ctrl+T toggles native thinking visibility; fullscreen also supports clicking a block.
This changes terminal presentation only, not recorded reasoning or HTML exports.
Select **thinking: native** in `/ui` to restore the original presentation.

Ctrl+O expands recorded tool details. Paths, failures, truncation notices, and
full-output links remain visible; display shortening does not change tool results.
Background calls and completion messages use the same identity/outcome layout.
Job summaries keep capture and cleanup warnings visible; expand or use `/jobs`
for full evidence. Codemode child previews cannot show nested results that Pi did not record.

Theme selection stays in Pi's `/settings`. The base `dots` theme follows
`~/.config/theme/current/pi.json`; the companion also supplies
`dots-light/dots-dark` for automatic light/dark selection. Package theme and central
theme-link changes need `/reload`.

`/context` shows reported usage and estimated contributions, not exact billing.
The [presentation library](../src/pi/src/README.md) documents lifecycle constraints
for companion developers.

## Workers

Workers are background Pi RPC children with their own context and session.
Use `worker_spawn` for a bounded brief; `message` steers or resumes the same
worker. Completions, failures, and questions arrive as events—do not poll for them.
Use `worker_wait` only when there is nothing else to do, and `worker_status` or
`/workers` to inspect activity.

Model selection accepts `provider/model[:thinking]` or a role from
`~/.pi/agent/dots/workers.json` under `modelRoles`. Omission inherits the parent's
model and thinking level. An empty tool allowlist permits only `message`.

A worker asks its parent through `message` with `wait: true`; timeout or
cancellation is not approval. Sibling relays are send-only and do not authorize
scope expansion. Keep decisions in the parent conversation.

Children stop when the parent exits and resume as paused sessions through
`message`. Completed means a result is ready, not necessarily that the child
process exited. `worker_stop { all: true }` closes owned processes and descendants
while retaining results. Sessions live under `~/.pi/agent/dots/workers/`; the
observation database is not the authority for resume state.

Web research comes from the pinned `pi-web-access` package. Optional provider
settings belong in `~/.pi/agent/web-search.json`; browser-cookie access and hosted
fetch fallbacks remain opt-in.

## Goals

Use `/goal new IDEA` to start a planning conversation. The agent can write the
brief, create initial slices, and follow the result with `goal_focus`; this does
not request implementation. Your desired involvement stays in the conversation
or brief, not a separate execution policy.

For direct setup, `dots task goal create TITLE --brief PATH` creates a root and
attaches an existing Markdown brief together. Use
`dots task goal attach ID --brief PATH` to adopt an existing root. These require a
`dots` build with the `task goal` commands; Pi never installs it automatically.

`/goal follow ID` adds a goal to this session (`focus` remains an alias).
Follow several goals at once; selecting another never pauses or closes earlier
work. `/goal` chooses which followed goal to inspect, with `1` slices, `2` brief,
and `3` record details. `/goal show ID` selects directly; `/goal history ID`
includes closed details. Selection affects display, not which goals get context.

`/goal list` lists shared open/held goals and marks this session's follows;
`/goal followed` lists the session set. `/goal unfollow ID` removes one;
`/goal clear` removes all (`unfocus` without an ID remains a clear-all alias).
Agents use `goal_focus {id}`, `goal_list {}`, and `goal_unfocus {id}` or
`goal_unfocus {all: true}`. These change no shared tasks, claims, workers, or
other sessions. Unfollowing removes future injected context, not prior messages.

The followed set and display selection survive branch reload/resume; older
single-focus sessions restore as a one-goal set. Fresh sessions follow nothing.
The compact widget shows recorded progress, not live execution; `/goal refresh`
refreshes the whole set without polling.

Tasks are project-scoped, shared across linked worktrees. `dots task list` shows
open/held roots, not every slice. Use `--parent ID` to drill down, `--flat` for all
levels, and `--all` to include closed history. Goal lists also hide done/dropped
roots by default. Records are retained, not automatically deleted; close finished
work and drop abandoned work to keep the active view useful.

Pi refreshes every followed goal's local tasks and brief before each new agent
run and refreshes the widget afterward. Brief context is bounded to 24 KiB per
goal, with a reference for the rest; total context grows with the followed set.
The multi-goal widget reports approximate brief-text size. Display limits never
silently remove goals from context. Missing briefs or CLI failures stay
visible rather than reusing stale context. Counts are recorded task state—not
verified PR or worker status. Focusing, inspecting, or restoring never authorizes
execution or automatic continuation. See the [goal-mode direction](goal-mode.md).

## Jobs

`job_start`, `job_status`, and `job_stop` run trusted finite shell commands on
macOS in a kept-alive TUI/RPC session. They do not support one-shot print mode,
stdin, PTYs, service readiness, or reload survival. Cancellation after admission
does not stop the job; use `job_stop`.

`/jobs` inspects outcomes, cleanup evidence, and output paths without starting a
model turn. Historical jobs are inspection-only; unfinished restored jobs are
unknown, never resumed. Reload/shutdown stops owned jobs. Aborting a model run
suppresses automatic job wakes until the next user-started run.

Output under `~/.pi/agent/dots/jobs/` retains a bounded prefix and tail, not
necessarily a complete log. Use `read` on the returned paths and inspect capture
warnings. Artifact eviction is runtime-local, not a global disk quota.

Root outcome, stop intent, cleanup evidence, and capture completeness are
separate facts. Process-group cleanup cannot contain detached descendants,
supervisor failure, or all simultaneous shutdown races. Do not treat successful
root exit as proof that every descendant stopped.

## Wakes

`wake` schedules a future turn, once or periodically. An optional shell
`check` gates model wakes on output changes, success, or failure. Use coarse
intervals and a finite limit; `wake_list`, `wake_cancel`, `wake_run`, and
`/wakes` manage them.

Wakes persist in the session, but nothing fires while Pi is closed. A missed
wake fires once on return rather than replaying every missed interval.
Manual fires count toward the limit; overlapping checks are rejected.

Checks have bounded execution and pipe draining. Cancellation/reload discards
late results; deliberately detached descendants may survive. Use trusted finite
checks, not daemons.

## Memory

`/memory` shows global and project indexes under `~/.local/share/dots/memory/`.
Linked worktrees share the project index; project notes take precedence over
global notes. Memory is learned context, not an instruction override.

Nothing is written automatically. The shared [memory skill](../skills/memory/SKILL.md)
provides recall, source-backed updates, correction, and maintenance. Indexes stay
compact; linked knowledge and selected evidence load on demand. Existing adjacent
topic files remain valid. Cooperating writers acquire a per-scope directory lock
and reread before editing; readers do not get atomic multi-file snapshots.
Goal briefs and tasks retain working context and progress separately.
Credentials, private captures, and transcripts do not belong in memory.
Contributor guidance lives in [AGENTS.md](../src/pi/AGENTS.md).
