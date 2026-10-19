# Pi

Pi uses its official managed installer. Dotfiles provide personal instructions
and a theme. `pi-web-access` provides web research tools; our local companion
package provides `/context`, project memory, background workers, compact tool
displays, and a one-line footer.

## Setup

Use the pinned tools from [the build setup](build.md), plus GNU Stow.
Development Node is separate from Pi's private runtime.

```sh
mise run pi-cli:install
mise run install                         # base Stow links
mise run //src/pi:deps
mise run //src/pi:check
mise run //src/pi:upstream-install    # pinned third-party packages
mise run //src/pi:install             # prepared companion release
```

The companion runs from immutable releases beneath
`~/.pi/agent/dots/packages/pi-dots/`, not a mutable checkout or build directory.
For a one-time transition from a registered source directory, close running Pi
sessions and use `mise run //src/pi:install -- --replace-source /path/to/old/package`.
Resource filters are preserved. Do not run concurrent package writers during
cutover: Pi stores registrations as one array. No installer reloads a session.
Old releases remain available for rollback; do not edit them in place.

Settings, credentials, models, and sessions stay machine-local under
`~/.pi/agent`. Do not register additional copies of the managed packages.
Use `pi list` to inspect registrations and `pi config` to choose package resources.

## Updates

```sh
mise run pi-cli:update   # update the CLI
mise run //src/pi:deps         # restore locked package dependencies
mise run //src/pi:check       # typechecks and tests
```

**Restart after a CLI update.** Run `pi -c` from the same project to resume.
`/reload` reloads package resources, not the running SDK. Keep the companion
package's exact development SDK version and lockfile aligned with the installed
CLI; the `deps` task restores pins, it does not upgrade them.
After checking source changes, run `mise run //src/pi:install` and restart Pi.
`mise run //src/pi:verify-host` exercises a packaged child and reload in an isolated,
provider-free host; it does not change registrations.

Third-party packages use exact npm versions in `pi-packages.json`; there is no
local fork. To upgrade, edit the pin, remove the old registration with
`pi remove npm:<package>@<old-version>`, then run
`mise run //src/pi:upstream-install` and `mise run //src/pi:check`
and restart Pi. Preserve any package resource filters when replacing the entry.
Pinned versions do not advance with `pi update --extensions`.

The companion `deps` and `check` tasks cover the local companion and package-manager tests,
not upstream's test suite. After an upgrade, also spawn one worker and read its
result in a fresh Pi session.

## Everyday use

- `/login` — authenticate a provider.
- `/model` — select a model; Ctrl+S saves the default for new sessions.
- `/scoped-models` — choose models for cycling.
- `/settings` — choose a theme; `system` follows the terminal palette and `dots`
  is our custom light theme.
- `/context` — show reported context usage and estimated contributions from
  instructions, skills, tools, and conversation. Tool estimates are omitted when
  codemode rewrites declarations; estimates are not exact billing.
- Codemode — enable with `"defaultTools": ["+codemode"]` in personal settings to
  orchestrate tools with JavaScript while keeping direct calls available.
- Fullscreen is the default; set `"tuiMode": "regular"` for terminal scrollback.

For ChatGPT subscriptions, `/login openai` supersedes the legacy OpenAI Codex
provider. Test an entitled model before saving it with Ctrl+S in `/model`.

## Presentation

Tool rows put the subject first, with quiet outcome metadata nearby. Paths inside
the working directory are relative; narrow rows preserve filename tails.
Expanded details share a quiet margin and show the complete recorded path.

Direct `edit` and `write` calls show compact summaries: action, path, then quiet
metadata. Spacing separates groups; the subject has a stable left anchor and
metadata follows nearby rather than at the screen edge.
Tool names are bold; successful calls omit redundant status words/checks. A static
state gutter distinguishes receiving (·), queued (◦), running (◌), failure (✗),
and known success (blank). Failures carry an indented diagnostic without a
redundant status badge.
Timings under one second appear only when expanded. Short action labels share a small soft slot;
its padding yields in narrow panes. Ctrl+O expands edits
into content-sized old/new columns in wide panes (at least 120 available content
columns), or a unified diff when narrow or wrap-heavy. Expanded paths are shown
only when the header does not already contain the full path. Changed lines have subtle backgrounds; paired
replacements emphasize changed words. Writes show numbered source, not a
before/after diff. While write arguments stream, a live character count shows
progress; the static ◌ marker indicates execution. Errors stay visible when collapsed.

