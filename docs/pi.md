# Pi

Pi uses its official managed installer. Dotfiles provide personal instructions
and a theme. `pi-web-access` provides web research tools; our local companion
package provides `/context`, project memory, background workers, compact tool
displays, and a one-line footer.

## Setup

Requires GNU Make, Python 3.11+, npm/Node 24+, and GNU Stow.
Package targets prefer Homebrew's Node, separate from Pi's private runtime;
set `NODE_PREFIX` to use another installation.

```sh
make pi-cli-install  # install Pi and its managed runtime
make                 # stow dotfiles
make pi-install      # install pinned upstream packages and the local companion
```

Settings, credentials, models, and sessions stay machine-local under
`~/.pi/agent`. Do not register additional copies of the managed packages.
Use `pi list` to inspect registrations and `pi config` to choose package resources.

## Updates

```sh
make pi-cli-update   # update the CLI
make pi-deps         # restore locked package dependencies
make pi-check       # typechecks and tests
```

**Restart after a CLI update.** Run `pi -c` from the same project to resume.
`/reload` reloads package resources, not the running SDK. Keep the companion
package's exact development SDK version and lockfile aligned with the installed
CLI; `pi-deps` restores pins, it does not upgrade them.

Third-party packages use exact npm versions in `pi-packages.json`; there is no
local fork. To upgrade, edit the pin, remove the old registration with
`pi remove npm:<package>@<old-version>`, then run `make pi-install pi-check`
and restart Pi. Preserve any package resource filters when replacing the entry.
Pinned versions do not advance with `pi update --extensions`.

`pi-deps` and `pi-check` cover the local companion and package-manager tests,
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

Tool rows put the subject on the left and outcomes on the right. Paths inside
the working directory are relative; narrow rows preserve filename tails.
Expanded details share a quiet margin and show the complete recorded path.

Direct `edit` and `write` calls show compact summaries. Ctrl+O expands edits
into old/new columns in wide panes (at least 120 content columns), or a unified
diff when narrow or wrap-heavy. Changed lines have subtle backgrounds; paired
replacements emphasize changed words. Writes show numbered source, not a
before/after diff. While write arguments stream, a live character count shows
progress; `writing…` marks execution. Errors stay visible when collapsed.

Both use Shiki's GitHub light/dark syntax palettes, following Pi's appearance.
Highlighting loads on expansion and is cached; oversized previews have explicit
display limits. Tool execution and calls nested inside codemode are unchanged.
Run `make pi-deps` after dependency updates, then reload or restart Pi.
The displays, footer, editor, and `/ui` menu share `extensions/ui/index.ts`;
disable that resource in
`pi config` to restore the native UI.

Direct `bash` and `read` calls also collapse to compact summaries. Ctrl+O shows
the full shell command and recorded output, or highlighted source with original
file line numbers. Errors, truncation notices, continuation offsets, and full-output
paths remain visible. Pi still owns image previews; `!` commands and specialized
tool UIs are unchanged.

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
rather than silently replacing them.

```json
{ "version": 1, "tools": ["read", "write", "edit", "bash"], "footer": "compact", "editor": "soft" }
```

The reusable library is exposed as `@joshrwolf/pi-dots/ui` within the companion
package. See the [architecture](pi-ui-design.md) for boundaries and extension
points. Theme selection and startup settings remain Pi-owned.

## Workers

A worker is a background `pi --mode rpc` child with its own context, working
directory (usually a worktree), model, tool allowlist, and session file. The
runtime has five primitives: `worker_spawn`, `message`, `worker_wait`,
`worker_status`, `worker_stop`. There are no roles or workflows: a spawn carries
a self-contained brief, and a fixed child contract is appended (the final
message is the result; questions go to the parent; stay inside the brief's
surface).

`message { to, text, wait? }` is the one communication primitive and is not
directional: a session sends to a worker it owns (by id or label), to a sibling
(by id; the owner relays it and records the relay), or to `parent` when it is a
worker. Delivery adapts to the target: it answers a worker blocked on a
question, steers a running one between tool calls, or continues a finished or
paused one in its own session. `wait: true` blocks until the target replies or
settles; that is how a worker asks its parent.

Return is the event: a worker's completion, failure, question, or message
arrives as a transcript message that starts the owner's next turn, or is steered
into the current turn when the owner is busy, so nothing polls. `worker_wait`
blocks for the next such event when a session has nothing else to do. A live
`workers (n)` line below the editor lists running and asking workers with
elapsed time, tokens, and the current tool; `/workers` lists every worker and
`/workers <id>` shows what one has been doing. Children stop when the parent
exits and come back as `paused`; `message` continues one in its own session with
its model, tools, and contract re-applied, as does `pi --session <file>` by
hand. Sessions live under `~/.pi/agent/dots/workers/`.

Model choice is per spawn: a `provider/model[:thinking]` id, or `@name`
resolved through machine-local `~/.pi/agent/dots/workers.json`:

```json
{ "modelRoles": { "worker": "anthropic/claude-sonnet-5-5", "review": "anthropic/claude-opus-5-5:high" } }
```

Omitting `model` inherits the parent's. Workers load the companion package, so
they see project memory; they are told not to edit tasks or memory.

Web search for research workers comes from `pi-web-access`: keyless Exa and
existing Pi ChatGPT authentication work without configuration. Optional provider
settings belong in machine-local `~/.pi/agent/web-search.json`. Browser cookie
access and hosted page-fetch fallbacks remain opt-in.

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
under the conventions in `pi/.pi/agent/AGENTS.md`.

## Files

| Path | Purpose |
|---|---|
| `pi/.pi/agent/AGENTS.md` | Personal Pi instructions |
| `pi/.pi/agent/themes/` | Theme discovery |
| `pi-plugins/dots/` | Our extensions |
| `pi-packages.json` | Exact upstream npm versions |

Package development instructions: [`pi-plugins/AGENTS.md`](../pi-plugins/AGENTS.md).
