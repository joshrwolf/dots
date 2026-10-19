# Gitsign cache

An optional macOS LaunchAgent caches signing credentials for repositories that
explicitly select its wrapper.

## Setup

Requires Homebrew at `/opt/homebrew`, with `gitsign` and Python from the Brewfile.
Preview before enabling:

```sh
mise run --dry-run gitsign:install
stow --dir=home --no-folding --simulate --verbose --target="$HOME" --restow gitsign
```

Activate when ready:

```sh
mise run gitsign:install
git config --local gpg.x509.program "$HOME/.local/bin/gitsign-cached"
```

Installation enables/loads the service but does not change Git configuration.
Select the wrapper in each already-configured repository; existing signing
settings remain necessary. Reinstallation leaves a running cache intact.
A loaded plist change requires unloading before reinstalling.

## Use

```sh
launchctl print "gui/$(id -u)/dev.joshrwolf.gitsign-credential-cache"
tail -f "$HOME/Library/Logs/gitsign/credential-cache.log"
```

The service opens authentication links on cache misses. It has its own environment
and no interactive terminal input. If authentication requires terminal input,
disable it and use ordinary gitsign.

To disable across logins and unload now:

```sh
launchctl disable "gui/$(id -u)/dev.joshrwolf.gitsign-credential-cache"
launchctl bootout "gui/$(id -u)/dev.joshrwolf.gitsign-credential-cache"
```

`bootout` errors if already unloaded. Restore a repository's previous
`gpg.x509.program` to stop using the wrapper. The install task enables it again.

## Constraints

The default socket is `~/Library/Caches/sigstore/gitsign/cache.sock`; an explicit
`GITSIGN_CREDENTIAL_CACHE` overrides it. Missing/refused sockets fall back to
ordinary authentication, including stale sockets after shutdown.

The connection probe is not an RPC health check. A nonresponsive service can
still hang signing, and a service exiting after the probe can cause failure.
The wrapper never retries a failed signing operation.

Credentials live in memory, scoped by hostname and working directory. Restarting
clears them; signing still needs network access. Anyone able to access the socket
can retrieve cached credentials. See [upstream cache behavior](https://github.com/sigstore/gitsign/blob/main/cmd/gitsign-credential-cache/README.md).

The private log persists across restarts and can contain authentication URLs,
identity, and working-directory information. Never commit or share it unredacted.
