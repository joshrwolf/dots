# Rex

Configuration for [Rex](https://www.superlogical.com/rex/docs), the terminal and
multiplexer. Sessions are projects or worktrees; search-first views open as
popups from hotkeys.

## Setup

[Machine setup](../../../../bootstrap/README.md) deploys this configuration
file by file under `~/.config/rex`. The views run in `dots-pick`, installed with
`mise run //src/pick:install`. Rex reads the configuration on `rex config reload`. In Rex's
settings, turn on **Remote Control** under **Rex Server** so actions can drive
the app.

## Use

| Key | Does |
| --- | --- |
| Cmd+P | Go on the current host: agents needing you, tabs, worktrees (`@`), directories (`/`) |
| In Go: Ctrl+Y | Switch host with Rex's host selector; Cmd+P opens Go there |
| In Go: Ctrl+R / Ctrl+T / Ctrl+O | Start Pi in a worktree: review the typed PR, work on the typed Linear issue, or a new session in the selected row's repo |
| Cmd+O | Back to the last tab you jumped from; again to return |
| Cmd+Shift+R | PRs: the watch list agents keep, with review and CI status |
| Cmd+Shift+A | Rex's activity panel: everything reporting status, across sessions |
| Cmd+T | New tab |
| Cmd+H / Cmd+K, Cmd+J / Cmd+L | Previous, next tab |
| Cmd+Shift+H / Cmd+Shift+L | Move tab back, forward |
| Cmd+B | Show or hide the sidebar |
| Ctrl+S, then \| / - | Split right, down |
| Cmd+Shift+Enter | Zoom pane |
| Ctrl+H/J/K/L | Move between panes, or within Neovim splits |
| Ctrl+Shift+H/J/K/L | Resize pane |
| Cmd+Up / Cmd+Down | Jump to the previous, next shell prompt |

`rex keymap` lists every binding with the line that made it, and the command
palette (Cmd+Shift+P) has every action, including "Agents needing attention…",
a ranked list of everything reporting status. A tab shows one view at a
time: a view's key closes it, and another view's key replaces it. Esc closes
a view and returns focus to the pane you were in, zoomed again if it was.

In Go, each row is one piece of work: an agent that needs you (blocked,
failed, or done), or a tab showing the status of the agent at work in it. A
row reads status, session, what it is, then what it says. What it is comes
from a tab or pane label you set, the agent's title, the directory (a
worktree as `repo@branch`, dropped when it only repeats the session's name),
or the agent's name, with the tab's position telling apart rows that would
read alike. Enter focuses an agent that needs you, switches to a tab, or
opens a session for a worktree or directory, creating one if needed. Rows
follow sessions and status as they change, and the preview shows the
selected terminal's screen. Tabs you jumped to recently come first.

Go names its destination host in the prompt. Sessions, directories, Git lookups
and starts stay on that host; it does not combine hosts' files or fall back to
another machine. Install this configuration and `dots-pick` on each host. Host
selection and unavailable-host handling belong to Rex; switching hosts leaves
the picker, and Cmd+P opens it on the selected host.

Go also starts work. Paste a GitHub PR URL and press Ctrl+R to review it
with Pi in the PR's worktree, in the checkout whose GitHub remote matches:
the current repo, one zoxide knows, or the one at the repository's checkout
path. When none exists, the new session first clones the repository to that
path with `gh repo clone`; Go refuses when the path holds anything else.
Type or paste a Linear issue ID or URL and press Ctrl+T to start Pi on it in
a worktree whose branch is the lowercase ID, in the one repo with a branch
for it, else the repo with the most branches for its team; when neither
picks one repo, Go lists the known repos. Select a repo's row (type `/` or
`@` to find one) and press Ctrl+O for a new session: a home worktree with a
generated two-word name, like `twilight-prairie`, opened at a shell, so
you start Pi or anything else yourself. Work in that session merges into
the home branch, so `git diff main...` there shows all of it.
The session label names the repo, as in `api:ABC-123`. When a
session already has a terminal at the worktree, Go shows it instead of
starting another. Otherwise the new session runs
worktrunk, which creates or reuses the worktree (hooks and approvals
included) and starts Pi there; when Pi exits, the pane is a shell in the
worktree. A malformed line, or Ctrl+O without a repo row, starts nothing:
the popup says why until you press Enter. When worktrunk fails, its error
stays in the session. The keys are the `KEYS` table at the top of
`dots/go.lua`.

### Checkout paths

JSON files in `~/.config/rex/checkouts/` say where a repository's primary
checkout lives. Each has a version and rules from a key to a path, which
starts with `~/` or `/` and may use `{owner}` and `{name}`:

```json
{
  "version": 1,
  "rules": {
    "*": "~/dev/personal/{owner}/{name}",
    "example-org/*": "~/dev/work/{name}",
    "example-org/side-project": "~/dev/personal/example-org/side-project"
  }
}
```

An `owner/name` key beats an `owner/*` key, which beats `*`, whichever file
holds them. Keys ignore case. Two files giving one key different paths is an
error naming both. This configuration ships only the `*` rule, in
`checkouts/default.json`; other machine setup can add files beside it.

A jump is a move to another tab through Go, the PR view, or Cmd+O. Cmd+O
also counts the tab you press it in, so a second press returns there even if
you arrived another way, such as from the sidebar. Stepping through tabs with
Cmd+H/J/K/L is not a jump. The jump history belongs to the selected host. Reloading its configuration forgets the jumps.

The PR view lists only PRs agents added with `dots github watch add` (see
[GitHub watch list](../../../../docs/github-watch.md)); it opens from cached
status and refreshes what is due while open. Each PR names the session its
work belongs to, recorded when an agent added it, or else the session working
at its checkout; the preview sits below the list. Merged and closed PRs show
only the 10 most recently checked. Enter goes to the recorded session while it
is live, otherwise to the PR's checkout, in its session if it has one, or the
link tab when there is none; Ctrl+O shows a clickable link and Ctrl+X stops
watching. The link opens on the viewing client when clicked; no browser runs on
the server. Close the link tab when finished.

In Rex terminals, zsh marks prompts (for prompt jumps and clearing what a
crashed program left behind) and reports commands: working while one runs,
then done or failed once it took 10 seconds or more, so long builds show in the
activity panel and notify. Ctrl+C counts as a cancel; editors, pagers, and
agents that report themselves are skipped. `DOTS_STATUS_AFTER` and
`DOTS_STATUS_IGNORE` in `~/.config/zsh/rex.zsh` tune both.