Both use Shiki's GitHub light/dark syntax palettes, following Pi's appearance.
Highlighting loads on expansion and is cached; oversized previews have explicit
display limits. Tool execution is unchanged.
Run the companion `deps`, `check`, and `install` tasks after dependency updates,
then restart Pi.
The displays, footer, editor, and `/ui` menu share `extensions/ui/index.ts`;
disable that resource in
`pi config` to restore the native UI.

Direct `bash` and `read` calls also collapse to compact summaries. Ctrl+O shows
the full shell command and recorded output, or highlighted source with original
file line numbers. Errors, truncation notices, continuation offsets, and full-output
paths remain visible. Pi still owns image previews; `!` commands and specialized
tool UIs are unchanged.

Codemode shows live activity counts and a compact child-call list, prioritizing
failures and running calls. Ctrl+O expands recorded argument/error summaries,
timing/cost, highlighted script, and script output. Pi does not record nested
results, so child source/diff previews are unavailable; abbreviated arguments
are never reconstructed. Errors and full-output paths stay visible when collapsed.
Existing explicit tool selections are preserved: enable `codemode: dots` in `/ui`
and apply/reload to add it.

The footer shows project/branch, model, thinking level when space allows, and
context usage. Other extensions retain their status line when present. Use
`/session` and `/context` for details.

The editor retains Pi's editing, shortcuts, autocomplete, and working status,
with softened borders that preserve thinking/bash-mode colors. An existing
custom editor is left alone.

Use `/ui` to select individual tool displays, footer, and editor, or switch all
to native. Nothing changes until **Apply and reload**; Cancel discards edits.
Preferences live in `~/.pi/agent/dots-ui.json`. Omitted fields use these defaults;
an empty tool list and `"native"` footer/editor leave those surfaces to Pi or
other extensions. Reload after manual edits. Invalid preferences report an error
rather than silently replacing them. Shutdown/replacement cancels an open menu.
Intervening edits are detected on a best-effort basis; simultaneous applies from
two Pi processes are last-writer-wins.

```json
{ "version": 1, "tools": ["read", "write", "edit", "bash", "codemode"], "footer": "compact", "editor": "soft" }
```

The reusable library is exposed as `@joshrwolf/pi-dots/ui` within the companion
package. See the [library reference](../src/pi/src/README.md) for its API.
Theme selection and startup settings remain Pi-owned.

### Light and dark palettes

The companion includes `dots-light` and `dots-dark`: neutral user-message panels,
unfilled ordinary tool/custom-message backgrounds, and a faint error tint for
native failed tools. Links, warnings, failures, and additions/removals retain
semantic colors. Diff syntax colors adapt when a changed-word background would
reduce readability.

After `/reload`, open `/settings` → **Theme** and select the pair in automatic
mode (light first, dark second). The equivalent Pi setting—not `dots-ui.json`—is:

```json
{ "theme": "dots-light/dots-dark" }
```

The palettes target near-white and dark-charcoal terminals. Use `system` instead
when you want Pi to derive all colors from an arbitrary terminal palette.
Package theme source edits require rebuilding/installing the companion before
`/reload` or restart.

## Workers

A worker is a background `pi --mode rpc` child with its own context, working
directory (usually a worktree), model, tool allowlist, and session file. The
runtime has five primitives: `worker_spawn`, `message`, `worker_wait`,
`worker_status`, `worker_stop`. There are no built-in workflow roles: a spawn carries
a self-contained brief, and a fixed child contract is appended (the final
message is the result; questions go to the parent; stay inside the brief's
surface).

`message { to, text, wait? }` is the one communication primitive and is not
directional: a session sends to a worker it owns (by id or label), to a sibling
(by id; the owner relays it and records the relay), or to `parent` when it is a
worker. Delivery adapts to the target: it answers a worker blocked on a
question, steers a running one between tool calls, or continues a finished,
stopped, or paused one in its own session. `wait: true` blocks until the target
replies or settles; that is how a worker asks its parent. Parent questions honor `timeoutMs`
(default ten minutes); cancellation or timeout is not approval. Sibling relays
are send-only: `wait: true` is rejected; ask the parent to coordinate a reply.
Ask the parent one question at a time. Multiple dialogs from child tools are
queued oldest-first; expired dialogs are not treated as answerable questions.
Messages to one worker serialize; stopping it preempts pending deliveries.
Command acknowledgements have a one-minute deadline, separate from model work.

