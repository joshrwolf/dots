# Commit signing

Commits are signed with stock [gitsign](https://github.com/sigstore/gitsign).
Its system credential cache keeps credentials in the macOS Keychain, so
repeated signing skips the browser flow without a daemon or wrapper.

## Setup

Enable the Keychain cache once:

```sh
git config --global gitsign.credentialCacheMode system
```

Then opt in each repository:

```sh
git config --local gpg.format x509
git config --local gpg.x509.program gitsign
git config --local commit.gpgsign true
```

If the cache cannot be set up, gitsign falls back to ordinary OIDC
authentication.

## Use

```sh
gitsign credentials list   # show cached credentials
gitsign credentials clear  # remove them
```

## Constraints

The Keychain cache is on gitsign `main` but unreleased after v0.17.1, so the
Brewfile installs gitsign from `HEAD`. After the next release, drop
`args: ["HEAD"]` and reinstall the bottle:
`brew uninstall gitsign && brew install gitsign`.
