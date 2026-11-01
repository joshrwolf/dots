# GitHub PR watch list

`dots github watch` keeps an agent-curated list of pull requests with cached
status, so a picker or agent can see what is in flight without querying GitHub
for every PR.

## Use

```sh
dots github watch add https://github.com/example/project/pull/7 --json
dots github watch list --json
dots github watch show https://github.com/example/project/pull/7 --color
dots github watch refresh --json
dots github watch follow
dots github watch remove https://github.com/example/project/pull/7 --json
```

Agents add the PRs they create or work on. `add` is idempotent: it records the
current checkout (default: the working directory), the invoking agent
session (Pi, Claude Code, or Codex), and the Rex session the work belongs
to, then checks that PR once. The Rex session is `REX_SESSION` in a Rex
terminal, otherwise `DOTS_REX_SESSION`, which Pi workers inherit from the
session that started them, so a headless worker's PR names its top session. `list` and `show` read only the local cache: `show`
prints one PR as plain text for a preview pane, including the description
stored by its last check. `show --color` adds basic ANSI styling; set `COLUMNS`
to a positive cell width to wrap either format. JSON output is unaffected.
`refresh` checks PRs that are due; `--all` checks
every open PR.

`follow` is a picker's watch command. It prints `ready`, then refreshes PRs as
they become due and prints `changed` whenever the watched rows change: after
every completed check, so check ages stay current, and when other sessions
add or remove entries. It exits when its output is
closed or it receives SIGTERM, SIGINT, or SIGHUP.

## Constraints

There is no daemon: PRs are checked only by `add`, `refresh`, and a `follow`
that a reader keeps open. Each PR stores when it is next due: immediately
when added, a minute later while its status is unknown or CI is pending, ten
minutes once settled, five minutes after a failed check. Merged and closed
PRs are never checked again and leave the list a day after their closure was
observed. A refresher claims due PRs with a two-minute lease, so concurrent
refreshers never fetch the same PR at once; `add` and `refresh --all` leave a
claimed PR to the check already in flight. A result is recorded only
for the claim that fetched it, so a late response from an expired claim never
overwrites a newer one.

Refresh uses existing `gh` authentication and one GraphQL request per 50 PRs.
Stored descriptions keep their first 16 KiB and say when they were cut. A
failed check keeps the previous status and records the error on that PR; a
failed add still lists the PR. A GitHub error within one PR's part of the
response, or a PR returned without its head commit, fails only that PR's
check; an error GitHub does not attribute to one PR fails every PR in the
request.

Adding a PR records intent only. It does not authorize fixing CI, monitoring, or
merging. Storage lives under `$XDG_STATE_HOME/dots/github/`, defaulting to
`~/.local/state/dots/github/`.
