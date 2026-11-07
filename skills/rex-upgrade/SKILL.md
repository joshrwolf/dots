---
name: rex-upgrade
description: Upgrade Rex on this Mac (app and embedded server) and on Linux hosts running the dots Rex login service, then reopen Pi sessions. Use when the user asks to update or upgrade Rex, its servers or clients.
---

# Rex upgrade

Temporary: remove this skill once Rex keeps running programs across server
upgrades or ships its own upgrade path for login-service hosts.

A request to upgrade authorizes these restarts and the host bootstrap. Starting
a stopped machine still needs the user's go-ahead.

## Find what is current

- Release notes and known problems: the superlogical MCP `list_changelog` and
  `read_changelog` (separate Mac, CLI, iOS and Linux Server posts), and
  `search_feedback` for active bugs about server restart or session restore.
- Mac: `rex server status` (running server version, host, sessions) and
  `rex version` (the app's bundled CLI). With Remote Control on,
  `rex -C <client> do client.update.status` reports the app update state.
- Linux: `curl -fsSL https://blob.superlogical.com/api/releases/rex/preview`
  gives the channel's commit, version and per-archive sha256. Archives live at
  `https://blob.superlogical.com/download/rex/preview/<commit>/rex-linux-<arch>.tar.gz`.
  Mac commits are often not published for Linux. `rex -S <host> server status`
  shows what a host runs; dots pins it in `bootstrap/tools.toml` (`http:rex`).

## Before any server restart

A server restart ends every program in its sessions; Rex restores layout, IDs
and directories only. Check that Pis are idle, then record each pane's block ID,
directory and Pi session file in `~/.local/state/rex-restore/`. Global memory
`wiki/rex-restore.md` has the capture method and pitfalls.

## Mac

1. Update the app first (programs survive an app relaunch).
2. The user restarts the server; this Pi ends with it, so the plan file must
   already exist.
3. Expect the same host ID and session count in `rex server status`. With zero
   sessions, restart it with
   `--state-dir "$HOME/Library/Application Support/rex"`.
4. Reopen Pis from the plan with `rex send -s <session> -b <block>`.

## Linux host

1. Update the pin to the channel's commit, verify both checksums by download,
   commit and push dots `main`.
2. On the host, under a login shell: `mise bootstrap --only
   files,repos,dotfiles,task --update` from
   `~/.local/share/dots/checkouts/dots/bootstrap/source`, then `mise bootstrap`
   from HOME. Confirm `/usr/local/bin/rex version`.
3. Restart `rex-login-server`. The account server keeps the old binary until
   stopped, and remote `server stop` is refused, so on the host run
   `rex --autostart=false -S unix:///run/user/$(id -u)/rex/server.sock server stop`.
4. The next client connection starts the new server. Confirm version, host ID
   and sessions, then reopen Pis.
