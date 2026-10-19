# Optional gitsign cache

This package targets macOS with Homebrew at `/opt/homebrew`. Homebrew's
`gitsign` formula supplies both the signer and the credential-cache helper.
The wrapper uses Homebrew Python, already included in the Brewfile. Plain
`mise run install` does not install this package.

## Design

- A per-user LaunchAgent runs one `gitsign-credential-cache` process at login
  and restarts it if it exits. A small launcher captures its authentication
  output in a private log under `~/Library/Logs/gitsign/`.
- `~/.local/bin/gitsign-cached` supplies the socket environment variable to
  gitsign. No shell startup configuration is required.
- Each selected repository uses the wrapper through `gpg.x509.program`.
  Installation does not change any Git configuration.

The default socket is `~/Library/Caches/sigstore/gitsign/cache.sock`. The
wrapper also honors an explicit `GITSIGN_CREDENTIAL_CACHE`, for example for a
forwarded socket. The wrapper checks that the socket accepts a connection using
Python with a 250 ms timeout, without sending data or reading Git's signing input. If
the socket is absent or refuses connections, it unsets the variable and invokes
gitsign with normal authentication. This includes the stale socket left behind
when the service is stopped.

The probe is not an RPC health check: a service that accepts connections but
does not respond can still hang signing because upstream has no RPC deadline.
The service can also stop between the probe and gitsign's connection, producing
a signing error. The wrapper does not retry a failed signing operation.

The cache holds private keys and certificates in memory, separately for each
hostname and working directory. Authentication normally lasts about ten
minutes per worktree; signing does not extend that window. Restarting the
service clears the cache. Signing still needs network access. Processes able
to access the socket can retrieve the cached signing credentials.

See the upstream [cache documentation](https://github.com/sigstore/gitsign/blob/main/cmd/gitsign-credential-cache/README.md).

## Preview without enabling

```sh
mise run --dry-run gitsign:install
stow --dir=home --no-folding --simulate --verbose --target="$HOME" --restow gitsign
plutil -lint home/gitsign/Library/LaunchAgents/dev.joshrwolf.gitsign-credential-cache.plist
python3 -c 'import ast, pathlib; ast.parse(pathlib.Path("home/gitsign/.local/bin/gitsign-cached").read_text())'
sh -n home/gitsign/.local/libexec/gitsign-cache-service
```

## Enable when ready

```sh
mise run gitsign:install
```

This links the wrapper and LaunchAgent, enables the job, and loads it if it is
not already loaded. Running the target again leaves a running cache intact.
Changes to an already loaded plist require unloading and running the target
again.

In each repository already configured for gitsign, select the wrapper:

```sh
git config --local gpg.x509.program "$HOME/.local/bin/gitsign-cached"
```

Using an absolute path also works in Git clients that do not inherit the shell's
`PATH`. Existing signing settings such as `gpg.format` and `commit.gpgsign`
remain necessary. Worktrees follow Git's existing configuration scopes.

## Inspect or disable

```sh
launchctl print "gui/$(id -u)/dev.joshrwolf.gitsign-credential-cache"
tail -f "$HOME/Library/Logs/gitsign/credential-cache.log"
```

On a cache miss, the service itself opens the browser and obtains credentials.
Its stdout and stderr, including authentication URLs and failure messages, go
to `credential-cache.log` (mode `0600`, inside a `0700` directory). This file is
machine-local and appended across restarts; it may contain identity and working
directory information, so do not commit or share it unredacted.

The LaunchAgent uses its own environment, not the committing shell's browser
settings, and has no interactive terminal input. The log makes browser URLs and
device-flow instructions visible but does not provide terminal-code input. If
authentication requires terminal interaction, disable the service and use the
wrapper's ordinary gitsign fallback, or select the original signer directly.

To disable the service across logins and unload it now:

```sh
launchctl disable "gui/$(id -u)/dev.joshrwolf.gitsign-credential-cache"
launchctl bootout "gui/$(id -u)/dev.joshrwolf.gitsign-credential-cache"
```

After the service stops, selected repositories fall back to ordinary gitsign
authentication even if its socket remains. `bootout` reports an error if the
job is already unloaded. To stop using the
wrapper in a repository, restore its previous `gpg.x509.program` value
(normally `gitsign`). `mise run gitsign:install` enables the service again.