Return is the event: a worker's completion, failure, question, or message
arrives as a transcript message that starts the owner's next turn, or is steered
into the current turn when the owner is busy, so nothing polls. `worker_wait`
blocks for the next such event when a session has nothing else to do. A live
widget below the editor lists running and asking workers with elapsed time,
tokens, calls, activity, and attention markers; the footer summarizes the fleet.
Completed ancestors remain visible while descendants are active; the count
excludes those contextual ancestor rows. The widget is terminal-only; RPC still
supports questions and notifications. `/workers` lists every worker and
`/workers <id>` shows what one has been doing. Children stop when the parent
exits and come back as `paused`; `message` continues one in its own session with
its model, tools, and contract re-applied, as does `pi --session <file>` by
hand. `worker_stop { all: true }` closes all owned processes, including idle
owners and their descendants, while retaining completed results. Sessions live
under `~/.pi/agent/dots/workers/`. `completed` means a result is ready, not that
its process has exited: status distinguishes an idle process from a closed one.
Stop-all reports how many processes it closed, even when none were actively working.
Displayed age is time since creation; token counts describe the latest assistant
response (including cached input), not cumulative spend.

For efficient delegation, give each helper a bounded, substantive task and the
smallest useful tool allowlist. Reuse it with `message` for related follow-ups.
Specify the desired output format in the brief (for example, a small JSON object
for aggregation); no special worker schema is needed. For multi-phase work,
state what later messages may request rather than saying “do nothing else.”
Sibling relays do not grant authority to expand the owner's scope.

Model choice is per spawn: a `provider/model[:thinking]` id, or `@name`
resolved through machine-local `~/.pi/agent/dots/workers.json`:

```json
{ "modelRoles": { "worker": "anthropic/claude-sonnet-5-5", "review": "anthropic/claude-opus-5-5:high" } }
```

Omitting `model` snapshots the parent's model and thinking level for the child.
Omitting `tools` uses the defaults; an empty allowlist permits only `message`.
`maxDepth` (default 3) limits nested spawning; `widgetRows` (default 8) folds the
live widget. Both must be positive integers. The best-effort observation registry
is `~/.pi/agent/dots/workers.db`; session entries, not that database, own resume
state. Activity history is bounded to 200 events per worker and 10,000 overall.
Events carry excerpts; `worker_status` retains up to 12,000 characters of an owned
worker's final result. Descendant lookup is read-only and limited to this tree;
it shows activity and a transcript tail, not the owner's retained result.
Tails read at most the last MiB and show at most 100 lines; complete history
remains in the child's session file.

Workers load the companion package, so
they see project memory; they are told not to edit tasks or memory.

Web search for research workers comes from `pi-web-access`: keyless Exa and
existing Pi ChatGPT authentication work without configuration. Optional provider
settings belong in machine-local `~/.pi/agent/web-search.json`. Browser cookie
access and hosted page-fetch fallbacks remain opt-in.

## Jobs

`job_start { command, cwd?, timeoutMs? }` admits a finite shell job without waiting
for completion. `job_status { id? }` inspects jobs; `job_stop { id }` requests a
stop. Jobs are a separately selectable
`extensions/jobs/index.ts` resource.
They currently require macOS and a kept-alive TUI or RPC session, not print/JSON
one-shot mode. The default timeout is 30 minutes; the maximum is 24 hours, with
four active slots. Caller cancellation after admission does not cancel the job.

The TUI shows a compact job widget. `/jobs [id]` opens a picker to inspect root
outcomes, cleanup evidence, capture warnings, and output paths, or confirm a stop
of a currently owned job. Unknown/historical jobs are inspection-only. Inspection
scrolls with arrow/Page keys; Enter/Esc closes it. Cancel changes nothing.
Human inspection and stops do not start a model turn; a human-stopped job's
completion is recorded without an automatic wake. In RPC/headless modes `/jobs`
prints a non-waking inspection record instead of opening terminal UI.

Results are recorded separately from automatic wakes. Busy runs continue only
when necessary to see a new result. Aborting a run preserves its result records
and suppresses further automatic job wakes until the next user-started run.
Shutdown/reload stops owned jobs without waking the model. Restored unfinished
jobs are **unknown**, never resumed; tree navigation is blocked while jobs live.

