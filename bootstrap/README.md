# Machine setup

Install selected personal configuration without loading the dots development
SDKs. The base installs nothing; choose `agents` for portable instructions and
coding skills, or `desktop` for the existing macOS laptop configuration.

## Use

Use mise 2026.10.14 or newer and GNU Stow. Review the configuration before
trusting it, and keep this checkout at a persistent path:

```sh
cd bootstrap
mise trust
mise -E agents run preview             # Stow conflict check; changes nothing
mise -E agents bootstrap --dry-run     # preview the full mise operation
mise -E agents bootstrap
```

Use `-E desktop` instead for macOS arm64. The agent selection also supports
Linux amd64 and arm64. These are alternative selections, not additive profiles;
choose one. No selection installs host packages or agent binaries. Install those
separately when needed. The existing root `install` and `unstow` remain available.

The portable selection links Codex, Claude, and Pi instructions and the Go/Rust
coding skills. It leaves shell/editor settings, Pi themes, agent credentials,
workflow-specific skills, and ignored private skills alone. Some instructions
refer to optional tools or private workflows; this selection does not supply them.
Desktop retains the full existing Stow package selection, including locally
present skills. Neither selection activates services or changes the login shell.

Run from this directory, not with `mise -C bootstrap`: the supported mise version
reads discovery settings before applying `-C`. `.miserc.toml` stops discovery
before the enclosing checkout, excluding its development SDKs and tasks. Global
mise configuration still applies; inspect `mise config` before applying setup.

## Private configuration

An optional independent Git repository can live at `private/` beside this
directory. Public Git ignores it; public setup never clones or activates it.
Keep its remote URL and company-specific source in private or machine-local
configuration. An ignore rule prevents ordinary staging, not access by local
tools or agents.

Private source follows the same layout: `home/`, canonical `skills/`, and a
`bootstrap/` entry point with its own discovery boundary. Apply it separately:

```sh
cd ../private/bootstrap
mise trust
mise -E skills run preview
mise -E skills bootstrap
```

The private repository owns its selections and checks. Optional `private/*`
config roots are discoverable in the development task graph, but machine setup
runs from its own bootstrap directory to avoid development SDK inheritance.
Use distinct installed paths for public and private content; neither installer
should replace files owned by the other. Existing ignored local skills are not
moved or installed by the portable selection.

## Constraints

Stow owns these links, not mise's native dotfile manager. Mise's dry run prints
the task commands; `run preview` actually checks the Stow destinations. Apply
also checks conflicts across all selected packages before changing any links.
Do not force replacements or adopt unmanaged files to resolve a conflict.
Reapplication is supported, but a failed bootstrap is not a transaction.
Selecting fewer files does not remove files installed by an earlier selection.

For SSH hosts, use the [minimal remote bootstrap project](remote/README.md).
Do not send the whole dots checkout: ignored files are not a safe transfer
allowlist. Remote setup clones pinned revisions into persistent source paths;
installed files never link into mise's temporary staging directory.

See mise's [bootstrap](https://mise.jdx.dev/bootstrap.html) and
[machine modules](https://mise.jdx.dev/bootstrap/modules.html) documentation.
