# Machine setup

Mise installs committed source into `~/.local/share/dots/checkouts/dots`,
registers its machine modules, then applies them from HOME. Development
checkouts and machine-local credentials, settings and sessions stay separate.

## Setup

Use Git and mise 2026.10.25 or newer. From reviewed source:

```sh
cd bootstrap/source
mise trust
mise vars "dots_public_url=$(git rev-parse --show-toplevel)"
mise bootstrap --only files,repos,dotfiles,task
```

The selected Git path supplies committed HEAD, not working-tree changes.
Use a credential-free Git URL instead to install published source. Mise
remembers that selection; it does not fall back to another repository.

The base includes shell, editor, Rex, theme, skills, Pi and source-built
commands. Dependencies live in [tools.toml](tools.toml) and
[packages.toml](packages.toml). On Mac, missing Command Line Tools open Apple's
attended installer. Rustup owns Rust; Cargo owns the commands it builds.

### Optional modules

Select additions through native environments:

```sh
mise -E desktop bootstrap --only files,repos,dotfiles,task
```

- `desktop`: macOS arm64 desktop apps and configuration; Rex requires macOS 15+.
- `bash`: Bash activation without changing the login shell.
- `rex-server`: Linux login service; requires glibc, GNU coreutils, systemd 253+,
  SSH PAM and an enrolled Tailscale node with account policy.

For private tools and skills, supply their repository and select `private`:

```sh
mise vars dots_private_url=ssh://git@example.net/owner/private-dots.git
mise -E private,desktop bootstrap --only files,repos,dotfiles,task
```

Private source lives in the acquired checkout's ignored `private/` directory.
Its modules declare their own requirements. Both sources deploy tracked skills
into `~/.local/share/dots/skills`; the two agent discovery directories link there.
User-local and generated skills are not pruned automatically.

Environment selection persists in the machine's `miserc.local.toml`. Removing
an environment does not uninstall its resources; preview native unapply/prune
before removing anything.

## Inspect and update

From the source project:

```sh
mise bootstrap --only files,repos,dotfiles,task --dry-run
mise bootstrap --only files,repos,dotfiles,task --update
```

Normal apply keeps deployed source revisions. `--update` fetches and
fast-forwards; dirty sources and conflicting origins stop it. A first-use dry
run stops before loading an uncloned module; it is not a complete install preview.

From HOME:

```sh
mise bootstrap status
mise dot status
mise bootstrap
mise bootstrap --upgrade
```

Reapply reconciles the installed modules; `--upgrade` also upgrades their tools
and packages. Machine tools normally use latest. Project SDK/dependency locks
and Pi's host/SDK pin retain their own version contracts.

While Rex is beta, mise records a pinned initial installer and the signed Rex
updater owns later app releases. Reapply preserves a self-updated app; mise's
receipt reports the installer version, not the running build. Adopting an
unmanaged app requires an identical bundle and explicit approval.

Rex login-service updates do not restart it automatically. Activate one with
`mise run rex:restart-login`. Agent packages and source-loaded apps have their
own reload/start boundaries.

## SSH

Stage only the authored files in [source/](source/) and a `mise.local.toml`
containing the reviewed, credential-free source inputs:

```sh
mise bootstrap remote --host devbox --source ./prepared-setup --install-mise --only files,repos,dotfiles,task
```

Use `--remote-env` for target environments; local `-E` selects local config.
Mise owns SSH transport and host-key checks. Private Git access uses the host's
credentials or an explicitly selected read-only relay, never copied credentials.

Setup does not enroll devices, enable file history/synchronization or broadly
prune packages. Existing unmanaged files stop deployment rather than being
silently overwritten. See [SDK checks](../docs/build.md) for repository work.