Output lives in private generated directories under `~/.pi/agent/dots/jobs/`:
`output.log` retains an 8 MiB prefix and `tail.log` the most recent 64 KiB at final
flush. Stdout/stderr are combined in arrival order. Use ordinary `read` for deeper
output. Capture facts expose truncation, disk errors, missing tails, and incomplete
flushes. Writes have a 256 KiB pending budget plus at most one 64 KiB chunk;
excess output is drained, not used as a reason to kill a healthy command.
Each runtime reserves at most 16 artifact directories (about 144 MiB allowance)
and evicts only its own finalized, observed artifacts. This is not a global disk
quota; historical or uncertain artifacts require manual removal when safe.

Each job has a retained Node process-group leader. Root exit, timeout, stop, and
owner disconnect initiate TERM, a fixed three-second grace, then group KILL.
Root outcome, stop intent, cleanup evidence, and capture completeness remain
separate facts. Supervisor crash/kill/hang, simultaneous owner/supervisor death,
job-control groups, and daemon/session escapes can leave descendants alive.
There is no numeric-PID fallback, universal containment, PTY/stdin, service
readiness, or reload survival. Use jobs only for trusted finite commands.

## Wakes

`wake` lets a session schedule its own future turn: once (`in: "20m"`,
`at: <ISO>`) or on an interval (`every: "10m"`), with a note to self. The fire
arrives as a transcript message that starts a turn when the session is idle or
follows the current one. With `check`, a shell command runs at each tick in the
session's cwd and the session wakes only per `on` (`change` in output, default;
`failure`; `success`; `always`), so polling CI or a long job costs no model
tokens until something happens; the command's output rides along in the fire.

Wakes are owned by the creating session (workers can set their own), persisted
in the session file, and re-armed on reload: a wake missed while the session was
away fires once, marked late, and a recurring one continues from now rather than
replaying missed slots. Intervals advance from the planned time, a fire is
skipped rather than stacked while the previous one is still unconsumed, every
recurring wake stops after `limit` fires (default 50), intervals are at least one
minute, and a session holds at most 20 active wakes. `wake_list`, `wake_cancel`,
and `wake_run` (fire now, ignoring the gate) manage them; `/wakes` lists them.
Manual runs count toward the same limit and reject overlapping checks; otherwise
they preserve the schedule. Checks have a two-minute deadline, bounded output
previews, and full-stream stdout/stderr hashes for change detection. Timeouts and
signal termination count as failures. Cancellation/reload aborts active checks
and discards their late results. Pipe draining is bounded; deliberately detached
descendants may survive, and incomplete pipe draining is reported. Use trusted, finite
checks, not daemons.
Nothing fires while Pi is closed.

## Project memory

Memory is learned, agent-maintained context, kept apart from the stable
instructions in `AGENTS.md`. The companion loads two indexes from
`~/.local/share/dots/memory/`: `_global/MEMORY.md` for every repository, then
`<main-checkout-key>/MEMORY.md` for the current one, so notes made in one
worktree are visible in every other worktree of the same repository and never
appear in a diff. A project line wins over a global one. The key is the main
checkout path (`git rev-parse --git-common-dir`, parent directory) with the
leading slash dropped and every run of characters other than letters, digits,
`.`, `_`, and `-` replaced by `-`. Both layers load as one block with today's
date and the conventions stated once; a layer without an index yet shows its
path instead, so the first "remember this" writes to the right place.
Concurrent sessions should edit the index with anchored edits; there is no
locking. Each index is capped at 150 lines or 24 KiB, with a notice naming how
much was cut; put detail in topic files beside it and reference them from the
index. `/memory` prints both locations and their current indexes. Nothing is
written automatically; the agent edits the files at checkpoints
under the conventions in `home/pi/.pi/agent/AGENTS.md`.

## Files

| Path | Purpose |
|---|---|
| `home/pi/.pi/agent/AGENTS.md` | Personal Pi instructions |
| `home/pi/.pi/agent/themes/` | Theme discovery |
| `src/pi/` | Our extensions |
| `pi-packages.json` | Exact upstream npm versions |

Package development instructions: [`src/pi/AGENTS.md`](../src/pi/AGENTS.md).
