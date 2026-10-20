# Bootstrap over SSH

Clone pinned dots revisions into persistent directories, then apply the selected
public and optional private configuration. This project supplies configuration
to native `mise bootstrap remote`; it does not create machines or provision
credentials.

## Prepare

Use an SSH host you already trust and can reach. It needs Git, GNU Stow, and
mise's [remote prerequisites](https://mise.jdx.dev/bootstrap/remote.html).
Installing host packages is a separate, explicit operation. Native mise normally
stages a matching executable for the host. If the local mise build cannot provide
a cross-platform release, use its `--mise-bin` option with a compatible executable
or `--remote-mise` with one already installed; see the remote prerequisites above.

From the dots root, stage only these authored configuration files:

```sh
stage=$(mktemp -d)
cp bootstrap/remote/.miserc.toml bootstrap/remote/mise.toml \
   bootstrap/remote/mise.private.toml "$stage/"
cd "$stage"
```

Create `mise.local.toml` there with the full commit SHA of a reviewed, published
revision containing the bootstrap entry point:

```toml
[vars]
public_ref = "<full commit SHA>"
```

The default destination is `~/.local/share/dots/checkouts/dots` on the host and
the public selection is `agents`. Override `checkout` with an absolute remote
path or `public_profile` with another selection when needed. Branch names and
tags are rejected to prevent implicit updates on repeat runs.

For private setup, also set `private_url` and `private_ref` in that local file.
The private checkout goes inside the public checkout at `private/`; its default
selection is `skills`, overridable with `private_profile`. Never put a token in
a URL or configuration file. Do not copy other local files into the staging
directory: mise's archive does not use Git ignore rules as a security boundary.

## Apply

Review and trust the staged project, then preview and apply:

```sh
mise trust
mise bootstrap remote --host devbox --source . --install-mise --dry-run
mise bootstrap remote --host devbox --source . --install-mise
```

Private configuration needs an explicit remote environment and Git access. For
GitHub, borrow read-only access to just the private repository during the run:

```sh
mise bootstrap remote --host devbox --source . --install-mise \
  --remote-env private \
  --github-relay-read-only --github-relay-repo OWNER/REPOSITORY
```

Add `--dry-run` to preview that private operation. A remote dry run still opens
SSH, transfers files and stages mise; it is not an offline check. A first-run
preview cannot check installed-file conflicts inside repositories that are not
yet cloned. Apply preflights both installers before installing either selection.

Remove the local staging directory when finished. Installed links point into
the persistent checkouts, not staging. Mise is retained at `~/.local/bin/mise`;
this does not alter shell configuration or put it on PATH. Repeat bootstrap with
the same reviewed inputs to reconcile setup. Select new commit SHAs explicitly
to update; never force-reset a dirty checkout.

## Limits

Public and private source must already be published at the chosen revisions.
Bootstrap verifies HEAD before running each checkout's installer. It trusts the
selected configuration for execution, so review both repositories first.
Repository cloning is not transactional; installation failure can leave the
checkouts in place. Native mise global configuration still applies on the host.

The relay grants read access only during the connection. It does not authenticate
remote `gh`, allow pushes, or supply ongoing credentials. It is GitHub-specific;
other Git hosts need separately provisioned access. Workstation creation, project
checkouts, agent login and interactive sessions remain separate workflows.
